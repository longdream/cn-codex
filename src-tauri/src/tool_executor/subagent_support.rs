use super::*;

pub(crate) fn subagent_state_path(workspace_config_dir: &Path) -> PathBuf {
    workspace_config_dir.join("subagents").join("state.json")
}


pub(crate) fn load_subagent_records(workspace_config_dir: &Path) -> HashMap<String, SubagentRecord> {
    let path = subagent_state_path(workspace_config_dir);
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(_) => return HashMap::new(),
    };
    let mut records = match serde_json::from_slice::<Vec<SubagentRecord>>(&bytes) {
        Ok(records) => records,
        Err(_) => return HashMap::new(),
    };
    let loaded_at_ms = now_millis();
    for record in &mut records {
        record.process_id = None;
        if record.status == "running" {
            record.status = "interrupted".to_string();
            record.completed_at_ms.get_or_insert(loaded_at_ms);
            record
                .duration_ms
                .get_or_insert_with(|| loaded_at_ms.saturating_sub(record.started_at_ms));
            record.error.get_or_insert_with(|| {
                "Subagent was running when CN-Codex last stopped; process state could not be restored."
                    .to_string()
            });
        }
    }
    records
        .into_iter()
        .map(|record| (record.id.clone(), record))
        .collect()
}


pub(crate) async fn persist_subagent_records(
    workspace_config_dir: &Path,
    subagents: &Arc<Mutex<HashMap<String, SubagentRecord>>>,
) {
    let mut records = subagents.lock().await.values().cloned().collect::<Vec<_>>();
    records.sort_by(|left, right| left.started_at_ms.cmp(&right.started_at_ms));
    let path = subagent_state_path(workspace_config_dir);
    if let Some(parent) = path.parent() {
        if tokio::fs::create_dir_all(parent).await.is_err() {
            return;
        }
    }
    if let Ok(bytes) = serde_json::to_vec_pretty(&records) {
        let _ = tokio::fs::write(path, bytes).await;
    }
}


pub(crate) fn send_input_target(args: &SendInputArgs) -> Option<String> {
    [
        args.target.as_deref(),
        args.agent_id.as_deref(),
        args.id.as_deref(),
    ]
    .into_iter()
    .flatten()
    .map(|value| value.trim().to_string())
    .find(|value| !value.is_empty())
}


pub(crate) fn send_input_message(args: &SendInputArgs) -> Result<String, String> {
    if let Some(message) = args
        .message
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Ok(message.to_string());
    }

    let Some(items) = args.items.as_ref() else {
        return Err("send_input message or items must not be empty".to_string());
    };
    let parts = items
        .iter()
        .filter_map(send_input_item_text)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    if parts.is_empty() {
        Err("send_input message or items must not be empty".to_string())
    } else {
        Ok(parts.join("\n"))
    }
}


pub(crate) fn send_input_item_text(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(text) => Some(text.clone()),
        serde_json::Value::Object(map) => {
            for key in ["text", "content", "input_text", "message"] {
                if let Some(text) = map.get(key).and_then(serde_json::Value::as_str) {
                    return Some(text.to_string());
                }
            }
            Some(value.to_string())
        }
        _ => Some(value.to_string()),
    }
}


pub(crate) fn resume_agent_target(args: &ResumeAgentArgs) -> Option<String> {
    [
        args.id.as_deref(),
        args.target.as_deref(),
        args.agent_id.as_deref(),
    ]
    .into_iter()
    .flatten()
    .map(|value| value.trim().to_string())
    .find(|value| !value.is_empty())
}


pub(crate) fn close_agent_target(args: CloseAgentArgs) -> Option<String> {
    [args.target, args.agent_id, args.id]
        .into_iter()
        .flatten()
        .map(|value| value.trim().to_string())
        .find(|value| !value.is_empty())
}


pub(crate) async fn collect_wait_agent_ids(
    subagents: &Arc<Mutex<HashMap<String, SubagentRecord>>>,
    agent_id: Option<String>,
    agent_ids: Option<Vec<String>>,
) -> Vec<String> {
    let mut ids = Vec::new();
    if let Some(id) = agent_id.map(|value| value.trim().to_string()) {
        if !id.is_empty() {
            ids.push(id);
        }
    }
    if let Some(values) = agent_ids {
        for id in values {
            let id = id.trim().to_string();
            if !id.is_empty() && !ids.contains(&id) {
                ids.push(id);
            }
        }
    }

    if !ids.is_empty() {
        return ids;
    }

    subagents
        .lock()
        .await
        .values()
        .filter(|record| record.status == "running")
        .map(|record| record.id.clone())
        .collect()
}


pub(crate) async fn wait_for_subagents(
    subagents: Arc<Mutex<HashMap<String, SubagentRecord>>>,
    ids: Vec<String>,
    timeout_ms: u64,
) -> SubagentWaitResult {
    let started = std::time::Instant::now();
    loop {
        let (records, missing, all_finished) = {
            let subagents = subagents.lock().await;
            let mut records = Vec::new();
            let mut missing = Vec::new();
            let mut all_finished = true;
            for id in &ids {
                match subagents.get(id) {
                    Some(record) => {
                        if record.status == "running" {
                            all_finished = false;
                        }
                        records.push(record.clone());
                    }
                    None => missing.push(id.clone()),
                }
            }
            (records, missing, all_finished)
        };

        if all_finished || started.elapsed().as_millis() >= u128::from(timeout_ms) {
            let has_missing = !missing.is_empty();
            let has_failed = records
                .iter()
                .any(|record| matches!(record.status.as_str(), "failed" | "timed_out"));
            return SubagentWaitResult {
                output: format_subagent_wait_output(&records, &missing, all_finished),
                has_missing,
                has_failed,
            };
        }

        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
}


pub(crate) fn format_subagent_wait_output(
    records: &[SubagentRecord],
    missing: &[String],
    completed: bool,
) -> String {
    serde_json::to_string_pretty(&serde_json::json!({
        "completed": completed,
        "missing": missing,
        "agents": records,
    }))
    .unwrap_or_default()
}


pub(crate) fn format_subagent_records(records: &[SubagentRecord], completed: bool) -> String {
    serde_json::to_string_pretty(&serde_json::json!({
        "completed": completed,
        "agents": records,
    }))
    .unwrap_or_default()
}


pub(crate) fn resolve_subagent_cwd(root: &Path, input: Option<&str>) -> Result<PathBuf, String> {
    let path = match input.map(str::trim).filter(|value| !value.is_empty()) {
        Some(value) => {
            let candidate = PathBuf::from(value);
            if candidate.is_absolute() {
                candidate
            } else {
                root.join(candidate)
            }
        }
        None => root.to_path_buf(),
    };
    if !path.is_dir() {
        return Err(format!(
            "Subagent cwd is not a directory: {}",
            path.display()
        ));
    }
    Ok(path.canonicalize().unwrap_or(path))
}


impl ToolExecutor {
    /// Close/cancel a subagent by id from the UI or other non-tool call sites.
    ///
    /// Mirrors `close_agent` tool semantics without emitting tool-exec events.
    pub async fn close_subagent(
        &self,
        app_handle: &AppHandle,
        thread_id: &str,
        target: &str,
    ) -> AppResult<serde_json::Value> {
        let target = target.trim();
        if target.is_empty() {
            return Err(crate::error::AppError::Custom(
                "subagent target must not be empty".to_string(),
            ));
        }

        let previous_status;
        let agent_snapshot;
        {
            let mut subagents = self.subagents.lock().await;
            match subagents.get_mut(target) {
                None => {
                    return Err(crate::error::AppError::Custom(format!(
                        "No subagent found with id: {target}"
                    )));
                }
                Some(record) => {
                    previous_status = record.status.clone();
                    if record.status == "running" {
                        record.status = "closed".to_string();
                        let completed_at_ms = now_millis();
                        record.completed_at_ms = Some(completed_at_ms);
                        if record.duration_ms.is_none() {
                            record.duration_ms =
                                Some(completed_at_ms.saturating_sub(record.started_at_ms));
                        }
                    }
                    agent_snapshot = record.clone();
                }
            }
        }

        if previous_status == "running" {
            let handles = self.subagent_handles.lock().await;
            if let Some(handle) = handles.get(target) {
                handle.cancel_flag.store(true, Ordering::SeqCst);
            }
        }

        persist_subagent_records(&self.workspace_config_dir, &self.subagents).await;
        Self::emit_subagent_status(app_handle, thread_id, &agent_snapshot);

        Ok(serde_json::json!({
            "target": target,
            "closed": previous_status == "running",
            "previousStatus": previous_status,
            "status": agent_snapshot.status,
            "message": if previous_status == "running" {
                "Subagent closed"
            } else {
                "Subagent already finished"
            },
            "agent": agent_snapshot,
        }))
    }


    pub(crate) async fn exec_spawn_agent(
        &self,
        arguments: &str,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
    ) -> AppResult<String> {
        let args: SpawnAgentArgs = match serde_json::from_str(arguments) {
            Ok(args) => args,
            Err(e) => {
                let msg = format!("Invalid spawn_agent args: {e}");
                self.emit_tool_start(app_handle, thread_id, call_id, "spawn_agent", "invalid");
                self.emit_tool_end(app_handle, thread_id, call_id, "spawn_agent", -1, &msg);
                return Ok(msg);
            }
        };

        let prompt = args.prompt.trim();
        let role = args
            .role
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("agent")
            .to_string();
        self.emit_tool_start(app_handle, thread_id, call_id, "spawn_agent", &role);

        if prompt.is_empty() {
            let msg = "spawn_agent prompt must not be empty".to_string();
            self.emit_tool_end(app_handle, thread_id, call_id, "spawn_agent", -1, &msg);
            return Ok(msg);
        }

        let cwd = match resolve_subagent_cwd(&self.cwd, args.cwd.as_deref()) {
            Ok(path) => path,
            Err(msg) => {
                self.emit_tool_end(app_handle, thread_id, call_id, "spawn_agent", -1, &msg);
                return Ok(msg);
            }
        };

        let timeout_ms = args.timeout_ms.unwrap_or(600_000).clamp(1_000, 1_800_000);
        let wait = args.wait.unwrap_or(false);
        let id = format!("agent-{}", uuid::Uuid::new_v4().simple());
        let started_at_ms = now_millis();

        let provider_config = self.subagent_provider_config.lock().await.clone();
        let model = args
            .model
            .as_deref()
            .unwrap_or(&provider_config.model)
            .to_string();
        let system_prompt = format!(
            "{}\n\nYou are a sub-agent with role: {}. Your working directory is: {}",
            provider_config.system_prompt_prefix,
            role,
            cwd.display()
        );

        let record = SubagentRecord {
            id: id.clone(),
            role: role.clone(),
            status: "running".to_string(),
            prompt: prompt.to_string(),
            cwd: cwd.to_string_lossy().to_string(),
            command: format!("[internal:{}]", model),
            process_id: None,
            started_at_ms,
            completed_at_ms: None,
            duration_ms: None,
            exit_code: None,
            output: None,
            error: None,
            input_history: Vec::new(),
            last_input_at_ms: None,
        };

        {
            let mut subagents = self.subagents.lock().await;
            subagents.insert(id.clone(), record.clone());
        }
        persist_subagent_records(&self.workspace_config_dir, &self.subagents).await;
        Self::emit_subagent_status(app_handle, thread_id, &record);

        let subagent_config = crate::subagent_engine::SubagentConfig {
            base_url: provider_config.base_url.clone(),
            api_key: provider_config.api_key.clone(),
            model,
            wire_api: provider_config.wire_api.clone(),
            system_prompt,
            cwd,
            timeout_ms,
            max_iterations: 25,
            max_output_tokens: provider_config.max_output_tokens,
        };

        let tool_executor_for_subagent = Arc::new(tokio::sync::RwLock::new(
            ToolExecutor::with_workspace_config_dir(
                subagent_config.cwd.clone(),
                self.workspace_config_dir.clone(),
            ),
        ));

        let (handle, result_rx) = crate::subagent_engine::spawn_subagent(
            subagent_config,
            prompt.to_string(),
            tool_executor_for_subagent,
            app_handle.clone(),
            thread_id.to_string(),
            id.clone(),
        );

        self.subagent_handles
            .lock()
            .await
            .insert(id.clone(), handle);

        let (output, exit_code) = if wait {
            let result =
                wait_for_subagents(self.subagents.clone(), vec![id.clone()], timeout_ms).await;
            let exit_code = if result.has_missing || result.has_failed {
                -1
            } else {
                0
            };
            (result.output, exit_code)
        } else {
            (format_subagent_records(&[record], false), 0)
        };

        // Spawn a background task to update the record when the subagent finishes
        let subagents_clone = self.subagents.clone();
        let workspace_config_dir = self.workspace_config_dir.clone();
        let subagent_handles_clone = self.subagent_handles.clone();
        let finished_id = id.clone();
        let app_handle_bg = app_handle.clone();
        let thread_id_bg = thread_id.to_string();
        tokio::spawn(async move {
            if let Ok(result) = result_rx.await {
                let (status, output_val, error_val, exit_code) = match result.status {
                    crate::subagent_engine::SubagentStatus::Completed { output } => {
                        ("completed", Some(output), None, Some(0))
                    }
                    crate::subagent_engine::SubagentStatus::Failed { error } => {
                        ("failed", None, Some(error), Some(1))
                    }
                    crate::subagent_engine::SubagentStatus::TimedOut => (
                        "timed_out",
                        None,
                        Some("Subagent timed out".to_string()),
                        Some(124),
                    ),
                    crate::subagent_engine::SubagentStatus::Cancelled => {
                        ("closed", None, None, None)
                    }
                    crate::subagent_engine::SubagentStatus::Running => {
                        ("running", None, None, None)
                    }
                };
                let completed_at_ms = now_millis();
                let mut emit_record: Option<SubagentRecord> = None;
                {
                    let mut subagents = subagents_clone.lock().await;
                    if let Some(record) = subagents.get_mut(&finished_id) {
                        record.status = status.to_string();
                        record.completed_at_ms = Some(completed_at_ms);
                        record.duration_ms = Some(result.duration_ms as i64);
                        record.exit_code = exit_code;
                        record.output = output_val;
                        record.error = error_val;
                        emit_record = Some(record.clone());
                    }
                }
                persist_subagent_records(&workspace_config_dir, &subagents_clone).await;
                subagent_handles_clone.lock().await.remove(&finished_id);
                if let Some(record) = emit_record {
                    ToolExecutor::emit_subagent_status(&app_handle_bg, &thread_id_bg, &record);
                }
            }
        });

        self.emit_tool_end(
            app_handle,
            thread_id,
            call_id,
            "spawn_agent",
            exit_code,
            &output,
        );
        Ok(output)
    }


    pub(crate) async fn exec_wait_agent(
        &self,
        arguments: &str,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
    ) -> AppResult<String> {
        let args: WaitAgentArgs = match serde_json::from_str(arguments) {
            Ok(args) => args,
            Err(e) => {
                let msg = format!("Invalid wait_agent args: {e}");
                self.emit_tool_start(app_handle, thread_id, call_id, "wait_agent", "invalid");
                self.emit_tool_end(app_handle, thread_id, call_id, "wait_agent", -1, &msg);
                return Ok(msg);
            }
        };

        let ids = collect_wait_agent_ids(&self.subagents, args.agent_id, args.agent_ids).await;
        let display = if ids.is_empty() {
            "agents".to_string()
        } else if ids.len() == 1 {
            ids[0].clone()
        } else {
            format!("{} agents", ids.len())
        };
        self.emit_tool_start(app_handle, thread_id, call_id, "wait_agent", &display);

        if ids.is_empty() {
            let msg = "No subagents found to wait for".to_string();
            self.emit_tool_end(app_handle, thread_id, call_id, "wait_agent", -1, &msg);
            return Ok(msg);
        }

        let timeout_ms = args.timeout_ms.unwrap_or(60_000).clamp(0, 1_800_000);
        let result = wait_for_subagents(self.subagents.clone(), ids, timeout_ms).await;
        let exit_code = if result.has_missing { -1 } else { 0 };
        self.emit_tool_end(
            app_handle,
            thread_id,
            call_id,
            "wait_agent",
            exit_code,
            &result.output,
        );
        Ok(result.output)
    }


    pub(crate) async fn exec_send_input(
        &self,
        arguments: &str,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
    ) -> AppResult<String> {
        let args: SendInputArgs = match serde_json::from_str(arguments) {
            Ok(args) => args,
            Err(e) => {
                let msg = format!("Invalid send_input args: {e}");
                self.emit_tool_start(app_handle, thread_id, call_id, "send_input", "invalid");
                self.emit_tool_end(app_handle, thread_id, call_id, "send_input", -1, &msg);
                return Ok(msg);
            }
        };

        let target = match send_input_target(&args) {
            Some(target) => target,
            None => {
                let msg = "send_input target must not be empty".to_string();
                self.emit_tool_start(app_handle, thread_id, call_id, "send_input", "invalid");
                self.emit_tool_end(app_handle, thread_id, call_id, "send_input", -1, &msg);
                return Ok(msg);
            }
        };
        let message = match send_input_message(&args) {
            Ok(message) => message,
            Err(msg) => {
                self.emit_tool_start(app_handle, thread_id, call_id, "send_input", &target);
                self.emit_tool_end(app_handle, thread_id, call_id, "send_input", -1, &msg);
                return Ok(msg);
            }
        };

        self.emit_tool_start(app_handle, thread_id, call_id, "send_input", &target);

        // Check if subagent exists
        let agent_status = {
            let subagents = self.subagents.lock().await;
            subagents.get(&target).map(|r| r.status.clone())
        };

        match agent_status {
            None => {
                let msg = format!("No subagent found with id: {target}");
                self.emit_tool_end(app_handle, thread_id, call_id, "send_input", -1, &msg);
                Ok(msg)
            }
            Some(status) if status != "running" => {
                let msg = format!("Subagent {target} is not running (status: {status})");
                self.emit_tool_end(app_handle, thread_id, call_id, "send_input", -1, &msg);
                Ok(msg)
            }
            Some(_) => {
                let delivered = {
                    let handles = self.subagent_handles.lock().await;
                    if let Some(handle) = handles.get(&target) {
                        handle.input_tx.send(message.clone()).await.is_ok()
                    } else {
                        false
                    }
                };

                // Record in input history
                let submission_id = uuid::Uuid::new_v4().to_string();
                {
                    let mut subagents = self.subagents.lock().await;
                    if let Some(record) = subagents.get_mut(&target) {
                        record.input_history.push(SubagentInputRecord {
                            submission_id: submission_id.clone(),
                            message: message.clone(),
                            submitted_at_ms: now_millis(),
                            interrupt: args.interrupt.unwrap_or(false),
                            delivered_to_stdin: delivered,
                        });
                        record.last_input_at_ms = Some(now_millis());
                    }
                }
                persist_subagent_records(&self.workspace_config_dir, &self.subagents).await;

                let result = SendInputResult {
                    target: target.clone(),
                    submission_id,
                    status: "running".to_string(),
                    delivered_to_stdin: delivered,
                    queued: !delivered,
                    note: if delivered {
                        "Message delivered to subagent".to_string()
                    } else {
                        "Message queued (subagent channel unavailable)".to_string()
                    },
                };
                let output = serde_json::to_string_pretty(&result).unwrap_or_default();
                self.emit_tool_end(app_handle, thread_id, call_id, "send_input", 0, &output);
                Ok(output)
            }
        }
    }


    pub(crate) async fn exec_resume_agent(
        &self,
        arguments: &str,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
    ) -> AppResult<String> {
        let args: ResumeAgentArgs = match serde_json::from_str(arguments) {
            Ok(args) => args,
            Err(e) => {
                let msg = format!("Invalid resume_agent args: {e}");
                self.emit_tool_start(app_handle, thread_id, call_id, "resume_agent", "invalid");
                self.emit_tool_end(app_handle, thread_id, call_id, "resume_agent", -1, &msg);
                return Ok(msg);
            }
        };

        let target = match resume_agent_target(&args) {
            Some(target) => target,
            None => {
                let msg = "resume_agent id must not be empty".to_string();
                self.emit_tool_start(app_handle, thread_id, call_id, "resume_agent", "invalid");
                self.emit_tool_end(app_handle, thread_id, call_id, "resume_agent", -1, &msg);
                return Ok(msg);
            }
        };
        self.emit_tool_start(app_handle, thread_id, call_id, "resume_agent", &target);

        let timeout_ms = args.timeout_ms.unwrap_or(600_000).clamp(1_000, 1_800_000);

        // Get the original prompt and check if it can be resumed
        let (previous_status, prompt, cwd_str) = {
            let subagents = self.subagents.lock().await;
            match subagents.get(&target) {
                None => {
                    let msg = format!("No subagent found with id: {target}");
                    self.emit_tool_end(app_handle, thread_id, call_id, "resume_agent", -1, &msg);
                    return Ok(msg);
                }
                Some(record) => {
                    if record.status == "running" {
                        let msg = format!("Subagent {target} is already running");
                        self.emit_tool_end(
                            app_handle,
                            thread_id,
                            call_id,
                            "resume_agent",
                            -1,
                            &msg,
                        );
                        return Ok(msg);
                    }
                    (
                        record.status.clone(),
                        record.prompt.clone(),
                        record.cwd.clone(),
                    )
                }
            }
        };

        let provider_config = self.subagent_provider_config.lock().await.clone();
        let cwd = PathBuf::from(&cwd_str);
        let system_prompt = format!(
            "{}\n\nYou are a resumed sub-agent. Your working directory is: {}",
            provider_config.system_prompt_prefix,
            cwd.display()
        );

        let subagent_config = crate::subagent_engine::SubagentConfig {
            base_url: provider_config.base_url.clone(),
            api_key: provider_config.api_key.clone(),
            model: provider_config.model.clone(),
            wire_api: provider_config.wire_api.clone(),
            system_prompt,
            cwd: cwd.clone(),
            timeout_ms,
            max_iterations: 25,
            max_output_tokens: provider_config.max_output_tokens,
        };

        // Update status to running
        {
            let mut subagents = self.subagents.lock().await;
            if let Some(record) = subagents.get_mut(&target) {
                record.status = "running".to_string();
                record.started_at_ms = now_millis();
                record.completed_at_ms = None;
                record.duration_ms = None;
                record.exit_code = None;
                record.output = None;
                record.error = None;
            }
        }
        persist_subagent_records(&self.workspace_config_dir, &self.subagents).await;
        if let Some(record) = self.subagents.lock().await.get(&target).cloned() {
            Self::emit_subagent_status(app_handle, thread_id, &record);
        }

        let tool_executor_for_subagent = Arc::new(tokio::sync::RwLock::new(
            ToolExecutor::with_workspace_config_dir(cwd, self.workspace_config_dir.clone()),
        ));

        let (handle, result_rx) = crate::subagent_engine::spawn_subagent(
            subagent_config,
            prompt.clone(),
            tool_executor_for_subagent,
            app_handle.clone(),
            thread_id.to_string(),
            target.clone(),
        );

        self.subagent_handles
            .lock()
            .await
            .insert(target.clone(), handle);

        // Spawn background updater
        let subagents_clone = self.subagents.clone();
        let workspace_config_dir = self.workspace_config_dir.clone();
        let subagent_handles_clone = self.subagent_handles.clone();
        let finished_id = target.clone();
        let app_handle_bg = app_handle.clone();
        let thread_id_bg = thread_id.to_string();
        tokio::spawn(async move {
            if let Ok(result) = result_rx.await {
                let (status, output_val, error_val, exit_code) = match result.status {
                    crate::subagent_engine::SubagentStatus::Completed { output } => {
                        ("completed", Some(output), None, Some(0))
                    }
                    crate::subagent_engine::SubagentStatus::Failed { error } => {
                        ("failed", None, Some(error), Some(1))
                    }
                    crate::subagent_engine::SubagentStatus::TimedOut => (
                        "timed_out",
                        None,
                        Some("Subagent timed out".to_string()),
                        Some(124),
                    ),
                    crate::subagent_engine::SubagentStatus::Cancelled => {
                        ("closed", None, None, None)
                    }
                    crate::subagent_engine::SubagentStatus::Running => {
                        ("running", None, None, None)
                    }
                };
                let completed_at_ms = now_millis();
                let mut emit_record: Option<SubagentRecord> = None;
                {
                    let mut subagents = subagents_clone.lock().await;
                    if let Some(record) = subagents.get_mut(&finished_id) {
                        record.status = status.to_string();
                        record.completed_at_ms = Some(completed_at_ms);
                        record.duration_ms = Some(result.duration_ms as i64);
                        record.exit_code = exit_code;
                        record.output = output_val;
                        record.error = error_val;
                        emit_record = Some(record.clone());
                    }
                }
                persist_subagent_records(&workspace_config_dir, &subagents_clone).await;
                subagent_handles_clone.lock().await.remove(&finished_id);
                if let Some(record) = emit_record {
                    ToolExecutor::emit_subagent_status(&app_handle_bg, &thread_id_bg, &record);
                }
            }
        });

        let result = ResumeAgentResult {
            id: target.clone(),
            resumed: true,
            previous_status,
            status: "running".to_string(),
            note: "Subagent resumed with internal engine".to_string(),
            agent: None,
        };

        let output = if args.wait.unwrap_or(false) {
            let wait =
                wait_for_subagents(self.subagents.clone(), vec![target.clone()], timeout_ms).await;
            wait.output
        } else {
            serde_json::to_string_pretty(&result).unwrap_or_default()
        };
        self.emit_tool_end(app_handle, thread_id, call_id, "resume_agent", 0, &output);
        Ok(output)
    }


    pub(crate) async fn exec_list_agents(
        &self,
        arguments: &str,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
    ) -> AppResult<String> {
        let args: ListAgentsArgs = serde_json::from_str(arguments).unwrap_or_default();
        let status_filter = args
            .status
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string);
        let display = status_filter.as_deref().unwrap_or("agents");
        self.emit_tool_start(app_handle, thread_id, call_id, "list_agents", display);

        let mut records = self
            .subagents
            .lock()
            .await
            .values()
            .filter(|record| {
                status_filter
                    .as_deref()
                    .is_none_or(|status| record.status == status)
            })
            .cloned()
            .collect::<Vec<_>>();
        records.sort_by(|left, right| left.started_at_ms.cmp(&right.started_at_ms));
        let output = format_subagent_records(&records, false);
        self.emit_tool_end(app_handle, thread_id, call_id, "list_agents", 0, &output);
        Ok(output)
    }


    pub(crate) async fn exec_close_agent(
        &self,
        arguments: &str,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
    ) -> AppResult<String> {
        let args: CloseAgentArgs = match serde_json::from_str(arguments) {
            Ok(args) => args,
            Err(e) => {
                let msg = format!("Invalid close_agent args: {e}");
                self.emit_tool_start(app_handle, thread_id, call_id, "close_agent", "invalid");
                self.emit_tool_end(app_handle, thread_id, call_id, "close_agent", -1, &msg);
                return Ok(msg);
            }
        };

        let target = match close_agent_target(args) {
            Some(target) => target,
            None => {
                let msg = "close_agent target must not be empty".to_string();
                self.emit_tool_start(app_handle, thread_id, call_id, "close_agent", "invalid");
                self.emit_tool_end(app_handle, thread_id, call_id, "close_agent", -1, &msg);
                return Ok(msg);
            }
        };

        self.emit_tool_start(app_handle, thread_id, call_id, "close_agent", &target);

        let previous_status;
        let agent_snapshot;
        {
            let mut subagents = self.subagents.lock().await;
            match subagents.get_mut(&target) {
                None => {
                    let msg = format!("No subagent found with id: {target}");
                    self.emit_tool_end(app_handle, thread_id, call_id, "close_agent", -1, &msg);
                    return Ok(msg);
                }
                Some(record) => {
                    previous_status = record.status.clone();
                    if record.status == "running" {
                        record.status = "closed".to_string();
                        record.completed_at_ms = Some(now_millis());
                    }
                    agent_snapshot = Some(record.clone());
                }
            }
        }

        // Signal the subagent to cancel via its AtomicBool flag
        if previous_status == "running" {
            let handles = self.subagent_handles.lock().await;
            if let Some(handle) = handles.get(&target) {
                handle.cancel_flag.store(true, Ordering::SeqCst);
            }
        }

        persist_subagent_records(&self.workspace_config_dir, &self.subagents).await;

        let result = SubagentCloseResult {
            target: target.clone(),
            closed: previous_status == "running",
            previous_status,
            message: "Subagent closed".to_string(),
            agent: agent_snapshot,
        };
        if let Some(record) = result.agent.as_ref() {
            Self::emit_subagent_status(app_handle, thread_id, record);
        }
        let output = serde_json::to_string_pretty(&result).unwrap_or_default();
        self.emit_tool_end(app_handle, thread_id, call_id, "close_agent", 0, &output);
        Ok(output)
    }

}
