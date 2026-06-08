//! OpenAI Responses API adapter (wire_api = "responses")
//! Codex 的原生协议，PackyCode/AIGoCode/Cubence 等中转站支持此格式。
//! 与 Chat Completions 的核心差异：
//! - URL: {base_url}/responses
//! - Body: { model, input: [...], stream: true }
//! - SSE events: response.output_text.delta, response.completed 等

use async_trait::async_trait;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};

use super::types::{InternalMessage, StreamEvent, UsageInfo};
use super::ProviderAdapter;

pub struct ResponsesAdapter;

#[async_trait]
impl ProviderAdapter for ResponsesAdapter {
    fn build_url(&self, base_url: &str, _model: &str) -> String {
        let base = base_url.trim_end_matches('/');
        if base.ends_with("/responses") {
            return base.to_string();
        }
        // 如果是 chat/completions 结尾，替换
        if base.ends_with("/chat/completions") {
            let prefix = &base[..base.len() - "/chat/completions".len()];
            return format!("{prefix}/responses");
        }
        format!("{base}/responses")
    }

    fn build_headers(&self, api_key: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        if !api_key.is_empty() {
            if let Ok(val) = HeaderValue::from_str(&format!("Bearer {api_key}")) {
                headers.insert(AUTHORIZATION, val);
            }
        }
        headers
    }

    fn build_body(
        &self,
        model: &str,
        messages: &[InternalMessage],
        tools: Option<&[serde_json::Value]>,
    ) -> serde_json::Value {
        // Responses API 使用 input 数组代替 messages
        let input: Vec<serde_json::Value> = messages
            .iter()
            .map(|msg| {
                if msg.role == "system" {
                    // system message 作为 developer 类型
                    serde_json::json!({
                        "role": "developer",
                        "content": msg.content.clone().unwrap_or_default()
                    })
                } else if msg.role == "tool" {
                    // tool result
                    serde_json::json!({
                        "type": "function_call_output",
                        "call_id": msg.tool_call_id.clone().unwrap_or_default(),
                        "output": msg.content.clone().unwrap_or_default()
                    })
                } else if msg.role == "assistant" && msg.tool_calls.is_some() {
                    // assistant with tool calls → function_call items
                    let tcs = msg.tool_calls.as_ref().unwrap();
                    let items: Vec<serde_json::Value> = tcs
                        .iter()
                        .map(|tc| {
                            serde_json::json!({
                                "type": "function_call",
                                "call_id": tc.id,
                                "name": tc.function.name,
                                "arguments": tc.function.arguments
                            })
                        })
                        .collect();
                    // 如果有前置文本，也包含
                    let mut parts = Vec::new();
                    if let Some(ref content) = msg.content {
                        if !content.is_empty() {
                            parts.push(serde_json::json!({
                                "type": "output_text",
                                "text": content
                            }));
                        }
                    }
                    parts.extend(items);
                    serde_json::json!({
                        "role": "assistant",
                        "content": parts
                    })
                } else {
                    // user / assistant (text only)
                    serde_json::json!({
                        "role": msg.role,
                        "content": msg.content.clone().unwrap_or_default()
                    })
                }
            })
            .collect();

        let mut body = serde_json::json!({
            "model": model,
            "input": input,
            "stream": true,
        });

        // tools 转为 Responses API 格式
        if let Some(tools) = tools {
            if !tools.is_empty() {
                let resp_tools: Vec<serde_json::Value> = tools
                    .iter()
                    .filter_map(|t| {
                        let func = t.get("function")?;
                        Some(serde_json::json!({
                            "type": "function",
                            "name": func.get("name")?,
                            "description": func.get("description").and_then(|d| d.as_str()).unwrap_or(""),
                            "parameters": func.get("parameters").cloned().unwrap_or(serde_json::json!({}))
                        }))
                    })
                    .collect();
                body["tools"] = serde_json::Value::Array(resp_tools);
            }
        }

        body
    }

    fn is_stream_done(&self, line: &str) -> bool {
        let trimmed = line.trim();
        // Responses API 使用 event: response.completed 或 data: [DONE]
        trimmed == "data: [DONE]"
            || trimmed.contains("\"type\":\"response.completed\"")
    }

    fn parse_stream_line(&self, line: &str) -> Vec<StreamEvent> {
        let mut events = Vec::new();

        let data = match line.strip_prefix("data: ") {
            Some(d) => d.trim(),
            None => return events,
        };

        if data == "[DONE]" {
            events.push(StreamEvent::Done { finish_reason: Some("stop".to_string()) });
            return events;
        }

        let parsed: serde_json::Value = match serde_json::from_str(data) {
            Ok(v) => v,
            Err(_) => return events,
        };

        let event_type = parsed.get("type").and_then(|v| v.as_str()).unwrap_or("");

        match event_type {
            // 文本增量
            "response.output_text.delta" => {
                if let Some(delta) = parsed.get("delta").and_then(|v| v.as_str()) {
                    if !delta.is_empty() {
                        events.push(StreamEvent::TextDelta(delta.to_string()));
                    }
                }
            }
            // 函数调用参数增量
            "response.function_call_arguments.delta" => {
                let call_id = parsed.get("call_id").and_then(|v| v.as_str()).unwrap_or("");
                let delta = parsed.get("delta").and_then(|v| v.as_str()).unwrap_or("");
                // index 从 item_id 推断（简化处理，用 0）
                let idx = parsed.get("output_index").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                let name = parsed.get("name").and_then(|v| v.as_str()).map(|s| s.to_string());
                events.push(StreamEvent::ToolCallDelta {
                    index: idx,
                    id: if call_id.is_empty() { None } else { Some(call_id.to_string()) },
                    name,
                    arguments: if delta.is_empty() { None } else { Some(delta.to_string()) },
                });
            }
            // 函数调用开始（包含 name 和 call_id）
            "response.output_item.added" => {
                if let Some(item) = parsed.get("item") {
                    let item_type = item.get("type").and_then(|v| v.as_str()).unwrap_or("");
                    if item_type == "function_call" {
                        let call_id = item.get("call_id").and_then(|v| v.as_str()).unwrap_or("");
                        let name = item.get("name").and_then(|v| v.as_str()).unwrap_or("");
                        let idx = parsed.get("output_index").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                        events.push(StreamEvent::ToolCallDelta {
                            index: idx,
                            id: Some(call_id.to_string()),
                            name: Some(name.to_string()),
                            arguments: None,
                        });
                    }
                }
            }
            // 流结束
            "response.completed" => {
                // 提取 usage
                if let Some(response) = parsed.get("response") {
                    if let Some(usage) = response.get("usage") {
                        let prompt = usage.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
                        let completion = usage.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
                        events.push(StreamEvent::Usage(UsageInfo {
                            prompt_tokens: prompt,
                            completion_tokens: completion,
                            total_tokens: prompt + completion,
                        }));
                    }
                }
                events.push(StreamEvent::Done {
                    finish_reason: Some("stop".to_string()),
                });
            }
            "response.output_text.done" | "response.output_item.done" => {
                // 中间完成事件，不需要特殊处理
            }
            _ => {}
        }

        events
    }
}
