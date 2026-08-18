//! OpenAI Chat Completions API adapter (wire_api = "chat")
//! 这是大多数供应商的默认格式：DeepSeek、国产模型、各种中转站等

use async_trait::async_trait;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};
use serde::Deserialize;

use super::ProviderAdapter;
use super::types::{InternalMessage, StreamEvent, UsageInfo, safe_max_output_tokens};

pub struct ChatCompletionsAdapter;

/// SSE chunk 中的 choice 结构
#[derive(Debug, Deserialize)]
struct StreamChoice {
    delta: Option<DeltaContent>,
    /// 兼容部分 OpenAI Chat 中转站：流式帧使用完整的 message 而非 delta。
    message: Option<DeltaContent>,
    finish_reason: Option<String>,
}

/// SSE chunk 中的 delta 内容
#[derive(Debug, Deserialize)]
struct DeltaContent {
    content: Option<String>,
    #[serde(default, alias = "reasoning", alias = "reasoning_text")]
    reasoning_content: Option<String>,
    tool_calls: Option<Vec<DeltaToolCall>>,
}

/// delta 中的 tool call 碎片
#[derive(Debug, Deserialize)]
struct DeltaToolCall {
    index: Option<usize>,
    id: Option<String>,
    function: Option<DeltaFunction>,
}

/// delta 中的 function 碎片
#[derive(Debug, Deserialize)]
struct DeltaFunction {
    name: Option<String>,
    arguments: Option<String>,
}

/// SSE chunk 顶层结构
#[derive(Debug, Deserialize)]
struct StreamChunk {
    choices: Option<Vec<StreamChoice>>,
    usage: Option<ChunkUsage>,
    #[serde(default)]
    error: Option<serde_json::Value>,
}

/// usage 字段（部分供应商在最后一个 chunk 中返回）
#[derive(Debug, Deserialize, Default)]
struct TokenDetails {
    #[serde(default)]
    cached_tokens: Option<u64>,
    #[serde(default)]
    cache_creation_tokens: Option<u64>,
}

#[derive(Debug, Deserialize, Default)]
struct CompletionDetails {
    #[serde(default)]
    reasoning_tokens: Option<u64>,
}

/// usage 字段（部分供应商在最后一个 chunk 中返回）
#[derive(Debug, Deserialize)]
struct ChunkUsage {
    #[serde(alias = "input_tokens")]
    prompt_tokens: Option<u64>,
    #[serde(alias = "output_tokens")]
    completion_tokens: Option<u64>,
    total_tokens: Option<u64>,
    #[serde(default)]
    prompt_tokens_details: Option<TokenDetails>,
    #[serde(default)]
    completion_tokens_details: Option<CompletionDetails>,
}

#[async_trait]
impl ProviderAdapter for ChatCompletionsAdapter {
    fn build_url(&self, base_url: &str, _model: &str) -> String {
        let base = base_url.trim_end_matches('/');
        if base.ends_with("/chat/completions") {
            return base.to_string();
        }
        if base.ends_with("/responses") {
            let prefix = &base[..base.len() - "/responses".len()];
            return format!("{prefix}/chat/completions");
        }
        format!("{base}/chat/completions")
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
        max_tokens: Option<i64>,
    ) -> serde_json::Value {
        // Qwen / 部分国产 chat 网关要求：
        // 1) system 只能出现在 messages 开头
        // 2) 通常只接受一条 system（多条 system 会被判定为“不在开头”）
        // 因此在序列化前合并所有 system，并保证其位于最前。
        let formatted_messages = build_chat_completions_messages(messages, model);

        let mut body = serde_json::json!({
            "model": model,
            "messages": formatted_messages,
            "max_tokens": safe_max_output_tokens(max_tokens),
            "stream": true,
            "stream_options": { "include_usage": true },
        });
        if let Some(tools) = tools {
            if !tools.is_empty() {
                body["tools"] = serde_json::Value::Array(tools.to_vec());
            }
        }
        body
    }

    fn is_stream_done(&self, line: &str) -> bool {
        let trimmed = line.trim();
        trimmed == "data: [DONE]" || trimmed == "data:[DONE]"
    }

    fn parse_stream_line(&self, line: &str) -> Vec<StreamEvent> {
        let mut events = Vec::new();

        // 兼容 "data: {...}" 和 "data:{...}" 两种格式
        let data = if let Some(d) = line.strip_prefix("data: ") {
            d.trim()
        } else if let Some(d) = line.strip_prefix("data:") {
            d.trim()
        } else {
            // 某些中转站可能直接返回 JSON 对象（非标准 SSE）
            let trimmed = line.trim();
            if trimmed.starts_with('{') {
                trimmed
            } else {
                return events;
            }
        };

        if data == "[DONE]" {
            return events;
        }

        let chunk: StreamChunk = match serde_json::from_str(data) {
            Ok(c) => c,
            Err(e) => {
                tracing::debug!("Chat SSE parse skip: {e}, raw: {data}");
                return events;
            }
        };

        if let Some(error) = &chunk.error {
            let message = error
                .get("message")
                .and_then(serde_json::Value::as_str)
                .or_else(|| error.as_str())
                .unwrap_or("Unknown provider stream error");
            events.push(StreamEvent::Error(format!(
                "LLM API stream error: {message}"
            )));
            return events;
        }

        // 处理 usage-only chunk（部分供应商在流末尾单独发送 usage）
        if let Some(usage) = &chunk.usage {
            let prompt_details = usage.prompt_tokens_details.as_ref();
            let completion_details = usage.completion_tokens_details.as_ref();
            events.push(StreamEvent::Usage(UsageInfo {
                prompt_tokens: usage.prompt_tokens.unwrap_or(0),
                completion_tokens: usage.completion_tokens.unwrap_or(0),
                total_tokens: usage.total_tokens.unwrap_or(0),
                cached_tokens: prompt_details
                    .and_then(|details| details.cached_tokens)
                    .unwrap_or(0),
                cache_creation_tokens: prompt_details
                    .and_then(|details| details.cache_creation_tokens)
                    .unwrap_or(0),
                reasoning_tokens: completion_details
                    .and_then(|details| details.reasoning_tokens)
                    .unwrap_or(0),
            }));
        }

        // 处理 choices
        if let Some(choices) = chunk.choices {
            for choice in choices {
                let StreamChoice {
                    delta,
                    message,
                    finish_reason,
                } = choice;
                // DeepSeek-compatible gateways use an empty string while a
                // choice is still streaming. Only a non-empty value is terminal.
                let finish_reason = finish_reason.filter(|reason| !reason.trim().is_empty());

                // 标准 SSE 使用 delta；部分中转站在流式帧中返回完整 message。
                // 完整 message 是累计值，只在终止帧消费一次，否则每帧都会重复拼接。
                let payload =
                    delta.or_else(|| finish_reason.is_some().then_some(message).flatten());
                if let Some(delta) = payload {
                    if let Some(ref reasoning) = delta.reasoning_content {
                        if !reasoning.is_empty() {
                            events.push(StreamEvent::ReasoningDelta(reasoning.clone()));
                        }
                    }

                    // 文本增量
                    if let Some(ref content) = delta.content {
                        if !content.is_empty() {
                            events.push(StreamEvent::TextDelta(content.clone()));
                        }
                    }

                    // tool call 增量
                    if let Some(tc_list) = delta.tool_calls {
                        for dtc in tc_list {
                            let idx = dtc.index.unwrap_or(0);
                            let func = dtc.function.as_ref();
                            events.push(StreamEvent::ToolCallDelta {
                                index: idx,
                                id: dtc.id,
                                name: func.and_then(|f| {
                                    f.name.as_ref().filter(|n| !n.is_empty()).cloned()
                                }),
                                arguments: func.and_then(|f| f.arguments.clone()),
                            });
                        }
                    }
                }

                if let Some(reason) = finish_reason {
                    events.push(StreamEvent::Done {
                        finish_reason: Some(reason),
                    });
                }
            }
        }

        events
    }
}

/// 将 InternalMessage 转换为严格符合 Chat Completions API 格式的 JSON
fn chat_completions_message(msg: &InternalMessage, replay_reasoning: bool) -> serde_json::Value {
    match msg.role.as_str() {
        "system" => {
            serde_json::json!({
                "role": "system",
                "content": content_to_string(&msg.content),
            })
        }
        "user" => {
            // user content 可能是字符串或多模态数组，直接传递
            let content = msg
                .content
                .clone()
                .unwrap_or(serde_json::Value::String(String::new()));
            serde_json::json!({
                "role": "user",
                "content": content,
            })
        }
        "assistant" => {
            let mut m = serde_json::json!({ "role": "assistant" });
            // Keep text-less tool turns as "". DeepSeek and some compatible gateways reject null.
            match &msg.content {
                Some(c) if !content_is_empty_value(c) => {
                    m["content"] = serde_json::Value::String(content_to_string(&msg.content));
                }
                _ => {
                    m["content"] = serde_json::Value::String(String::new());
                }
            }
            if let Some(ref tcs) = msg.tool_calls {
                let reasoning_content = tcs
                    .iter()
                    .find_map(|tc| tc.reasoning_content.as_deref())
                    .filter(|reasoning| !reasoning.is_empty());
                let tool_calls: Vec<serde_json::Value> = tcs
                    .iter()
                    .map(|tc| {
                        serde_json::json!({
                            "id": tc.id,
                            "type": "function",
                            "function": {
                                "name": tc.function.name,
                                "arguments": tc.function.arguments,
                            }
                        })
                    })
                    .collect();
                m["tool_calls"] = serde_json::Value::Array(tool_calls);
                if replay_reasoning {
                    if let Some(reasoning) = reasoning_content {
                        m["reasoning_content"] = serde_json::Value::String(reasoning.to_string());
                    }
                }
            }
            m
        }
        "tool" => {
            // tool 消息：必须有 tool_call_id 和 content（字符串），不含 name 字段
            serde_json::json!({
                "role": "tool",
                "tool_call_id": msg.tool_call_id.clone().unwrap_or_default(),
                "content": content_to_string(&msg.content),
            })
        }
        _ => {
            // 其他角色原样序列化
            serde_json::to_value(msg).unwrap_or(serde_json::Value::Null)
        }
    }
}

/// 将 content Option<Value> 转为纯文本字符串
fn content_to_string(content: &Option<serde_json::Value>) -> String {
    match content {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(serde_json::Value::Array(parts)) => parts
            .iter()
            .filter_map(|part| part.get("text").and_then(|v| v.as_str()))
            .collect::<Vec<_>>()
            .join("\n"),
        Some(serde_json::Value::Null) | None => String::new(),
        Some(other) => other.to_string(),
    }
}

fn content_is_empty_value(val: &serde_json::Value) -> bool {
    match val {
        serde_json::Value::Null => true,
        serde_json::Value::String(s) => s.is_empty(),
        serde_json::Value::Array(arr) => arr.is_empty(),
        _ => false,
    }
}

/// 构造符合严格 chat 网关约束的 messages 数组。
///
/// - 合并全部 system 内容为一条，并放在数组最前面
/// - 其余非 system 消息保持原有相对顺序
/// - 过滤空 system 片段，避免发出无意义的 system 消息
fn build_chat_completions_messages(
    messages: &[InternalMessage],
    model: &str,
) -> Vec<serde_json::Value> {
    let mut system_parts: Vec<String> = Vec::new();
    let mut non_system: Vec<serde_json::Value> = Vec::new();

    for msg in messages {
        if msg.role == "system" {
            let text = content_to_string(&msg.content);
            if !text.trim().is_empty() {
                system_parts.push(text);
            }
            continue;
        }
        non_system.push(chat_completions_message(
            msg,
            super::is_deepseek_model(model),
        ));
    }

    let mut formatted = Vec::with_capacity(non_system.len() + 1);
    if !system_parts.is_empty() {
        formatted.push(serde_json::json!({
            "role": "system",
            "content": system_parts.join("\n\n"),
        }));
    }
    formatted.extend(non_system);
    formatted
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::types::{InternalFunctionCall, InternalToolCall, text_content};

    fn msg(role: &str, content: &str) -> InternalMessage {
        InternalMessage {
            role: role.to_string(),
            content: text_content(content.to_string()),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        }
    }

    #[test]
    fn build_body_merges_system_messages_to_front() {
        let adapter = ChatCompletionsAdapter;
        let messages = vec![
            msg("system", "main rules"),
            msg("user", "hello"),
            msg("system", "runtime note"),
            msg("assistant", "hi"),
            msg("system", "continue"),
        ];

        let body = adapter.build_body("qwen3.6-27b", &messages, None, Some(1024));
        let formatted = body["messages"].as_array().expect("messages array");

        assert_eq!(formatted.len(), 3);
        assert_eq!(formatted[0]["role"], "system");
        assert_eq!(
            formatted[0]["content"],
            "main rules\n\nruntime note\n\ncontinue"
        );
        assert_eq!(formatted[1]["role"], "user");
        assert_eq!(formatted[1]["content"], "hello");
        assert_eq!(formatted[2]["role"], "assistant");
        assert_eq!(formatted[2]["content"], "hi");
    }

    #[test]
    fn build_body_skips_empty_system_and_keeps_non_system_order() {
        let adapter = ChatCompletionsAdapter;
        let messages = vec![
            msg("system", "   "),
            msg("user", "q1"),
            msg("assistant", "a1"),
            msg("tool", "tool-result"),
        ];
        let mut tool_msg = messages[3].clone();
        tool_msg.tool_call_id = Some("call-1".to_string());
        let messages = vec![
            messages[0].clone(),
            messages[1].clone(),
            messages[2].clone(),
            tool_msg,
        ];

        let body = adapter.build_body("qwen3.6-27b", &messages, None, Some(1024));
        let formatted = body["messages"].as_array().expect("messages array");

        // 没有有效 system 时，不应插入空 system
        assert_eq!(formatted[0]["role"], "user");
        assert_eq!(formatted[1]["role"], "assistant");
        assert_eq!(formatted[2]["role"], "tool");
        assert_eq!(formatted[2]["tool_call_id"], "call-1");
    }

    #[test]
    fn build_body_replays_reasoning_on_deepseek_tool_call_turns() {
        let adapter = ChatCompletionsAdapter;
        let messages = vec![InternalMessage {
            role: "assistant".to_string(),
            content: None,
            tool_calls: Some(vec![InternalToolCall {
                id: "call-weather".to_string(),
                call_type: "function".to_string(),
                function: InternalFunctionCall {
                    name: "get_weather".to_string(),
                    arguments: r#"{"city":"Paris"}"#.to_string(),
                },
                reasoning_content: Some("provider thinking".to_string()),
            }]),
            tool_call_id: None,
            name: None,
        }];

        let deepseek = adapter.build_body("c-deepseek-v4-flash", &messages, None, Some(1024));
        let assistant = &deepseek["messages"][0];
        assert_eq!(assistant["content"], "");
        assert_eq!(assistant["reasoning_content"], "provider thinking");
        assert_eq!(
            assistant["tool_calls"][0]["function"]["name"],
            "get_weather"
        );

        let other_model = adapter.build_body("qwen3.6-27b", &messages, None, Some(1024));
        assert!(
            other_model["messages"][0]
                .get("reasoning_content")
                .is_none()
        );
    }

    #[test]
    fn parse_stream_line_reads_message_content_from_chat_gateway() {
        let adapter = ChatCompletionsAdapter;

        let events = adapter.parse_stream_line(
            r#"data: {"choices":[{"message":{"content":"Hello from message"},"finish_reason":"stop"}]}"#,
        );

        assert!(matches!(
            events.as_slice(),
            [
                StreamEvent::TextDelta(text),
                StreamEvent::Done {
                    finish_reason: Some(reason)
                }
            ] if reason == "stop" && text == "Hello from message"
        ));
    }

    #[test]
    fn parse_stream_line_keeps_deepseek_empty_finish_reason_open() {
        let adapter = ChatCompletionsAdapter;

        let events = adapter.parse_stream_line(
            r#"data: {"choices":[{"index":0,"delta":{"role":"assistant","reasoning_content":"thinking"},"finish_reason":""}]}"#,
        );

        assert!(matches!(
            events.as_slice(),
            [StreamEvent::ReasoningDelta(reasoning)] if reasoning == "thinking"
        ));
    }

    #[test]
    fn parse_stream_line_reads_deepseek_content_before_terminal_frame() {
        let adapter = ChatCompletionsAdapter;

        let content_events = adapter.parse_stream_line(
            r#"data: {"choices":[{"index":0,"delta":{"content":"OK"},"finish_reason":""}]}"#,
        );
        let terminal_events = adapter.parse_stream_line(
            r#"data: {"choices":[{"index":0,"delta":{"content":""},"finish_reason":"stop"}]}"#,
        );

        assert!(matches!(
            content_events.as_slice(),
            [StreamEvent::TextDelta(text)] if text == "OK"
        ));
        assert!(matches!(
            terminal_events.as_slice(),
            [StreamEvent::Done { finish_reason: Some(reason) }] if reason == "stop"
        ));
    }

    #[test]
    fn parse_stream_line_surfaces_provider_error() {
        let adapter = ChatCompletionsAdapter;

        let events = adapter.parse_stream_line(
            r#"data: {"error":{"message":"model unavailable","type":"upstream_error"}}"#,
        );

        assert!(matches!(
            events.as_slice(),
            [StreamEvent::Error(message)] if message.contains("model unavailable")
        ));
    }

    #[test]
    fn parse_stream_line_ignores_cumulative_message_before_terminal_frame() {
        let adapter = ChatCompletionsAdapter;

        let events = adapter.parse_stream_line(
            r#"data: {"choices":[{"message":{"content":"cumulative text"},"finish_reason":null}]}"#,
        );

        assert!(events.is_empty());
    }
}
