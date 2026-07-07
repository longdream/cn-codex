//! Anthropic Messages API adapter (wire_api = "anthropic")
//! 支持 Claude 系列模型的原生 API 格式。
//! 与 OpenAI 的核心差异：
//! - 鉴权: x-api-key header
//! - URL: {base_url}/messages
//! - Body: { model, messages, max_tokens, stream: true }
//! - SSE events: content_block_delta, message_stop 等

use async_trait::async_trait;
use reqwest::header::{CONTENT_TYPE, HeaderMap, HeaderValue};

use super::ProviderAdapter;
use super::types::{
    InternalMessage, StreamEvent, UsageInfo, content_as_text, content_to_anthropic_blocks,
};

/// Anthropic API 版本号
const ANTHROPIC_VERSION: &str = "2023-06-01";

pub struct AnthropicAdapter;

#[async_trait]
impl ProviderAdapter for AnthropicAdapter {
    fn build_url(&self, base_url: &str, _model: &str) -> String {
        let base = base_url.trim_end_matches('/');
        if base.ends_with("/messages") {
            return base.to_string();
        }
        format!("{base}/messages")
    }

    fn build_headers(&self, api_key: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert(
            "anthropic-version",
            HeaderValue::from_static(ANTHROPIC_VERSION),
        );
        if !api_key.is_empty() {
            if let Ok(val) = HeaderValue::from_str(api_key) {
                headers.insert("x-api-key", val);
            }
        }
        headers
    }

    fn build_body(
        &self,
        model: &str,
        messages: &[InternalMessage],
        tools: Option<&[serde_json::Value]>,
        max_tokens: Option<i64>,
    ) -> serde_json::Value {
        // Anthropic 的 system message 需要提取为顶层 system 字段
        let mut system_text = String::new();
        let mut api_messages: Vec<serde_json::Value> = Vec::new();

        for msg in messages {
            match msg.role.as_str() {
                "system" => {
                    let content = content_as_text(&msg.content);
                    if !content.is_empty() {
                        if !system_text.is_empty() {
                            system_text.push('\n');
                        }
                        system_text.push_str(&content);
                    }
                }
                "assistant" => {
                    let mut content_blocks: Vec<serde_json::Value> = Vec::new();

                    // 文本部分
                    content_blocks.extend(content_to_anthropic_blocks(&msg.content));

                    // tool_use 部分
                    if let Some(ref tcs) = msg.tool_calls {
                        for tc in tcs {
                            let args: serde_json::Value =
                                serde_json::from_str(&tc.function.arguments)
                                    .unwrap_or(serde_json::json!({}));
                            content_blocks.push(serde_json::json!({
                                "type": "tool_use",
                                "id": tc.id,
                                "name": tc.function.name,
                                "input": args
                            }));
                        }
                    }

                    if !content_blocks.is_empty() {
                        api_messages.push(serde_json::json!({
                            "role": "assistant",
                            "content": content_blocks
                        }));
                    }
                }
                "tool" => {
                    // tool result → user message with tool_result content block
                    api_messages.push(serde_json::json!({
                        "role": "user",
                        "content": [{
                            "type": "tool_result",
                            "tool_use_id": msg.tool_call_id.clone().unwrap_or_default(),
                            "content": content_as_text(&msg.content)
                        }]
                    }));
                }
                "user" => {
                    api_messages.push(serde_json::json!({
                        "role": "user",
                        "content": content_to_anthropic_blocks(&msg.content)
                    }));
                }
                _ => {}
            }
        }

        let mut body = serde_json::json!({
            "model": model,
            "messages": api_messages,
            "max_tokens": max_tokens.unwrap_or(131072),
            "stream": true,
        });

        if !system_text.is_empty() {
            body["system"] = serde_json::Value::String(system_text);
        }

        // tools 转为 Anthropic 格式
        if let Some(tools) = tools {
            if !tools.is_empty() {
                let anthropic_tools: Vec<serde_json::Value> = tools
                    .iter()
                    .filter_map(|t| {
                        let func = t.get("function")?;
                        Some(serde_json::json!({
                            "name": func.get("name")?,
                            "description": func.get("description").and_then(|d| d.as_str()).unwrap_or(""),
                            "input_schema": func.get("parameters").cloned().unwrap_or(serde_json::json!({"type": "object"}))
                        }))
                    })
                    .collect();
                body["tools"] = serde_json::Value::Array(anthropic_tools);
            }
        }

        body
    }

    fn is_stream_done(&self, line: &str) -> bool {
        let trimmed = line.trim();
        trimmed.contains("\"type\":\"message_stop\"")
            || trimmed == "event: message_stop"
            || trimmed == "message_stop"
    }

    fn parse_stream_line(&self, line: &str) -> Vec<StreamEvent> {
        let mut events = Vec::new();

        let data = if let Some(d) = line.strip_prefix("data: ") {
            d.trim()
        } else if let Some(d) = line.strip_prefix("data:") {
            d.trim()
        } else {
            line.trim()
        };

        let parsed: serde_json::Value = match serde_json::from_str(data) {
            Ok(v) => v,
            Err(_) => return events,
        };

        let event_type = parsed.get("type").and_then(|v| v.as_str()).unwrap_or("");

        match event_type {
            // message_start 包含 usage 信息
            "message_start" => {
                if let Some(message) = parsed.get("message") {
                    if let Some(usage) = message.get("usage") {
                        let input = usage
                            .get("input_tokens")
                            .and_then(|v| v.as_u64())
                            .unwrap_or(0);
                        let cached = usage
                            .get("cache_read_input_tokens")
                            .and_then(|v| v.as_u64())
                            .unwrap_or(0);
                        let cache_creation = usage
                            .get("cache_creation_input_tokens")
                            .and_then(|v| v.as_u64())
                            .unwrap_or(0);
                        events.push(StreamEvent::Usage(UsageInfo {
                            prompt_tokens: input,
                            completion_tokens: 0,
                            total_tokens: input,
                            cached_tokens: cached,
                            cache_creation_tokens: cache_creation,
                            reasoning_tokens: 0,
                        }));
                    }
                }
            }
            // content_block_start: 新的内容块开始
            "content_block_start" => {
                if let Some(content_block) = parsed.get("content_block") {
                    let block_type = content_block
                        .get("type")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    if block_type == "tool_use" {
                        let idx =
                            parsed.get("index").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                        let id = content_block
                            .get("id")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        let name = content_block
                            .get("name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        events.push(StreamEvent::ToolCallDelta {
                            index: idx,
                            id: Some(id.to_string()),
                            name: Some(name.to_string()),
                            arguments: None,
                        });
                    }
                }
            }
            // content_block_delta: 内容增量
            "content_block_delta" => {
                if let Some(delta) = parsed.get("delta") {
                    let delta_type = delta.get("type").and_then(|v| v.as_str()).unwrap_or("");
                    match delta_type {
                        "text_delta" => {
                            if let Some(text) = delta.get("text").and_then(|v| v.as_str()) {
                                if !text.is_empty() {
                                    events.push(StreamEvent::TextDelta(text.to_string()));
                                }
                            }
                        }
                        "input_json_delta" => {
                            // tool call arguments 增量
                            if let Some(partial_json) =
                                delta.get("partial_json").and_then(|v| v.as_str())
                            {
                                let idx = parsed.get("index").and_then(|v| v.as_u64()).unwrap_or(0)
                                    as usize;
                                events.push(StreamEvent::ToolCallDelta {
                                    index: idx,
                                    id: None,
                                    name: None,
                                    arguments: Some(partial_json.to_string()),
                                });
                            }
                        }
                        _ => {}
                    }
                }
            }
            // message_delta: 包含 stop_reason 和 output tokens
            "message_delta" => {
                if let Some(delta) = parsed.get("delta") {
                    let stop_reason = delta.get("stop_reason").and_then(|v| v.as_str());
                    if let Some(reason) = stop_reason {
                        // 映射 Anthropic 的 stop_reason 到标准格式
                        let mapped_reason = match reason {
                            "end_turn" => "stop",
                            "tool_use" => "tool_calls",
                            other => other,
                        };
                        events.push(StreamEvent::Done {
                            finish_reason: Some(mapped_reason.to_string()),
                        });
                    }
                }
                // output tokens 用量
                if let Some(usage) = parsed.get("usage") {
                    let output = usage
                        .get("output_tokens")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0);
                    events.push(StreamEvent::Usage(UsageInfo {
                        prompt_tokens: 0,
                        completion_tokens: output,
                        total_tokens: output,
                        cached_tokens: 0,
                        cache_creation_tokens: 0,
                        reasoning_tokens: 0,
                    }));
                }
            }
            "message_stop" => {
                // 确保流结束
                events.push(StreamEvent::Done {
                    finish_reason: Some("stop".to_string()),
                });
            }
            _ => {}
        }

        events
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_stream_line_supports_non_prefixed_data() {
        let adapter = AnthropicAdapter;
        let events =
            adapter.parse_stream_line(r#"{"type":"content_block_delta","delta":{"text":"hi"}}"#);
        assert!(!events.is_empty());
    }
}
