//! Google Gemini API adapter (wire_api = "gemini")
//! 支持 Gemini 系列模型的原生 API 格式（streamGenerateContent）。
//! 注意：大多数用户通过 Google 的 OpenAI 兼容端点（/v1beta/openai/...）使用 Gemini，
//! 这种情况使用 chat_completions adapter 即可。本 adapter 用于原生 Gemini REST API。
//! - URL: {base_url}/models/{model}:streamGenerateContent?key={api_key}
//! - Body: { contents: [...], tools: [...] }
//! - Response: JSON 流（非标准 SSE），每行是一个完整 JSON 对象

use async_trait::async_trait;
use reqwest::header::{CONTENT_TYPE, HeaderMap, HeaderValue};

use super::ProviderAdapter;
use super::types::{
    InternalMessage, StreamEvent, UsageInfo, content_as_text, content_to_gemini_parts,
    safe_max_output_tokens,
};

pub struct GoogleAdapter;

#[async_trait]
impl ProviderAdapter for GoogleAdapter {
    fn build_url(&self, base_url: &str, model: &str) -> String {
        let base = base_url.trim_end_matches('/');
        // 原生 Gemini API: https://generativelanguage.googleapis.com/v1beta
        format!("{base}/models/{model}:streamGenerateContent?alt=sse")
    }

    fn build_headers(&self, api_key: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        // Gemini 原生 API 使用 query param key 或 x-goog-api-key header
        if !api_key.is_empty() {
            if let Ok(val) = HeaderValue::from_str(api_key) {
                headers.insert("x-goog-api-key", val);
            }
        }
        headers
    }

    fn build_body(
        &self,
        _model: &str,
        messages: &[InternalMessage],
        tools: Option<&[serde_json::Value]>,
        max_tokens: Option<i64>,
    ) -> serde_json::Value {
        // 转换为 Gemini contents 格式
        let mut system_text = String::new();
        let mut contents: Vec<serde_json::Value> = Vec::new();

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
                "user" => {
                    contents.push(serde_json::json!({
                        "role": "user",
                        "parts": content_to_gemini_parts(&msg.content)
                    }));
                }
                "assistant" => {
                    let mut parts: Vec<serde_json::Value> = Vec::new();
                    parts.extend(content_to_gemini_parts(&msg.content));
                    if let Some(ref tcs) = msg.tool_calls {
                        for tc in tcs {
                            let args: serde_json::Value =
                                serde_json::from_str(&tc.function.arguments)
                                    .unwrap_or(serde_json::json!({}));
                            parts.push(serde_json::json!({
                                "functionCall": {
                                    "name": tc.function.name,
                                    "args": args
                                }
                            }));
                        }
                    }
                    if !parts.is_empty() {
                        contents.push(serde_json::json!({
                            "role": "model",
                            "parts": parts
                        }));
                    }
                }
                "tool" => {
                    // functionResponse
                    let response_val: serde_json::Value =
                        serde_json::from_str(&content_as_text(&msg.content)).unwrap_or_else(
                            |_| serde_json::json!({"result": content_as_text(&msg.content)}),
                        );
                    contents.push(serde_json::json!({
                        "role": "user",
                        "parts": [{
                            "functionResponse": {
                                "name": msg.name.clone().unwrap_or_default(),
                                "response": response_val
                            }
                        }]
                    }));
                }
                _ => {}
            }
        }

        let mut body = serde_json::json!({
            "contents": contents,
        });

        // system instruction
        if !system_text.is_empty() {
            body["systemInstruction"] = serde_json::json!({
                "parts": [{ "text": system_text }]
            });
        }

        // tools 转换为 Gemini 格式
        if let Some(tools) = tools {
            if !tools.is_empty() {
                let function_declarations: Vec<serde_json::Value> = tools
                    .iter()
                    .filter_map(|t| {
                        let func = t.get("function")?;
                        Some(serde_json::json!({
                            "name": func.get("name")?,
                            "description": func.get("description").and_then(|d| d.as_str()).unwrap_or(""),
                            "parameters": func.get("parameters").cloned().unwrap_or(serde_json::json!({}))
                        }))
                    })
                    .collect();
                body["tools"] = serde_json::json!([{
                    "functionDeclarations": function_declarations
                }]);
            }
        }

        body["generationConfig"] = serde_json::json!({
            "maxOutputTokens": safe_max_output_tokens(max_tokens)
        });

        body
    }

    fn is_stream_done(&self, line: &str) -> bool {
        // Gemini 的 SSE 模式（alt=sse）：不使用 [DONE] 标志
        // 当包含 finishReason 时表示结束
        line.contains("\"finishReason\"") || line.trim() == "[DONE]"
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

        if data == "[DONE]" {
            events.push(StreamEvent::Done {
                finish_reason: Some("stop".to_string()),
            });
            return events;
        }

        let parsed: serde_json::Value = match serde_json::from_str(data) {
            Ok(v) => v,
            Err(_) => return events,
        };

        // Gemini 响应结构: { candidates: [{ content: { parts: [...] }, finishReason: ... }], usageMetadata: {...} }
        if let Some(candidates) = parsed.get("candidates").and_then(|v| v.as_array()) {
            for candidate in candidates {
                // 检查 finishReason
                if let Some(reason) = candidate.get("finishReason").and_then(|v| v.as_str()) {
                    let mapped = match reason {
                        "STOP" => "stop",
                        "MAX_TOKENS" => "length",
                        _ => reason,
                    };
                    events.push(StreamEvent::Done {
                        finish_reason: Some(mapped.to_string()),
                    });
                }

                // 解析 content.parts
                if let Some(content) = candidate.get("content") {
                    if let Some(parts) = content.get("parts").and_then(|v| v.as_array()) {
                        for (idx, part) in parts.iter().enumerate() {
                            // 文本
                            if let Some(text) = part.get("text").and_then(|v| v.as_str()) {
                                if !text.is_empty() {
                                    events.push(StreamEvent::TextDelta(text.to_string()));
                                }
                            }
                            // functionCall
                            if let Some(fc) = part.get("functionCall") {
                                let name = fc.get("name").and_then(|v| v.as_str()).unwrap_or("");
                                let args =
                                    fc.get("args").map(|v| v.to_string()).unwrap_or_default();
                                // Gemini 不提供 call_id，我们生成一个
                                let call_id = format!("call_{}", uuid::Uuid::new_v4());
                                events.push(StreamEvent::ToolCallDelta {
                                    index: idx,
                                    id: Some(call_id),
                                    name: Some(name.to_string()),
                                    arguments: Some(args),
                                });
                            }
                        }
                    }
                }
            }
        }

        // usageMetadata
        if let Some(usage) = parsed.get("usageMetadata") {
            let prompt = usage
                .get("promptTokenCount")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let completion = usage
                .get("candidatesTokenCount")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let total = usage
                .get("totalTokenCount")
                .and_then(|v| v.as_u64())
                .unwrap_or(prompt + completion);
            events.push(StreamEvent::Usage(UsageInfo {
                prompt_tokens: prompt,
                completion_tokens: completion,
                total_tokens: total,
                cached_tokens: 0,
                cache_creation_tokens: 0,
                reasoning_tokens: 0,
            }));
        }

        events
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_stream_line_supports_non_prefixed_data() {
        let adapter = GoogleAdapter;
        let events =
            adapter.parse_stream_line(r#"{"candidates":[{"content":{"parts":[{"text":"hey"}]}}]}"#);
        assert!(!events.is_empty());
    }
}
