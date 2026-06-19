use std::path::Path;
use std::sync::Arc;

use futures_util::StreamExt;
use tracing::{error, info, warn};

use crate::adapter::types::{InternalMessage, text_content};
use crate::adapter::{self, types::StreamEvent};
use crate::config_system::{ConfigToml, SmartBrainConfig};
use crate::thread_store::{StoredThread, ThreadStore};

use super::index::{ExperienceEntry, ExperienceIndex};
use super::prompts;

/// Extract experiences from recently completed sessions.
///
/// Scans the thread store for eligible sessions that haven't been extracted yet
/// (or are stale), calls the LLM to extract durable knowledge, and writes the
/// results to `codey/memories/experiences/raw/{thread_id}.md`.
pub async fn run_extraction(
    http: &reqwest::Client,
    config: &ConfigToml,
    thread_store: &Arc<ThreadStore>,
    experiences_dir: &Path,
) {
    let exp_config = config.smartbrain_config();
    if !exp_config.is_active() || !exp_config.auto_extract {
        return;
    }

    let (provider_id, provider) = config.resolve_provider();
    let base_url = match provider.resolve_base_url() {
        Some(url) => url,
        None => {
            info!("Experience extraction skipped: no base URL for provider '{provider_id}'");
            return;
        }
    };
    let api_key = provider.resolve_api_key().unwrap_or_default();
    if api_key.is_empty() {
        info!("Experience extraction skipped: no API key configured");
        return;
    }
    let model = config.resolve_model();
    if model.is_empty() {
        info!("Experience extraction skipped: no model configured");
        return;
    }
    let wire_api = provider.wire_api.as_deref().unwrap_or("chat");

    let mut index = ExperienceIndex::load(experiences_dir);
    let threads = thread_store.list_threads().await;
    let eligible = find_eligible_threads(&threads, &index, &exp_config);

    if eligible.is_empty() {
        return;
    }

    let to_process = eligible
        .into_iter()
        .take(exp_config.max_rollouts_per_startup)
        .collect::<Vec<_>>();

    info!(
        "Experience extraction: processing {} eligible sessions",
        to_process.len()
    );

    let raw_dir = experiences_dir.join("raw");
    let _ = std::fs::create_dir_all(&raw_dir);

    for thread in &to_process {
        let messages = thread_store.get_thread_messages(&thread.id).await;
        match extract_single(
            http,
            &base_url,
            &api_key,
            &model,
            wire_api,
            &messages,
            config.max_output_tokens,
        )
        .await
        {
            Ok(Some(parsed)) => {
                let raw_path = raw_dir.join(format!("{}.md", thread.id));
                if let Err(e) = std::fs::write(&raw_path, &parsed.full_text) {
                    error!("Failed to write raw experience for {}: {e}", thread.id);
                    continue;
                }

                let entry = ExperienceEntry {
                    thread_id: thread.id.clone(),
                    extracted_at: now_secs(),
                    source_updated_at: thread.updated_at,
                    usage_count: 0,
                    last_used_at: None,
                    summary_slug: parsed.slug,
                    categories: parsed.categories,
                };
                index.upsert_entry(entry);
                info!("Extracted experience from thread {}", thread.id);
            }
            Ok(None) => {
                info!(
                    "No extractable experience from thread {} (too trivial)",
                    thread.id
                );
                let entry = ExperienceEntry {
                    thread_id: thread.id.clone(),
                    extracted_at: now_secs(),
                    source_updated_at: thread.updated_at,
                    usage_count: 0,
                    last_used_at: None,
                    summary_slug: None,
                    categories: vec![],
                };
                index.upsert_entry(entry);
            }
            Err(e) => {
                warn!("Experience extraction failed for thread {}: {e}", thread.id);
            }
        }
    }

    index.prune_expired(exp_config.max_unused_days);
    index.enforce_capacity(exp_config.max_raw_experiences);
    if let Err(e) = index.save(experiences_dir) {
        error!("Failed to save experience index: {e}");
    }
}

fn find_eligible_threads<'a>(
    threads: &'a [StoredThread],
    index: &ExperienceIndex,
    config: &SmartBrainConfig,
) -> Vec<&'a StoredThread> {
    threads
        .iter()
        .filter(|t| {
            let msg_count = t.all_messages().len();
            if msg_count < config.min_session_messages {
                return false;
            }

            let has_tool_calls = t
                .all_messages()
                .iter()
                .any(|m| m.tool_calls.as_ref().is_some_and(|tc| !tc.is_empty()));
            if !has_tool_calls {
                return false;
            }

            if !index.has_entry(&t.id) {
                return true;
            }

            index.is_stale(&t.id, t.updated_at)
        })
        .collect()
}

async fn extract_single(
    http: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    model: &str,
    wire_api: &str,
    history: &[crate::thread_store::ThreadMessage],
    max_tokens: Option<i64>,
) -> Result<Option<prompts::ParsedExtraction>, String> {
    let prompt_messages = prompts::build_extraction_messages(history);
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
    let body = adapter.build_body(model, &internal_messages, None, max_tokens);

    let response = http
        .post(&url)
        .headers(headers)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Experience extraction HTTP error: {e}"))?;

    if !response.status().is_success() {
        let status = response.status();
        let body_text = response.text().await.unwrap_or_default();
        return Err(format!(
            "Experience extraction LLM error ({status}): {body_text}"
        ));
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
        return Err("Experience extraction returned empty response".to_string());
    }

    Ok(prompts::parse_extraction_output(&result_text))
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
