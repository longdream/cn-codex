//! Adapter 层统一类型定义
//! 所有 adapter 返回相同的 StreamEvent，由 agent.rs 统一处理

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Hard guardrails for a single model response. These protect every consumer
/// from malformed provider indexes and unbounded streams.
pub const MAX_TOOL_CALLS_PER_RESPONSE: usize = 64;
pub const MAX_STREAMED_RESPONSE_BYTES: usize = 8_000_000;

/// 单次请求中 LLM 返回的 token 用量信息
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UsageInfo {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
    /// 缓存命中 token 数（prompt_tokens_details.cached_tokens / cache_read_input_tokens）
    #[serde(default)]
    pub cached_tokens: u64,
    /// 缓存写入 token 数（prompt_tokens_details.cache_creation / cache_creation_input_tokens）
    #[serde(default)]
    pub cache_creation_tokens: u64,
    /// 思考（reasoning）token 数（completion_tokens_details.reasoning_tokens）
    #[serde(default)]
    pub reasoning_tokens: u64,
}

/// tool call 累积器（逐步拼接 SSE 中的碎片）
#[derive(Debug, Clone, Default)]
pub struct ToolCallAccumulator {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

/// adapter 返回给 agent 的统一流式事件
#[derive(Debug, Clone)]
pub enum StreamEvent {
    /// 文本内容增量
    TextDelta(String),

    /// Provider-native reasoning/thinking content. This must not be mixed into
    /// the visible assistant answer or persisted as normal message text.
    ReasoningDelta(String),

    /// tool call 增量（index, id 可选, name 可选, arguments 片段）
    ToolCallDelta {
        index: usize,
        id: Option<String>,
        name: Option<String>,
        arguments: Option<String>,
    },

    /// Canonical completed tool item. Consumers must replace accumulated
    /// arguments with this value instead of appending it again.
    ToolCallDone {
        index: usize,
        id: Option<String>,
        name: Option<String>,
        arguments: Option<String>,
    },

    /// Provider-declared terminal failure or incomplete response.
    Error(String),

    /// 流结束，附带 finish reason
    Done { finish_reason: Option<String> },

    /// 用量信息（通常在流结束时由最后一个 chunk 返回）
    Usage(UsageInfo),
}

/// adapter 处理完整个 stream 后返回的最终结果
#[derive(Debug)]
pub struct CompletionOutput {
    /// 完整文本内容
    pub text: String,
    /// 所有 tool call
    pub tool_calls: Vec<ToolCallResult>,
    /// finish reason
    pub finish_reason: Option<String>,
    /// token 用量（如果供应商返回了的话）
    pub usage: Option<UsageInfo>,
}

/// 最终解析出的单个 tool call
#[derive(Debug, Clone)]
pub struct ToolCallResult {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

/// 内部消息格式（从 agent.rs 的 ApiMessage 统一到此处）
#[derive(Debug, Clone, Serialize)]
pub struct InternalMessage {
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<InternalToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// 内部 tool call 表示
#[derive(Debug, Clone, Serialize)]
pub struct InternalToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub call_type: String,
    pub function: InternalFunctionCall,
}

/// 内部函数调用表示
#[derive(Debug, Clone, Serialize)]
pub struct InternalFunctionCall {
    pub name: String,
    pub arguments: String,
}

pub fn text_content(text: impl Into<String>) -> Option<Value> {
    Some(Value::String(text.into()))
}

pub fn content_as_text(content: &Option<Value>) -> String {
    match content {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(parts)) => parts
            .iter()
            .filter_map(|part| part.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
        Some(other) => other.to_string(),
        None => String::new(),
    }
}

pub fn content_is_empty(content: &Option<Value>) -> bool {
    match content {
        None => true,
        Some(Value::String(text)) => text.is_empty(),
        Some(Value::Array(parts)) => parts.is_empty(),
        Some(Value::Null) => true,
        Some(_) => false,
    }
}

pub fn content_to_responses_content(content: &Option<Value>) -> Value {
    match content {
        Some(Value::Array(parts)) => Value::Array(
            parts
                .iter()
                .filter_map(|part| {
                    let kind = part.get("type").and_then(Value::as_str)?;
                    match kind {
                        "text" => Some(serde_json::json!({
                            "type": "input_text",
                            "text": part.get("text").and_then(Value::as_str).unwrap_or_default()
                        })),
                        "image_url" => Some(serde_json::json!({
                            "type": "input_image",
                            "image_url": part
                                .get("image_url")
                                .and_then(|value| value.get("url"))
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                        })),
                        _ => None,
                    }
                })
                .collect(),
        ),
        Some(Value::String(text)) => Value::String(text.clone()),
        Some(other) => Value::String(other.to_string()),
        None => Value::String(String::new()),
    }
}

pub fn content_to_anthropic_blocks(content: &Option<Value>) -> Vec<Value> {
    match content {
        Some(Value::Array(parts)) => parts
            .iter()
            .filter_map(|part| {
                let kind = part.get("type").and_then(Value::as_str)?;
                match kind {
                    "text" => Some(serde_json::json!({
                        "type": "text",
                        "text": part.get("text").and_then(Value::as_str).unwrap_or_default()
                    })),
                    "image_url" => {
                        let url = part
                            .get("image_url")
                            .and_then(|value| value.get("url"))
                            .and_then(Value::as_str)?;
                        let (media_type, data) = parse_data_url(url)?;
                        Some(serde_json::json!({
                            "type": "image",
                            "source": {
                                "type": "base64",
                                "media_type": media_type,
                                "data": data
                            }
                        }))
                    }
                    _ => None,
                }
            })
            .collect(),
        Some(Value::String(text)) if !text.is_empty() => vec![serde_json::json!({
            "type": "text",
            "text": text
        })],
        Some(other) => vec![serde_json::json!({
            "type": "text",
            "text": other.to_string()
        })],
        None => Vec::new(),
    }
}

pub fn content_to_gemini_parts(content: &Option<Value>) -> Vec<Value> {
    match content {
        Some(Value::Array(parts)) => parts
            .iter()
            .filter_map(|part| {
                let kind = part.get("type").and_then(Value::as_str)?;
                match kind {
                    "text" => Some(serde_json::json!({
                        "text": part.get("text").and_then(Value::as_str).unwrap_or_default()
                    })),
                    "image_url" => {
                        let url = part
                            .get("image_url")
                            .and_then(|value| value.get("url"))
                            .and_then(Value::as_str)?;
                        let (mime_type, data) = parse_data_url(url)?;
                        Some(serde_json::json!({
                            "inlineData": {
                                "mimeType": mime_type,
                                "data": data
                            }
                        }))
                    }
                    _ => None,
                }
            })
            .collect(),
        Some(Value::String(text)) if !text.is_empty() => vec![serde_json::json!({ "text": text })],
        Some(other) => vec![serde_json::json!({ "text": other.to_string() })],
        None => Vec::new(),
    }
}

fn parse_data_url(url: &str) -> Option<(String, String)> {
    let rest = url.strip_prefix("data:")?;
    let (meta, data) = rest.split_once(',')?;
    let mut parts = meta.split(';');
    let mime = parts.next()?.trim();
    if mime.is_empty() || !parts.any(|part| part.eq_ignore_ascii_case("base64")) {
        return None;
    }
    Some((mime.to_string(), data.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multimodal_content_converts_to_provider_shapes() {
        let content = Some(serde_json::json!([
            { "type": "text", "text": "describe this" },
            { "type": "image_url", "image_url": { "url": "data:image/png;base64,abc123" } }
        ]));

        let responses = content_to_responses_content(&content);
        assert_eq!(responses[0]["type"], "input_text");
        assert_eq!(responses[1]["type"], "input_image");

        let anthropic = content_to_anthropic_blocks(&content);
        assert_eq!(anthropic[0]["type"], "text");
        assert_eq!(anthropic[1]["source"]["media_type"], "image/png");
        assert_eq!(anthropic[1]["source"]["data"], "abc123");

        let gemini = content_to_gemini_parts(&content);
        assert_eq!(gemini[0]["text"], "describe this");
        assert_eq!(gemini[1]["inlineData"]["mimeType"], "image/png");
        assert_eq!(gemini[1]["inlineData"]["data"], "abc123");
    }
}
