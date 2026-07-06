//! OpenAI Responses API adapter (wire_api = "responses").
//! Converts CN-Codex's internal message/tool shape into Responses API wire
//! payloads and parses streamed Responses API events back into internal events.

use async_trait::async_trait;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};

use super::ProviderAdapter;
use super::types::{
    InternalMessage, StreamEvent, UsageInfo, content_as_text, content_to_responses_content,
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
            "max_output_tokens": max_tokens.unwrap_or(131072),
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
        line.trim() == "data: [DONE]"
    }

    fn parse_stream_line(&self, line: &str) -> Vec<StreamEvent> {
        let mut events = Vec::new();

        let data = match line.strip_prefix("data: ") {
            Some(d) => d.trim(),
            None => return events,
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
                    if item_type == "tool_search_call" {
                        events.push(tool_search_call_event(&parsed, item, true));
                    }
                }
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
            "description": "Use the `apply_patch` tool to edit files. This is a FREEFORM tool, so do not wrap the patch in JSON.",
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::types::{InternalFunctionCall, InternalToolCall, text_content};

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
}
