//! API 格式适配层
//! 根据供应商的 wire_api 类型选择对应的 adapter 进行请求/响应格式转换。
//! 支持: chat (OpenAI Chat Completions), responses (OpenAI Responses API),
//!       anthropic (Anthropic Messages API), gemini (Google Gemini API)

pub mod anthropic;
pub mod chat_completions;
pub mod google;
pub mod responses;
pub mod types;

use async_trait::async_trait;
use reqwest::Url;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use std::collections::HashMap;
use types::{CompletionOutput, InternalMessage, StreamEvent};

/// 所有供应商 adapter 必须实现的 trait
#[async_trait]
pub trait ProviderAdapter: Send + Sync {
    /// 构建请求 URL
    fn build_url(&self, base_url: &str, model: &str) -> String;

    /// 构建请求 headers
    fn build_headers(&self, api_key: &str) -> HeaderMap;

    /// 将内部统一消息格式转换为供应商 API 请求 body
    fn build_body(
        &self,
        model: &str,
        messages: &[InternalMessage],
        tools: Option<&[serde_json::Value]>,
        max_tokens: Option<i64>,
    ) -> serde_json::Value;

    /// 判断 SSE 行是否表示流结束
    fn is_stream_done(&self, line: &str) -> bool;

    /// 解析单条 SSE data 行，返回零或多个 StreamEvent
    fn parse_stream_line(&self, line: &str) -> Vec<StreamEvent>;

    /// 解析完整的非流式 JSON 响应。
    /// 默认 adapter 继续由上层使用其兼容路径；Responses adapter 提供完整 output[] 解析。
    fn parse_non_streaming(&self, _body: &str) -> Result<CompletionOutput, String> {
        Err("non-streaming JSON parsing is not implemented for this wire API".to_string())
    }
}

/// 根据 wire_api 字符串选择对应的 adapter 实例
pub fn get_adapter(wire_api: &str) -> Box<dyn ProviderAdapter> {
    match wire_api {
        "responses" => Box::new(responses::ResponsesAdapter),
        "anthropic" => Box::new(anthropic::AnthropicAdapter),
        "gemini" => Box::new(google::GoogleAdapter),
        // 默认使用 OpenAI Chat Completions
        _ => Box::new(chat_completions::ChatCompletionsAdapter),
    }
}

/// 构建"非流式"请求 body。
///
/// 部分 adapter 的 `build_body` 会无条件写入 `stream: true` 与
/// `stream_options`（如 OpenAI Chat Completions 的 `include_usage`）。
/// 当以非流式方式（`stream: false`）调用时，某些严格校验的网关/API
/// （如 NVIDIA `integrate.api.nvidia.com`）会拒绝同时出现 `stream_options`，
/// 返回 400：`The 'stream_options' field is only allowed when 'stream' is set to true.`
///
/// 因此这里集中处理：在 `build_body` 基础上把 `stream` 设为 `false`，
/// 并移除 `stream_options`。对不写该字段的 adapter（anthropic/responses/gemini）
/// 而言 `remove` 是 no-op，安全无副作用。
pub fn build_non_stream_body(
    adapter: &dyn ProviderAdapter,
    model: &str,
    messages: &[InternalMessage],
    tools: Option<&[serde_json::Value]>,
    max_tokens: Option<i64>,
) -> serde_json::Value {
    let mut body = adapter.build_body(model, messages, tools, max_tokens);
    if let Some(obj) = body.as_object_mut() {
        obj.insert("stream".to_string(), serde_json::Value::Bool(false));
        obj.remove("stream_options");
    }
    body
}

/// 规范化推理强度配置值。
///
/// 返回 `None` 表示不向供应商发送推理强度相关字段（关闭/空）。
pub fn normalize_reasoning_effort(raw: Option<&str>) -> Option<String> {
    let trimmed = raw?.trim();
    if trimmed.is_empty() {
        return None;
    }
    match trimmed.to_ascii_lowercase().as_str() {
        "none" | "off" | "disable" | "disabled" | "false" | "0" => None,
        "minimal" | "min" => Some("minimal".to_string()),
        "low" => Some("low".to_string()),
        "medium" | "med" | "default" | "normal" => Some("medium".to_string()),
        "high" => Some("high".to_string()),
        "xhigh" | "x-high" | "extra_high" | "extra-high" | "max" | "highest" => {
            Some("xhigh".to_string())
        }
        other => Some(other.to_string()),
    }
}

fn reasoning_budget_tokens(effort: &str) -> i64 {
    match effort {
        "minimal" => 1_024,
        "low" => 2_048,
        "medium" => 8_192,
        "high" => 16_384,
        "xhigh" => 32_768,
        _ => 8_192,
    }
}

/// chat 协议下是否适合发送 `reasoning_effort`。
///
/// 多数普通 chat 模型不认识该字段，部分网关会直接 400；
/// 因此只对明显的推理模型/系列做 best-effort 注入。
fn chat_model_supports_reasoning_effort(model: &str) -> bool {
    let m = model.to_ascii_lowercase();
    const KEYS: &[&str] = &[
        "o1",
        "o3",
        "o4",
        "gpt-5",
        "gpt5",
        "reason",
        "r1",
        "qwq",
        "thinking",
        "deepseek-r",
        "deepseek-reasoner",
        "gemini",
        "grok-3-mini",
        "grok-4",
    ];
    KEYS.iter().any(|key| m.contains(key))
}

fn anthropic_model_supports_thinking(model: &str) -> bool {
    let m = model.to_ascii_lowercase();
    m.contains("claude")
        || m.contains("sonnet")
        || m.contains("opus")
        || m.contains("haiku")
        || m.contains("thinking")
}

/// 按 wire_api / 模型兼容性，把推理强度写入请求 body。
///
/// - responses: `reasoning.effort`
/// - chat: `reasoning_effort`（仅推理类模型）
/// - anthropic: `thinking.budget_tokens`
/// - gemini: `generationConfig.thinkingConfig.thinkingBudget`
pub fn apply_reasoning_effort_to_body(
    body: &mut serde_json::Value,
    wire_api: &str,
    model: &str,
    effort_raw: Option<&str>,
) {
    let Some(effort) = normalize_reasoning_effort(effort_raw) else {
        return;
    };
    let Some(obj) = body.as_object_mut() else {
        return;
    };
    let wire = wire_api.trim().to_ascii_lowercase();

    match wire.as_str() {
        "responses" => {
            obj.insert(
                "reasoning".to_string(),
                serde_json::json!({ "effort": effort }),
            );
        }
        "anthropic" => {
            if !anthropic_model_supports_thinking(model) {
                return;
            }
            obj.insert(
                "thinking".to_string(),
                serde_json::json!({
                    "type": "enabled",
                    "budget_tokens": reasoning_budget_tokens(&effort),
                }),
            );
        }
        "gemini" => {
            let budget = reasoning_budget_tokens(&effort);
            let generation = obj
                .entry("generationConfig".to_string())
                .or_insert_with(|| serde_json::json!({}));
            if let Some(gen_obj) = generation.as_object_mut() {
                gen_obj.insert(
                    "thinkingConfig".to_string(),
                    serde_json::json!({ "thinkingBudget": budget }),
                );
            }
        }
        // chat / OpenAI-compatible default
        _ => {
            if !chat_model_supports_reasoning_effort(model) {
                return;
            }
            obj.insert(
                "reasoning_effort".to_string(),
                serde_json::Value::String(effort),
            );
        }
    }
}

/// 合并 provider 级别的 query/header 覆盖项（用于网关白名单、反抓取 header 等场景）
pub fn apply_request_overrides(
    url: String,
    mut headers: HeaderMap,
    query_params: Option<&HashMap<String, String>>,
    extra_headers: Option<&HashMap<String, String>>,
) -> Result<(String, HeaderMap), String> {
    let mut parsed_url =
        Url::parse(&url).map_err(|e| format!("Invalid request URL '{url}': {e}"))?;
    if let Some(query_params) = query_params {
        let mut qp = parsed_url.query_pairs_mut();
        for (key, value) in query_params {
            if key.trim().is_empty() {
                continue;
            }
            qp.append_pair(key, value);
        }
    }

    if let Some(extra_headers) = extra_headers {
        for (key, value) in extra_headers {
            if key.trim().is_empty() {
                continue;
            }
            let name = HeaderName::from_bytes(key.as_bytes())
                .map_err(|e| format!("Invalid header name '{key}': {e}"))?;
            let header_value = HeaderValue::from_str(value)
                .map_err(|e| format!("Invalid header value for '{key}': {e}"))?;
            headers.insert(name, header_value);
        }
    }

    Ok((parsed_url.to_string(), headers))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_request_overrides_merges_query_and_headers() {
        let mut query = HashMap::new();
        query.insert("api-version".to_string(), "2024-01-01".to_string());
        let mut extra_headers = HashMap::new();
        extra_headers.insert("x-test-header".to_string(), "yes".to_string());

        let headers = HeaderMap::new();
        let (url, headers) = apply_request_overrides(
            "https://example.com/v1/chat/completions".to_string(),
            headers,
            Some(&query),
            Some(&extra_headers),
        )
        .unwrap();

        assert!(url.contains("api-version=2024-01-01"));
        assert_eq!(
            headers
                .get("x-test-header")
                .and_then(|v| v.to_str().ok())
                .unwrap_or(""),
            "yes"
        );
    }

    #[test]
    fn normalize_reasoning_effort_maps_aliases() {
        assert_eq!(normalize_reasoning_effort(Some("MEDIUM")), Some("medium".into()));
        assert_eq!(normalize_reasoning_effort(Some("off")), None);
        assert_eq!(normalize_reasoning_effort(Some("x-high")), Some("xhigh".into()));
    }

    #[test]
    fn apply_reasoning_effort_by_wire_api() {
        let mut responses_body = serde_json::json!({"model":"gpt-5"});
        apply_reasoning_effort_to_body(&mut responses_body, "responses", "gpt-5", Some("high"));
        assert_eq!(
            responses_body.pointer("/reasoning/effort").and_then(|v| v.as_str()),
            Some("high")
        );

        let mut chat_body = serde_json::json!({"model":"deepseek-r1"});
        apply_reasoning_effort_to_body(&mut chat_body, "chat", "deepseek-r1", Some("low"));
        assert_eq!(
            chat_body.get("reasoning_effort").and_then(|v| v.as_str()),
            Some("low")
        );

        let mut plain_chat = serde_json::json!({"model":"gpt-4.1"});
        apply_reasoning_effort_to_body(&mut plain_chat, "chat", "gpt-4.1", Some("medium"));
        assert!(plain_chat.get("reasoning_effort").is_none());

        let mut anthropic_body = serde_json::json!({"model":"claude-sonnet-4"});
        apply_reasoning_effort_to_body(
            &mut anthropic_body,
            "anthropic",
            "claude-sonnet-4",
            Some("medium"),
        );
        assert_eq!(
            anthropic_body
                .pointer("/thinking/budget_tokens")
                .and_then(|v| v.as_i64()),
            Some(8192)
        );

        let mut gemini_body = serde_json::json!({"generationConfig":{"maxOutputTokens":1024}});
        apply_reasoning_effort_to_body(&mut gemini_body, "gemini", "gemini-2.5-pro", Some("high"));
        assert_eq!(
            gemini_body
                .pointer("/generationConfig/thinkingConfig/thinkingBudget")
                .and_then(|v| v.as_i64()),
            Some(16384)
        );
    }
}
