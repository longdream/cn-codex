use super::*;

pub(crate) fn echarts_report_display(args: &EchartsReportArgs) -> String {
    if let Some(title) = args
        .title
        .as_deref()
        .map(condense_whitespace)
        .filter(|value| !value.is_empty())
    {
        if title.chars().count() > 64 {
            return format!("{}...", title.chars().take(64).collect::<String>());
        }
        return title;
    }

    if let Some(chart_type) = args
        .chart_type
        .as_deref()
        .map(condense_whitespace)
        .filter(|value| !value.is_empty())
    {
        return chart_type;
    }

    "echarts_report".to_string()
}


pub(crate) fn format_plan_update(explanation: Option<&str>, plan: &[PlanItemArg]) -> Result<String, String> {
    if plan.is_empty() {
        return Err("Error: update_plan requires at least one plan item".to_string());
    }

    let mut in_progress = 0usize;
    let mut normalized = Vec::with_capacity(plan.len());
    for item in plan {
        let step = item.step.trim();
        if step.is_empty() {
            // Models sometimes re-insert a blank placeholder when revising a plan.
            // Skip it instead of failing the whole update.
            continue;
        }
        let status = normalize_plan_status(&item.status);
        if status == "in_progress" {
            in_progress += 1;
        }
        normalized.push(PlanItemArg {
            step: step.to_string(),
            status,
        });
    }

    if normalized.is_empty() {
        return Err("Error: update_plan requires at least one plan item".to_string());
    }

    if in_progress > 1 {
        // Keep the last in-progress item when the model re-inserts a step
        // without first demoting the previous one.
        let mut seen_in_progress = false;
        for item in normalized.iter_mut().rev() {
            if item.status != "in_progress" {
                continue;
            }
            if seen_in_progress {
                item.status = "pending".to_string();
            } else {
                seen_in_progress = true;
            }
        }
    }

    let mut output = String::from("Plan updated");
    if let Some(explanation) = explanation.map(str::trim).filter(|value| !value.is_empty()) {
        output.push_str(": ");
        output.push_str(explanation);
    }
    output.push('\n');

    for item in &normalized {
        output.push_str(&format!("- [{}] {}\n", item.status, item.step));
    }

    Ok(output.trim_end().to_string())
}

fn normalize_plan_status(value: &str) -> String {
    match value.trim().to_ascii_lowercase().as_str() {
        "completed" | "complete" | "done" | "finished" => "completed".to_string(),
        "in_progress" | "in-progress" | "inprogress" | "active" | "doing" | "current" => {
            "in_progress".to_string()
        }
        _ => "pending".to_string(),
    }
}


pub(crate) fn format_echarts_report(args: &EchartsReportArgs) -> Result<String, String> {
    if !args.option.is_object() {
        return Err("Error: echarts_report option must be a JSON object".to_string());
    }

    let title = args
        .title
        .as_deref()
        .map(condense_whitespace)
        .filter(|value| !value.is_empty());
    let chart_type = args
        .chart_type
        .as_deref()
        .map(condense_whitespace)
        .filter(|value| !value.is_empty());
    let notes = args
        .notes
        .as_deref()
        .map(condense_whitespace)
        .filter(|value| !value.is_empty());
    let option_json = serde_json::to_string_pretty(&args.option)
        .map_err(|e| format!("Error: echarts_report option serialization failed: {e}"))?;

    let mut output = String::from("ECharts report ready");
    if let Some(title) = title.as_deref() {
        output.push_str(": ");
        output.push_str(title);
    }
    if let Some(chart_type) = chart_type.as_deref() {
        output.push_str("\nChart type: ");
        output.push_str(chart_type);
    }
    if let Some(notes) = notes.as_deref() {
        output.push_str("\nNotes: ");
        output.push_str(notes);
    }

    output.push_str("\nOption JSON:\n");
    output.push_str(&option_json);
    output.push_str("\n\n```echarts\n");
    output.push_str(&option_json);
    output.push_str("\n```");
    Ok(output)
}


pub(crate) fn format_json_value(value: &serde_json::Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
}


pub(crate) fn option_is_recommended(option: &RequestUserInputQuestionOption) -> bool {
    let label_lower = option.label.to_ascii_lowercase();
    let desc_lower = option.description.to_ascii_lowercase();
    label_lower.contains("recommended")
        || desc_lower.contains("recommended")
        || option.label.contains("推荐")
        || option.description.contains("推荐")
}


pub(crate) fn normalize_request_user_input_args(args: &mut RequestUserInputArgs) {
    for question in &mut args.questions {
        if question.options.len() <= 1 {
            continue;
        }
        if let Some(index) = question.options.iter().position(option_is_recommended) {
            if index > 0 {
                let option = question.options.remove(index);
                question.options.insert(0, option);
            }
        }
    }
}


pub(crate) fn validate_request_user_input_args(args: &RequestUserInputArgs) -> Result<(), String> {
    if args.questions.is_empty() {
        return Err("Error: request_user_input requires at least one question".to_string());
    }
    if args.questions.len() > 3 {
        return Err("Error: request_user_input supports at most three questions".to_string());
    }

    for question in &args.questions {
        if question.id.trim().is_empty() {
            return Err("Error: request_user_input question id must not be empty".to_string());
        }
        if question.header.trim().is_empty() {
            return Err(format!(
                "Error: request_user_input question '{}' header must not be empty",
                question.id
            ));
        }
        if question.question.trim().is_empty() {
            return Err(format!(
                "Error: request_user_input question '{}' prompt must not be empty",
                question.id
            ));
        }
        if question.options.len() > 3 {
            return Err(format!(
                "Error: request_user_input question '{}' supports at most three options",
                question.id
            ));
        }
        for option in &question.options {
            if option.label.trim().is_empty() {
                return Err(format!(
                    "Error: request_user_input question '{}' option label must not be empty",
                    question.id
                ));
            }
            if option.description.trim().is_empty() {
                return Err(format!(
                    "Error: request_user_input question '{}' option description must not be empty",
                    question.id
                ));
            }
        }
    }

    Ok(())
}


pub(crate) fn validate_request_permissions_args(args: &RequestPermissionsArgs) -> Result<(), String> {
    let Some(object) = args.permissions.as_object() else {
        return Err("Error: request_permissions permissions must be an object".to_string());
    };

    if object.is_empty() {
        return Err("Error: request_permissions requires at least one permission".to_string());
    }

    let has_known_permission = object.get("network").is_some_and(|value| !value.is_null())
        || object
            .get("file_system")
            .or_else(|| object.get("fileSystem"))
            .is_some_and(|value| !value.is_null());
    if !has_known_permission {
        return Err(
            "Error: request_permissions requires network or file_system permissions".to_string(),
        );
    }

    Ok(())
}


pub(crate) fn granted_permissions_from_result(result: &serde_json::Value) -> Option<serde_json::Value> {
    let permissions = result.get("permissions")?;
    if !permissions.is_object()
        || permissions
            .as_object()
            .is_some_and(|object| object.is_empty())
    {
        return None;
    }
    Some(permissions.clone())
}


pub(crate) fn permission_profile_covers(granted: &serde_json::Value, requested: &serde_json::Value) -> bool {
    match (granted, requested) {
        (serde_json::Value::Object(granted), serde_json::Value::Object(requested)) => {
            requested.iter().all(|(key, requested_value)| {
                granted.get(key).is_some_and(|granted_value| {
                    permission_profile_covers(granted_value, requested_value)
                })
            })
        }
        (serde_json::Value::Array(granted), serde_json::Value::Array(requested)) => {
            requested.iter().all(|requested_value| {
                granted
                    .iter()
                    .any(|granted_value| granted_value == requested_value)
            })
        }
        _ => granted == requested,
    }
}


pub(crate) fn request_id_matches(left: &RequestId, right: &RequestId) -> bool {
    match (left, right) {
        (RequestId::Integer(left), RequestId::Integer(right)) => left == right,
        (RequestId::String(left), RequestId::String(right)) => left == right,
        (RequestId::Integer(left), RequestId::String(right)) => left.to_string() == right.as_str(),
        (RequestId::String(left), RequestId::Integer(right)) => left.as_str() == right.to_string(),
    }
}


pub(crate) async fn wait_for_approval_result(
    app_handle: &AppHandle,
    request_id: &RequestId,
    timeout_ms: u64,
) -> Result<serde_json::Value, String> {
    let state = app_handle.state::<AppState>();
    let mut receiver = {
        let mut guard = state.approval_rx.write().await;
        guard
            .take()
            .ok_or_else(|| "approval response receiver is already in use".to_string())?
    };

    let wait_result = tokio::time::timeout(Duration::from_millis(timeout_ms), async {
        loop {
            let Some(action) = receiver.recv().await else {
                return Err("approval response channel closed".to_string());
            };

            match action {
                ApprovalAction::Resolve {
                    request_id: response_id,
                    result,
                } if request_id_matches(&response_id, request_id) => {
                    return Ok(result);
                }
                ApprovalAction::Reject {
                    request_id: response_id,
                    error,
                } if request_id_matches(&response_id, request_id) => {
                    return Err(error.message);
                }
                _ => {}
            }
        }
    })
    .await
    .unwrap_or_else(|_| Err("timed out waiting for user input".to_string()));

    let mut guard = state.approval_rx.write().await;
    *guard = Some(receiver);

    wait_result
}


/// 供 agent.rs 等模块在 approval 通道上等待用户回复（含超时）。
pub(crate) async fn wait_for_approval_result_public(
    app_handle: &AppHandle,
    request_id: &RequestId,
    timeout_ms: u64,
) -> Result<serde_json::Value, String> {
    wait_for_approval_result(app_handle, request_id, timeout_ms).await
}

impl ToolExecutor {
    pub(crate) async fn exec_update_plan(
        &self,
        arguments: &str,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
    ) -> AppResult<String> {
        let args: PlanUpdateArgs = match serde_json::from_str(arguments) {
            Ok(args) => args,
            Err(e) => {
                let msg = format!("Invalid update_plan args: {e}");
                self.emit_tool_start(app_handle, thread_id, call_id, "update_plan", "invalid");
                self.emit_tool_end(app_handle, thread_id, call_id, "update_plan", -1, &msg);
                return Ok(msg);
            }
        };

        let display = format!("{} steps", args.plan.len());
        self.emit_tool_start(app_handle, thread_id, call_id, "update_plan", &display);

        let result = format_plan_update(args.explanation.as_deref(), &args.plan);
        let (exit_code, output) = match result {
            Ok(output) => (0, output),
            Err(msg) => (-1, msg),
        };
        self.emit_tool_end(
            app_handle,
            thread_id,
            call_id,
            "update_plan",
            exit_code,
            &output,
        );
        Ok(output)
    }


    pub(crate) async fn exec_echarts_report(
        &self,
        arguments: &str,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
    ) -> AppResult<String> {
        let args: EchartsReportArgs = match serde_json::from_str(arguments) {
            Ok(args) => args,
            Err(e) => {
                let msg = format!("Invalid echarts_report args: {e}");
                self.emit_tool_start(app_handle, thread_id, call_id, "echarts_report", "invalid");
                self.emit_tool_end(app_handle, thread_id, call_id, "echarts_report", -1, &msg);
                return Ok(msg);
            }
        };

        let display = echarts_report_display(&args);
        self.emit_tool_start(app_handle, thread_id, call_id, "echarts_report", &display);

        let result = format_echarts_report(&args);
        let (exit_code, output) = match result {
            Ok(output) => (0, output),
            Err(msg) => (-1, msg),
        };
        self.emit_tool_end(
            app_handle,
            thread_id,
            call_id,
            "echarts_report",
            exit_code,
            &output,
        );
        Ok(output)
    }


    pub(crate) async fn exec_request_user_input(
        &self,
        arguments: &str,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
    ) -> AppResult<String> {
        let mut args: RequestUserInputArgs = match serde_json::from_str(arguments) {
            Ok(args) => args,
            Err(e) => {
                let msg = format!("Invalid request_user_input args: {e}");
                self.emit_tool_start(
                    app_handle,
                    thread_id,
                    call_id,
                    "request_user_input",
                    "invalid",
                );
                self.emit_tool_end(
                    app_handle,
                    thread_id,
                    call_id,
                    "request_user_input",
                    -1,
                    &msg,
                );
                return Ok(msg);
            }
        };

        let display = format!("{} question(s)", args.questions.len());
        self.emit_tool_start(
            app_handle,
            thread_id,
            call_id,
            "request_user_input",
            &display,
        );

        if let Err(msg) = validate_request_user_input_args(&args) {
            self.emit_tool_end(
                app_handle,
                thread_id,
                call_id,
                "request_user_input",
                -1,
                &msg,
            );
            return Ok(msg);
        }
        normalize_request_user_input_args(&mut args);

        let request_id = RequestId::String(call_id.to_string());
        app_handle
            .emit(
                "server-request",
                serde_json::json!({
                    "requestId": call_id,
                    "id": call_id,
                    "method": "request_user_input",
                    "params": {
                        "threadId": thread_id,
                        "callId": call_id,
                        "questions": args.questions,
                    },
                }),
            )
            .ok();

        let output = match wait_for_approval_result(app_handle, &request_id, 600_000).await {
            Ok(result) => {
                if let Some(profile) = granted_permissions_from_result(&result) {
                    self.remember_permission_grant(profile).await;
                }
                let output =
                    serde_json::to_string_pretty(&result).unwrap_or_else(|_| result.to_string());
                self.emit_tool_end(
                    app_handle,
                    thread_id,
                    call_id,
                    "request_user_input",
                    0,
                    &output,
                );
                output
            }
            Err(msg) => {
                let output = format!("request_user_input failed: {msg}");
                self.emit_tool_end(
                    app_handle,
                    thread_id,
                    call_id,
                    "request_user_input",
                    -1,
                    &output,
                );
                output
            }
        };

        Ok(output)
    }


    pub(crate) async fn exec_request_permissions(
        &self,
        arguments: &str,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
    ) -> AppResult<String> {
        let args: RequestPermissionsArgs = match serde_json::from_str(arguments) {
            Ok(args) => args,
            Err(e) => {
                let msg = format!("Invalid request_permissions args: {e}");
                self.emit_tool_start(
                    app_handle,
                    thread_id,
                    call_id,
                    "request_permissions",
                    "invalid",
                );
                self.emit_tool_end(
                    app_handle,
                    thread_id,
                    call_id,
                    "request_permissions",
                    -1,
                    &msg,
                );
                return Ok(msg);
            }
        };

        let display = args
            .reason
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("permissions");
        self.emit_tool_start(
            app_handle,
            thread_id,
            call_id,
            "request_permissions",
            display,
        );

        if let Err(msg) = validate_request_permissions_args(&args) {
            self.emit_tool_end(
                app_handle,
                thread_id,
                call_id,
                "request_permissions",
                -1,
                &msg,
            );
            return Ok(msg);
        }

        let request_id = RequestId::String(call_id.to_string());
        app_handle
            .emit(
                "server-request",
                serde_json::json!({
                    "requestId": call_id,
                    "id": call_id,
                    "method": "request_permissions",
                    "params": {
                        "threadId": thread_id,
                        "callId": call_id,
                        "environmentId": args.environment_id,
                        "startedAtMs": now_millis(),
                        "reason": args.reason,
                        "permissions": args.permissions,
                        "cwd": self.cwd.to_string_lossy(),
                    },
                }),
            )
            .ok();

        let output = match wait_for_approval_result(app_handle, &request_id, 600_000).await {
            Ok(result) => {
                let output =
                    serde_json::to_string_pretty(&result).unwrap_or_else(|_| result.to_string());
                self.emit_tool_end(
                    app_handle,
                    thread_id,
                    call_id,
                    "request_permissions",
                    0,
                    &output,
                );
                output
            }
            Err(msg) => {
                let output = format!("request_permissions failed: {msg}");
                self.emit_tool_end(
                    app_handle,
                    thread_id,
                    call_id,
                    "request_permissions",
                    -1,
                    &output,
                );
                output
            }
        };

        Ok(output)
    }

}
