use super::*;

pub(crate) fn emit_robot_progress_updated(
    app_handle: &AppHandle,
    thread_id: &str,
    robot_state: Option<&ThreadRobotState>,
) {
    emit_and_broadcast(
        app_handle,
        "robot-progress-updated",
        serde_json::json!({
            "threadId": thread_id,
            "robotState": robot_state,
        }),
    );
}


pub(crate) fn mid_turn_compaction_allowed(
    last_compaction_call_count: Option<u32>,
    llm_call_count: u32,
    robot_active: bool,
) -> bool {
    match last_compaction_call_count {
        None => true,
        Some(last_call_count) => {
            robot_active
                && llm_call_count.saturating_sub(last_call_count) >= ROBOT_COMPACTION_COOLDOWN_CALLS
        }
    }
}


pub(crate) fn reset_robot_node_runtime_counters(
    iteration: &mut u32,
    last_prompt_tokens: &mut u64,
    last_mid_turn_compaction_call_count: &mut Option<u32>,
) {
    *iteration = 0;
    *last_prompt_tokens = 0;
    *last_mid_turn_compaction_call_count = None;
}


pub(crate) fn estimate_robot_checkpoint_tokens(messages: &[ThreadMessage]) -> u64 {
    let chars = messages.iter().fold(0usize, |total, message| {
        let tool_call_chars = message
            .tool_calls
            .as_ref()
            .map(|calls| {
                calls.iter().fold(0usize, |call_total, call| {
                    call_total
                        .saturating_add(call.name.chars().count())
                        .saturating_add(call.arguments.chars().count())
                })
            })
            .unwrap_or_default();
        total
            .saturating_add(message.content.chars().count())
            .saturating_add(tool_call_chars)
    });
    chars.div_ceil(3).max(1) as u64
}


pub(crate) async fn checkpoint_robot_model_history(
    thread_store: &ThreadStore,
    thread_id: &str,
    state: &ThreadRobotState,
) -> AppResult<u64> {
    let history = thread_store.get_model_history(thread_id).await;
    let focused_history = build_robot_model_history(&history, state);
    let estimated_tokens = estimate_robot_checkpoint_tokens(&focused_history);
    if focused_history.len() < history.len() {
        thread_store
            .replace_model_history(thread_id, focused_history, estimated_tokens)
            .await?;
    }
    Ok(estimated_tokens)
}


pub(crate) fn extract_update_goal_status(arguments: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(arguments)
        .ok()?
        .get("status")?
        .as_str()
        .map(|status| status.trim().to_string())
        .filter(|status| !status.is_empty())
}


pub(crate) async fn advance_robot_workflow_from_goal_completion(
    thread_store: &ThreadStore,
    robot_orchestrator: &RobotOrchestrator,
    thread_id: &str,
    progress_state: ThreadRobotState,
) -> Result<RobotGoalCompletionOutcome, String> {
    match robot_orchestrator
        .apply_node_progress(thread_store, thread_id, progress_state, true, None)
        .await
        .map_err(|e| e.to_string())?
    {
        NodeProgressResult::ContinueCurrent { .. } => Err(
            "robot workflow refused to advance after update_goal marked the node complete"
                .to_string(),
        ),
        NodeProgressResult::Advanced { state, nudge } => {
            let boundary_id = uuid::Uuid::new_v4().to_string();
            let mut advanced_state = state;
            advanced_state.current_node_start_message_id = Some(boundary_id.clone());
            thread_store
                .set_thread_robot_state(thread_id, advanced_state.clone())
                .await
                .map_err(|e| e.to_string())?;
            thread_store
                .add_message(
                    thread_id,
                    ThreadMessage {
                        id: boundary_id,
                        role: "system".to_string(),
                        content: nudge,
                        timestamp: now_secs(),
                        tool_call_id: None,
                        tool_name: None,
                        tool_calls: None,
                        attachments: Vec::new(),
                    },
                )
                .await
                .map_err(|e| e.to_string())?;
            checkpoint_robot_model_history(thread_store, thread_id, &advanced_state)
                .await
                .map_err(|e| e.to_string())?;
            Ok(RobotGoalCompletionOutcome::Advanced(advanced_state))
        }
        NodeProgressResult::Completed { state } => Ok(RobotGoalCompletionOutcome::Completed(state)),
    }
}


