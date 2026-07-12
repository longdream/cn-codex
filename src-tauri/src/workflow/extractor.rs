use std::collections::HashMap;
use std::sync::Arc;

use futures_util::StreamExt;
use tracing::info;

use crate::adapter::types::{InternalMessage, text_content};
use crate::adapter::{self, types::StreamEvent};
use crate::config_system::ConfigToml;
use crate::thread_store::ThreadStore;

use super::WorkflowDef;
use super::prompts;

/// Extract a workflow from a completed thread.
/// Returns the parsed WorkflowDef on success.
pub async fn extract_workflow_from_thread(
    http: &reqwest::Client,
    config: &ConfigToml,
    thread_store: &Arc<ThreadStore>,
    thread_id: &str,
) -> Result<WorkflowDef, String> {
    let (base_url, api_key, wire_api, model, query_params, extra_headers) =
        resolve_llm_endpoint(config)?;
    if model.is_empty() {
        return Err("Workflow extraction failed: no model configured".to_string());
    }

    let messages = thread_store.get_thread_messages(thread_id).await;
    if messages.is_empty() {
        return Err("Workflow extraction failed: thread is empty".to_string());
    }

    let prompt_messages = prompts::build_extraction_messages(&messages);
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

    let adapter = adapter::get_adapter(&wire_api);
    let url = adapter.build_url(&base_url, &model);
    let headers = adapter.build_headers(&api_key);
    let (url, headers) = adapter::apply_request_overrides(
        url,
        headers,
        query_params.as_ref(),
        extra_headers.as_ref(),
    )?;
    let body = adapter.build_body(&model, &internal_messages, None, config.max_output_tokens);

    let response = http
        .post(&url)
        .headers(headers)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Workflow extraction HTTP error: {e}"))?;

    if !response.status().is_success() {
        let status = response.status();
        let body_text = response.text().await.unwrap_or_default();
        return Err(format!(
            "Workflow extraction LLM error ({status}): {body_text}"
        ));
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
        return Err("Workflow extraction returned empty response".to_string());
    }

    info!("Workflow extraction completed, parsing result...");
    let mut def = prompts::parse_extraction_output(&result_text)?;

    // Fill in metadata
    def.source_thread_id = Some(thread_id.to_string());
    if def.created_at.is_empty() {
        def.created_at = chrono::Utc::now().to_rfc3339();
    }

    Ok(def)
}

/// Resolve LLM endpoint configuration (supports local-pool).
fn resolve_llm_endpoint(
    config: &ConfigToml,
) -> Result<
    (
        String,
        String,
        String,
        String,
        Option<HashMap<String, String>>,
        Option<HashMap<String, String>>,
    ),
    String,
> {
    let default_model = config.resolve_model();
    if !config.model_endpoints.is_empty() {
        let idx = config.active_endpoint_index.unwrap_or(0);
        let ep = &config.model_endpoints[idx.min(config.model_endpoints.len() - 1)];
        let (_, provider) = config.resolve_provider();
        let wire_api = ep
            .wire_api
            .as_deref()
            .or(provider.wire_api.as_deref())
            .unwrap_or("chat")
            .to_string();
        let model = ep
            .model
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(default_model.as_str())
            .to_string();
        Ok((
            ep.url.clone(),
            ep.api_key.clone().unwrap_or_default(),
            wire_api,
            model,
            None,
            None,
        ))
    } else {
        let (provider_id, provider) = config.resolve_provider();
        let url = provider.resolve_base_url().ok_or_else(|| {
            format!("Workflow extraction failed: no base URL for provider '{provider_id}'")
        })?;
        let key = provider.resolve_api_key().unwrap_or_default();
        if key.is_empty() {
            return Err("Workflow extraction failed: no API key configured".to_string());
        }
        let wire_api = provider.wire_api.as_deref().unwrap_or("chat").to_string();
        Ok((
            url,
            key,
            wire_api,
            default_model,
            provider.query_params.clone(),
            provider.http_headers.clone(),
        ))
    }
}
