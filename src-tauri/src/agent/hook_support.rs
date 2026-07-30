use super::*;

pub(crate) fn tool_call_hook_context(turn_id: &str, call: &ToolCallRequest) -> serde_json::Value {
    let parsed_args = serde_json::from_str::<serde_json::Value>(&call.arguments).ok();
    let command_display = parsed_args
        .as_ref()
        .and_then(|value| value.get("command"))
        .map(|command| {
            if let Some(items) = command.as_array() {
                items
                    .iter()
                    .filter_map(serde_json::Value::as_str)
                    .collect::<Vec<_>>()
                    .join(" ")
            } else {
                command.as_str().unwrap_or_default().to_string()
            }
        })
        .filter(|command| !command.trim().is_empty());

    serde_json::json!({
        "turnId": turn_id,
        "toolCallId": &call.id,
        "toolName": &call.name,
        "arguments": parsed_args.unwrap_or_else(|| serde_json::Value::String(call.arguments.clone())),
        "command": command_display,
    })
}


pub(crate) fn tool_result_hook_context(
    turn_id: &str,
    call: &ToolCallRequest,
    output: &str,
    success: bool,
) -> serde_json::Value {
    let parsed_args = serde_json::from_str::<serde_json::Value>(&call.arguments).ok();
    let command_display = parsed_args
        .as_ref()
        .and_then(|value| value.get("command"))
        .map(|command| {
            if let Some(items) = command.as_array() {
                items
                    .iter()
                    .filter_map(serde_json::Value::as_str)
                    .collect::<Vec<_>>()
                    .join(" ")
            } else {
                command.as_str().unwrap_or_default().to_string()
            }
        })
        .filter(|command| !command.trim().is_empty());

    serde_json::json!({
        "turnId": turn_id,
        "toolCallId": &call.id,
        "toolName": &call.name,
        "arguments": parsed_args.unwrap_or_else(|| serde_json::Value::String(call.arguments.clone())),
        "command": command_display,
        "success": success,
        "output": output,
    })
}


pub(crate) fn blocked_tool_call_output(tool_name: &str, hook: &HookRunResult) -> String {
    let reason = hook
        .reason
        .as_deref()
        .map(str::trim)
        .filter(|reason| !reason.is_empty())
        .unwrap_or("blocked by hook");
    format!(
        "Tool call blocked by PreToolUse hook: {reason}. Tool: {tool_name}. Hook: {} ({})",
        hook.command, hook.source_name
    )
}


pub(crate) fn user_prompt_submit_hook_context(
    turn_id: &str,
    mode: &str,
    cwd: &Path,
    model: &str,
    prompt: &str,
) -> serde_json::Value {
    serde_json::json!({
        "hookEventName": "UserPromptSubmit",
        "turnId": turn_id,
        "mode": mode,
        "cwd": cwd.to_string_lossy(),
        "model": model,
        "permissionMode": "default",
        "prompt": prompt,
    })
}


pub(crate) fn format_user_prompt_submit_hook_feedback(feedback: Vec<String>, blocked: bool) -> String {
    let heading = if blocked {
        "[UserPromptSubmit hook blocked prompt]"
    } else {
        "[UserPromptSubmit hook context]"
    };
    format!("{heading}\n{}", feedback.join("\n"))
}


#[allow(clippy::too_many_arguments)]
pub(crate) fn stop_hook_context(
    turn_id: &str,
    mode: &str,
    cwd: &Path,
    duration_ms: u64,
    changed_files: &[FileChange],
    usage: &TurnUsage,
    goal_budget_tokens: Option<u64>,
    budget_limited: bool,
    model: &str,
    stop_hook_active: bool,
    last_assistant_message: &str,
) -> serde_json::Value {
    let last_assistant_message = if last_assistant_message.trim().is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::Value::String(last_assistant_message.to_string())
    };

    serde_json::json!({
        "hookEventName": "Stop",
        "turnId": turn_id,
        "mode": mode,
        "cwd": cwd.to_string_lossy(),
        "durationMs": duration_ms,
        "changedFiles": changed_files,
        "usage": nonzero_turn_usage(usage),
        "goalBudgetTokens": goal_budget_tokens,
        "budgetLimited": budget_limited,
        "model": model,
        "permissionMode": "default",
        "stopHookActive": stop_hook_active,
        "lastAssistantMessage": last_assistant_message,
    })
}


pub(crate) fn stop_hook_continuation_message(results: &[HookRunResult]) -> Option<String> {
    let feedback = hook_feedback_for_model(results);
    if feedback.is_empty() {
        return None;
    }

    Some(format!("[Stop hook continuation]\n{}", feedback.join("\n")))
}


pub(crate) fn subagent_stop_hook_context(
    turn_id: &str,
    mode: &str,
    cwd: &Path,
    model: &str,
    call: &ToolCallRequest,
    close_output: &str,
) -> serde_json::Value {
    let args = serde_json::from_str::<serde_json::Value>(&call.arguments).ok();
    let close_result = serde_json::from_str::<serde_json::Value>(close_output)
        .unwrap_or_else(|_| serde_json::Value::String(close_output.to_string()));
    let agent = close_result
        .get("agent")
        .and_then(serde_json::Value::as_object);

    let agent_id = agent
        .and_then(|agent| agent.get("id"))
        .and_then(serde_json::Value::as_str)
        .or_else(|| {
            close_result
                .get("target")
                .and_then(serde_json::Value::as_str)
        })
        .or_else(|| {
            args.as_ref()
                .and_then(|args| args.get("target"))
                .or_else(|| args.as_ref().and_then(|args| args.get("agent_id")))
                .or_else(|| args.as_ref().and_then(|args| args.get("id")))
                .and_then(serde_json::Value::as_str)
        })
        .unwrap_or("unknown");
    let agent_type = agent
        .and_then(|agent| agent.get("role"))
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("subagent");
    let last_assistant_message = agent
        .and_then(|agent| agent.get("output"))
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let agent_transcript_path = cwd
        .join("codey")
        .join("subagents")
        .join(agent_id)
        .join("last-message.txt");

    serde_json::json!({
        "hookEventName": "SubagentStop",
        "turnId": turn_id,
        "mode": mode,
        "cwd": cwd.to_string_lossy(),
        "model": model,
        "permissionMode": "default",
        "stopHookActive": false,
        "agentId": agent_id,
        "agent_id": agent_id,
        "agentType": agent_type,
        "agent_type": agent_type,
        "agentTranscriptPath": agent_transcript_path,
        "agent_transcript_path": agent_transcript_path,
        "lastAssistantMessage": last_assistant_message
            .map(|value| serde_json::Value::String(value.to_string()))
            .unwrap_or(serde_json::Value::Null),
        "closeResult": close_result,
    })
}


pub(crate) fn append_subagent_stop_hook_feedback(output: String, feedback: Vec<String>) -> String {
    if feedback.is_empty() {
        return output;
    }

    let mut combined = output;
    if !combined.ends_with('\n') {
        combined.push('\n');
    }
    combined.push_str("\n[SubagentStop hook feedback]\n");
    combined.push_str(&feedback.join("\n"));
    combined
}


pub(crate) fn append_post_tool_hook_feedback(output: String, feedback: Vec<String>) -> String {
    if feedback.is_empty() {
        return output;
    }

    let mut combined = output;
    if !combined.ends_with('\n') {
        combined.push('\n');
    }
    combined.push_str("\n[PostToolUse hook feedback]\n");
    combined.push_str(&feedback.join("\n"));
    combined
}


