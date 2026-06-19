use std::path::Path;

use futures_util::StreamExt;
use tracing::{error, info, warn};

use crate::adapter::types::{InternalMessage, text_content};
use crate::adapter::{self, types::StreamEvent};
use crate::config_system::ConfigToml;

use super::index::{ExperienceIndex, now_secs};
use super::okf::{OkfDocument, OkfFrontmatter};
use super::prompts;

/// Consolidate raw experience files into a compact summary and a detailed handbook.
pub async fn run_consolidation(
    http: &reqwest::Client,
    config: &ConfigToml,
    experiences_dir: &Path,
) {
    let sb_config = config.smartbrain_config();
    if !sb_config.is_active() || !sb_config.auto_consolidate {
        return;
    }

    let index = ExperienceIndex::load(experiences_dir);
    if index.entries.is_empty() {
        info!("Experience consolidation skipped: no entries to consolidate");
        return;
    }

    let (provider_id, provider) = config.resolve_provider();
    let base_url = match provider.resolve_base_url() {
        Some(url) => url,
        None => {
            info!("Experience consolidation skipped: no base URL for provider '{provider_id}'");
            return;
        }
    };
    let api_key = provider.resolve_api_key().unwrap_or_default();
    if api_key.is_empty() {
        info!("Experience consolidation skipped: no API key configured");
        return;
    }
    let model = config.resolve_model();
    if model.is_empty() {
        info!("Experience consolidation skipped: no model configured");
        return;
    }
    let wire_api = provider.wire_api.as_deref().unwrap_or("chat");

    let raw_dir = experiences_dir.join("raw");
    let ranked = index.ranked_entries();
    let top_entries: Vec<_> = ranked
        .into_iter()
        .take(sb_config.max_consolidation_entries)
        .collect();

    let mut raw_experiences: Vec<(String, String, u32)> = Vec::new();
    for entry in &top_entries {
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

    if raw_experiences.is_empty() {
        info!("Experience consolidation skipped: no raw experience files found");
        return;
    }

    info!(
        "Experience consolidation: merging {} raw experiences",
        raw_experiences.len()
    );

    match consolidate_via_llm(
        http,
        &base_url,
        &api_key,
        &model,
        wire_api,
        &raw_experiences,
        sb_config.summary_max_tokens,
        config.max_output_tokens,
    )
    .await
    {
        Ok((summary, handbook)) => {
            let timestamp = now_secs();
            if !summary.is_empty() {
                let summary_fm = OkfFrontmatter::new("Summary")
                    .with_title("Experience Summary")
                    .with_description("Compact summary of learned experiences for prompt injection")
                    .with_timestamp(timestamp);
                let summary_doc = OkfDocument::new(summary_fm, &summary);
                let summary_path = experiences_dir.join("experience_summary.md");
                if let Err(e) = summary_doc.write_to(&summary_path) {
                    error!("Failed to write experience_summary.md: {e}");
                }
            }
            if !handbook.is_empty() {
                let handbook_fm = OkfFrontmatter::new("Handbook")
                    .with_title("Experience Handbook")
                    .with_description("Detailed consolidated reference of all experiences")
                    .with_timestamp(timestamp);
                let handbook_doc = OkfDocument::new(handbook_fm, &handbook);
                let handbook_path = experiences_dir.join("experience_handbook.md");
                if let Err(e) = handbook_doc.write_to(&handbook_path) {
                    error!("Failed to write experience_handbook.md: {e}");
                }
            }

            let mut index = ExperienceIndex::load(experiences_dir);
            index.last_consolidated_at = Some(now_secs());
            if let Err(e) = index.save(experiences_dir) {
                error!("Failed to update experience index after consolidation: {e}");
            }

            super::append_log(
                experiences_dir,
                "Update",
                &format!(
                    "Consolidated {} experiences into summary and handbook",
                    raw_experiences.len()
                ),
            );

            info!(
                "Experience consolidation complete: summary={} chars, handbook={} chars",
                summary.len(),
                handbook.len()
            );
        }
        Err(e) => {
            warn!("Experience consolidation failed: {e}");
        }
    }
}

async fn consolidate_via_llm(
    http: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    model: &str,
    wire_api: &str,
    raw_experiences: &[(String, String, u32)],
    summary_max_tokens: usize,
    max_output_tokens: Option<i64>,
) -> Result<(String, String), String> {
    let prompt_messages =
        prompts::build_consolidation_messages(raw_experiences, summary_max_tokens);
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
    let body = adapter.build_body(model, &internal_messages, None, max_output_tokens);

    let response = http
        .post(&url)
        .headers(headers)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Consolidation HTTP error: {e}"))?;

    if !response.status().is_success() {
        let status = response.status();
        let body_text = response.text().await.unwrap_or_default();
        return Err(format!("Consolidation LLM error ({status}): {body_text}"));
    }

    let mut result_text = String::new();
    let mut stream = response.bytes_stream();
    let mut buffer = String::new();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("Stream error: {e}"))?;
        buffer.push_str(&String::from_utf8_lossy(&chunk));

        while let Some(line_end) = buffer.find('\n') {
            let line = buffer[..line_end].trim().to_string();
            buffer = buffer[line_end + 1..].to_string();

            if line.is_empty() || !line.starts_with("data: ") {
                continue;
            }

            let data = &line[6..];
            if adapter.is_stream_done(data) {
                break;
            }

            for event in adapter.parse_stream_line(data) {
                if let StreamEvent::TextDelta(delta) = event {
                    result_text.push_str(&delta);
                }
            }
        }
    }

    let result_text = result_text.trim().to_string();
    if result_text.is_empty() {
        return Err("Consolidation returned empty response".to_string());
    }

    Ok(prompts::parse_consolidation_output(&result_text))
}
