use serde_json::Value;

use super::{ToolCallRequest, sanitize_tool_name};

/// tool call 累积器（逐步拼接 SSE 中的碎片）
#[derive(Default)]
pub(crate) struct ToolCallAccumulator {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) arguments: String,
}

const THINK_OPEN_TAG: &str = "<think>";
const THINK_CLOSE_TAG: &str = "</think>";
const DSML_TOOL_CALLS_OPEN_TAG: &str = "<｜｜DSML｜｜tool_calls>";
const DSML_TOOL_CALLS_CLOSE_TAG: &str = "</｜｜DSML｜｜tool_calls>";
const DSML_INVOKE_OPEN_TAG: &str = "<｜｜DSML｜｜invoke";
const DSML_INVOKE_CLOSE_TAG: &str = "</｜｜DSML｜｜invoke>";
const DSML_PARAMETER_OPEN_TAG: &str = "<｜｜DSML｜｜parameter";
const DSML_PARAMETER_CLOSE_TAG: &str = "</｜｜DSML｜｜parameter>";

#[derive(Clone, Copy)]
enum ProtocolToken {
    ThinkOpen,
    ThinkClose,
    DsmlOpen,
    DsmlClose,
}

#[derive(Default)]
pub(crate) struct ProtocolStreamState {
    pub(crate) inside_think: bool,
    pub(crate) inside_dsml: bool,
    pub(crate) pending: String,
    pub(crate) dsml_buffer: String,
}

#[derive(Default)]
pub(crate) struct ProtocolDeltaResult {
    pub(crate) visible: String,
    pub(crate) reasoning: String,
    pub(crate) dsml_blocks: Vec<String>,
}

fn next_protocol_token(text: &str) -> Option<(usize, ProtocolToken)> {
    [
        (THINK_OPEN_TAG, ProtocolToken::ThinkOpen),
        (THINK_CLOSE_TAG, ProtocolToken::ThinkClose),
        (DSML_TOOL_CALLS_OPEN_TAG, ProtocolToken::DsmlOpen),
        (DSML_TOOL_CALLS_CLOSE_TAG, ProtocolToken::DsmlClose),
    ]
    .iter()
    .filter_map(|(tag, token)| text.find(tag).map(|pos| (pos, *token)))
    .min_by_key(|(pos, _)| *pos)
}

fn split_trailing_partial_tag<'a>(text: &'a str, tags: &[&str]) -> (&'a str, &'a str) {
    let mut best_len = 0_usize;
    for tag in tags {
        for (prefix_len, _) in tag.char_indices().skip(1) {
            let prefix = &tag[..prefix_len];
            if text.ends_with(prefix) && prefix_len > best_len {
                best_len = prefix_len;
            }
        }
    }
    if best_len == 0 {
        (text, "")
    } else {
        text.split_at(text.len().saturating_sub(best_len))
    }
}

pub(crate) fn consume_protocol_text_delta(
    state: &mut ProtocolStreamState,
    chunk: &str,
) -> ProtocolDeltaResult {
    let mut result = ProtocolDeltaResult::default();
    let mut input = String::new();
    if !state.pending.is_empty() {
        input.push_str(&state.pending);
        state.pending.clear();
    }
    input.push_str(chunk);

    let mut rest = input.as_str();
    while !rest.is_empty() {
        if state.inside_dsml {
            if let Some(close_pos) = rest.find(DSML_TOOL_CALLS_CLOSE_TAG) {
                state.dsml_buffer.push_str(&rest[..close_pos]);
                if !state.dsml_buffer.trim().is_empty() {
                    result
                        .dsml_blocks
                        .push(std::mem::take(&mut state.dsml_buffer));
                } else {
                    state.dsml_buffer.clear();
                }
                state.inside_dsml = false;
                rest = &rest[close_pos + DSML_TOOL_CALLS_CLOSE_TAG.len()..];
                continue;
            }
            let (emit, pending) = split_trailing_partial_tag(rest, &[DSML_TOOL_CALLS_CLOSE_TAG]);
            state.dsml_buffer.push_str(emit);
            state.pending = pending.to_string();
            break;
        }

        if state.inside_think {
            if let Some(close_pos) = rest.find(THINK_CLOSE_TAG) {
                result.reasoning.push_str(&rest[..close_pos]);
                state.inside_think = false;
                rest = &rest[close_pos + THINK_CLOSE_TAG.len()..];
                continue;
            }
            let (emit, pending) = split_trailing_partial_tag(rest, &[THINK_CLOSE_TAG]);
            result.reasoning.push_str(emit);
            state.pending = pending.to_string();
            break;
        }

        if let Some((token_pos, token)) = next_protocol_token(rest) {
            result.visible.push_str(&rest[..token_pos]);
            rest = &rest[token_pos..];
            match token {
                ProtocolToken::ThinkOpen => {
                    state.inside_think = true;
                    rest = &rest[THINK_OPEN_TAG.len()..];
                }
                ProtocolToken::DsmlOpen => {
                    state.inside_dsml = true;
                    state.dsml_buffer.clear();
                    rest = &rest[DSML_TOOL_CALLS_OPEN_TAG.len()..];
                }
                ProtocolToken::ThinkClose => {
                    rest = &rest[THINK_CLOSE_TAG.len()..];
                }
                ProtocolToken::DsmlClose => {
                    rest = &rest[DSML_TOOL_CALLS_CLOSE_TAG.len()..];
                }
            }
            continue;
        }

        let (emit, pending) = split_trailing_partial_tag(
            rest,
            &[
                THINK_OPEN_TAG,
                THINK_CLOSE_TAG,
                DSML_TOOL_CALLS_OPEN_TAG,
                DSML_TOOL_CALLS_CLOSE_TAG,
            ],
        );
        result.visible.push_str(emit);
        state.pending = pending.to_string();
        break;
    }

    result
}

pub(crate) fn flush_protocol_stream_state(state: &mut ProtocolStreamState) -> ProtocolDeltaResult {
    let mut result = ProtocolDeltaResult::default();
    if state.inside_dsml {
        if !state.pending.is_empty() {
            state.dsml_buffer.push_str(&state.pending);
            state.pending.clear();
        }
        if !state.dsml_buffer.trim().is_empty() {
            result
                .dsml_blocks
                .push(std::mem::take(&mut state.dsml_buffer));
        } else {
            state.dsml_buffer.clear();
        }
        state.inside_dsml = false;
        return result;
    }
    if state.inside_think {
        if !state.pending.is_empty() {
            result.reasoning.push_str(&state.pending);
            state.pending.clear();
        }
        state.inside_think = false;
        return result;
    }
    if !state.pending.is_empty() {
        result.visible.push_str(&state.pending);
        state.pending.clear();
    }
    result
}

pub(crate) fn parse_protocol_text(text: &str) -> ProtocolDeltaResult {
    let mut state = ProtocolStreamState::default();
    let mut result = consume_protocol_text_delta(&mut state, text);
    let tail = flush_protocol_stream_state(&mut state);
    result.visible.push_str(&tail.visible);
    result.reasoning.push_str(&tail.reasoning);
    result.dsml_blocks.extend(tail.dsml_blocks);
    result
}

fn extract_tag_attr(tag: &str, attr: &str) -> Option<String> {
    let needle = format!("{attr}=\"");
    let start = tag.find(&needle)? + needle.len();
    let value = &tag[start..];
    let end = value.find('"')?;
    Some(value[..end].to_string())
}

pub(crate) fn parse_dsml_tool_calls_block(block: &str) -> Vec<ToolCallRequest> {
    let mut calls = Vec::new();
    let mut rest = block;
    let mut invoke_index = 0_u32;

    while let Some(invoke_start) = rest.find(DSML_INVOKE_OPEN_TAG) {
        rest = &rest[invoke_start..];
        let Some(invoke_tag_end) = rest.find('>') else {
            break;
        };
        let invoke_tag = &rest[..=invoke_tag_end];
        let Some(tool_name) = extract_tag_attr(invoke_tag, "name") else {
            rest = &rest[invoke_tag_end + 1..];
            continue;
        };

        let invoke_body_start = invoke_tag_end + 1;
        let Some(invoke_close_pos) = rest[invoke_body_start..].find(DSML_INVOKE_CLOSE_TAG) else {
            break;
        };
        let invoke_body_end = invoke_body_start + invoke_close_pos;
        let invoke_body = &rest[invoke_body_start..invoke_body_end];

        let mut arguments = serde_json::Map::new();
        let mut body_rest = invoke_body;
        while let Some(parameter_start) = body_rest.find(DSML_PARAMETER_OPEN_TAG) {
            body_rest = &body_rest[parameter_start..];
            let Some(parameter_tag_end) = body_rest.find('>') else {
                break;
            };
            let parameter_tag = &body_rest[..=parameter_tag_end];
            let Some(parameter_name) = extract_tag_attr(parameter_tag, "name") else {
                body_rest = &body_rest[parameter_tag_end + 1..];
                continue;
            };
            let is_string_parameter = extract_tag_attr(parameter_tag, "string")
                .map(|value| value.eq_ignore_ascii_case("true"))
                .unwrap_or(false);
            let parameter_body_start = parameter_tag_end + 1;
            let Some(parameter_close_pos) =
                body_rest[parameter_body_start..].find(DSML_PARAMETER_CLOSE_TAG)
            else {
                break;
            };
            let parameter_body_end = parameter_body_start + parameter_close_pos;
            let parameter_value_text = body_rest[parameter_body_start..parameter_body_end].trim();
            let parameter_value = if is_string_parameter {
                Value::String(parameter_value_text.to_string())
            } else {
                serde_json::from_str::<Value>(parameter_value_text)
                    .unwrap_or_else(|_| Value::String(parameter_value_text.to_string()))
            };
            arguments.insert(parameter_name, parameter_value);
            body_rest = &body_rest[parameter_body_end + DSML_PARAMETER_CLOSE_TAG.len()..];
        }

        invoke_index = invoke_index.saturating_add(1);
        calls.push(ToolCallRequest {
            id: format!("dsml_{}_{}", sanitize_tool_name(&tool_name), invoke_index),
            name: tool_name,
            arguments: Value::Object(arguments).to_string(),
        });
        rest = &rest[invoke_body_end + DSML_INVOKE_CLOSE_TAG.len()..];
    }

    calls
}
