use std::process::Stdio;
use std::time::Duration;

use futures_util::StreamExt;
use tauri::{AppHandle, Emitter, State};
use tokio::sync::RwLock;
use tracing::info;

use crate::adapter;
use crate::adapter::types::{InternalMessage, StreamEvent, text_content};
use crate::agent::UserAttachment;
use crate::error::{AppError, AppResult};
use crate::state::AppState;
use crate::thread_store::ThreadGoalStatus;

pub struct StandaloneState {
    pub active: RwLock<bool>,
}

impl StandaloneState {
    pub fn new() -> Self {
        Self {
            active: RwLock::new(false),
        }
    }
}

const FORTUNE_SYSTEM_PROMPT: &str = "你是一位精通中国传统玄学的大师，擅长奇门遁甲和紫微斗数。请用严谨的方式进行推演。只输出有效 JSON 对象，不要输出推理过程。";
const PLAYWRIGHT_MCP_SERVER_NAME: &str = "playwright";
const PLAYWRIGHT_MCP_PACKAGE: &str = "@playwright/mcp@latest";
const PLAYWRIGHT_MCP_WARMUP_TIMEOUT_SECS: u64 = 180;

fn playwright_mcp_config_value() -> serde_json::Value {
    serde_json::json!({
        "command": "npx",
        "args": ["-y", PLAYWRIGHT_MCP_PACKAGE],
        "disabled": false
    })
}

fn trim_and_truncate(text: &str, max_chars: usize) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let chars: Vec<char> = trimmed.chars().collect();
    if chars.len() <= max_chars {
        return trimmed.to_string();
    }
    format!("{}...", chars[..max_chars].iter().collect::<String>())
}

async fn warmup_playwright_mcp_install() -> Result<String, String> {
    let mut command = tokio::process::Command::new("npx");
    command
        .arg("-y")
        .arg(PLAYWRIGHT_MCP_PACKAGE)
        .arg("--help")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let output = tokio::time::timeout(
        Duration::from_secs(PLAYWRIGHT_MCP_WARMUP_TIMEOUT_SECS),
        command.output(),
    )
    .await
    .map_err(|_| {
        format!(
            "Playwright MCP warmup timed out after {} seconds",
            PLAYWRIGHT_MCP_WARMUP_TIMEOUT_SECS
        )
    })?
    .map_err(|error| format!("Failed to run npx for Playwright MCP warmup: {error}"))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let detail = if !stdout.trim().is_empty() {
        trim_and_truncate(&stdout, 600)
    } else {
        trim_and_truncate(&stderr, 600)
    };

    if output.status.success() {
        Ok(if detail.is_empty() {
            "Playwright MCP warmup completed.".to_string()
        } else {
            detail
        })
    } else {
        let exit_desc = output
            .status
            .code()
            .map(|code| format!("exit code {code}"))
            .unwrap_or_else(|| "terminated by signal".to_string());
        Err(format!(
            "Playwright MCP warmup failed ({exit_desc}). {}",
            if detail.is_empty() {
                "No output from npx command.".to_string()
            } else {
                detail
            }
        ))
    }
}

fn build_openai_fortune_payload(
    model: &str,
    prompt: &str,
    with_json_mode: bool,
) -> serde_json::Value {
    let mut payload = serde_json::json!({
        "model": model,
        "messages": [
            { "role": "system", "content": FORTUNE_SYSTEM_PROMPT },
            { "role": "user", "content": prompt },
        ],
        "temperature": 0.2,
        "max_tokens": 4096,
    });
    if with_json_mode {
        payload["response_format"] = serde_json::json!({ "type": "json_object" });
    }
    payload
}

fn looks_like_json_mode_unsupported(body: &str) -> bool {
    let lower = body.to_ascii_lowercase();
    (lower.contains("response_format") && lower.contains("unsupported"))
        || (lower.contains("unknown parameter") && lower.contains("response_format"))
        || lower.contains("invalid parameter: response_format")
        || lower.contains("response_format.type")
}

fn extract_openai_message_content_text(message: Option<&serde_json::Value>) -> String {
    let Some(message) = message else {
        return String::new();
    };
    let Some(content) = message.get("content") else {
        return String::new();
    };
    match content {
        serde_json::Value::String(text) => text.trim().to_string(),
        serde_json::Value::Array(items) => {
            let mut parts: Vec<String> = Vec::new();
            for item in items {
                if let Some(text) = item.as_str() {
                    let trimmed = text.trim();
                    if !trimmed.is_empty() {
                        parts.push(trimmed.to_string());
                    }
                    continue;
                }
                if let Some(text) = item.get("text").and_then(|v| v.as_str()) {
                    let trimmed = text.trim();
                    if !trimmed.is_empty() {
                        parts.push(trimmed.to_string());
                    }
                    continue;
                }
                if let Some(text) = item.get("content").and_then(|v| v.as_str()) {
                    let trimmed = text.trim();
                    if !trimmed.is_empty() {
                        parts.push(trimmed.to_string());
                    }
                }
            }
            parts.join("\n")
        }
        serde_json::Value::Object(map) => map
            .get("text")
            .and_then(|v| v.as_str())
            .map(|v| v.trim().to_string())
            .unwrap_or_default(),
        _ => String::new(),
    }
}

fn emit_fortune_detail_event(app_handle: &AppHandle, event_name: &str, payload: serde_json::Value) {
    if let Err(err) = app_handle.emit(event_name, payload.clone()) {
        tracing::warn!("[fortune_detail_stream] failed to emit {event_name}: {err}");
    }
    crate::mobile_server::broadcast(event_name, payload);
}

fn extract_non_streaming_fortune_text(raw_body: &str) -> AppResult<String> {
    let fallback_text = raw_body.trim().to_string();
    let parsed: serde_json::Value = match serde_json::from_str(raw_body) {
        Ok(value) => value,
        Err(_) => {
            if fallback_text.is_empty() {
                return Err(AppError::Custom(
                    "Fortune detail response is empty".to_string(),
                ));
            }
            return Ok(fallback_text);
        }
    };

    let openai_content = extract_openai_message_content_text(parsed.pointer("/choices/0/message"));
    if !openai_content.trim().is_empty() {
        return Ok(openai_content);
    }

    if let Some(text) = parsed.pointer("/content/0/text").and_then(|v| v.as_str()) {
        let trimmed = text.trim();
        if !trimmed.is_empty() {
            return Ok(trimmed.to_string());
        }
    }

    if let Some(text) = parsed.get("output_text").and_then(|v| v.as_str()) {
        let trimmed = text.trim();
        if !trimmed.is_empty() {
            return Ok(trimmed.to_string());
        }
    }

    if let Some(text) = parsed
        .pointer("/output/0/content/0/text")
        .and_then(|v| v.as_str())
    {
        let trimmed = text.trim();
        if !trimmed.is_empty() {
            return Ok(trimmed.to_string());
        }
    }

    if let Some(parts) = parsed
        .pointer("/candidates/0/content/parts")
        .and_then(|v| v.as_array())
    {
        let merged = parts
            .iter()
            .filter_map(|part| part.get("text").and_then(|v| v.as_str()))
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        if !merged.is_empty() {
            return Ok(merged);
        }
    }

    if let Some(text) = parsed.get("text").and_then(|v| v.as_str()) {
        let trimmed = text.trim();
        if !trimmed.is_empty() {
            return Ok(trimmed.to_string());
        }
    }

    if !fallback_text.is_empty() {
        return Ok(fallback_text);
    }

    Err(AppError::Custom(
        "Fortune detail response does not contain readable text".to_string(),
    ))
}

fn process_fortune_stream_line(
    line: &str,
    adapter: &dyn adapter::ProviderAdapter,
    app_handle: &AppHandle,
    request_id: &str,
    full_text: &mut String,
    finish_reason: &mut Option<String>,
) {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return;
    }

    if adapter.is_stream_done(trimmed) {
        if finish_reason.is_none() {
            *finish_reason = Some("stop".to_string());
        }
        return;
    }

    for event in adapter.parse_stream_line(trimmed) {
        match event {
            StreamEvent::TextDelta(delta) => {
                if delta.is_empty() {
                    continue;
                }
                full_text.push_str(&delta);
                emit_fortune_detail_event(
                    app_handle,
                    "fortune-detail-delta",
                    serde_json::json!({
                        "requestId": request_id,
                        "delta": delta,
                    }),
                );
            }
            StreamEvent::Done {
                finish_reason: reason,
            } => {
                *finish_reason = reason.or(finish_reason.take());
            }
            StreamEvent::ToolCallDelta { .. } | StreamEvent::Usage(_) => {
                // Fortune detail stream only consumes text deltas.
            }
        }
    }
}

async fn run_fortune_detail_stream(
    app_handle: &AppHandle,
    request_id: &str,
    base_url: String,
    api_key: String,
    model: String,
    wire_api: String,
    prompt: String,
) -> AppResult<()> {
    info!(
        "[fortune_detail_stream] request_id={request_id}, base_url={base_url}, model={model}, wire_api={wire_api}, prompt_len={}",
        prompt.len()
    );

    let http = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(30))
        .read_timeout(std::time::Duration::from_secs(300))
        .build()
        .map_err(|e| AppError::Custom(format!("Failed to create HTTP client: {e}")))?;

    let adapter = adapter::get_adapter(&wire_api);
    let url = adapter.build_url(&base_url, &model);
    let headers = adapter.build_headers(&api_key);
    let messages = vec![
        InternalMessage {
            role: "system".to_string(),
            content: text_content(FORTUNE_SYSTEM_PROMPT),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        },
        InternalMessage {
            role: "user".to_string(),
            content: text_content(prompt),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        },
    ];
    let body = adapter.build_body(&model, &messages, None, Some(6144));

    let response = http
        .post(&url)
        .headers(headers)
        .json(&body)
        .send()
        .await
        .map_err(|e| AppError::Custom(format!("Fortune detail request failed: {e}")))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(AppError::Custom(format!(
            "Fortune detail API error {status}: {body}"
        )));
    }

    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    info!("[fortune_detail_stream] response content-type: {content_type}");

    if content_type.contains("application/json") && !content_type.contains("stream") {
        let raw_body = response
            .text()
            .await
            .map_err(|e| AppError::Custom(format!("Failed to read fortune detail body: {e}")))?;
        let text = extract_non_streaming_fortune_text(&raw_body)?;
        if text.trim().is_empty() {
            return Err(AppError::Custom(
                "Fortune detail response is empty".to_string(),
            ));
        }
        emit_fortune_detail_event(
            app_handle,
            "fortune-detail-delta",
            serde_json::json!({
                "requestId": request_id,
                "delta": text.clone(),
            }),
        );
        emit_fortune_detail_event(
            app_handle,
            "fortune-detail-completed",
            serde_json::json!({
                "requestId": request_id,
                "text": text,
                "finishReason": "stop",
            }),
        );
        return Ok(());
    }

    let mut stream = response.bytes_stream();
    let mut buffer = String::new();
    let mut full_text = String::new();
    let mut finish_reason: Option<String> = None;

    while let Some(chunk) = stream.next().await {
        let chunk = match chunk {
            Ok(value) => value,
            Err(err) => {
                if full_text.trim().is_empty() {
                    return Err(AppError::Custom(format!(
                        "Fortune detail stream read failed: {err}"
                    )));
                }
                tracing::warn!(
                    "[fortune_detail_stream] request_id={request_id} read error after partial output: {err}"
                );
                if finish_reason.is_none() {
                    finish_reason = Some("stream_error".to_string());
                }
                break;
            }
        };

        buffer.push_str(&String::from_utf8_lossy(&chunk));
        while let Some(line_end) = buffer.find('\n') {
            let line = buffer[..line_end].to_string();
            buffer = buffer[line_end + 1..].to_string();
            process_fortune_stream_line(
                &line,
                adapter.as_ref(),
                app_handle,
                request_id,
                &mut full_text,
                &mut finish_reason,
            );
        }
    }

    if !buffer.trim().is_empty() {
        process_fortune_stream_line(
            &buffer,
            adapter.as_ref(),
            app_handle,
            request_id,
            &mut full_text,
            &mut finish_reason,
        );
    }

    if full_text.trim().is_empty() {
        return Err(AppError::Custom(
            "Fortune detail stream returned empty content".to_string(),
        ));
    }

    emit_fortune_detail_event(
        app_handle,
        "fortune-detail-completed",
        serde_json::json!({
            "requestId": request_id,
            "text": full_text,
            "finishReason": finish_reason.unwrap_or_else(|| "stop".to_string()),
        }),
    );
    Ok(())
}

#[tauri::command]
pub async fn standalone_init(state: State<'_, AppState>) -> AppResult<String> {
    let mut active = state.standalone.active.write().await;
    *active = true;
    info!("Standalone mode activated");
    Ok("standalone mode initialized".to_string())
}

#[tauri::command]
pub async fn standalone_config_read(state: State<'_, AppState>) -> AppResult<serde_json::Value> {
    let config = state.config_manager.read()?;
    Ok(serde_json::json!({
        "config": config.to_json(),
        "filePath": state.config_path.to_string_lossy(),
    }))
}

#[tauri::command]
pub async fn standalone_config_write(
    state: State<'_, AppState>,
    edits: Vec<serde_json::Value>,
) -> AppResult<serde_json::Value> {
    let edit_pairs: Vec<(String, serde_json::Value)> = edits
        .iter()
        .filter_map(|edit| {
            let key = edit.get("keyPath")?.as_str()?.to_string();
            let value = edit.get("value")?.clone();
            Some((key, value))
        })
        .collect();

    state.config_manager.write(&edit_pairs)?;

    Ok(serde_json::json!({
        "status": "ok",
        "filePath": state.config_path.to_string_lossy(),
    }))
}

#[tauri::command]
pub async fn standalone_mcp_enable_playwright(
    state: State<'_, AppState>,
) -> AppResult<serde_json::Value> {
    let edits = vec![(
        format!("mcp_servers.{PLAYWRIGHT_MCP_SERVER_NAME}"),
        playwright_mcp_config_value(),
    )];
    state.config_manager.write(&edits)?;

    let install_result = warmup_playwright_mcp_install().await;
    let (install_status, detail, error) = match install_result {
        Ok(detail) => ("succeeded", Some(detail), None),
        Err(error) => ("failed", None, Some(error)),
    };

    Ok(serde_json::json!({
        "status": "ok",
        "filePath": state.config_path.to_string_lossy(),
        "serverName": PLAYWRIGHT_MCP_SERVER_NAME,
        "configured": true,
        "installStarted": true,
        "installStatus": install_status,
        "detail": detail,
        "error": error,
    }))
}

#[tauri::command]
pub async fn standalone_smartbrain_enable(
    app_handle: AppHandle,
    state: State<'_, AppState>,
    extract_all_history: bool,
) -> AppResult<serde_json::Value> {
    let extraction_start_at = if extract_all_history {
        serde_json::Value::Null
    } else {
        serde_json::json!(crate::smartbrain::index::now_secs())
    };

    let edits = vec![
        ("smartbrain.enabled".to_string(), serde_json::json!(true)),
        (
            "smartbrain.extraction_start_at".to_string(),
            extraction_start_at.clone(),
        ),
    ];
    let updated_config = state.config_manager.write(&edits)?;

    if extract_all_history {
        let thread_store = state.thread_store.clone();
        let workspace_config_dir = state.workspace_config_dir.clone();
        let config_for_task = updated_config.clone();
        let app_handle_for_task = app_handle.clone();
        tokio::spawn(async move {
            let experiences_dir = crate::smartbrain::experiences_dir(&workspace_config_dir);
            let _ = std::fs::create_dir_all(experiences_dir.join("raw"));

            let http = reqwest::Client::builder()
                .connect_timeout(std::time::Duration::from_secs(30))
                .read_timeout(std::time::Duration::from_secs(300))
                .build()
                .unwrap_or_default();

            crate::smartbrain::extractor::run_extraction_backfill(
                &http,
                &config_for_task,
                &thread_store,
                &experiences_dir,
                Some(&app_handle_for_task),
            )
            .await;
        });
    }

    Ok(serde_json::json!({
        "status": "ok",
        "filePath": state.config_path.to_string_lossy(),
        "enabled": true,
        "extractAllHistory": extract_all_history,
        "extractionStartAt": extraction_start_at,
    }))
}

#[tauri::command]
pub async fn standalone_thread_create(state: State<'_, AppState>) -> AppResult<serde_json::Value> {
    let config = state.config_manager.read()?;
    let thread = state
        .thread_store
        .create_thread(config.model.clone())
        .await?;
    *state.current_thread_id.write().await = Some(thread.id.clone());
    crate::mobile_server::broadcast(
        "active-thread-changed",
        serde_json::json!({ "threadId": thread.id }),
    );

    Ok(serde_json::json!({
        "thread": {
            "id": thread.id,
        }
    }))
}

#[tauri::command]
pub async fn standalone_thread_list(state: State<'_, AppState>) -> AppResult<serde_json::Value> {
    let threads = state.thread_store.list_threads().await;
    let list: Vec<serde_json::Value> = threads
        .iter()
        .filter(|t| !t.turns.is_empty())
        .map(|t| {
            serde_json::json!({
                "id": t.id,
                "name": t.name,
                "preview": t.preview(),
                "updatedAt": t.updated_at,
            })
        })
        .collect();

    Ok(serde_json::json!({ "data": list }))
}

/// Read-only peek at a thread's goal without switching the active thread.
#[tauri::command]
pub async fn standalone_thread_peek_goal(
    state: State<'_, AppState>,
    thread_id: String,
) -> AppResult<serde_json::Value> {
    let thread = state
        .thread_store
        .get_thread(&thread_id)
        .await
        .ok_or_else(|| AppError::Custom(format!("Thread not found: {thread_id}")))?;
    let goal_val = match thread.goal {
        Some(ref goal) => serde_json::to_value(goal).unwrap_or(serde_json::Value::Null),
        None => serde_json::Value::Null,
    };
    Ok(serde_json::json!({ "goal": goal_val }))
}

#[tauri::command]
pub async fn standalone_thread_read(
    state: State<'_, AppState>,
    thread_id: String,
) -> AppResult<serde_json::Value> {
    let thread = state
        .thread_store
        .get_thread(&thread_id)
        .await
        .ok_or_else(|| AppError::Custom(format!("Thread not found: {thread_id}")))?;
    *state.current_thread_id.write().await = Some(thread_id.clone());
    crate::mobile_server::broadcast(
        "active-thread-changed",
        serde_json::json!({ "threadId": thread_id }),
    );

    let turns: Vec<serde_json::Value> = thread
        .turns
        .iter()
        .map(|turn| {
            let items: Vec<serde_json::Value> = turn
                .messages
                .iter()
                .filter_map(|m| match m.role.as_str() {
                    "user" => Some(serde_json::json!({
                        "type": "userMessage",
                        "id": m.id,
                        "text": m.content,
                        "content": [{ "type": "text", "text": m.content }],
                    })),
                    "assistant" if m.tool_calls.is_some() => {
                        let tcs = m.tool_calls.as_ref().unwrap();
                        Some(serde_json::json!({
                            "type": "toolUse",
                            "id": m.id,
                            "calls": tcs.iter().map(|tc| serde_json::json!({
                                "id": tc.id,
                                "name": tc.name,
                                "arguments": tc.arguments,
                            })).collect::<Vec<_>>(),
                        }))
                    }
                    "assistant" => Some(serde_json::json!({
                        "type": "agentMessage",
                        "id": m.id,
                        "text": m.content,
                        "content": [{ "type": "text", "text": m.content }],
                    })),
                    "tool" => Some(serde_json::json!({
                        "type": "toolResult",
                        "id": m.id,
                        "text": m.content,
                        "toolName": m.tool_name,
                        "toolCallId": m.tool_call_id,
                    })),
                    _ => Some(serde_json::json!({
                        "type": "systemMessage",
                        "id": m.id,
                        "text": m.content,
                    })),
                })
                .collect();

            serde_json::json!({
                "id": turn.turn_id,
                "items": items,
                "startedAt": turn.started_at,
                "completedAt": turn.completed_at,
                "mode": turn.mode.clone(),
                "durationMs": turn.duration_ms,
                "changedFiles": turn.changed_files.clone(),
                "usage": turn.usage.clone(),
                "goalBudgetTokens": turn.goal_budget_tokens,
                "budgetLimited": turn.budget_limited,
            })
        })
        .collect();

    Ok(serde_json::json!({
        "thread": {
            "id": thread.id,
            "name": thread.name,
            "goal": thread.goal,
            "turns": turns,
        }
    }))
}

#[tauri::command]
pub async fn standalone_thread_goal_set(
    state: State<'_, AppState>,
    thread_id: String,
    objective: String,
    status: Option<String>,
    goal_budget_tokens: Option<u64>,
) -> AppResult<serde_json::Value> {
    let status = parse_goal_status(status.as_deref())?.unwrap_or(ThreadGoalStatus::Active);
    let goal = state
        .thread_store
        .set_thread_goal(&thread_id, objective, status, goal_budget_tokens)
        .await?;

    Ok(serde_json::json!({ "goal": goal }))
}

#[tauri::command]
pub async fn standalone_thread_goal_status(
    state: State<'_, AppState>,
    thread_id: String,
    status: String,
) -> AppResult<serde_json::Value> {
    let status = parse_goal_status(Some(status.as_str()))?
        .ok_or_else(|| AppError::Custom("Goal status is required".to_string()))?;
    let goal = state
        .thread_store
        .set_thread_goal_status(&thread_id, status)
        .await?;

    Ok(serde_json::json!({ "goal": goal }))
}

#[tauri::command]
pub async fn standalone_thread_goal_edit(
    state: State<'_, AppState>,
    thread_id: String,
    objective: String,
    goal_budget_tokens: Option<u64>,
) -> AppResult<serde_json::Value> {
    let goal = state
        .thread_store
        .edit_thread_goal(&thread_id, objective, goal_budget_tokens)
        .await?;

    Ok(serde_json::json!({ "goal": goal }))
}

#[tauri::command]
pub async fn standalone_thread_goal_clear(
    state: State<'_, AppState>,
    thread_id: String,
) -> AppResult<serde_json::Value> {
    state.thread_store.clear_thread_goal(&thread_id).await?;
    Ok(serde_json::json!({ "goal": serde_json::Value::Null }))
}

#[tauri::command]
pub async fn standalone_chat(
    app_handle: AppHandle,
    state: State<'_, AppState>,
    thread_id: String,
    message: String,
    attachments: Option<Vec<UserAttachment>>,
    cwd: Option<String>,
    mode: Option<String>,
    goal_budget_tokens: Option<u64>,
    robot_id: Option<String>,
) -> AppResult<serde_json::Value> {
    let config = state.config_manager.read()?;
    let override_cwd = cwd.map(std::path::PathBuf::from);
    let is_goal_mode = mode.as_deref() == Some("goal");
    let mode = mode.as_deref();
    // 接口层防护：仅在 goal 模式下向 agent 传递 robot_id，
    // 避免普通 chat 路径受到机器人编排逻辑影响。
    let robot_id_for_turn = resolve_robot_id_for_run_turn(mode, robot_id.as_deref());
    *state.current_thread_id.write().await = Some(thread_id.clone());

    let result = state
        .agent_engine
        .run_turn(
            &app_handle,
            &config,
            &thread_id,
            &message,
            attachments.unwrap_or_default(),
            override_cwd.as_deref(),
            mode,
            goal_budget_tokens,
            robot_id_for_turn,
        )
        .await;

    if let Err(ref err) = result {
        // Goal 模式下出错时将 goal 回退为 paused，避免前端状态卡死
        if is_goal_mode {
            info!("standalone_chat error in goal mode, reverting goal to paused: {err}");
            let _ = state
                .thread_store
                .set_thread_goal_status(&thread_id, ThreadGoalStatus::Paused)
                .await;
        }
    }

    result?;
    Ok(serde_json::json!({ "status": "ok" }))
}

fn resolve_robot_id_for_run_turn<'a>(
    mode: Option<&str>,
    robot_id: Option<&'a str>,
) -> Option<&'a str> {
    if matches!(mode, Some("goal" | "robot-modify")) {
        robot_id
    } else {
        None
    }
}

#[tauri::command]
pub async fn standalone_turn_interrupt(state: State<'_, AppState>) -> AppResult<serde_json::Value> {
    info!("Turn interrupt requested by user");
    state.agent_engine.interrupt();
    let current_thread_id = state.current_thread_id.read().await.clone();
    let interrupted_tools = state
        .agent_engine
        .interrupt_active_tools(current_thread_id.as_deref())
        .await;
    info!(
        "Turn interrupt completed: thread={:?}, interrupted_tools={interrupted_tools}",
        current_thread_id
    );
    Ok(serde_json::json!({ "status": "interrupted" }))
}

#[tauri::command]
pub async fn standalone_plan_open(path: String) -> AppResult<serde_json::Value> {
    info!("Opening plan file: {path}");
    let plan_path = std::path::Path::new(&path);
    if !plan_path.exists() {
        return Err(AppError::Custom(format!("Plan file not found: {path}")));
    }

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        std::process::Command::new("cmd")
            .args(["/C", "start", "", &path.replace('/', "\\")])
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map_err(|e| AppError::Custom(format!("Failed to open plan file: {e}")))?;
    }

    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&path)
            .spawn()
            .map_err(|e| AppError::Custom(format!("Failed to open plan file: {e}")))?;
    }

    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(&path)
            .spawn()
            .map_err(|e| AppError::Custom(format!("Failed to open plan file: {e}")))?;
    }

    Ok(serde_json::json!({ "status": "ok" }))
}

/// Proxy an LLM chat-completion call through the backend to avoid webview
/// gateway/CORS restrictions. Completely independent of the agent engine and
/// thread state.
#[tauri::command]
pub async fn fortune_llm_call(
    base_url: String,
    api_key: String,
    model: String,
    wire_api: String,
    prompt: String,
) -> AppResult<String> {
    info!(
        "[fortune_llm_call] base_url={base_url}, model={model}, wire_api={wire_api}, prompt_len={}",
        prompt.len()
    );

    let http = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(30))
        .read_timeout(std::time::Duration::from_secs(180))
        .build()
        .map_err(|e| AppError::Custom(format!("Failed to create HTTP client: {e}")))?;

    if wire_api == "anthropic" {
        let url = {
            let base = base_url.trim_end_matches('/');
            if base.ends_with("/messages") {
                base.to_string()
            } else {
                format!("{base}/messages")
            }
        };
        info!("[fortune_llm_call] Anthropic POST {url}");
        let resp = http
            .post(&url)
            .header("Content-Type", "application/json")
            .header("x-api-key", &api_key)
            .header("anthropic-version", "2023-06-01")
            .json(&serde_json::json!({
                "model": model,
                "max_tokens": 2048,
                "messages": [{ "role": "user", "content": prompt }],
            }))
            .send()
            .await
            .map_err(|e| {
                tracing::error!("[fortune_llm_call] Anthropic request failed: {e}");
                AppError::Custom(format!("Anthropic request failed: {e}"))
            })?;

        let status = resp.status();
        info!("[fortune_llm_call] Anthropic response status: {status}");
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            tracing::error!("[fortune_llm_call] Anthropic error {status}: {body}");
            return Err(AppError::Custom(format!(
                "Anthropic API error {status}: {body}"
            )));
        }

        let data: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| AppError::Custom(format!("Failed to parse Anthropic response: {e}")))?;
        let text = data
            .pointer("/content/0/text")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        info!("[fortune_llm_call] Anthropic response len={}", text.len());
        return Ok(text);
    }

    // OpenAI-compatible
    let url = {
        let base = base_url.trim_end_matches('/');
        if base.ends_with("/chat/completions") {
            base.to_string()
        } else {
            format!("{base}/chat/completions")
        }
    };
    info!("[fortune_llm_call] OpenAI-compat POST {url}");

    let mut req = http.post(&url).header("Content-Type", "application/json");
    if !api_key.is_empty() {
        req = req.header("Authorization", format!("Bearer {api_key}"));
    }
    let payload_with_json_mode = build_openai_fortune_payload(&model, &prompt, true);
    let mut resp = req
        .json(&payload_with_json_mode)
        .send()
        .await
        .map_err(|e| {
            tracing::error!("[fortune_llm_call] LLM request failed: {e}");
            AppError::Custom(format!("LLM request failed: {e}"))
        })?;

    let mut used_json_mode = true;
    let mut status = resp.status();
    info!("[fortune_llm_call] OpenAI-compat response status: {status}");
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        if looks_like_json_mode_unsupported(&body) {
            info!("[fortune_llm_call] response_format unsupported, retrying without json mode");
            used_json_mode = false;
            let mut retry_req = http.post(&url).header("Content-Type", "application/json");
            if !api_key.is_empty() {
                retry_req = retry_req.header("Authorization", format!("Bearer {api_key}"));
            }
            let payload_without_json_mode = build_openai_fortune_payload(&model, &prompt, false);
            resp = retry_req
                .json(&payload_without_json_mode)
                .send()
                .await
                .map_err(|e| {
                    tracing::error!(
                        "[fortune_llm_call] LLM request failed after json mode fallback: {e}"
                    );
                    AppError::Custom(format!("LLM request failed after fallback: {e}"))
                })?;
            status = resp.status();
            info!("[fortune_llm_call] OpenAI-compat fallback response status: {status}");
            if !status.is_success() {
                let fallback_body = resp.text().await.unwrap_or_default();
                tracing::error!("[fortune_llm_call] LLM fallback error {status}: {fallback_body}");
                return Err(AppError::Custom(format!(
                    "LLM API error {status}: {fallback_body}"
                )));
            }
        } else {
            tracing::error!("[fortune_llm_call] LLM error {status}: {body}");
            return Err(AppError::Custom(format!("LLM API error {status}: {body}")));
        }
    }

    let raw_body = resp
        .text()
        .await
        .map_err(|e| AppError::Custom(format!("Failed to read LLM response body: {e}")))?;
    let data: serde_json::Value = serde_json::from_str(&raw_body)
        .map_err(|e| AppError::Custom(format!("Failed to parse LLM response JSON: {e}")))?;

    let message = data.pointer("/choices/0/message");
    let content = extract_openai_message_content_text(message);
    let reasoning_len = message
        .and_then(|m| m.get("reasoning_content"))
        .and_then(|v| v.as_str())
        .map(|text| text.len())
        .unwrap_or(0);

    if content.trim().is_empty() {
        let finish_reason = data
            .pointer("/choices/0/finish_reason")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        tracing::error!(
            "[fortune_llm_call] Empty assistant content. finish_reason={finish_reason}, reasoning_len={reasoning_len}"
        );
        return Err(AppError::Custom(format!(
            "LLM returned empty assistant content (finish_reason={finish_reason})"
        )));
    }

    info!(
        "[fortune_llm_call] OpenAI-compat response len={}, json_mode={used_json_mode}, reasoning_len={reasoning_len}",
        content.len()
    );
    Ok(content)
}

#[tauri::command]
pub async fn fortune_detail_stream_start(
    app_handle: AppHandle,
    base_url: String,
    api_key: String,
    model: String,
    wire_api: String,
    prompt: String,
    request_id: Option<String>,
) -> AppResult<serde_json::Value> {
    let request_id = request_id
        .map(|id| id.trim().to_string())
        .filter(|id| !id.is_empty())
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    let app_handle_for_task = app_handle.clone();
    let request_id_for_task = request_id.clone();
    tokio::spawn(async move {
        emit_fortune_detail_event(
            &app_handle_for_task,
            "fortune-detail-started",
            serde_json::json!({
                "requestId": request_id_for_task.clone(),
            }),
        );
        if let Err(err) = run_fortune_detail_stream(
            &app_handle_for_task,
            &request_id_for_task,
            base_url,
            api_key,
            model,
            wire_api,
            prompt,
        )
        .await
        {
            let message = err.to_string();
            tracing::error!(
                "[fortune_detail_stream] request_id={} failed: {}",
                request_id_for_task,
                message
            );
            emit_fortune_detail_event(
                &app_handle_for_task,
                "fortune-detail-error",
                serde_json::json!({
                    "requestId": request_id_for_task,
                    "message": message,
                }),
            );
        }
    });

    Ok(serde_json::json!({
        "requestId": request_id,
    }))
}

fn parse_goal_status(value: Option<&str>) -> AppResult<Option<ThreadGoalStatus>> {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };

    let status = match value.to_ascii_lowercase().as_str() {
        "active" => ThreadGoalStatus::Active,
        "paused" | "pause" => ThreadGoalStatus::Paused,
        "blocked" => ThreadGoalStatus::Blocked,
        "usage_limited" | "usage-limited" | "usagelimited" => ThreadGoalStatus::UsageLimited,
        "budget_limited" | "budget-limited" | "budgetlimited" => ThreadGoalStatus::BudgetLimited,
        "complete" | "completed" => ThreadGoalStatus::Complete,
        other => {
            return Err(AppError::Custom(format!(
                "Unsupported goal status '{other}'"
            )));
        }
    };

    Ok(Some(status))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{playwright_mcp_config_value, resolve_robot_id_for_run_turn};

    #[test]
    fn resolve_robot_id_for_run_turn_enables_in_goal_and_robot_modify_modes() {
        assert_eq!(
            resolve_robot_id_for_run_turn(Some("chat"), Some("robot-a")),
            None
        );
        assert_eq!(
            resolve_robot_id_for_run_turn(Some("robot-create"), Some("robot-a")),
            None
        );
        assert_eq!(resolve_robot_id_for_run_turn(Some("goal"), None), None);
        assert_eq!(
            resolve_robot_id_for_run_turn(Some("goal"), Some("robot-a")),
            Some("robot-a")
        );
        assert_eq!(
            resolve_robot_id_for_run_turn(Some("robot-modify"), Some("robot-a")),
            Some("robot-a")
        );
    }

    #[test]
    fn playwright_mcp_config_value_uses_expected_defaults() {
        let value = playwright_mcp_config_value();
        assert_eq!(value.get("command").and_then(|v| v.as_str()), Some("npx"));
        assert_eq!(value.get("disabled").and_then(|v| v.as_bool()), Some(false));
        assert_eq!(
            value.get("args").and_then(|v| v.as_array()),
            Some(&vec![json!("-y"), json!("@playwright/mcp@latest")])
        );
    }
}
