use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use futures_util::StreamExt;
use tauri::{AppHandle, Emitter};
use tracing::{error, info, warn};

use crate::adapter::types::{InternalMessage, text_content};
use crate::adapter::{self, types::StreamEvent};
use crate::config_system::{ConfigToml, SmartBrainConfig};
use crate::thread_store::{StoredThread, ThreadStore};

use super::index::{ExperienceEntry, ExperienceIndex, now_secs};
use super::okf::{OkfDocument, OkfFrontmatter};
use super::prompts;

enum ExtractionSelection<'a> {
    All { max_threads: Option<usize> },
    Thread { thread_id: &'a str },
}

/// Extract experiences from recently completed sessions.
pub async fn run_extraction(
    http: &reqwest::Client,
    config: &ConfigToml,
    thread_store: &Arc<ThreadStore>,
    experiences_dir: &Path,
) {
    let max_threads = config.smartbrain_config().max_rollouts_per_startup;
    run_extraction_internal(
        http,
        config,
        thread_store,
        experiences_dir,
        ExtractionSelection::All {
            max_threads: Some(max_threads),
        },
        None,
    )
    .await;
}

/// Run a silent full sweep for any missing eligible experience extraction jobs.
pub async fn run_extraction_backfill(
    http: &reqwest::Client,
    config: &ConfigToml,
    thread_store: &Arc<ThreadStore>,
    experiences_dir: &Path,
    app_handle: Option<&AppHandle>,
) {
    run_extraction_internal(
        http,
        config,
        thread_store,
        experiences_dir,
        ExtractionSelection::All { max_threads: None },
        app_handle,
    )
    .await;
}

/// Extract experience for one specific thread if it is eligible.
pub async fn run_extraction_for_thread(
    http: &reqwest::Client,
    config: &ConfigToml,
    thread_store: &Arc<ThreadStore>,
    experiences_dir: &Path,
    thread_id: &str,
    app_handle: Option<&AppHandle>,
) {
    run_extraction_internal(
        http,
        config,
        thread_store,
        experiences_dir,
        ExtractionSelection::Thread { thread_id },
        app_handle,
    )
    .await;
}

async fn run_extraction_internal(
    http: &reqwest::Client,
    config: &ConfigToml,
    thread_store: &Arc<ThreadStore>,
    experiences_dir: &Path,
    selection: ExtractionSelection<'_>,
    app_handle: Option<&AppHandle>,
) {
    let sb_config = config.smartbrain_config();
    if !sb_config.is_active() || !sb_config.auto_extract {
        return;
    }

    let mut index = ExperienceIndex::load(experiences_dir);
    let to_process: Vec<StoredThread> = match selection {
        ExtractionSelection::All { max_threads } => {
            let threads = thread_store.list_threads().await;
            let mut eligible = find_eligible_threads(threads, &index, &sb_config);
            if let Some(limit) = max_threads {
                eligible.truncate(limit);
            }
            eligible
        }
        ExtractionSelection::Thread { thread_id } => {
            let Some(thread) = thread_store.get_thread(thread_id).await else {
                return;
            };
            if thread_is_eligible(&thread, &index, &sb_config) {
                vec![thread]
            } else {
                Vec::new()
            }
        }
    };

    if to_process.is_empty() {
        return;
    }

    let (provider_id, provider) = config.resolve_provider();
    let mut model = config.resolve_model();
    let (base_url, api_key, wire_api, query_params, extra_headers) = if !config
        .model_endpoints
        .is_empty()
    {
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
            model = ep_model.to_string();
        }
        (
            ep.url.clone(),
            ep.api_key.clone().unwrap_or_default(),
            ep_wire_api.to_string(),
            None,
            None,
        )
    } else {
        let url = match provider.resolve_base_url() {
            Some(url) => url,
            None => {
                info!("Experience extraction skipped: no base URL for provider '{provider_id}'");
                return;
            }
        };
        let key = provider.resolve_api_key().unwrap_or_default();
        if key.is_empty() {
            info!("Experience extraction skipped: no API key configured");
            return;
        }
        let wire = provider.wire_api.as_deref().unwrap_or("chat").to_string();
        (
            url,
            key,
            wire,
            provider.query_params.clone(),
            provider.http_headers.clone(),
        )
    };
    if model.is_empty() {
        info!("Experience extraction skipped: no model configured");
        return;
    }

    emit_extraction_event(
        app_handle,
        "smartbrain-extraction-started",
        serde_json::json!({
            "total": to_process.len(),
        }),
    );

    info!(
        "Experience extraction: processing {} eligible sessions",
        to_process.len()
    );

    let raw_dir = experiences_dir.join("raw");
    let _ = std::fs::create_dir_all(&raw_dir);

    let mut succeeded_count: usize = 0;
    let mut failed_count: usize = 0;
    let total = to_process.len();

    for (idx, thread) in to_process.iter().enumerate() {
        let messages = thread_store.get_thread_messages(&thread.id).await;
        match extract_single(
            http,
            &base_url,
            &api_key,
            &model,
            &wire_api,
            &messages,
            config.max_output_tokens,
            query_params.as_ref(),
            extra_headers.as_ref(),
        )
        .await
        {
            Ok(Some(parsed)) => {
                succeeded_count += 1;
                let timestamp = now_secs();
                let mut frontmatter = OkfFrontmatter::new("Experience")
                    .with_tags(parsed.categories.clone())
                    .with_timestamp(timestamp)
                    .with_extension("thread_id", serde_json::json!(thread.id))
                    .with_extension("usage_count", serde_json::json!(0));

                if let Some(slug) = &parsed.slug {
                    frontmatter = frontmatter.with_title(slug);
                }

                let okf_doc = OkfDocument::new(frontmatter, &parsed.full_text);
                let raw_path = raw_dir.join(format!("{}.md", thread.id));
                if let Err(e) = okf_doc.write_to(&raw_path) {
                    error!("Failed to write raw experience for {}: {e}", thread.id);
                    continue;
                }

                let entry = ExperienceEntry {
                    thread_id: thread.id.clone(),
                    extracted_at: timestamp,
                    source_updated_at: thread.updated_at,
                    usage_count: 0,
                    last_used_at: None,
                    summary_slug: parsed.slug,
                    title: parsed.title,
                    summary: parsed.summary,
                    categories: parsed.categories,
                };
                index.upsert_entry(entry);
                info!("Extracted experience from thread {}", thread.id);
            }
            Ok(None) => {
                succeeded_count += 1;
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
                    title: None,
                    summary: None,
                    categories: vec![],
                };
                index.upsert_entry(entry);
            }
            Err(e) => {
                failed_count += 1;
                warn!("Experience extraction failed for thread {}: {e}", thread.id);
            }
        }

        emit_extraction_event(
            app_handle,
            "smartbrain-extraction-progress",
            serde_json::json!({
                "current": idx + 1,
                "total": total,
                "threadId": thread.id,
            }),
        );
    }

    index.prune_expired(sb_config.max_unused_days);
    index.enforce_capacity(sb_config.max_raw_experiences);
    if let Err(e) = index.save(experiences_dir) {
        error!("Failed to save experience index: {e}");
    }

    let workspace_config_dir = experiences_dir
        .parent()
        .and_then(|p| p.parent())
        .unwrap_or(experiences_dir);
    super::regenerate_experiences_index_md(workspace_config_dir);

    let processed_count = to_process.len();
    if processed_count > 0 {
        super::append_log(
            experiences_dir,
            "Update",
            &format!("Extracted experiences from {processed_count} sessions"),
        );
    }

    emit_extraction_event(
        app_handle,
        "smartbrain-extraction-completed",
        serde_json::json!({
            "total": total,
            "succeeded": succeeded_count,
            "failed": failed_count,
        }),
    );
}

fn emit_extraction_event(
    app_handle: Option<&AppHandle>,
    event_name: &str,
    payload: serde_json::Value,
) {
    if let Some(handle) = app_handle {
        if let Err(err) = handle.emit(event_name, payload.clone()) {
            warn!("Failed to emit {event_name}: {err}");
        }
    }
    crate::mobile_server::broadcast(event_name, payload);
}

fn find_eligible_threads(
    threads: Vec<StoredThread>,
    index: &ExperienceIndex,
    config: &SmartBrainConfig,
) -> Vec<StoredThread> {
    threads
        .into_iter()
        .filter(|t| thread_is_eligible(t, index, config))
        .collect()
}

fn thread_is_eligible(
    thread: &StoredThread,
    index: &ExperienceIndex,
    config: &SmartBrainConfig,
) -> bool {
    let msg_count = thread.all_messages().len();
    if msg_count < config.min_session_messages {
        return false;
    }

    if let Some(start_at) = config.extraction_start_at.filter(|value| *value > 0) {
        if thread.updated_at <= start_at {
            return false;
        }
    }

    let has_tool_calls = thread
        .all_messages()
        .iter()
        .any(|m| m.tool_calls.as_ref().is_some_and(|tc| !tc.is_empty()));
    if !has_tool_calls {
        return false;
    }

    if !index.has_entry(&thread.id) {
        return true;
    }

    index.is_stale(&thread.id, thread.updated_at)
}

async fn extract_single(
    http: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    model: &str,
    wire_api: &str,
    history: &[crate::thread_store::ThreadMessage],
    max_tokens: Option<i64>,
    query_params: Option<&HashMap<String, String>>,
    extra_headers: Option<&HashMap<String, String>>,
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
    let (url, headers) =
        adapter::apply_request_overrides(url, headers, query_params, extra_headers)?;
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
        return Err("Experience extraction returned empty response".to_string());
    }

    Ok(prompts::parse_extraction_output(&result_text))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::thread_store::{StoredThread, StoredTurn, ThreadMessage, ToolCallInfo};

    fn build_thread(thread_id: &str, updated_at: i64) -> StoredThread {
        StoredThread {
            id: thread_id.to_string(),
            name: None,
            created_at: updated_at - 10,
            updated_at,
            model: None,
            goal: None,
            active_plan: None,
            robot_state: None,
            turns: vec![StoredTurn {
                turn_id: "turn-1".to_string(),
                started_at: updated_at - 5,
                completed_at: Some(updated_at),
                mode: None,
                duration_ms: None,
                changed_files: vec![],
                usage: None,
                goal_budget_tokens: None,
                budget_limited: false,
                messages: vec![
                    ThreadMessage {
                        id: "m-user".to_string(),
                        role: "user".to_string(),
                        content: "please fix build".to_string(),
                        timestamp: updated_at - 4,
                        tool_call_id: None,
                        tool_name: None,
                        tool_calls: None,
                        attachments: Vec::new(),
                    },
                    ThreadMessage {
                        id: "m-assistant".to_string(),
                        role: "assistant".to_string(),
                        content: String::new(),
                        timestamp: updated_at - 3,
                        tool_call_id: None,
                        tool_name: None,
                        tool_calls: Some(vec![ToolCallInfo {
                            id: "tc-1".to_string(),
                            name: "shell".to_string(),
                            arguments: "cargo check".to_string(),
                        }]),
                        attachments: Vec::new(),
                    },
                ],
            }],
        }
    }

    #[test]
    fn thread_is_eligible_respects_extraction_start_at() {
        let mut config = SmartBrainConfig::default();
        config.min_session_messages = 1;
        config.extraction_start_at = Some(100);

        let index = ExperienceIndex::default();
        let old_thread = build_thread("t-old", 90);
        let new_thread = build_thread("t-new", 110);

        assert!(!thread_is_eligible(&old_thread, &index, &config));
        assert!(thread_is_eligible(&new_thread, &index, &config));
    }
}
