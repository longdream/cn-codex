use std::collections::{HashMap, HashSet};
use std::process::Stdio;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use futures_util::StreamExt;
use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Emitter, State};
use tokio::sync::RwLock;
use tracing::{info, warn};

use crate::adapter;
use crate::adapter::types::{InternalMessage, StreamEvent, text_content};
use crate::agent::UserAttachment;
use crate::error::{AppError, AppResult};
use crate::state::AppState;
use crate::thread_store::{ThreadGoal, ThreadGoalStatus};

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
#[cfg(target_os = "windows")]
const PLAYWRIGHT_MCP_COMMAND: &str = "npx.cmd";
#[cfg(not(target_os = "windows"))]
const PLAYWRIGHT_MCP_COMMAND: &str = "npx";

fn bundled_node_bin_dir(workspace_config_dir: &std::path::Path) -> Option<std::path::PathBuf> {
    let node_dir = workspace_config_dir.join("node");
    if node_dir.is_dir() {
        Some(node_dir)
    } else {
        None
    }
}

fn prepend_path_value(
    base_path: Option<std::ffi::OsString>,
    prepend_dir: &std::path::Path,
) -> std::ffi::OsString {
    let mut path_entries = vec![prepend_dir.to_path_buf()];
    if let Some(existing) = base_path {
        path_entries.extend(std::env::split_paths(&existing));
    }
    std::env::join_paths(path_entries).unwrap_or_else(|_| prepend_dir.as_os_str().to_os_string())
}

fn playwright_mcp_config_value() -> serde_json::Value {
    serde_json::json!({
        "command": PLAYWRIGHT_MCP_COMMAND,
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

async fn warmup_playwright_mcp_install(
    workspace_config_dir: &std::path::Path,
) -> Result<String, String> {
    let mut command = tokio::process::Command::new(PLAYWRIGHT_MCP_COMMAND);
    if let Some(node_dir) = bundled_node_bin_dir(workspace_config_dir) {
        let merged_path = prepend_path_value(std::env::var_os("PATH"), &node_dir);
        command.env("PATH", &merged_path);
        #[cfg(target_os = "windows")]
        command.env("Path", merged_path);
    }
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
    .map_err(|error| {
        format!(
            "Failed to run {} for Playwright MCP warmup: {error}",
            PLAYWRIGHT_MCP_COMMAND
        )
    })?;

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
                format!("No output from {} command.", PLAYWRIGHT_MCP_COMMAND)
            } else {
                detail
            }
        ))
    }
}

fn looks_like_json_mode_unsupported(body: &str) -> bool {
    let lower = body.to_ascii_lowercase();
    (lower.contains("response_format") && lower.contains("unsupported"))
        || (lower.contains("unknown parameter") && lower.contains("response_format"))
        || lower.contains("invalid parameter: response_format")
        || lower.contains("response_format.type")
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProviderCapabilityProbeResult {
    success: bool,
    cached: bool,
    fingerprint: String,
    probed_at: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    latency_ms: Option<u64>,
    capabilities: ProviderCapabilityValues,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProviderCapabilityValues {
    structured_tools: Option<bool>,
    streaming: Option<bool>,
    reasoning: Option<bool>,
    usage: Option<bool>,
    parallel_tool_calls: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    recommended_wire_api: Option<String>,
}

static CAPABILITY_PROBE_CACHE: OnceLock<Mutex<HashMap<String, ProviderCapabilityProbeResult>>> =
    OnceLock::new();

fn capability_probe_cache() -> &'static Mutex<HashMap<String, ProviderCapabilityProbeResult>> {
    CAPABILITY_PROBE_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn capability_fingerprint(
    provider_key: &str,
    base_url: &str,
    model: &str,
    wire_api: &str,
) -> String {
    format!(
        "{}|{}|{}|{}",
        provider_key.trim(),
        base_url.trim().trim_end_matches('/').to_ascii_lowercase(),
        model.trim(),
        wire_api.trim().to_ascii_lowercase()
    )
}

fn capability_probe_tool(name: &str) -> Value {
    serde_json::json!({
        "type": "function",
        "function": {
            "name": name,
            "description": "Internal capability probe. Do not execute.",
            "parameters": { "type": "object", "properties": {}, "additionalProperties": false }
        }
    })
}

fn probe_tool_count(value: &Value, wire_api: &str) -> usize {
    if wire_api.eq_ignore_ascii_case("responses") {
        return value
            .pointer("/output")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter(|item| {
                        item.get("type").and_then(Value::as_str) == Some("function_call")
                    })
                    .count()
            })
            .unwrap_or(0);
    }
    value
        .pointer("/choices/0/message/tool_calls")
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or(0)
}

async fn probe_json_request(
    http: &reqwest::Client,
    url: &str,
    headers: &reqwest::header::HeaderMap,
    body: &Value,
) -> Result<(u16, Value), String> {
    let response = http
        .post(url)
        .headers(headers.clone())
        .json(body)
        .send()
        .await
        .map_err(|error| format!("Request failed: {error}"))?;
    let status = response.status().as_u16();
    let raw = response
        .text()
        .await
        .map_err(|error| format!("Failed to read response: {error}"))?;
    let value = serde_json::from_str::<Value>(&raw).unwrap_or_else(|_| serde_json::json!({}));
    if !(200..300).contains(&status) {
        let detail = value
            .pointer("/error/message")
            .and_then(Value::as_str)
            .unwrap_or_else(|| raw.trim());
        return Err(format!(
            "HTTP {status}: {}",
            detail.chars().take(500).collect::<String>()
        ));
    }
    Ok((status, value))
}

async fn probe_streaming(
    http: &reqwest::Client,
    adapter: &dyn adapter::ProviderAdapter,
    url: &str,
    headers: &reqwest::header::HeaderMap,
    body: &Value,
) -> Result<bool, String> {
    let response = http
        .post(url)
        .headers(headers.clone())
        .json(body)
        .send()
        .await
        .map_err(|error| format!("Streaming request failed: {error}"))?;
    if !response.status().is_success() {
        return Ok(false);
    }
    let mut stream = response.bytes_stream();
    let mut buffer = String::new();
    let mut bytes = 0usize;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|error| format!("Streaming read failed: {error}"))?;
        bytes = bytes.saturating_add(chunk.len());
        if bytes > 256 * 1024 {
            return Ok(false);
        }
        buffer.push_str(&String::from_utf8_lossy(&chunk));
        while let Some(pos) = buffer.find('\n') {
            let line = buffer[..pos].trim_end_matches('\r').to_string();
            buffer.drain(..=pos);
            if adapter.is_stream_done(&line)
                || adapter
                    .parse_stream_line(&line)
                    .iter()
                    .any(|event| matches!(event, adapter::types::StreamEvent::Done { .. }))
            {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

/// 以流式方式探测 tools/usage 能力。
///
/// 部分网关（如 CodeBuddy）仅支持流式请求，非流式 JSON 探测会直接 400；
/// 此时用 stream:true 携带 tools 重新探测，按 SSE 事件聚合：
/// 返回 Some((tool_call_count, usage_seen)) 表示探测成功，None 表示失败。
async fn probe_streaming_tool_caps(
    http: &reqwest::Client,
    adapter: &dyn adapter::ProviderAdapter,
    url: &str,
    headers: &reqwest::header::HeaderMap,
    body: &Value,
) -> Option<(usize, bool)> {
    let response = http
        .post(url)
        .headers(headers.clone())
        .json(body)
        .send()
        .await
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    let mut stream = response.bytes_stream();
    let mut buffer = String::new();
    let mut bytes = 0usize;
    let mut tool_indexes = std::collections::HashSet::new();
    let mut usage_seen = false;
    let mut completed = false;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.ok()?;
        bytes = bytes.saturating_add(chunk.len());
        if bytes > 256 * 1024 {
            break;
        }
        buffer.push_str(&String::from_utf8_lossy(&chunk));
        while let Some(pos) = buffer.find('\n') {
            let line = buffer[..pos].trim_end_matches('\r').to_string();
            buffer.drain(..=pos);
            if adapter.is_stream_done(&line) {
                completed = true;
                break;
            }
            for event in adapter.parse_stream_line(&line) {
                match event {
                    adapter::types::StreamEvent::ToolCallDelta { index, .. }
                    | adapter::types::StreamEvent::ToolCallDone { index, .. } => {
                        tool_indexes.insert(index);
                    }
                    adapter::types::StreamEvent::Usage(_) => usage_seen = true,
                    adapter::types::StreamEvent::Done { .. } => completed = true,
                    _ => {}
                }
            }
            if completed {
                break;
            }
        }
        if completed {
            break;
        }
    }
    if completed || !tool_indexes.is_empty() || usage_seen {
        Some((tool_indexes.len(), usage_seen))
    } else {
        None
    }
}

#[tauri::command]
pub async fn probe_model_capabilities(
    base_url: String,
    api_key: String,
    model: String,
    wire_api: String,
    provider_key: Option<String>,
    force_refresh: Option<bool>,
) -> AppResult<serde_json::Value> {
    let provider_key = provider_key.unwrap_or_default();
    let fingerprint = capability_fingerprint(&provider_key, &base_url, &model, &wire_api);
    if !force_refresh.unwrap_or(false) {
        if let Some(cached) = capability_probe_cache()
            .lock()
            .ok()
            .and_then(|cache| cache.get(&fingerprint).cloned())
        {
            let mut result = cached;
            result.cached = true;
            return serde_json::to_value(result)
                .map_err(|error| AppError::Custom(format!("Probe serialization failed: {error}")));
        }
    }

    let start = std::time::Instant::now();
    let http = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(35))
        .build()
        .map_err(|error| AppError::Custom(format!("HTTP client error: {error}")))?;
    let adapter = adapter::get_adapter(&wire_api);
    let url = adapter.build_url(&base_url, &model);
    let headers = adapter.build_headers(&api_key);
    // 部分网关（如 CodeBuddy）要求 messages 至少包含 2 条消息，
    // 前置一条 system 消息以保证探测请求被接受（对所有 OpenAI 兼容供应商均无害）。
    let messages = vec![
        InternalMessage {
            role: "system".to_string(),
            content: text_content("You are a helpful assistant."),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        },
        InternalMessage {
            role: "user".to_string(),
            content: text_content(
                "Capability probe: call both supplied probe functions exactly once, in parallel if supported.",
            ),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        },
    ];
    let tools = [
        capability_probe_tool("__cn_codex_capability_probe_a"),
        capability_probe_tool("__cn_codex_capability_probe_b"),
    ];
    let mut tool_body =
        adapter::build_non_stream_body(&*adapter, &model, &messages, Some(&tools), Some(32));
    if let Some(object) = tool_body.as_object_mut() {
        object.insert("parallel_tool_calls".to_string(), Value::Bool(true));
    }

    let mut capabilities = ProviderCapabilityValues {
        structured_tools: None,
        streaming: None,
        reasoning: None,
        usage: None,
        parallel_tool_calls: None,
        recommended_wire_api: None,
    };
    let mut error = None;
    match probe_json_request(&http, &url, &headers, &tool_body).await {
        Ok((_, value)) => {
            let count = probe_tool_count(&value, &wire_api);
            capabilities.structured_tools = Some(count > 0);
            capabilities.parallel_tool_calls = Some(count > 1);
            capabilities.usage = Some(
                value.pointer("/usage").is_some() || value.pointer("/response/usage").is_some(),
            );
        }
        Err(message) => {
            // 部分网关（如 CodeBuddy）仅支持流式请求，非流式探测会直接报错；
            // 此时回退到流式探测，成功则不视为探测失败。
            let mut stream_tool_body =
                adapter.build_body(&model, &messages, Some(&tools), Some(128));
            if let Some(object) = stream_tool_body.as_object_mut() {
                object.insert("stream".to_string(), Value::Bool(true));
            }
            match probe_streaming_tool_caps(&http, &*adapter, &url, &headers, &stream_tool_body)
                .await
            {
                Some((count, usage_seen)) => {
                    capabilities.structured_tools = Some(count > 0);
                    capabilities.parallel_tool_calls = Some(count > 1);
                    capabilities.usage = Some(usage_seen);
                }
                None => error = Some(message),
            }
        }
    }

    let mut stream_body = adapter.build_body(&model, &messages, None, Some(16));
    if let Some(object) = stream_body.as_object_mut() {
        object.insert("stream".to_string(), Value::Bool(true));
    }
    capabilities.streaming = Some(
        probe_streaming(&http, &*adapter, &url, &headers, &stream_body)
            .await
            .unwrap_or(false),
    );

    if wire_api.eq_ignore_ascii_case("responses") || wire_api.eq_ignore_ascii_case("chat") {
        let mut reasoning_body =
            adapter::build_non_stream_body(&*adapter, &model, &messages, None, Some(32));
        if let Some(object) = reasoning_body.as_object_mut() {
            if wire_api.eq_ignore_ascii_case("responses") {
                object.insert(
                    "reasoning".to_string(),
                    serde_json::json!({ "effort": "low" }),
                );
            } else {
                object.insert(
                    "reasoning_effort".to_string(),
                    Value::String("low".to_string()),
                );
            }
        }
        if let Ok((_, value)) = probe_json_request(&http, &url, &headers, &reasoning_body).await {
            capabilities.reasoning = Some(
                value
                    .pointer("/output")
                    .and_then(Value::as_array)
                    .map(|items| {
                        items.iter().any(|item| {
                            item.get("type").and_then(Value::as_str) == Some("reasoning")
                        })
                    })
                    .unwrap_or(false)
                    || value
                        .pointer("/usage/output_tokens_details/reasoning_tokens")
                        .and_then(Value::as_u64)
                        .unwrap_or(0)
                        > 0
                    || value
                        .pointer("/usage/completion_tokens_details/reasoning_tokens")
                        .and_then(Value::as_u64)
                        .unwrap_or(0)
                        > 0,
            );
        }
    }

    capabilities.recommended_wire_api =
        if capabilities.structured_tools == Some(false) && wire_api.eq_ignore_ascii_case("chat") {
            Some("responses".to_string())
        } else if capabilities.structured_tools == Some(true) {
            Some(wire_api.clone())
        } else {
            None
        };

    let result = ProviderCapabilityProbeResult {
        success: error.is_none(),
        cached: false,
        fingerprint,
        probed_at: chrono::Utc::now().timestamp_millis().max(0) as u64,
        latency_ms: Some(start.elapsed().as_millis() as u64),
        capabilities,
        error,
    };
    if result.success {
        if let Ok(mut cache) = capability_probe_cache().lock() {
            cache.insert(result.fingerprint.clone(), result.clone());
        }
    }
    serde_json::to_value(result)
        .map_err(|error| AppError::Custom(format!("Probe serialization failed: {error}")))
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RemoteProviderModel {
    pub id: String,
    pub label: String,
    pub supports_vision: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_length: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u64>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FetchProviderModelsResult {
    pub supported: bool,
    pub models: Vec<RemoteProviderModel>,
}

/// 仅本对话生效的供应商/模型覆盖（由前端传入完整快照，不写回全局 config.toml）。
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadChatProviderOverride {
    pub provider_key: String,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub wire_api: Option<String>,
    #[serde(default)]
    pub requires_openai_auth: Option<bool>,
    #[serde(default)]
    pub model_id: Option<String>,
    #[serde(default)]
    pub reasoning_effort: Option<String>,
    #[serde(default)]
    pub model_context_window: Option<i64>,
    #[serde(default)]
    pub max_output_tokens: Option<i64>,
    #[serde(default)]
    pub model_supports_vision: Option<bool>,
    #[serde(default)]
    pub vision_fallback_kind: Option<String>,
    #[serde(default)]
    pub vision_fallback_provider: Option<String>,
    #[serde(default)]
    pub vision_fallback_model: Option<String>,
    #[serde(default)]
    pub model_endpoints: Option<Vec<ThreadChatModelEndpoint>>,
    #[serde(default)]
    pub active_endpoint_index: Option<usize>,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadChatModelEndpoint {
    pub url: String,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub wire_api: Option<String>,
}

fn build_models_url(base_url: &str, wire_api: &str) -> String {
    let base = base_url.trim().trim_end_matches('/');
    if base.ends_with("/models") {
        return base.to_string();
    }

    let strip_suffix = |suffix: &str| -> Option<String> {
        base.strip_suffix(suffix)
            .map(|prefix| prefix.trim_end_matches('/').to_string())
    };

    match wire_api {
        "anthropic" => {
            if let Some(prefix) = strip_suffix("/messages") {
                return format!("{prefix}/models");
            }
        }
        "gemini" => {
            if let Some(prefix) = strip_suffix("/models") {
                return format!("{prefix}/models");
            }
        }
        _ => {
            if let Some(prefix) = strip_suffix("/chat/completions") {
                return format!("{prefix}/models");
            }
            if let Some(prefix) = strip_suffix("/responses") {
                return format!("{prefix}/models");
            }
        }
    }

    format!("{base}/models")
}

fn normalize_remote_model_id(raw: &str) -> String {
    raw.trim().trim_start_matches("models/").to_string()
}

fn get_string_at_paths<'a>(value: &'a Value, paths: &[&str]) -> Option<&'a str> {
    paths
        .iter()
        .find_map(|path| value.pointer(path).and_then(Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn get_u64_at_paths(value: &Value, paths: &[&str]) -> Option<u64> {
    paths.iter().find_map(|path| {
        let candidate = value.pointer(path)?;
        candidate
            .as_u64()
            .or_else(|| candidate.as_i64().and_then(|v| u64::try_from(v).ok()))
            .or_else(|| {
                candidate
                    .as_str()
                    .and_then(|v| v.trim().parse::<u64>().ok())
            })
    })
}

fn array_path_contains_image(value: &Value, paths: &[&str]) -> bool {
    paths.iter().any(|path| {
        value
            .pointer(path)
            .and_then(Value::as_array)
            .map(|items| {
                items.iter().any(|item| {
                    item.as_str()
                        .map(|text| {
                            let lowered = text.trim().to_ascii_lowercase();
                            lowered.contains("image") || lowered.contains("vision")
                        })
                        .unwrap_or(false)
                })
            })
            .unwrap_or(false)
    })
}

fn parse_remote_model_entry(item: &Value) -> Option<RemoteProviderModel> {
    let raw_id = get_string_at_paths(item, &["/id", "/name", "/model"])?;
    let id = normalize_remote_model_id(raw_id);
    if id.is_empty() {
        return None;
    }

    let label = get_string_at_paths(item, &["/display_name", "/displayName", "/label"])
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| id.clone());

    let supports_vision = get_string_at_paths(item, &["/supports_vision", "/supportsVision"])
        .and_then(|value| value.parse::<bool>().ok())
        .or_else(|| item.pointer("/supports_vision").and_then(Value::as_bool))
        .or_else(|| item.pointer("/supportsVision").and_then(Value::as_bool))
        .or_else(|| {
            item.pointer("/capabilities/vision")
                .and_then(Value::as_bool)
        })
        .unwrap_or(false)
        || array_path_contains_image(
            item,
            &[
                "/modalities",
                "/input_modalities",
                "/inputModalities",
                "/supported_input_modalities",
                "/capabilities/modalities",
                "/capabilities/input_modalities",
                "/capabilities/inputModalities",
            ],
        );

    let context_length = get_u64_at_paths(
        item,
        &[
            "/context_length",
            "/contextLength",
            "/max_context_tokens",
            "/max_input_tokens",
            "/inputTokenLimit",
            "/capabilities/max_input_tokens",
        ],
    );

    let max_output_tokens = get_u64_at_paths(
        item,
        &[
            "/max_output_tokens",
            "/maxOutputTokens",
            "/max_tokens",
            "/outputTokenLimit",
            "/capabilities/max_output_tokens",
        ],
    );

    Some(RemoteProviderModel {
        id,
        label,
        supports_vision,
        context_length,
        max_output_tokens,
    })
}

fn parse_remote_models_response(value: &Value) -> Vec<RemoteProviderModel> {
    let entries = value
        .pointer("/data")
        .and_then(Value::as_array)
        .or_else(|| value.pointer("/models").and_then(Value::as_array))
        .or_else(|| value.as_array())
        .cloned()
        .unwrap_or_default();

    let mut seen = HashSet::new();
    let mut models = Vec::new();
    for item in entries {
        let Some(model) = parse_remote_model_entry(&item) else {
            continue;
        };
        if seen.insert(model.id.clone()) {
            models.push(model);
        }
    }
    models
}

fn looks_like_models_unsupported(status: reqwest::StatusCode, body: &str) -> bool {
    if matches!(
        status,
        reqwest::StatusCode::NOT_FOUND
            | reqwest::StatusCode::METHOD_NOT_ALLOWED
            | reqwest::StatusCode::GONE
            | reqwest::StatusCode::NOT_IMPLEMENTED
    ) {
        return true;
    }

    let lowered = body.to_ascii_lowercase();
    lowered.contains("not support")
        || lowered.contains("unsupported")
        || lowered.contains("not found")
        || lowered.contains("no route")
        || lowered.contains("cannot get /models")
        || lowered.contains("unknown url")
        || body.contains("不支持")
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

fn emit_goal_updated_event(app_handle: &AppHandle, thread_id: &str, goal: &ThreadGoal) {
    let payload = serde_json::json!({
        "threadId": thread_id,
        "goal": goal,
    });
    if let Err(err) = app_handle.emit("thread-goal-updated", payload.clone()) {
        warn!("failed to emit thread-goal-updated: {err}");
    }
    crate::mobile_server::broadcast("thread-goal-updated", payload);
}

fn emit_goal_cleared_event(app_handle: &AppHandle, thread_id: &str) {
    let payload = serde_json::json!({
        "threadId": thread_id,
    });
    if let Err(err) = app_handle.emit("thread-goal-cleared", payload.clone()) {
        warn!("failed to emit thread-goal-cleared: {err}");
    }
    crate::mobile_server::broadcast("thread-goal-cleared", payload);
}

pub(crate) fn extract_non_streaming_fortune_text(raw_body: &str) -> AppResult<String> {
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
            StreamEvent::Error(message) => {
                *finish_reason = Some(format!("error: {message}"));
            }
            StreamEvent::ToolCallDelta { .. }
            | StreamEvent::ToolCallDone { .. }
            | StreamEvent::ReasoningDelta(_)
            | StreamEvent::Usage(_) => {
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
    let (url, headers) =
        adapter::apply_request_overrides(url, headers, None, None).map_err(AppError::Custom)?;
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
    let mut utf8_decoder = crate::utf8_stream::Utf8StreamDecoder::new();
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

        utf8_decoder.push(&mut buffer, &chunk);
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

    let install_result = warmup_playwright_mcp_install(&state.workspace_config_dir).await;
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
    let workspace_cwd = state.cwd.read().await.clone();
    let workspace_root = std::path::PathBuf::from(workspace_cwd);
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
                        "attachments": m.attachments.iter().map(|attachment| serde_json::json!({
                            "name": attachment.name,
                            "type": attachment.mime_type,
                            "dataUrl": attachment.data_url,
                            "size": attachment.size,
                        })).collect::<Vec<_>>(),
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
    let active_plan_payload = thread.active_plan.as_ref().map(|plan| {
        let candidate = std::path::PathBuf::from(&plan.path);
        let resolved_path = if candidate.is_absolute() {
            candidate
        } else {
            workspace_root.join(candidate)
        };
        let content = std::fs::read_to_string(&resolved_path).unwrap_or_default();
        serde_json::json!({
            "path": resolved_path.to_string_lossy(),
            "content": content,
            "revision": plan.revision,
            "updatedAt": plan.updated_at,
        })
    });

    Ok(serde_json::json!({
        "thread": {
            "id": thread.id,
            "name": thread.name,
            "goal": thread.goal,
            "robotState": thread.robot_state,
            "activePlan": active_plan_payload,
            "turns": turns,
        }
    }))
}

#[tauri::command]
pub async fn standalone_thread_goal_set(
    app_handle: AppHandle,
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
    emit_goal_updated_event(&app_handle, &thread_id, &goal);

    Ok(serde_json::json!({ "goal": goal }))
}

#[tauri::command]
pub async fn standalone_thread_goal_status(
    app_handle: AppHandle,
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
    emit_goal_updated_event(&app_handle, &thread_id, &goal);

    Ok(serde_json::json!({ "goal": goal }))
}

#[tauri::command]
pub async fn standalone_thread_goal_edit(
    app_handle: AppHandle,
    state: State<'_, AppState>,
    thread_id: String,
    objective: String,
    goal_budget_tokens: Option<u64>,
) -> AppResult<serde_json::Value> {
    let goal = state
        .thread_store
        .edit_thread_goal(&thread_id, objective, goal_budget_tokens)
        .await?;
    emit_goal_updated_event(&app_handle, &thread_id, &goal);

    Ok(serde_json::json!({ "goal": goal }))
}

#[tauri::command]
pub async fn standalone_thread_goal_clear(
    app_handle: AppHandle,
    state: State<'_, AppState>,
    thread_id: String,
) -> AppResult<serde_json::Value> {
    state.thread_store.clear_thread_goal(&thread_id).await?;
    emit_goal_cleared_event(&app_handle, &thread_id);
    Ok(serde_json::json!({ "goal": serde_json::Value::Null }))
}

/// 编辑重发前截断：删除指定用户消息及其后的所有内容。
#[tauri::command]
pub async fn standalone_thread_truncate_before(
    state: State<'_, AppState>,
    thread_id: String,
    message_id: String,
    message_content: Option<String>,
) -> AppResult<serde_json::Value> {
    let kept = state
        .thread_store
        .truncate_after_message(&thread_id, &message_id, message_content.as_deref())
        .await?;
    Ok(serde_json::json!({
        "status": "ok",
        "keptCount": kept.len(),
        "messages": kept,
    }))
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
    provider: Option<ThreadChatProviderOverride>,
    smartbrain_enabled: Option<bool>,
    subagent_enabled: Option<bool>,
    client_message_id: Option<String>,
) -> AppResult<serde_json::Value> {
    let mut config = state.config_manager.read()?;
    apply_thread_chat_overrides(
        &mut config,
        provider.as_ref(),
        smartbrain_enabled,
        subagent_enabled,
    );
    let override_cwd = cwd.map(std::path::PathBuf::from);
    let is_goal_mode = mode.as_deref() == Some("goal");
    let mode = mode.as_deref();
    // 接口层防护：仅在 goal 模式下向 agent 传递 robot_id，
    // 避免普通 chat 路径受到机器人编排逻辑影响。
    let robot_id_for_turn = resolve_robot_id_for_run_turn(mode, robot_id.as_deref());
    *state.current_thread_id.write().await = Some(thread_id.clone());
    let active_turn_before = state
        .thread_store
        .get_active_turn(&thread_id)
        .await
        .map(|turn| turn.turn_id);

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
            client_message_id,
        )
        .await;

    if let Err(ref err) = result {
        let overlapping_turn = matches!(err, AppError::TurnAlreadyRunning { .. });
        let owned_active_turn = if overlapping_turn {
            None
        } else {
            state
                .thread_store
                .get_active_turn(&thread_id)
                .await
                .filter(|active_turn| {
                    active_turn_before.as_deref() != Some(active_turn.turn_id.as_str())
                })
        };
        if let Some(active_turn) = owned_active_turn.as_ref() {
            let completed_at = chrono::Utc::now().timestamp();
            let duration_ms =
                completed_at.saturating_sub(active_turn.started_at).max(0) as u64 * 1000;
            let _ = state
                .thread_store
                .end_turn(
                    &thread_id,
                    &active_turn.turn_id,
                    Some(duration_ms),
                    Vec::new(),
                    None,
                    false,
                )
                .await;
            crate::agent::emit_and_broadcast(
                &app_handle,
                "turn-failed",
                serde_json::json!({
                    "threadId": thread_id,
                    "status": "failed",
                    "error": err.to_string().chars().take(2000).collect::<String>(),
                    "turn": {
                        "id": active_turn.turn_id,
                        "mode": active_turn.mode,
                        "startedAt": active_turn.started_at * 1000,
                        "completedAt": completed_at * 1000,
                        "durationMs": duration_ms,
                    }
                }),
            );
        }
        // Goal 模式下出错时将 goal 回退为 paused，避免前端状态卡死
        if is_goal_mode && owned_active_turn.is_some() {
            info!("standalone_chat error in goal mode, reverting goal to paused: {err}");
            if let Ok(goal) = state
                .thread_store
                .set_thread_goal_status(&thread_id, ThreadGoalStatus::Paused)
                .await
            {
                emit_goal_updated_event(&app_handle, &thread_id, &goal);
            }
        }
    }

    if result.is_ok()
        && state.agent_engine.is_thread_cancelled(&thread_id)
        && state
            .thread_store
            .get_active_turn(&thread_id)
            .await
            .is_none()
    {
        crate::agent::emit_and_broadcast(
            &app_handle,
            "turn-cancelled",
            serde_json::json!({
                "threadId": thread_id,
                "status": "cancelled",
                "turn": {
                    "id": "pre-turn-cancelled",
                    "mode": mode,
                }
            }),
        );
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

/// 将“仅本对话”的供应商/模型/知识库开关合并到内存配置，不写回全局 config.toml。
pub(crate) fn apply_thread_chat_overrides(
    config: &mut crate::config_system::ConfigToml,
    provider: Option<&ThreadChatProviderOverride>,
    smartbrain_enabled: Option<bool>,
    subagent_enabled: Option<bool>,
) {
    if let Some(provider) = provider {
        let provider_key = provider.provider_key.as_str().trim().to_string();
        if !provider_key.is_empty() {
            config.model_provider = Some(provider_key.clone());

            let mut info = config
                .model_providers
                .get(&provider_key)
                .cloned()
                .unwrap_or_default();

            if let Some(base_url) = provider
                .base_url
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                info.base_url = Some(base_url.to_string());
            }
            if let Some(api_key) = provider
                .api_key
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                info.experimental_bearer_token = Some(api_key.to_string());
            }
            if let Some(wire_api) = provider
                .wire_api
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                info.wire_api = Some(wire_api.to_string());
            }
            if let Some(requires_openai_auth) = provider.requires_openai_auth {
                info.requires_openai_auth = Some(requires_openai_auth);
            }

            config.model_providers.insert(provider_key, info);
        }

        if let Some(model_id) = provider
            .model_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            config.model = Some(model_id.to_string());
        }
        config.model_reasoning_effort = provider
            .reasoning_effort
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);

        info!(
            "[thread_chat_override] provider={:?}, model={:?}, reasoning_effort={:?}, base_url={:?}, wire_api={:?}, endpoints={}",
            config.model_provider,
            config.model,
            config.model_reasoning_effort,
            provider.base_url.as_deref(),
            provider.wire_api.as_deref(),
            provider
                .model_endpoints
                .as_ref()
                .map(|items| items.len())
                .unwrap_or(0)
        );
        if let Some(context_window) = provider.model_context_window.filter(|value| *value > 0) {
            config.model_context_window = Some(context_window);
        }
        if let Some(max_output_tokens) = provider.max_output_tokens.filter(|value| *value > 0) {
            config.max_output_tokens = Some(max_output_tokens);
        }
        if let Some(model_supports_vision) = provider.model_supports_vision {
            config.model_supports_vision = Some(model_supports_vision);
        }
        if let Some(kind) = provider.vision_fallback_kind.as_ref() {
            config.vision_fallback_kind = if kind.trim().is_empty() {
                None
            } else {
                Some(kind.clone())
            };
        }
        if let Some(fallback_provider) = provider.vision_fallback_provider.as_ref() {
            config.vision_fallback_provider = if fallback_provider.trim().is_empty() {
                None
            } else {
                Some(fallback_provider.clone())
            };
        }
        if let Some(fallback_model) = provider.vision_fallback_model.as_ref() {
            config.vision_fallback_model = if fallback_model.trim().is_empty() {
                None
            } else {
                Some(fallback_model.clone())
            };
        }
        if let Some(model_endpoints) = provider.model_endpoints.as_ref() {
            config.model_endpoints = model_endpoints
                .iter()
                .filter(|endpoint| !endpoint.url.trim().is_empty())
                .map(|endpoint| crate::config_system::ModelEndpointInfo {
                    url: endpoint.url.trim().to_string(),
                    label: endpoint
                        .label
                        .as_deref()
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .map(str::to_string),
                    model: endpoint
                        .model
                        .as_deref()
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .map(str::to_string),
                    api_key: endpoint
                        .api_key
                        .as_deref()
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .map(str::to_string),
                    wire_api: endpoint
                        .wire_api
                        .as_deref()
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .map(str::to_string),
                })
                .collect();
            config.active_endpoint_index = if config.model_endpoints.is_empty() {
                None
            } else {
                provider
                    .active_endpoint_index
                    .or(Some(0))
                    .map(|index| index.min(config.model_endpoints.len().saturating_sub(1)))
            };
        } else if provider.provider_key.trim().is_empty() == false {
            // 非资源池供应商：清空全局遗留的 model_endpoints，避免继续打到旧端点。
            config.model_endpoints.clear();
            config.active_endpoint_index = None;
        }
    }
    if let Some(enabled) = smartbrain_enabled {
        let mut smartbrain = config.smartbrain_config();
        smartbrain.enabled = enabled;
        // 对话级开关完整控制本轮知识能力，不受全局 knowledge_enabled 残留值拦截。
        smartbrain.knowledge_enabled = enabled;
        config.smartbrain = Some(smartbrain);
    }
    if let Some(enabled) = subagent_enabled {
        config.subagent_enabled = Some(enabled);
    }
}

#[tauri::command]
pub async fn standalone_turn_interrupt(
    state: State<'_, AppState>,
    thread_id: Option<String>,
) -> AppResult<serde_json::Value> {
    let current_thread_id = state.current_thread_id.read().await.clone();
    let target_thread_id = thread_id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(|id| id.to_string())
        .or(current_thread_id.clone());

    info!(
        "Turn interrupt requested by user: thread={:?}",
        target_thread_id
    );

    if let Some(ref id) = target_thread_id {
        state.agent_engine.interrupt_thread(id);
    } else {
        // 无目标会话时保持旧行为：中断全部，避免丢停止请求。
        state.agent_engine.interrupt();
    }
    let interrupted_tools = state
        .agent_engine
        .interrupt_active_tools(target_thread_id.as_deref())
        .await;
    info!(
        "Turn interrupt completed: thread={:?}, interrupted_tools={interrupted_tools}",
        target_thread_id
    );
    Ok(serde_json::json!({ "status": "interrupted" }))
}

#[tauri::command]
pub async fn standalone_subagent_close(
    app_handle: AppHandle,
    state: State<'_, AppState>,
    thread_id: String,
    target: String,
) -> AppResult<serde_json::Value> {
    let thread_id = thread_id.trim().to_string();
    let target = target.trim().to_string();
    if thread_id.is_empty() {
        return Err(AppError::Custom("thread_id must not be empty".to_string()));
    }
    if target.is_empty() {
        return Err(AppError::Custom("target must not be empty".to_string()));
    }

    info!("Subagent close requested: thread={thread_id}, target={target}");
    state
        .agent_engine
        .close_subagent(&app_handle, &thread_id, &target)
        .await
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

    let adapter = adapter::get_adapter(&wire_api);
    let url = adapter.build_url(&base_url, &model);
    let headers = adapter.build_headers(&api_key);
    let (url, headers) =
        adapter::apply_request_overrides(url, headers, None, None).map_err(AppError::Custom)?;
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
            content: text_content(prompt.clone()),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        },
    ];
    let mut body = adapter::build_non_stream_body(&*adapter, &model, &messages, None, Some(4096));
    if let Some(obj) = body.as_object_mut() {
        if wire_api == "chat" {
            obj.insert(
                "response_format".to_string(),
                serde_json::json!({ "type": "json_object" }),
            );
        }
    }

    let mut response = http
        .post(&url)
        .headers(headers.clone())
        .json(&body)
        .send()
        .await
        .map_err(|e| AppError::Custom(format!("LLM request failed: {e}")))?;

    let mut used_json_mode = wire_api == "chat";
    if !response.status().is_success() {
        let status = response.status();
        let body_text = response.text().await.unwrap_or_default();
        if wire_api == "chat" && looks_like_json_mode_unsupported(&body_text) {
            info!("[fortune_llm_call] response_format unsupported, retrying without json mode");
            used_json_mode = false;
            let fallback_body =
                adapter::build_non_stream_body(&*adapter, &model, &messages, None, Some(4096));
            response = http
                .post(&url)
                .headers(headers)
                .json(&fallback_body)
                .send()
                .await
                .map_err(|e| AppError::Custom(format!("LLM fallback request failed: {e}")))?;
            if !response.status().is_success() {
                let fallback_status = response.status();
                let fallback_text = response.text().await.unwrap_or_default();
                return Err(AppError::Custom(format!(
                    "LLM API error {fallback_status}: {fallback_text}"
                )));
            }
        } else {
            return Err(AppError::Custom(format!(
                "LLM API error {status}: {body_text}"
            )));
        }
    }

    let raw_body = response
        .text()
        .await
        .map_err(|e| AppError::Custom(format!("Failed to read LLM response body: {e}")))?;
    let content = extract_non_streaming_fortune_text(&raw_body)?;
    if content.trim().is_empty() {
        return Err(AppError::Custom(
            "LLM returned empty assistant content".to_string(),
        ));
    }
    info!(
        "[fortune_llm_call] response len={}, json_mode={used_json_mode}",
        content.len(),
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

/// 测试模型连接：发送一个极短的 completion 请求，返回是否成功、延迟和 tokens/s 速度。
/// 支持 OpenAI-compatible (chat) 和 Anthropic 两种协议。
#[tauri::command]
pub async fn test_model_connection(
    base_url: String,
    api_key: String,
    model: String,
    wire_api: String,
) -> AppResult<serde_json::Value> {
    info!("[test_model] base_url={base_url}, model={model}, wire_api={wire_api}");

    let http = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(15))
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| AppError::Custom(format!("HTTP client error: {e}")))?;

    let start = std::time::Instant::now();
    let adapter = adapter::get_adapter(&wire_api);
    let url = adapter.build_url(&base_url, &model);
    let headers = adapter.build_headers(&api_key);
    let (url, headers) =
        adapter::apply_request_overrides(url, headers, None, None).map_err(AppError::Custom)?;
    let messages = vec![InternalMessage {
        role: "user".to_string(),
        content: text_content("Say hi"),
        tool_calls: None,
        tool_call_id: None,
        name: None,
    }];
    let body = adapter::build_non_stream_body(&*adapter, &model, &messages, None, Some(20));

    let resp = http
        .post(&url)
        .headers(headers)
        .json(&body)
        .send()
        .await
        .map_err(|e| AppError::Custom(format!("Request failed: {e}")))?;

    let elapsed_ms = start.elapsed().as_millis() as u64;
    let status_code = resp.status().as_u16();
    if !resp.status().is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Ok(serde_json::json!({
            "success": false,
            "statusCode": status_code,
            "error": body,
            "latencyMs": elapsed_ms,
        }));
    }

    let raw_body = resp.text().await.unwrap_or_default();
    let data: serde_json::Value = serde_json::from_str(&raw_body).unwrap_or_default();
    let output_tokens = [
        "/usage/completion_tokens",
        "/usage/output_tokens",
        "/response/usage/output_tokens",
        "/usageMetadata/candidatesTokenCount",
    ]
    .iter()
    .find_map(|ptr| data.pointer(ptr).and_then(|v| v.as_u64()))
    .unwrap_or(0);
    let tokens_per_sec = if elapsed_ms > 0 && output_tokens > 0 {
        (output_tokens as f64) / (elapsed_ms as f64 / 1000.0)
    } else {
        0.0
    };

    Ok(serde_json::json!({
        "success": true,
        "statusCode": status_code,
        "latencyMs": elapsed_ms,
        "outputTokens": output_tokens,
        "tokensPerSec": (tokens_per_sec * 10.0).round() / 10.0,
    }))
}

#[tauri::command]
pub async fn fetch_provider_models(
    base_url: String,
    api_key: String,
    wire_api: String,
) -> AppResult<FetchProviderModelsResult> {
    info!("[fetch_provider_models] base_url={base_url}, wire_api={wire_api}");

    let http = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(15))
        .timeout(std::time::Duration::from_secs(45))
        .build()
        .map_err(|e| AppError::Custom(format!("HTTP client error: {e}")))?;

    let adapter = adapter::get_adapter(&wire_api);
    let url = build_models_url(&base_url, &wire_api);
    let headers = adapter.build_headers(&api_key);
    let (url, headers) =
        adapter::apply_request_overrides(url, headers, None, None).map_err(AppError::Custom)?;

    let resp = http
        .get(&url)
        .headers(headers)
        .send()
        .await
        .map_err(|e| AppError::Custom(format!("Request failed: {e}")))?;

    let status = resp.status();
    let body = resp
        .text()
        .await
        .map_err(|e| AppError::Custom(format!("Failed to read response body: {e}")))?;

    if !status.is_success() {
        if looks_like_models_unsupported(status, &body) {
            return Ok(FetchProviderModelsResult {
                supported: false,
                models: Vec::new(),
            });
        }
        return Err(AppError::Custom(format!(
            "Models API error {status}: {body}"
        )));
    }

    let parsed: Value = serde_json::from_str(&body)
        .map_err(|e| AppError::Custom(format!("Invalid models response: {e}")))?;
    let models = parse_remote_models_response(&parsed);
    Ok(FetchProviderModelsResult {
        supported: true,
        models,
    })
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

    use super::{
        build_models_url, capability_fingerprint, looks_like_models_unsupported,
        parse_remote_models_response, playwright_mcp_config_value, probe_tool_count,
        resolve_robot_id_for_run_turn,
    };

    #[test]
    fn apply_thread_chat_overrides_drives_knowledge_enabled_with_dialog_switch() {
        let mut config = crate::config_system::ConfigToml::default();
        let mut smartbrain = config.smartbrain_config();
        smartbrain.enabled = false;
        smartbrain.knowledge_enabled = false;
        config.smartbrain = Some(smartbrain);

        super::apply_thread_chat_overrides(&mut config, None, Some(true), None);
        let active = config.smartbrain_config();
        assert!(active.enabled);
        assert!(active.knowledge_enabled);
        assert!(active.knowledge_is_active());

        super::apply_thread_chat_overrides(&mut config, None, Some(false), None);
        let inactive = config.smartbrain_config();
        assert!(!inactive.enabled);
        assert!(!inactive.knowledge_enabled);
        assert!(!inactive.knowledge_is_active());
    }

    #[test]
    fn apply_thread_chat_overrides_sets_subagent_enabled() {
        let mut config = crate::config_system::ConfigToml::default();
        assert!(!config.subagent_enabled());

        super::apply_thread_chat_overrides(&mut config, None, None, Some(true));
        assert!(config.subagent_enabled());

        super::apply_thread_chat_overrides(&mut config, None, None, Some(false));
        assert!(!config.subagent_enabled());
    }

    #[test]
    fn apply_thread_chat_overrides_snapshots_reasoning_effort() {
        let mut config = crate::config_system::ConfigToml {
            model_reasoning_effort: Some("low".to_string()),
            ..Default::default()
        };
        let high: super::ThreadChatProviderOverride = serde_json::from_value(serde_json::json!({
            "providerKey": "deepseek",
            "modelId": "deepseek-reasoner",
            "reasoningEffort": "high"
        }))
        .expect("deserialize provider override");

        super::apply_thread_chat_overrides(&mut config, Some(&high), None, None);
        assert_eq!(config.model_reasoning_effort.as_deref(), Some("high"));

        let cleared: super::ThreadChatProviderOverride = serde_json::from_value(serde_json::json!({
            "providerKey": "deepseek",
            "modelId": "deepseek-chat",
            "reasoningEffort": null
        }))
        .expect("deserialize provider override");
        super::apply_thread_chat_overrides(&mut config, Some(&cleared), None, None);
        assert_eq!(config.model_reasoning_effort, None);
    }

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
        assert_eq!(
            value.get("command").and_then(|v| v.as_str()),
            Some(super::PLAYWRIGHT_MCP_COMMAND)
        );
        assert_eq!(value.get("disabled").and_then(|v| v.as_bool()), Some(false));
        assert_eq!(
            value.get("args").and_then(|v| v.as_array()),
            Some(&vec![json!("-y"), json!("@playwright/mcp@latest")])
        );
    }

    #[test]
    fn build_models_url_normalizes_known_endpoints() {
        assert_eq!(
            build_models_url("https://api.openai.com/v1", "chat"),
            "https://api.openai.com/v1/models"
        );
        assert_eq!(
            build_models_url("https://api.openai.com/v1/chat/completions", "chat"),
            "https://api.openai.com/v1/models"
        );
        assert_eq!(
            build_models_url("https://api.openai.com/v1/responses", "responses"),
            "https://api.openai.com/v1/models"
        );
        assert_eq!(
            build_models_url("https://api.anthropic.com/v1/messages", "anthropic"),
            "https://api.anthropic.com/v1/models"
        );
        assert_eq!(
            build_models_url("https://generativelanguage.googleapis.com/v1beta", "gemini"),
            "https://generativelanguage.googleapis.com/v1beta/models"
        );
    }

    #[test]
    fn parse_remote_models_response_handles_openai_like_payloads() {
        let payload = json!({
            "data": [
                {
                    "id": "gpt-4.1",
                    "display_name": "GPT-4.1",
                    "context_length": 1048576,
                    "capabilities": {
                        "input_modalities": ["text", "image"]
                    }
                },
                {
                    "id": "gpt-4.1"
                },
                {
                    "id": "gpt-4.1-mini",
                    "label": "GPT-4.1 mini",
                    "supports_vision": false,
                    "max_output_tokens": 32768
                }
            ]
        });

        let models = parse_remote_models_response(&payload);
        assert_eq!(models.len(), 2);
        assert_eq!(models[0].id, "gpt-4.1");
        assert_eq!(models[0].label, "GPT-4.1");
        assert!(models[0].supports_vision);
        assert_eq!(models[0].context_length, Some(1_048_576));
        assert_eq!(models[1].id, "gpt-4.1-mini");
        assert_eq!(models[1].max_output_tokens, Some(32_768));
    }

    #[test]
    fn parse_remote_models_response_handles_gemini_payloads() {
        let payload = json!({
            "models": [
                {
                    "name": "models/gemini-2.5-pro",
                    "displayName": "Gemini 2.5 Pro",
                    "inputTokenLimit": 1048576,
                    "outputTokenLimit": 65536
                }
            ]
        });

        let models = parse_remote_models_response(&payload);
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].id, "gemini-2.5-pro");
        assert_eq!(models[0].label, "Gemini 2.5 Pro");
        assert_eq!(models[0].context_length, Some(1_048_576));
        assert_eq!(models[0].max_output_tokens, Some(65_536));
        assert!(!models[0].supports_vision);
    }

    #[test]
    fn looks_like_models_unsupported_detects_common_failures() {
        assert!(looks_like_models_unsupported(
            reqwest::StatusCode::NOT_FOUND,
            "{\"error\":\"not found\"}"
        ));
        assert!(looks_like_models_unsupported(
            reqwest::StatusCode::BAD_REQUEST,
            "provider does not support /models"
        ));
        assert!(!looks_like_models_unsupported(
            reqwest::StatusCode::UNAUTHORIZED,
            "invalid api key"
        ));
    }

    #[test]
    fn capability_probe_fingerprint_is_stable_and_protocol_specific() {
        assert_eq!(
            capability_fingerprint("provider-a", "https://example.test/v1/", "gpt-5", "chat"),
            capability_fingerprint("provider-a", "https://example.test/v1", "gpt-5", "chat")
        );
        assert_ne!(
            capability_fingerprint("provider-a", "https://example.test/v1", "gpt-5", "chat"),
            capability_fingerprint(
                "provider-a",
                "https://example.test/v1",
                "gpt-5",
                "responses"
            )
        );
        assert_ne!(
            capability_fingerprint("provider-a", "https://example.test/v1", "gpt-5", "chat"),
            capability_fingerprint("provider-b", "https://example.test/v1", "gpt-5", "chat")
        );
    }

    #[test]
    fn capability_probe_counts_structured_response_tools() {
        let chat = json!({
            "choices": [{ "message": { "tool_calls": [{ "id": "a" }, { "id": "b" }] } }]
        });
        assert_eq!(probe_tool_count(&chat, "chat"), 2);

        let responses = json!({
            "output": [
                { "type": "function_call", "call_id": "a" },
                { "type": "message", "content": [] }
            ]
        });
        assert_eq!(probe_tool_count(&responses, "responses"), 1);
    }
}
