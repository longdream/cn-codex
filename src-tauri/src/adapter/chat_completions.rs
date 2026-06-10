//! OpenAI Chat Completions API adapter (wire_api = "chat")
//! 这是大多数供应商的默认格式：DeepSeek、国产模型、各种中转站等

use async_trait::async_trait;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};
use serde::Deserialize;

use super::ProviderAdapter;
use super::types::{InternalMessage, StreamEvent, UsageInfo};

pub struct ChatCompletionsAdapter;

/// SSE chunk 中的 choice 结构
#[derive(Debug, Deserialize)]
struct StreamChoice {
    delta: Option<DeltaContent>,
    finish_reason: Option<String>,
}

/// SSE chunk 中的 delta 内容
#[derive(Debug, Deserialize)]
struct DeltaContent {
    content: Option<String>,
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
}

/// usage 字段（部分供应商在最后一个 chunk 中返回）
#[derive(Debug, Deserialize)]
struct ChunkUsage {
    #[serde(alias = "input_tokens")]
    prompt_tokens: Option<u64>,
    #[serde(alias = "output_tokens")]
    completion_tokens: Option<u64>,
    total_tokens: Option<u64>,
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
    ) -> serde_json::Value {
        let formatted_messages: Vec<serde_json::Value> = messages
            .iter()
            .map(|msg| chat_completions_message(msg))
            .collect();

        let mut body = serde_json::json!({
            "model": model,
            "messages": formatted_messages,
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

        // 处理 usage-only chunk（部分供应商在流末尾单独发送 usage）
        if let Some(usage) = &chunk.usage {
            events.push(StreamEvent::Usage(UsageInfo {
                prompt_tokens: usage.prompt_tokens.unwrap_or(0),
                completion_tokens: usage.completion_tokens.unwrap_or(0),
                total_tokens: usage.total_tokens.unwrap_or(0),
            }));
        }

        // 处理 choices
        if let Some(choices) = chunk.choices {
            for choice in choices {
                // finish_reason
                if let Some(ref reason) = choice.finish_reason {
                    events.push(StreamEvent::Done {
                        finish_reason: Some(reason.clone()),
                    });
                }

                if let Some(delta) = choice.delta {
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
                                name: func.and_then(|f| f.name.as_ref().filter(|n| !n.is_empty()).cloned()),
                                arguments: func.and_then(|f| f.arguments.clone()),
                            });
                        }
                    }
                }
            }
        }

        events
    }
}

/// 将 InternalMessage 转换为严格符合 Chat Completions API 格式的 JSON
fn chat_completions_message(msg: &InternalMessage) -> serde_json::Value {
    match msg.role.as_str() {
        "system" => {
            serde_json::json!({
                "role": "system",
                "content": content_to_string(&msg.content),
            })
        }
        "user" => {
            // user content 可能是字符串或多模态数组，直接传递
            let content = msg.content.clone().unwrap_or(serde_json::Value::String(String::new()));
            serde_json::json!({
                "role": "user",
                "content": content,
            })
        }
        "assistant" => {
            let mut m = serde_json::json!({ "role": "assistant" });
            // assistant content：有 tool_calls 时 content 可为 null，但某些 API 要求空字符串
            match &msg.content {
                Some(c) if !content_is_empty_value(c) => {
                    m["content"] = serde_json::Value::String(content_to_string(&msg.content));
                }
                _ => {
                    if msg.tool_calls.is_some() {
                        m["content"] = serde_json::Value::Null;
                    } else {
                        m["content"] = serde_json::Value::String(String::new());
                    }
                }
            }
            if let Some(ref tcs) = msg.tool_calls {
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

