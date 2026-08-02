//! OpenAI Responses API adapter (wire_api = "responses").
//! Converts CN-Codex's internal message/tool shape into Responses API wire
//! payloads and parses streamed Responses API events back into internal events.

use async_trait::async_trait;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};

use super::ProviderAdapter;
use super::types::{
    CompletionOutput, InternalMessage, StreamEvent, ToolCallResult, UsageInfo, content_as_text,
    content_to_responses_content, safe_max_output_tokens,
};

pub struct ResponsesAdapter;

const APPLY_PATCH_LARK_GRAMMAR: &str = r#"start: begin_patch hunk+ end_patch
begin_patch: "*** Begin Patch" LF
end_patch: "*** End Patch" LF?

hunk: add_hunk | delete_hunk | update_hunk
add_hunk: "*** Add File: " filename LF add_line+
delete_hunk: "*** Delete File: " filename LF
update_hunk: "*** Update File: " filename LF change_move? change?

filename: /(.+)/
add_line: "+" /(.*)/ LF -> line

change_move: "*** Move to: " filename LF
change: (change_context | change_line)+ eof_line?
change_context: ("@@" | "@@ " /(.+)/) LF
change_line: ("+" | "-" | " ") /(.*)/ LF
eof_line: "*** End of File" LF

%import common.LF"#;

#[async_trait]
impl ProviderAdapter for ResponsesAdapter {
    fn build_url(&self, base_url: &str, _model: &str) -> String {
        let base = base_url.trim_end_matches('/');
        if base.ends_with("/responses") {
            return base.to_string();
        }
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
        max_tokens: Option<i64>,
    ) -> serde_json::Value {
        let input: Vec<serde_json::Value> = messages
            .iter()
            .flat_map(responses_input_items_from_message)
            .collect();

        let mut body = serde_json::json!({
            "model": model,
            "input": input,
            "max_output_tokens": safe_max_output_tokens(max_tokens),
            "stream": true,
        });

        if let Some(tools) = tools {
            if !tools.is_empty() {
                let resp_tools: Vec<serde_json::Value> = tools
                    .iter()
                    .filter_map(responses_tool_from_function_spec)
                    .collect();
                body["tools"] = serde_json::Value::Array(resp_tools);
            }
        }

        body
    }

    fn is_stream_done(&self, line: &str) -> bool {
        let trimmed = line.trim();
        trimmed == "data: [DONE]" || trimmed == "data:[DONE]" || trimmed == "[DONE]"
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

        let event_type = parsed.get("type").and_then(|v| v.as_str()).unwrap_or("");

        match event_type {
            "response.output_text.delta" => {
                if let Some(delta) = parsed.get("delta").and_then(|v| v.as_str()) {
                    if !delta.is_empty() {
                        events.push(StreamEvent::TextDelta(delta.to_string()));
                    }
                }
            }
            "response.function_call_arguments.delta" => {
                events.push(tool_call_delta_event(
                    &parsed,
                    parsed.get("call_id").and_then(|v| v.as_str()),
                    parsed.get("name").and_then(|v| v.as_str()),
                    parsed.get("delta").and_then(|v| v.as_str()),
                ));
            }
            "response.custom_tool_call_input.delta" => {
                let call_id = parsed
                    .get("call_id")
                    .and_then(|v| v.as_str())
                    .or_else(|| parsed.get("item_id").and_then(|v| v.as_str()));
                events.push(tool_call_delta_event(
                    &parsed,
                    call_id,
                    None,
                    parsed.get("delta").and_then(|v| v.as_str()),
                ));
            }
            "response.output_item.added" => {
                if let Some(item) = parsed.get("item") {
                    let item_type = item.get("type").and_then(|v| v.as_str()).unwrap_or("");
                    if item_type == "function_call" || item_type == "custom_tool_call" {
                        events.push(tool_call_delta_event(
                            &parsed,
                            item.get("call_id").and_then(|v| v.as_str()),
                            item.get("name").and_then(|v| v.as_str()),
                            None,
                        ));
                    } else if item_type == "tool_search_call" {
                        events.push(tool_search_call_event(&parsed, item, false));
                    }
                }
            }
            "response.output_item.done" => {
                if let Some(item) = parsed.get("item") {
                    let item_type = item.get("type").and_then(|v| v.as_str()).unwrap_or("");
                    match item_type {
                        "function_call" => events.push(tool_call_done_event(
                            &parsed,
                            item,
                            item.get("arguments").and_then(|v| v.as_str()),
                        )),
                        "custom_tool_call" => events.push(tool_call_done_event(
                            &parsed,
                            item,
                            item.get("input").and_then(|v| v.as_str()),
                        )),
                        "tool_search_call" => {
                            events.push(tool_search_call_event(&parsed, item, true));
                        }
                        _ => {}
                    }
                }
            }
            "response.failed" => {
                events.push(StreamEvent::Error(response_failure_message(&parsed)));
            }
            "response.incomplete" => {
                let reason = parsed
                    .pointer("/response/incomplete_details/reason")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("unknown");
                events.push(StreamEvent::Error(format!(
                    "Incomplete response returned, reason: {reason}"
                )));
            }
            "response.completed" => {
                if let Some(response) = parsed.get("response") {
                    if let Some(usage) = response.get("usage") {
                        let prompt = usage
                            .get("input_tokens")
                            .and_then(|v| v.as_u64())
                            .unwrap_or(0);
                        let completion = usage
                            .get("output_tokens")
                            .and_then(|v| v.as_u64())
                            .unwrap_or(0);
                        let cached = usage
                            .get("input_tokens_details")
                            .and_then(|d| d.get("cached_tokens"))
                            .and_then(|v| v.as_u64())
                            .unwrap_or(0);
                        let reasoning = usage
                            .get("output_tokens_details")
                            .and_then(|d| d.get("reasoning_tokens"))
                            .and_then(|v| v.as_u64())
                            .unwrap_or(0);
                        events.push(StreamEvent::Usage(UsageInfo {
                            prompt_tokens: prompt,
                            completion_tokens: completion,
                            total_tokens: prompt + completion,
                            cached_tokens: cached,
                            cache_creation_tokens: 0,
                            reasoning_tokens: reasoning,
                        }));
                    }
                }
                events.push(StreamEvent::Done {
                    finish_reason: Some("stop".to_string()),
                });
            }
            "response.output_text.done" => {}
            _ => {}
        }

        events
    }

    fn parse_non_streaming(&self, body: &str) -> Result<CompletionOutput, String> {
        parse_non_streaming_responses(body)
    }
}

fn parse_non_streaming_responses(body: &str) -> Result<CompletionOutput, String> {
    let json: serde_json::Value = serde_json::from_str(body)
        .map_err(|error| format!("Failed to parse non-streaming Responses JSON: {error}"))?;

    if let Some(error) = json.get("error") {
        let message = error
            .get("message")
            .and_then(|value| value.as_str())
            .unwrap_or("Unknown Responses API error");
        return Err(format!("Responses API error: {message}"));
    }

    let status = json.get("status").and_then(|value| value.as_str());
    if matches!(
        status,
        Some("failed") | Some("incomplete") | Some("cancelled")
    ) {
        let detail = json
            .get("incomplete_details")
            .and_then(|value| value.get("reason"))
            .and_then(|value| value.as_str())
            .or_else(|| json.get("error").and_then(|value| value.as_str()))
            .unwrap_or(status.unwrap_or("unknown"));
        return Err(format!("Responses response {status:?}: {detail}"));
    }

    let output_items = json
        .get("output")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();
    let mut text_parts = Vec::new();
    let mut tool_calls = Vec::new();

    for item in &output_items {
        let item_type = item
            .get("type")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        match item_type {
            "message" => {
                if let Some(content) = item.get("content").and_then(|value| value.as_array()) {
                    for part in content {
                        if matches!(
                            part.get("type").and_then(|value| value.as_str()),
                            Some("output_text") | Some("text")
                        ) {
                            if let Some(text) = part.get("text").and_then(|value| value.as_str()) {
                                text_parts.push(text.to_string());
                            }
                        }
                    }
                }
            }
            "function_call" => {
                tool_calls.push(ToolCallResult {
                    id: item
                        .get("call_id")
                        .or_else(|| item.get("id"))
                        .and_then(|value| value.as_str())
                        .unwrap_or_default()
                        .to_string(),
                    name: item
                        .get("name")
                        .and_then(|value| value.as_str())
                        .unwrap_or_default()
                        .to_string(),
                    arguments: value_to_argument_string(item.get("arguments")),
                });
            }
            "custom_tool_call" => {
                tool_calls.push(ToolCallResult {
                    id: item
                        .get("call_id")
                        .or_else(|| item.get("id"))
                        .and_then(|value| value.as_str())
                        .unwrap_or_default()
                        .to_string(),
                    name: item
                        .get("name")
                        .and_then(|value| value.as_str())
                        .unwrap_or("apply_patch")
                        .to_string(),
                    arguments: value_to_argument_string(item.get("input")),
                });
            }
            "tool_search_call" => {
                tool_calls.push(ToolCallResult {
                    id: item
                        .get("call_id")
                        .or_else(|| item.get("id"))
                        .and_then(|value| value.as_str())
                        .unwrap_or_default()
                        .to_string(),
                    name: "tool_search".to_string(),
                    arguments: value_to_argument_string(item.get("arguments")),
                });
            }
            _ => {}
        }
    }

    let text = json
        .get("output_text")
        .and_then(|value| value.as_str())
        .map(str::to_string)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| text_parts.join(""));
    let usage = json.get("usage").map(parse_responses_usage);

    Ok(CompletionOutput {
        text,
        tool_calls,
        finish_reason: status.map(str::to_string),
        usage,
    })
}

fn value_to_argument_string(value: Option<&serde_json::Value>) -> String {
    match value {
        Some(serde_json::Value::String(text)) => text.clone(),
        Some(value) => value.to_string(),
        None => String::new(),
    }
}

fn parse_responses_usage(value: &serde_json::Value) -> UsageInfo {
    let prompt_tokens = value
        .get("input_tokens")
        .or_else(|| value.get("prompt_tokens"))
        .and_then(|value| value.as_u64())
        .unwrap_or(0);
    let completion_tokens = value
        .get("output_tokens")
        .or_else(|| value.get("completion_tokens"))
        .and_then(|value| value.as_u64())
        .unwrap_or(0);
    UsageInfo {
        prompt_tokens,
        completion_tokens,
        total_tokens: value
            .get("total_tokens")
            .and_then(|value| value.as_u64())
            .unwrap_or(prompt_tokens.saturating_add(completion_tokens)),
        cached_tokens: value
            .get("input_tokens_details")
            .and_then(|value| value.get("cached_tokens"))
            .and_then(|value| value.as_u64())
            .unwrap_or(0),
        cache_creation_tokens: 0,
        reasoning_tokens: value
            .get("output_tokens_details")
            .and_then(|value| value.get("reasoning_tokens"))
            .and_then(|value| value.as_u64())
            .unwrap_or(0),
    }
}

fn responses_input_items_from_message(msg: &InternalMessage) -> Vec<serde_json::Value> {
    if msg.role == "system" {
        return vec![serde_json::json!({
            "role": "developer",
            "content": content_as_text(&msg.content)
        })];
    }

    if msg.role == "tool" {
        let call_id = msg.tool_call_id.clone().unwrap_or_default();
        let output = content_as_text(&msg.content);
        if msg.name.as_deref() == Some("tool_search") {
            if let Some(tools) = tool_search_output_tools(&output) {
                return vec![serde_json::json!({
                    "type": "tool_search_output",
                    "call_id": call_id,
                    "status": "completed",
                    "execution": "client",
                    "tools": tools
                })];
            }
        }
        if msg.name.as_deref() == Some("apply_patch") {
            return vec![serde_json::json!({
                "type": "custom_tool_call_output",
                "call_id": call_id,
                "name": "apply_patch",
                "output": output
            })];
        }

        return vec![serde_json::json!({
            "type": "function_call_output",
            "call_id": call_id,
            "output": output
        })];
    }

    if msg.role == "assistant" && msg.tool_calls.is_some() {
        let tcs = msg.tool_calls.as_ref().unwrap();
        let mut items = Vec::new();

        let content = content_as_text(&msg.content);
        if !content.is_empty() {
            items.push(serde_json::json!({
                "role": "assistant",
                "content": content
            }));
        }

        for tc in tcs {
            if tc.function.name == "tool_search" {
                let arguments = serde_json::from_str::<serde_json::Value>(&tc.function.arguments)
                    .unwrap_or_else(|_| serde_json::json!({}));
                items.push(serde_json::json!({
                    "type": "tool_search_call",
                    "call_id": tc.id,
                    "status": "completed",
                    "execution": "client",
                    "arguments": arguments
                }));
            } else if tc.function.name == "apply_patch"
                && tc
                    .function
                    .arguments
                    .trim_start()
                    .starts_with("*** Begin Patch")
            {
                items.push(serde_json::json!({
                    "type": "custom_tool_call",
                    "call_id": tc.id,
                    "name": tc.function.name,
                    "input": tc.function.arguments
                }));
            } else {
                items.push(serde_json::json!({
                    "type": "function_call",
                    "call_id": tc.id,
                    "name": tc.function.name,
                    "arguments": tc.function.arguments
                }));
            }
        }

        return items;
    }

    vec![serde_json::json!({
        "role": msg.role,
        "content": content_to_responses_content(&msg.content)
    })]
}

fn responses_tool_from_function_spec(tool: &serde_json::Value) -> Option<serde_json::Value> {
    let func = tool.get("function")?;
    let name = func.get("name")?.as_str()?;
    if name == "apply_patch" {
        return Some(serde_json::json!({
            "type": "custom",
            "name": "apply_patch",
            "description": "Use `apply_patch` as the default for every edit to an existing text file, including single-file edits. It applies contextual diffs while preserving existing text encoding and line endings. Relative paths are preferred; absolute paths must remain inside the workspace. This is a FREEFORM tool, so do not wrap the patch in JSON.",
            "format": {
                "type": "grammar",
                "syntax": "lark",
                "definition": APPLY_PATCH_LARK_GRAMMAR
            }
        }));
    }

    let mut response_tool = serde_json::json!({
        "type": "function",
        "name": name,
        "description": func.get("description").and_then(|d| d.as_str()).unwrap_or(""),
        "parameters": func.get("parameters").cloned().unwrap_or(serde_json::json!({}))
    });
    if let Some(defer_loading) = tool
        .get("defer_loading")
        .and_then(serde_json::Value::as_bool)
    {
        response_tool["defer_loading"] = serde_json::Value::Bool(defer_loading);
    }
    Some(response_tool)
}

fn tool_search_call_event(
    parsed: &serde_json::Value,
    item: &serde_json::Value,
    include_arguments: bool,
) -> StreamEvent {
    let arguments = if include_arguments {
        item.get("arguments")
            .map(tool_search_arguments_to_string)
            .filter(|value| !value.is_empty())
    } else {
        None
    };
    tool_call_delta_event(
        parsed,
        item.get("call_id")
            .and_then(|v| v.as_str())
            .or_else(|| item.get("id").and_then(|v| v.as_str())),
        Some("tool_search"),
        arguments.as_deref(),
    )
}

fn tool_search_arguments_to_string(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(text) => text.clone(),
        other => serde_json::to_string(other).unwrap_or_default(),
    }
}

fn tool_search_output_tools(output: &str) -> Option<Vec<serde_json::Value>> {
    let parsed: serde_json::Value = serde_json::from_str(output).ok()?;
    if let Some(tools) = parsed.get("tools").and_then(serde_json::Value::as_array) {
        let tools = tools
            .iter()
            .filter(|tool| {
                tool.get("type")
                    .and_then(serde_json::Value::as_str)
                    .is_some()
            })
            .cloned()
            .collect::<Vec<_>>();
        if !tools.is_empty() {
            return Some(tools);
        }
    }

    let matches = parsed.get("matches")?.as_array()?;
    let tools: Vec<_> = matches
        .iter()
        .filter_map(|item| {
            let spec = item.get("spec")?;
            responses_tool_from_function_spec(spec).or_else(|| Some(spec.clone()))
        })
        .collect();
    (!tools.is_empty()).then_some(tools)
}

fn tool_call_delta_event(
    parsed: &serde_json::Value,
    call_id: Option<&str>,
    name: Option<&str>,
    arguments: Option<&str>,
) -> StreamEvent {
    let idx = parsed
        .get("output_index")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as usize;
    StreamEvent::ToolCallDelta {
        index: idx,
        id: call_id
            .filter(|value| !value.is_empty())
            .map(str::to_string),
        name: name.filter(|value| !value.is_empty()).map(str::to_string),
        arguments: arguments
            .filter(|value| !value.is_empty())
            .map(str::to_string),
    }
}

fn tool_call_done_event(
    parsed: &serde_json::Value,
    item: &serde_json::Value,
    arguments: Option<&str>,
) -> StreamEvent {
    let index = parsed
        .get("output_index")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0) as usize;
    StreamEvent::ToolCallDone {
        index,
        id: item
            .get("call_id")
            .and_then(serde_json::Value::as_str)
            .filter(|value| !value.is_empty())
            .map(str::to_string),
        name: item
            .get("name")
            .and_then(serde_json::Value::as_str)
            .filter(|value| !value.is_empty())
            .map(str::to_string),
        arguments: arguments.map(str::to_string),
    }
}

fn response_failure_message(parsed: &serde_json::Value) -> String {
    let error = parsed
        .pointer("/response/error")
        .or_else(|| parsed.get("error"));
    let code = error
        .and_then(|value| value.get("code"))
        .and_then(serde_json::Value::as_str);
    let message = error
        .and_then(|value| value.get("message"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("response.failed event received");
    match code {
        Some(code) if !code.is_empty() => format!("LLM response failed ({code}): {message}"),
        _ => format!("LLM response failed: {message}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::types::{InternalFunctionCall, InternalToolCall, text_content};

    #[test]
    fn parse_stream_line_supports_non_prefixed_data() {
        let adapter = ResponsesAdapter;
        let events =
            adapter.parse_stream_line(r#"{"type":"response.output_text.delta","delta":"hello"}"#);
        assert!(!events.is_empty());
    }

    #[test]
    fn responses_body_converts_apply_patch_to_custom_tool() {
        let adapter = ResponsesAdapter;
        let tools = vec![
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "apply_patch",
                    "description": "Apply a patch",
                    "parameters": {
                        "type": "object",
                        "properties": { "patch": { "type": "string" } }
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "read_file",
                    "description": "Read a file",
                    "parameters": { "type": "object" }
                }
            }),
        ];

        let body = adapter.build_body("gpt-5", &[], Some(&tools), None);

        assert_eq!(
            body.pointer("/tools/0/type")
                .and_then(serde_json::Value::as_str),
            Some("custom")
        );
        assert_eq!(
            body.pointer("/tools/0/name")
                .and_then(serde_json::Value::as_str),
            Some("apply_patch")
        );
        assert_eq!(
            body.pointer("/tools/0/format/syntax")
                .and_then(serde_json::Value::as_str),
            Some("lark")
        );
        assert!(
            body.pointer("/tools/0/format/definition")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .contains("*** Begin Patch")
        );
        assert_eq!(
            body.pointer("/tools/1/type")
                .and_then(serde_json::Value::as_str),
            Some("function")
        );
    }

    #[test]
    fn responses_body_uses_custom_tool_call_items_for_raw_apply_patch_history() {
        let adapter = ResponsesAdapter;
        let patch = "*** Begin Patch\n*** Add File: src/raw.txt\n+hi\n*** End Patch";
        let messages = vec![
            InternalMessage {
                role: "assistant".to_string(),
                content: None,
                tool_calls: Some(vec![InternalToolCall {
                    id: "call-patch".to_string(),
                    call_type: "function".to_string(),
                    function: InternalFunctionCall {
                        name: "apply_patch".to_string(),
                        arguments: patch.to_string(),
                    },
                }]),
                tool_call_id: None,
                name: None,
            },
            InternalMessage {
                role: "tool".to_string(),
                content: text_content("Success. Updated files."),
                tool_calls: None,
                tool_call_id: Some("call-patch".to_string()),
                name: Some("apply_patch".to_string()),
            },
        ];

        let body = adapter.build_body("gpt-5", &messages, None, None);

        assert_eq!(
            body.pointer("/input/0/type")
                .and_then(serde_json::Value::as_str),
            Some("custom_tool_call")
        );
        assert_eq!(
            body.pointer("/input/0/input")
                .and_then(serde_json::Value::as_str),
            Some(patch)
        );
        assert_eq!(
            body.pointer("/input/1/type")
                .and_then(serde_json::Value::as_str),
            Some("custom_tool_call_output")
        );
        assert_eq!(
            body.pointer("/input/1/name")
                .and_then(serde_json::Value::as_str),
            Some("apply_patch")
        );
    }

    #[test]
    fn responses_parser_accepts_custom_tool_call_events() {
        let adapter = ResponsesAdapter;
        let added = adapter.parse_stream_line(
            r#"data: {"type":"response.output_item.added","output_index":0,"item":{"type":"custom_tool_call","call_id":"call-1","name":"apply_patch"}}"#,
        );
        let delta = adapter.parse_stream_line(
            r#"data: {"type":"response.custom_tool_call_input.delta","output_index":0,"item_id":"ctc-1","call_id":"call-1","delta":"*** Begin Patch\n"}"#,
        );

        assert!(matches!(
            &added[0],
            StreamEvent::ToolCallDelta {
                index: 0,
                id: Some(id),
                name: Some(name),
                arguments: None,
            } if id == "call-1" && name == "apply_patch"
        ));
        assert!(matches!(
            &delta[0],
            StreamEvent::ToolCallDelta {
                index: 0,
                id: Some(id),
                name: None,
                arguments: Some(arguments),
            } if id == "call-1" && arguments == "*** Begin Patch\n"
        ));
    }

    #[test]
    fn responses_parser_emits_canonical_completed_function_call() {
        let adapter = ResponsesAdapter;
        let events = adapter.parse_stream_line(
            r#"data: {"type":"response.output_item.done","output_index":2,"item":{"type":"function_call","call_id":"call-2","name":"list_directory","arguments":"{\"path\":\".\"}"}}"#,
        );

        assert!(matches!(
            &events[0],
            StreamEvent::ToolCallDone {
                index: 2,
                id: Some(id),
                name: Some(name),
                arguments: Some(arguments),
            } if id == "call-2"
                && name == "list_directory"
                && arguments == r#"{"path":"."}"#
        ));
    }

    #[test]
    fn responses_parser_surfaces_failed_and_incomplete_events() {
        let adapter = ResponsesAdapter;
        let failed = adapter.parse_stream_line(
            r#"data: {"type":"response.failed","response":{"error":{"code":"server_error","message":"upstream unavailable"}}}"#,
        );
        let incomplete = adapter.parse_stream_line(
            r#"data: {"type":"response.incomplete","response":{"incomplete_details":{"reason":"max_output_tokens"}}}"#,
        );

        assert!(matches!(
            &failed[0],
            StreamEvent::Error(message)
                if message == "LLM response failed (server_error): upstream unavailable"
        ));
        assert!(matches!(
            &incomplete[0],
            StreamEvent::Error(message)
                if message == "Incomplete response returned, reason: max_output_tokens"
        ));
    }

    #[test]
    fn responses_parser_accepts_tool_search_call_items() {
        let adapter = ResponsesAdapter;
        let added = adapter.parse_stream_line(
            r#"data: {"type":"response.output_item.added","output_index":0,"item":{"type":"tool_search_call","call_id":"search-1","execution":"client"}}"#,
        );
        let done = adapter.parse_stream_line(
            r#"data: {"type":"response.output_item.done","output_index":0,"item":{"type":"tool_search_call","call_id":"search-1","execution":"client","arguments":{"query":"browser automation","limit":2}}}"#,
        );

        assert!(matches!(
            &added[0],
            StreamEvent::ToolCallDelta {
                index: 0,
                id: Some(id),
                name: Some(name),
                arguments: None,
            } if id == "search-1" && name == "tool_search"
        ));
        assert!(matches!(
            &done[0],
            StreamEvent::ToolCallDelta {
                index: 0,
                id: Some(id),
                name: Some(name),
                arguments: Some(arguments),
            } if id == "search-1"
                && name == "tool_search"
                && arguments == "{\"limit\":2,\"query\":\"browser automation\"}"
        ));
    }

    #[test]
    fn responses_body_roundtrips_tool_search_items_when_specs_are_available() {
        let adapter = ResponsesAdapter;
        let tool_search_output = serde_json::json!({
            "query": "browser",
            "matches": [
                {
                    "type": "tool",
                    "name": "browser_run",
                    "spec": {
                        "type": "function",
                        "function": {
                            "name": "browser_run",
                            "description": "Run browser",
                            "parameters": {
                                "type": "object",
                                "properties": {}
                            }
                        },
                        "defer_loading": true
                    }
                },
                {
                    "type": "skill",
                    "name": "Browser",
                    "path": "codey/skills/browser/SKILL.md"
                }
            ]
        })
        .to_string();
        let messages = vec![
            InternalMessage {
                role: "assistant".to_string(),
                content: None,
                tool_calls: Some(vec![InternalToolCall {
                    id: "search-1".to_string(),
                    call_type: "function".to_string(),
                    function: InternalFunctionCall {
                        name: "tool_search".to_string(),
                        arguments: r#"{"query":"browser","limit":1}"#.to_string(),
                    },
                }]),
                tool_call_id: None,
                name: None,
            },
            InternalMessage {
                role: "tool".to_string(),
                content: text_content(tool_search_output),
                tool_calls: None,
                tool_call_id: Some("search-1".to_string()),
                name: Some("tool_search".to_string()),
            },
        ];

        let body = adapter.build_body("gpt-5", &messages, None, None);

        assert_eq!(
            body.pointer("/input/0/type")
                .and_then(serde_json::Value::as_str),
            Some("tool_search_call")
        );
        assert_eq!(
            body.pointer("/input/0/arguments/query")
                .and_then(serde_json::Value::as_str),
            Some("browser")
        );
        assert_eq!(
            body.pointer("/input/1/type")
                .and_then(serde_json::Value::as_str),
            Some("tool_search_output")
        );
        assert_eq!(
            body.pointer("/input/1/tools/0/name")
                .and_then(serde_json::Value::as_str),
            Some("browser_run")
        );
        assert_eq!(
            body.pointer("/input/1/tools/0/defer_loading")
                .and_then(serde_json::Value::as_bool),
            Some(true)
        );
    }

    #[test]
    fn responses_body_prefers_tool_search_output_tools_array() {
        let adapter = ResponsesAdapter;
        let tool_search_output = serde_json::json!({
            "query": "project docs",
            "matches": [],
            "tools": [
                {
                    "type": "namespace",
                    "name": "mcp__docs",
                    "description": "Tools in the mcp__docs namespace.",
                    "tools": [
                        {
                            "type": "function",
                            "name": "search",
                            "description": "Search project docs",
                            "strict": false,
                            "defer_loading": true,
                            "parameters": {
                                "type": "object",
                                "properties": {}
                            }
                        },
                        {
                            "type": "function",
                            "name": "read",
                            "description": "Read project docs",
                            "strict": false,
                            "defer_loading": true,
                            "parameters": {
                                "type": "object",
                                "properties": {}
                            }
                        }
                    ]
                }
            ]
        })
        .to_string();
        let messages = vec![InternalMessage {
            role: "tool".to_string(),
            content: text_content(tool_search_output),
            tool_calls: None,
            tool_call_id: Some("search-1".to_string()),
            name: Some("tool_search".to_string()),
        }];

        let body = adapter.build_body("gpt-5", &messages, None, None);

        assert_eq!(
            body.pointer("/input/0/type")
                .and_then(serde_json::Value::as_str),
            Some("tool_search_output")
        );
        assert_eq!(
            body.pointer("/input/0/tools/0/type")
                .and_then(serde_json::Value::as_str),
            Some("namespace")
        );
        assert_eq!(
            body.pointer("/input/0/tools/0/name")
                .and_then(serde_json::Value::as_str),
            Some("mcp__docs")
        );
        assert_eq!(
            body.pointer("/input/0/tools/0/tools/1/name")
                .and_then(serde_json::Value::as_str),
            Some("read")
        );
    }

    #[test]
    fn responses_body_keeps_tool_search_output_as_function_output_without_specs() {
        let adapter = ResponsesAdapter;
        let messages = vec![InternalMessage {
            role: "tool".to_string(),
            content: text_content(r#"{"query":"browser","matches":[]}"#),
            tool_calls: None,
            tool_call_id: Some("search-empty".to_string()),
            name: Some("tool_search".to_string()),
        }];

        let body = adapter.build_body("gpt-5", &messages, None, None);

        assert_eq!(
            body.pointer("/input/0/type")
                .and_then(serde_json::Value::as_str),
            Some("function_call_output")
        );
    }

    #[test]
    fn responses_parser_reads_non_streaming_output_and_usage() {
        let adapter = ResponsesAdapter;
        let output = adapter
            .parse_non_streaming(
                r#"{
                    "status":"completed",
                    "output_text":"Done",
                    "output":[
                      {"type":"function_call","call_id":"call-1","name":"read_file","arguments":"{\"path\":\"README.md\"}"},
                      {"type":"custom_tool_call","call_id":"call-2","name":"apply_patch","input":"*** Begin Patch\n*** End Patch"},
                      {"type":"tool_search_call","call_id":"call-3","arguments":{"query":"browser","limit":2}}
                    ],
                    "usage":{"input_tokens":11,"output_tokens":7,"total_tokens":18,"input_tokens_details":{"cached_tokens":3},"output_tokens_details":{"reasoning_tokens":2}}
                }"#,
            )
            .unwrap();

        assert_eq!(output.text, "Done");
        assert_eq!(output.tool_calls.len(), 3);
        assert_eq!(output.tool_calls[0].name, "read_file");
        assert_eq!(
            output.tool_calls[1].arguments,
            "*** Begin Patch\n*** End Patch"
        );
        assert_eq!(output.tool_calls[2].name, "tool_search");
        assert_eq!(
            output.usage.as_ref().map(|usage| usage.prompt_tokens),
            Some(11)
        );
        assert_eq!(
            output.usage.as_ref().map(|usage| usage.cached_tokens),
            Some(3)
        );
        assert_eq!(
            output.usage.as_ref().map(|usage| usage.reasoning_tokens),
            Some(2)
        );
    }

    #[test]
    fn responses_parser_rejects_incomplete_non_streaming_response() {
        let error = ResponsesAdapter
            .parse_non_streaming(
                r#"{"status":"incomplete","incomplete_details":{"reason":"max_output_tokens"}}"#,
            )
            .unwrap_err();
        assert!(error.contains("incomplete"));
        assert!(error.contains("max_output_tokens"));
    }
}
