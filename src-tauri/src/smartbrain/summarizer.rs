use std::collections::HashMap;
use std::path::Path;

use futures_util::StreamExt;
use tracing::{error, info, warn};

use crate::adapter::types::{InternalMessage, text_content};
use crate::adapter::{self, types::StreamEvent};
use crate::config_system::ConfigToml;

use super::bm25_index::BM25Index;
use super::index::{ExperienceEntry, ExperienceIndex, now_secs};
use super::okf::{OkfDocument, OkfFrontmatter};
use super::prompts;

/// Statistics describing a summarize-merge run.
#[derive(Debug, Clone, Default)]
pub struct SummarizeMergeStats {
    /// Number of experiences before the merge.
    pub before_count: usize,
    /// Number of experiences after the merge.
    pub after_count: usize,
    /// Whether the LLM step succeeded.
    pub success: bool,
    /// Human-readable error message when the run failed.
    pub error: Option<String>,
    /// Machine-readable skip reason for non-fatal no-op outcomes.
    pub skip_reason: Option<String>,
}

impl SummarizeMergeStats {
    fn ok(before: usize, after: usize) -> Self {
        Self {
            before_count: before,
            after_count: after,
            success: true,
            error: None,
            skip_reason: None,
        }
    }

    fn skipped(before: usize, after: usize, skip_reason: impl Into<String>) -> Self {
        Self {
            before_count: before,
            after_count: after,
            success: true,
            error: None,
            skip_reason: Some(skip_reason.into()),
        }
    }

    fn failed(before: usize, error: String) -> Self {
        Self {
            before_count: before,
            after_count: before,
            success: false,
            error: Some(error),
            skip_reason: None,
        }
    }
}

/// Categorize and merge experiences into a smaller set of consolidated entries.
///
/// Unlike [`super::consolidator::run_consolidation`] (which only produces a
/// summary/handbook), this REPLACES the source experiences with fewer merged
/// experience entries, genuinely reducing the total experience count.
///
/// Returns stats describing the outcome. When `bm25_path` is provided, the BM25
/// index is rebuilt after the merge so search reflects the new entries.
pub async fn run_summarize_merge(
    http: &reqwest::Client,
    config: &ConfigToml,
    experiences_dir: &Path,
    bm25_path: Option<&Path>,
) -> SummarizeMergeStats {
    let mut index = ExperienceIndex::load(experiences_dir);
    if index.entries.len() < 2 {
        info!("Experience summarize-merge skipped: fewer than 2 entries");
        return SummarizeMergeStats::ok(index.entries.len(), index.entries.len());
    }

    let raw_dir = experiences_dir.join("raw");
    // Self-heal: drop index entries whose raw files no longer exist.
    index
        .entries
        .retain(|entry| raw_dir.join(format!("{}.md", entry.thread_id)).is_file());

    let before_count = index.entries.len();
    if before_count < 2 {
        return SummarizeMergeStats::ok(before_count, before_count);
    }

    let (provider_id, provider) = config.resolve_provider();
    let mut model = config.resolve_model();
    let (base_url, api_key, wire_api, query_params, extra_headers) = match resolve_endpoint(
        config, &provider, &mut model,
    ) {
        Some(resolved) => resolved,
        None => {
            let msg = format!(
                "Experience summarize-merge skipped: no base URL, API key, or model for provider '{provider_id}'"
            );
            info!("{msg}");
            return SummarizeMergeStats::failed(before_count, msg);
        }
    };

    let ranked = index.ranked_entries();
    let mut raw_experiences: Vec<(String, String, u32)> = Vec::with_capacity(before_count);
    for entry in &ranked {
        let raw_path = raw_dir.join(format!("{}.md", entry.thread_id));
        match std::fs::read_to_string(&raw_path) {
            Ok(content) if !content.trim().is_empty() => {
                raw_experiences.push((entry.thread_id.clone(), content, entry.usage_count));
            }
            Ok(_) => {}
            Err(e) => {
                warn!("Could not read raw experience for {}: {e}", entry.thread_id);
            }
        }
    }

    if raw_experiences.len() < 2 {
        info!("Experience summarize-merge skipped: not enough readable raw files");
        return SummarizeMergeStats::ok(before_count, before_count);
    }

    // Aim for roughly half the entries, always strictly fewer than the input.
    let target_count = (raw_experiences.len() / 2).max(1);

    info!(
        "Experience summarize-merge: merging {} experiences into at most {} entries",
        raw_experiences.len(),
        target_count
    );

    let result_text = match summarize_merge_via_llm(
        http,
        &base_url,
        &api_key,
        &model,
        &wire_api,
        &raw_experiences,
        target_count,
        config.max_output_tokens,
        query_params.as_ref(),
        extra_headers.as_ref(),
    )
    .await
    {
        Ok(output) => output,
        Err(e) => {
            warn!("Experience summarize-merge failed: {e}");
            return SummarizeMergeStats::failed(before_count, e);
        }
    };

    let merged = match prompts::parse_summarize_merge_output(&result_text) {
        Ok(entries) => {
            persist_last_summarize_output(experiences_dir, &result_text, None);
            entries
        }
        Err(reason) => {
            persist_last_summarize_output(experiences_dir, &result_text, Some(&reason));
            if reason == prompts::SummarizeMergeParseReason::EmptyArray {
                info!("Experience summarize-merge skipped: model returned empty array");
                return SummarizeMergeStats::skipped(before_count, before_count, "empty_array");
            }
            let msg = format!("Summarize-merge parse failed ({reason})");
            warn!("{msg}");
            return SummarizeMergeStats::failed(before_count, msg);
        }
    };

    // Safety net: only commit the merge if it actually reduces the count.
    if merged.len() >= raw_experiences.len() {
        info!(
            "Experience summarize-merge skipped: LLM returned {} entries, not fewer than {}",
            merged.len(),
            raw_experiences.len()
        );
        return SummarizeMergeStats::skipped(before_count, before_count, "not_reduced");
    }

    let timestamp = now_secs();
    let source_thread_ids: Vec<String> = raw_experiences
        .iter()
        .map(|(id, _, _)| id.clone())
        .collect();

    // Remove the source experiences (raw files + index entries).
    for thread_id in &source_thread_ids {
        let raw_path = raw_dir.join(format!("{thread_id}.md"));
        let _ = std::fs::remove_file(&raw_path);
        index.remove_entry(thread_id);
    }

    // Write the merged experience entries.
    // The LLM does not report which source experiences went into each merged
    // entry, so distribute the total usage evenly as a fair, non-zero default.
    let total_usage: u32 = raw_experiences.iter().map(|(_, _, c)| *c).sum();
    let per_entry_usage = if merged.is_empty() {
        0
    } else {
        total_usage / merged.len() as u32
    };

    for (idx, entry) in merged.iter().enumerate() {
        let merged_id = format!("merged-{timestamp}-{idx}");

        let mut frontmatter = OkfFrontmatter::new("Experience")
            .with_tags(entry.categories.clone())
            .with_timestamp(timestamp)
            .with_extension("thread_id", serde_json::json!(merged_id))
            .with_extension("usage_count", serde_json::json!(per_entry_usage))
            .with_extension("merged_from", serde_json::json!(source_thread_ids));

        if let Some(slug) = &entry.slug {
            frontmatter = frontmatter.with_title(slug);
        }

        let okf_doc = OkfDocument::new(frontmatter, &entry.content);
        let raw_path = raw_dir.join(format!("{merged_id}.md"));
        if let Err(e) = okf_doc.write_to(&raw_path) {
            error!("Failed to write merged experience {merged_id}: {e}");
            continue;
        }

        let exp_entry = ExperienceEntry {
            thread_id: merged_id,
            extracted_at: timestamp,
            source_updated_at: timestamp,
            usage_count: per_entry_usage,
            last_used_at: Some(timestamp),
            summary_slug: entry.slug.clone(),
            title: entry.title.clone(),
            summary: entry.summary.clone(),
            categories: entry.categories.clone(),
        };
        index.upsert_entry(exp_entry);
    }

    let after_count = index.entries.len();
    if let Err(e) = index.save(experiences_dir) {
        error!("Failed to save experience index after summarize-merge: {e}");
    }

    // Rebuild the BM25 index so search reflects the merged set.
    if let Some(bm25_path) = bm25_path {
        let workspace_config_dir = experiences_dir
            .parent()
            .and_then(|p| p.parent())
            .unwrap_or(experiences_dir);
        super::search::rebuild_index(workspace_config_dir, bm25_path);
    } else {
        // Still keep the on-disk BM25 index roughly in sync by removing stale docs.
        let mut bm25 = BM25Index::load(&super::bm25_index_path(
            experiences_dir
                .parent()
                .and_then(|p| p.parent())
                .unwrap_or(experiences_dir),
        ));
        for thread_id in &source_thread_ids {
            bm25.remove_document(&format!("exp:{thread_id}"));
        }
    }

    super::regenerate_experiences_index_md(
        experiences_dir
            .parent()
            .and_then(|p| p.parent())
            .unwrap_or(experiences_dir),
    );

    super::append_log(
        experiences_dir,
        "Update",
        &format!(
            "Summarize-merge: consolidated {} experiences into {}",
            before_count, after_count
        ),
    );

    info!(
        "Experience summarize-merge complete: {} -> {} entries",
        before_count, after_count
    );

    SummarizeMergeStats::ok(before_count, after_count)
}

/// Resolve the active endpoint/provider into `(base_url, api_key, wire_api)`.
///
/// Returns `None` when the configuration is incomplete (missing URL, key, or
/// model). Mirrors the resolution logic used by the extractor/consolidator so
/// the summarize-merge pipeline can run standalone.
fn resolve_endpoint(
    config: &ConfigToml,
    provider: &crate::config_system::ModelProviderInfo,
    model: &mut String,
) -> Option<(
    String,
    String,
    String,
    Option<HashMap<String, String>>,
    Option<HashMap<String, String>>,
)> {
    if !config.model_endpoints.is_empty() {
        let idx = config.active_endpoint_index.unwrap_or(0);
        let ep = &config.model_endpoints[idx.min(config.model_endpoints.len() - 1)];
        let ep_wire_api = ep
            .wire_api
            .as_deref()
            .or(provider.wire_api.as_deref())
            .unwrap_or("chat");
        if let Some(ep_model) = ep
            .model
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            *model = ep_model.to_string();
        }
        return Some((
            ep.url.clone(),
            ep.api_key.clone().unwrap_or_default(),
            ep_wire_api.to_string(),
            None,
            None,
        ));
    }

    let url = provider.resolve_base_url()?;
    let key = provider.resolve_api_key().unwrap_or_default();
    if key.is_empty() || model.is_empty() {
        return None;
    }
    let wire = provider.wire_api.as_deref().unwrap_or("chat").to_string();
    Some((
        url,
        key,
        wire,
        provider.query_params.clone(),
        provider.http_headers.clone(),
    ))
}

async fn summarize_merge_via_llm(
    http: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    model: &str,
    wire_api: &str,
    raw_experiences: &[(String, String, u32)],
    target_count: usize,
    max_tokens: Option<i64>,
    query_params: Option<&HashMap<String, String>>,
    extra_headers: Option<&HashMap<String, String>>,
) -> Result<String, String> {
    let prompt_messages = prompts::build_summarize_merge_messages(raw_experiences, target_count);
    let internal_messages: Vec<InternalMessage> = prompt_messages
        .into_iter()
        .map(|(role, content)| InternalMessage {
            role,
            content: text_content(content),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        })
        .collect();

    let adapter = adapter::get_adapter(wire_api);
    let url = adapter.build_url(base_url, model);
    let headers = adapter.build_headers(api_key);
    let (url, headers) =
        adapter::apply_request_overrides(url, headers, query_params, extra_headers)?;
    let body = adapter.build_body(model, &internal_messages, None, max_tokens);

    let response = http
        .post(&url)
        .headers(headers)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Summarize-merge HTTP error: {e}"))?;

    if !response.status().is_success() {
        let status = response.status();
        let body_text = response.text().await.unwrap_or_default();
        return Err(format!("Summarize-merge LLM error ({status}): {body_text}"));
    }

    let mut result_text = String::new();
    let mut stream = response.bytes_stream();
    let mut buffer = String::new();
    let mut utf8_decoder = crate::utf8_stream::Utf8StreamDecoder::new();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("Stream error: {e}"))?;
        utf8_decoder.push(&mut buffer, &chunk);

        while let Some(line_end) = buffer.find('\n') {
            let line = buffer[..line_end].trim().to_string();
            buffer = buffer[line_end + 1..].to_string();

            if line.is_empty() {
                continue;
            }

            if adapter.is_stream_done(&line) {
                break;
            }

            for event in adapter.parse_stream_line(&line) {
                if let StreamEvent::TextDelta(delta) = event {
                    result_text.push_str(&delta);
                }
            }
        }
    }

    let result_text = result_text.trim().to_string();
    if result_text.is_empty() {
        return Err("Summarize-merge returned empty response".to_string());
    }

    Ok(result_text)
}

fn persist_last_summarize_output(
    experiences_dir: &Path,
    result_text: &str,
    parse_reason: Option<&prompts::SummarizeMergeParseReason>,
) {
    let debug_path = experiences_dir.join(".last_summarize_output.txt");
    let mut debug_text = format!(
        "# summarize-merge debug output\n# generated_at: {}\n# output_chars: {}\n",
        now_secs(),
        result_text.chars().count()
    );
    if let Some(reason) = parse_reason {
        debug_text.push_str(&format!("# parse_reason: {reason}\n"));
    }
    debug_text.push('\n');
    debug_text.push_str(result_text);

    if let Err(error) = std::fs::write(&debug_path, debug_text) {
        warn!(
            "Could not persist summarize-merge debug output to {}: {error}",
            debug_path.display()
        );
    }
}
