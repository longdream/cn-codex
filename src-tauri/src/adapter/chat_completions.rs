//! OpenAI Chat Completions API adapter (wire_api = "chat")
//! 这是大多数供应商的默认格式：DeepSeek、国产模型、各种中转站等

use async_trait::async_trait;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use serde::Deserialize;

use super::types::{InternalMessage, StreamEvent, UsageInfo};
use super::ProviderAdapter;

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
    prompt_tokens: Option<u64>,
    completion_tokens: Option<u64>,
    total_tokens: Option<u64>,
}

#[async_trait]
impl ProviderAdapter for ChatCompletionsAdapter {
    fn build_url(&self, base_url: &str, _model: &str) -> String {
        let base = base_url.trim_end_matches('/');
        // 如果已经包含完整路径，直接使用
        if base.ends_with("/chat/completions") {
            return base.to_string();
        }
        // 如果是 responses 结尾，替换为 chat/completions
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
        let mut body = serde_json::json!({
            "model": model,
            "messages": messages,
            "stream": true,
        });
        if let Some(tools) = tools {
            if !tools.is_empty() {
                body["tools"] = serde_json::Value::Array(tools.to_vec());
            }
        }
        body
    }

    fn is_stream_done(&self, line: &str) -> bool {
        line.trim() == "data: [DONE]"
    }

    fn parse_stream_line(&self, line: &str) -> Vec<StreamEvent> {
        let mut events = Vec::new();

        let data = match line.strip_prefix("data: ") {
            Some(d) => d.trim(),
            None => return events,
        };

        if data == "[DONE]" {
            return events;
        }

        let chunk: StreamChunk = match serde_json::from_str(data) {
            Ok(c) => c,
            Err(_) => return events,
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
                                name: func.and_then(|f| f.name.clone()),
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
