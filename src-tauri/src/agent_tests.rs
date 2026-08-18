use super::*;
use crate::{thread_store::ThreadStore, tool_executor::ToolExecutor};

fn status_entry(status: &str, fingerprint: Option<u64>) -> GitStatusEntry {
    GitStatusEntry {
        status: status.to_string(),
        fingerprint,
    }
}

#[test]
fn skill_prompt_priority_prefers_high_frequency_skills() {
    let boosted = skill_prompt_priority_score("brainstorming", "explore ideas", "plugin");
    let random = skill_prompt_priority_score("lab-demo", "experimental lab skill", "local");
    assert!(boosted > random);
}

#[test]
fn safe_recall_path_rejects_paths_outside_memory_root() {
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let root = temp_dir.path().join("memories");
    std::fs::create_dir_all(&root).expect("memory root");
    std::fs::write(root.join("safe.okf"), "safe").expect("safe file");
    std::fs::write(temp_dir.path().join("secret.okf"), "secret").expect("secret file");

    assert!(safe_recall_path(&root, "safe.okf").is_some());
    assert!(safe_recall_path(&root, "../secret.okf").is_none());
}

#[test]
fn available_skills_prompt_is_bounded_and_points_to_tool_search() {
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let root = temp_dir.path().to_path_buf();
    let skills_dir = root.join("codey").join("skills");
    std::fs::create_dir_all(&skills_dir).expect("skills dir");

    for idx in 0..30 {
        let skill_id = format!("skill-{idx:02}");
        let skill_dir = skills_dir.join(&skill_id);
        std::fs::create_dir_all(&skill_dir).expect("skill dir");
        std::fs::write(
            skill_dir.join("SKILL.md"),
            format!(
                "---\nname: {skill_id}\ndescription: demo skill number {idx}\n---\n# {skill_id}\n"
            ),
        )
        .expect("skill md");
    }

    // One high-priority skill that should be preferred in the short list.
    let pinned_dir = skills_dir.join("brainstorming");
    std::fs::create_dir_all(&pinned_dir).expect("pinned dir");
    std::fs::write(
        pinned_dir.join("SKILL.md"),
        "---\nname: brainstorming\ndescription: explore ideas before building\n---\n# brainstorming\n",
    )
    .expect("pinned skill");

    let thread_store = Arc::new(ThreadStore::new(&root.join("codey")));
    let tool_executor = ToolExecutor::new(root.clone());
    let skills_abs_prefix = root
        .join("codey")
        .join("skills")
        .to_string_lossy()
        .replace('\\', "/");
    let engine = AgentEngine::new(thread_store, tool_executor, root).expect("engine");
    let prompt = engine.render_available_skills_prompt();

    assert!(prompt.contains("Available skills:"));
    assert!(prompt.contains("tool_search"));
    assert!(prompt.contains("brainstorming"));
    assert!(prompt.contains("additional skills omitted"));
    assert!(prompt.contains("codey/skills/brainstorming/SKILL.md"));
    assert!(!prompt.contains(&skills_abs_prefix));

    let listed = prompt
        .lines()
        .filter(|line| line.starts_with("- ") && !line.contains("additional skills omitted"))
        .count();
    assert!(
        listed <= 12,
        "skills prompt should list at most 12 skills, got {listed}"
    );
    assert!(prompt.chars().count() < 4_500);
}

#[test]
fn available_skills_prompt_excludes_deferred_skills() {
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let root = temp_dir.path().to_path_buf();
    let skills_dir = root.join("codey").join("skills");
    std::fs::create_dir_all(&skills_dir).expect("skills dir");

    let deferred_dir = skills_dir.join("using-superpowers");
    std::fs::create_dir_all(&deferred_dir).expect("deferred dir");
    std::fs::write(
        deferred_dir.join("SKILL.md"),
        "---\nname: using-superpowers\ndescription: establish how to find and use skills\n---\n# using-superpowers\n",
    )
    .expect("deferred skill");

    let normal_dir = skills_dir.join("lab-demo");
    std::fs::create_dir_all(&normal_dir).expect("normal dir");
    std::fs::write(
        normal_dir.join("SKILL.md"),
        "---\nname: lab-demo\ndescription: demo skill\n---\n# lab-demo\n",
    )
    .expect("normal skill");

    let thread_store = Arc::new(ThreadStore::new(&root.join("codey")));
    let tool_executor = ToolExecutor::new(root.clone());
    let engine = AgentEngine::new(thread_store, tool_executor, root).expect("engine");
    let prompt = engine.render_available_skills_prompt();

    assert!(prompt.contains("Available skills:"));
    assert!(prompt.contains("lab-demo"));
    assert!(
        !prompt.contains("using-superpowers"),
        "deferred skills must stay out of the always-on prompt catalog"
    );
}

fn test_thread_message(id: &str, role: &str, content: &str) -> ThreadMessage {
    ThreadMessage {
        id: id.to_string(),
        role: role.to_string(),
        content: content.to_string(),
        timestamp: 0,
        tool_call_id: None,
        tool_name: None,
        tool_calls: None,
        attachments: Vec::new(),
    }
}

#[test]
fn extract_proposed_plan_supports_inline_tags() {
    let text = "intro<proposed_plan>\n# Plan\n- step 1\n</proposed_plan>tail";
    assert_eq!(
        extract_proposed_plan(text),
        Some("# Plan\n- step 1".to_string())
    );
}

#[test]
fn resolve_effective_plan_content_prefers_stream_plan_text() {
    let resolved = resolve_effective_plan_content(
        "plan",
        Some("  from stream  "),
        "<proposed_plan>from tag</proposed_plan>",
        "fallback",
    );
    assert_eq!(resolved, Some("from stream".to_string()));
}

#[test]
fn resolve_effective_plan_content_uses_tagged_content() {
    let resolved = resolve_effective_plan_content(
        "plan",
        None,
        "prefix\n<proposed_plan>\n## Title\n1. one\n</proposed_plan>\nsuffix",
        "fallback",
    );
    assert_eq!(resolved, Some("## Title\n1. one".to_string()));
}

#[test]
fn resolve_effective_plan_content_falls_back_to_cleaned_text_for_plan_mode() {
    let resolved =
        resolve_effective_plan_content("plan", None, "No tags here", "  plain markdown  ");
    assert_eq!(resolved, Some("plain markdown".to_string()));
}

#[test]
fn resolve_effective_plan_content_keeps_non_plan_behavior() {
    let resolved = resolve_effective_plan_content(
        "chat",
        None,
        "<proposed_plan>ignored</proposed_plan>",
        "should-not-be-plan",
    );
    assert_eq!(resolved, None);
}

#[test]
fn rate_limit_backoff_ms_grows_exponentially_and_caps() {
    assert_eq!(rate_limit_backoff_ms(1), 1_000);
    assert_eq!(rate_limit_backoff_ms(2), 2_000);
    assert_eq!(rate_limit_backoff_ms(3), 4_000);
    assert_eq!(rate_limit_backoff_ms(4), 8_000);
    assert_eq!(rate_limit_backoff_ms(5), 16_000);
    assert_eq!(rate_limit_backoff_ms(6), 30_000);
    assert_eq!(rate_limit_backoff_ms(10), 30_000);
}

#[test]
fn robot_mid_turn_compaction_repeats_only_after_cooldown() {
    assert!(mid_turn_compaction_allowed(None, 1, false));
    assert!(!mid_turn_compaction_allowed(Some(10), 18, false));
    assert!(!mid_turn_compaction_allowed(Some(10), 17, true));
    assert!(mid_turn_compaction_allowed(Some(10), 18, true));
}

#[test]
fn robot_node_advance_resets_node_scoped_runtime_counters() {
    let mut iteration = 127;
    let mut last_prompt_tokens = 160_000;
    let mut last_compaction_call_count = Some(120);

    reset_robot_node_runtime_counters(
        &mut iteration,
        &mut last_prompt_tokens,
        &mut last_compaction_call_count,
    );

    assert_eq!(iteration, 0);
    assert_eq!(last_prompt_tokens, 0);
    assert_eq!(last_compaction_call_count, None);
}

#[test]
fn is_retryable_rate_limit_error_detects_429_and_rate_limit_text() {
    assert!(is_retryable_rate_limit_error(
        "LLM API error (429 Too Many Requests): overload"
    ));
    assert!(is_retryable_rate_limit_error("rate limit exceeded"));
    assert!(!is_retryable_rate_limit_error(
        "LLM API error (500): internal"
    ));
}

#[test]
fn interrupt_thread_is_isolated_per_thread() {
    let root = tempfile::tempdir().expect("tempdir");
    let cwd = root.path().to_path_buf();
    let thread_store = Arc::new(ThreadStore::new(&cwd.join("codey")));
    let tool_executor = ToolExecutor::new(cwd.clone());
    let engine = AgentEngine::new(thread_store, tool_executor, cwd).expect("engine");

    let flag_a = engine.reset_cancel_flag("thread-a");
    let flag_b = engine.reset_cancel_flag("thread-b");
    assert!(!flag_a.load(Ordering::SeqCst));
    assert!(!flag_b.load(Ordering::SeqCst));

    engine.interrupt_thread("thread-a");
    assert!(engine.is_thread_cancelled("thread-a"));
    assert!(!engine.is_thread_cancelled("thread-b"));
    assert!(flag_a.load(Ordering::SeqCst));
    assert!(!flag_b.load(Ordering::SeqCst));

    // 新一轮 thread-a 应重置自己的 flag，且不影响 thread-b 后续独立取消。
    let flag_a2 = engine.reset_cancel_flag("thread-a");
    assert!(!flag_a2.load(Ordering::SeqCst));
    assert!(!engine.is_thread_cancelled("thread-a"));
    assert!(!engine.is_thread_cancelled("thread-b"));

    engine.interrupt_thread("thread-b");
    assert!(!engine.is_thread_cancelled("thread-a"));
    assert!(engine.is_thread_cancelled("thread-b"));
}

#[test]
fn is_retryable_upstream_error_detects_transient_gateway_failures_only() {
    assert!(is_retryable_upstream_error(
        "LLM API error (502 Bad Gateway): {\"error\":{\"type\":\"upstream_error\"}}"
    ));
    assert!(is_retryable_upstream_error(
        "LLM API error (503 Service Unavailable)"
    ));
    assert!(is_retryable_upstream_error(
        "LLM API error (504 Gateway Timeout)"
    ));
    assert!(!is_retryable_upstream_error(
        "LLM API error (401 Unauthorized)"
    ));
    assert!(!is_retryable_upstream_error(
        "LLM API error (400 Bad Request)"
    ));
}

#[test]
fn is_retryable_transient_llm_error_detects_empty_stream_and_header_timeout() {
    assert!(is_retryable_transient_llm_error(
        "LLM returned an empty response. The provider may have rejected the model or returned an incompatible stream format."
    ));
    assert!(is_retryable_transient_llm_error(
        "LLM request timed out waiting for response headers after 60 seconds."
    ));
    assert!(is_retryable_transient_llm_error(
        "HTTP request failed: error sending request for url (https://api.example.com/v1/chat/completions)"
    ));
    assert!(!is_retryable_transient_llm_error(
        "LLM API error (401 Unauthorized)"
    ));
    assert!(!is_retryable_transient_llm_error(
        "LLM API error (400 Bad Request)"
    ));
}

#[test]
fn transient_llm_backoff_ms_grows_and_caps() {
    assert_eq!(transient_llm_backoff_ms(1), 1_000);
    assert_eq!(transient_llm_backoff_ms(2), 2_000);
    assert_eq!(transient_llm_backoff_ms(3), 4_000);
    assert_eq!(transient_llm_backoff_ms(4), 8_000);
    assert_eq!(transient_llm_backoff_ms(99), 8_000);
}

#[test]
fn should_continue_goal_loop_stops_after_fatal_llm_error() {
    assert!(
        !should_continue_goal_loop("goal", false, false, true, true, 0, 10),
        "fatal LLM errors must end the turn instead of goal continuation"
    );
    assert!(
        should_continue_goal_loop("goal", false, false, false, true, 0, 10),
        "healthy active goals may continue"
    );
    assert!(!should_continue_goal_loop(
        "goal", false, false, false, true, 10, 10
    ));
    assert!(!should_continue_goal_loop(
        "chat", false, false, false, true, 0, 10
    ));
    assert!(!should_continue_goal_loop(
        "goal", true, false, false, true, 0, 10
    ));
    assert!(!should_continue_goal_loop(
        "goal", false, true, false, true, 0, 10
    ));
    assert!(!should_continue_goal_loop(
        "goal", false, false, false, false, 0, 10
    ));
}

#[test]
fn repeated_goal_stop_response_detects_only_consecutive_nonempty_duplicates() {
    let mut last_response = None;

    assert!(!repeated_goal_stop_response(
        &mut last_response,
        "Patch prepared; apply it next."
    ));
    assert!(repeated_goal_stop_response(
        &mut last_response,
        "  Patch prepared; apply it next.  "
    ));
    assert!(!repeated_goal_stop_response(
        &mut last_response,
        "Applying the patch now."
    ));
    assert!(!repeated_goal_stop_response(&mut last_response, "   "));
}

#[test]
fn empty_response_and_header_timeout_end_goal_turn_after_termination() {
    for message in [
        "LLM returned an empty response. The provider may have rejected the model or returned an incompatible stream format. Check the provider/model configuration and retry.",
        "LLM request timed out waiting for response headers after 60 seconds.",
    ] {
        assert!(
            should_retry_transient_llm_error_before_ending_goal_turn(message, 0, 3),
            "empty/timeout must retry multiple times before ending the goal turn: {message}"
        );
        assert!(
            should_retry_transient_llm_error_before_ending_goal_turn(message, 2, 3),
            "empty/timeout should still retry while budget remains: {message}"
        );
        assert!(
            !should_retry_transient_llm_error_before_ending_goal_turn(message, 3, 3),
            "empty/timeout must stop retrying after the budget is exhausted: {message}"
        );
        assert!(
            should_end_goal_turn_after_llm_error(message, true),
            "terminated empty/timeout failures must end the goal turn: {message}"
        );
        assert!(
            !should_continue_goal_loop("goal", false, false, true, true, 0, 10),
            "goal continuation must stay blocked after empty/timeout termination"
        );
        assert!(
            !should_end_goal_turn_after_llm_error(message, false),
            "unterminated retries may still recover inside the agent loop"
        );
    }
}

#[test]
fn upstream_backoff_ms_grows_exponentially_and_caps() {
    assert_eq!(upstream_backoff_ms(1), 1_000);
    assert_eq!(upstream_backoff_ms(2), 2_000);
    assert_eq!(upstream_backoff_ms(3), 4_000);
    assert_eq!(upstream_backoff_ms(4), 8_000);
    assert_eq!(upstream_backoff_ms(10), 8_000);
}

#[test]
fn retryable_stream_read_error_detects_transient_body_failures() {
    for message in [
        "Stream read error after 6671 bytes, 125.3s elapsed: error decoding response body",
        "connection reset by peer",
        "unexpected EOF while reading response",
        "hyper error: incomplete message",
    ] {
        assert!(is_retryable_stream_read_error(message), "{message}");
    }
    assert!(!is_retryable_stream_read_error("LLM API error (401)"));
    assert!(!is_retryable_stream_read_error(
        "Stream idle timeout after 300 seconds"
    ));
}

#[test]
fn oversized_model_response_is_identified_for_single_recovery_retry() {
    assert!(is_oversized_model_response_error(
        "Model response exceeded the 8000000-byte safety limit (received 8000123 bytes in 42.0s; parsed_text_bytes=0, tool_call_argument_bytes=7999000)."
    ));
    assert!(!is_oversized_model_response_error(
        "Stream read error after 6671 bytes: connection reset by peer"
    ));
}

#[test]
fn effective_output_budget_respects_context_and_global_cap() {
    let mut config = ConfigToml {
        model_context_window: Some(128_000),
        max_output_tokens: None,
        ..ConfigToml::default()
    };
    let messages = vec![InternalMessage {
        role: "user".to_string(),
        content: Some(serde_json::Value::String("short request".to_string())),
        tool_calls: None,
        tool_call_id: None,
        name: None,
    }];
    assert_eq!(effective_max_output_tokens(&config, &messages), 32_768);

    config.max_output_tokens = Some(64_000);
    assert_eq!(effective_max_output_tokens(&config, &messages), 32_768);

    config.model_context_window = Some(10_000);
    assert!(effective_max_output_tokens(&config, &messages) < 10_000);
}

#[test]
fn partial_stream_disconnect_is_retryable_not_successful_finish() {
    // Regression for the bug where a mid-stream disconnect with partial text
    // was treated as finish_reason=stream_error success and ended the turn.
    let message =
        "Stream read error after 13891 bytes, 126.6s elapsed: error decoding response body";
    assert!(
        is_retryable_stream_read_error(message),
        "partial stream disconnect must stay on the retryable path"
    );
    assert!(
        !is_length_truncated(Some("stream_error")),
        "stream_error must not be treated as a length-truncated success"
    );
}

#[test]
fn eof_without_terminal_marker_is_retryable_not_successful_finish() {
    assert!(stream_ended_without_terminal_marker(None));
    assert!(!stream_ended_without_terminal_marker(Some("stop")));
    assert!(!stream_ended_without_terminal_marker(Some("tool_calls")));

    let message = "Stream read error after 12584 bytes, 109.2s elapsed: unexpected EOF before terminal marker";
    assert!(
        is_retryable_stream_read_error(message),
        "an EOF without [DONE] or finish_reason must use the stream retry path"
    );
}

#[test]
fn stream_read_backoff_is_short_and_bounded() {
    assert_eq!(stream_read_backoff_ms(1), 1_000);
    assert_eq!(stream_read_backoff_ms(2), 2_000);
    assert_eq!(stream_read_backoff_ms(3), 4_000);
    assert_eq!(stream_read_backoff_ms(99), 10_000);
}

#[test]
fn parse_protocol_text_extracts_think_blocks() {
    let parsed = parse_protocol_text("前文<think>推理过程</think>后文");
    assert_eq!(parsed.visible, "前文后文");
    assert_eq!(parsed.reasoning, "推理过程");
    assert!(parsed.dsml_blocks.is_empty());
}

#[test]
fn textual_tool_protocol_leak_detects_recipient_markers() {
    assert!(looks_like_textual_tool_protocol_leak(
        "checking<|channel|>commentary to=shell"
    ));
}

#[test]
fn textual_tool_protocol_leak_detects_runaway_numbered_shell_labels() {
    assert!(looks_like_textual_tool_protocol_leak(
        "先检查目录 shell2 shell3 shell4 shell5 shell6 shell7 shell8 shell9"
    ));
}

#[test]
fn textual_tool_protocol_leak_allows_normal_shell_discussion() {
    assert!(!looks_like_textual_tool_protocol_leak(
        "Use the shell tool once, then explain the result."
    ));
}

#[test]
fn parse_protocol_text_strips_dsml_block() {
    let parsed = parse_protocol_text(
        "before<｜｜DSML｜｜tool_calls><｜｜DSML｜｜invoke name=\"request_user_input\"></｜｜DSML｜｜invoke></｜｜DSML｜｜tool_calls>after",
    );
    assert_eq!(parsed.visible, "beforeafter");
    assert_eq!(parsed.dsml_blocks.len(), 1);
}

#[test]
fn parse_dsml_tool_calls_block_parses_parameters() {
    let block = "<｜｜DSML｜｜invoke name=\"request_user_input\">\n<｜｜DSML｜｜parameter name=\"questions\" string=\"false\">[{\"id\":\"q1\",\"prompt\":\"继续吗?\",\"options\":[{\"id\":\"yes\",\"label\":\"继续\"}]}]</｜｜DSML｜｜parameter>\n</｜｜DSML｜｜invoke>";
    let calls = parse_dsml_tool_calls_block(block);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].name, "request_user_input");
    let arguments: serde_json::Value = serde_json::from_str(&calls[0].arguments).unwrap();
    assert_eq!(arguments["questions"][0]["id"], "q1");
}

#[test]
fn consume_protocol_text_delta_supports_fragmented_think_tags() {
    let mut state = ProtocolStreamState::default();
    let mut visible = String::new();
    let mut reasoning = String::new();
    for chunk in ["前文<th", "ink>思", "考</thi", "nk>后文"] {
        let parsed = consume_protocol_text_delta(&mut state, chunk);
        visible.push_str(&parsed.visible);
        reasoning.push_str(&parsed.reasoning);
    }
    let tail = flush_protocol_stream_state(&mut state);
    visible.push_str(&tail.visible);
    reasoning.push_str(&tail.reasoning);
    assert_eq!(visible, "前文后文");
    assert_eq!(reasoning, "思考");
}

#[test]
fn merge_git_changes_detects_content_change_for_existing_dirty_file() {
    let mut changes = Vec::new();
    let before = BTreeMap::from([("src/app.ts".to_string(), status_entry("M", Some(10)))]);
    let after = BTreeMap::from([("src/app.ts".to_string(), status_entry("M", Some(11)))]);

    merge_git_changes(&mut changes, &before, &after);

    assert_eq!(
        changes,
        vec![FileChange {
            path: "src/app.ts".to_string(),
            action: "modified".to_string(),
        }]
    );
}

#[test]
fn merge_git_changes_ignores_unchanged_dirty_file() {
    let mut changes = Vec::new();
    let before = BTreeMap::from([("src/app.ts".to_string(), status_entry("M", Some(10)))]);
    let after = BTreeMap::from([("src/app.ts".to_string(), status_entry("M", Some(10)))]);

    merge_git_changes(&mut changes, &before, &after);

    assert!(changes.is_empty());
}

#[test]
fn parse_git_status_line_decodes_quoted_utf8_octal_path() {
    let line = r#" M "education/docs/\346\225\231\350\202\262IDE_\351\234\200\346\261\202\346\226\207\346\241\243_\345\217\257\345\217\202\350\265\233\347\211\210.md""#;

    assert_eq!(
        parse_git_status_line(line),
        Some((
            "education/docs/教育IDE_需求文档_可参赛版.md".to_string(),
            "M".to_string(),
        ))
    );
}

#[test]
fn parse_git_status_line_keeps_plain_path() {
    assert_eq!(
        parse_git_status_line("?? src/new file.ts"),
        Some(("src/new file.ts".to_string(), "??".to_string()))
    );
}

#[test]
fn file_changes_from_apply_patch_tool_call_detects_changed_files() {
    let call = ToolCallRequest {
        id: "call-1".to_string(),
        name: "apply_patch".to_string(),
        arguments: serde_json::json!({
            "patch": "*** Begin Patch\n*** Add File: src/new.ts\n+hello\n*** Update File: src/old.ts\n@@\n-old\n+new\n*** Delete File: src/gone.ts\n*** End Patch"
        })
        .to_string(),
    };

    assert_eq!(
        file_changes_from_tool_call(&call),
        vec![
            FileChange {
                path: "src/new.ts".to_string(),
                action: "created".to_string(),
            },
            FileChange {
                path: "src/old.ts".to_string(),
                action: "modified".to_string(),
            },
            FileChange {
                path: "src/gone.ts".to_string(),
                action: "deleted".to_string(),
            },
        ]
    );
}

#[test]
fn file_changes_from_apply_patch_tool_call_accepts_raw_patch_text() {
    let call = ToolCallRequest {
        id: "call-raw".to_string(),
        name: "apply_patch".to_string(),
        arguments: "*** Begin Patch\n*** Add File: src/raw.ts\n+hello\n*** End Patch"
            .to_string(),
    };

    assert_eq!(
        file_changes_from_tool_call(&call),
        vec![FileChange {
            path: "src/raw.ts".to_string(),
            action: "created".to_string(),
        }]
    );
}

#[test]
fn file_changes_from_shell_tool_call_detects_set_content_write() {
    let call = ToolCallRequest {
        id: "call-shell-write".to_string(),
        name: "shell".to_string(),
        arguments: serde_json::json!({
            "command": "Set-Content -Path 'D:\\cncodetest\\index.html' -Value '<title>BBB</title>'"
        })
        .to_string(),
    };

    assert_eq!(
        file_changes_from_tool_call(&call),
        vec![FileChange {
            path: "D:/cncodetest/index.html".to_string(),
            action: "modified".to_string(),
        }]
    );
}

#[test]
fn file_changes_from_shell_tool_call_detects_redirection_target() {
    let call = ToolCallRequest {
        id: "call-shell-redirect".to_string(),
        name: "shell".to_string(),
        arguments: serde_json::json!({
            "command": "echo hello > D:\\cncodetest\\output.txt"
        })
        .to_string(),
    };

    assert_eq!(
        file_changes_from_tool_call(&call),
        vec![FileChange {
            path: "D:/cncodetest/output.txt".to_string(),
            action: "modified".to_string(),
        }]
    );
}

#[test]
fn file_changes_from_shell_tool_call_detects_variable_path_out_file() {
    let call = ToolCallRequest {
        id: "call-shell-var".to_string(),
        name: "shell_command".to_string(),
        arguments: serde_json::json!({
            "command": "$path = \"D:\\cncodetest\\cn-codex-site\\index.html\"\n$content = [System.IO.File]::ReadAllText($path)\n$content -replace '<title>AA</title>', '<title>DD</title>' | Out-File -FilePath $path -Encoding UTF8"
        })
        .to_string(),
    };

    assert_eq!(
        file_changes_from_tool_call(&call),
        vec![FileChange {
            path: "D:/cncodetest/cn-codex-site/index.html".to_string(),
            action: "modified".to_string(),
        }]
    );
}

#[test]
fn shell_extract_variable_assignments_parses_simple_assignments() {
    let cmd = "$path = \"D:\\cncodetest\\index.html\"\n$content = [System.IO.File]::ReadAllText($path)\n$content | Out-File -FilePath $path";
    let vars = shell_extract_variable_assignments(cmd);
    assert!(vars.len() >= 1);
    assert_eq!(vars[0].0, "path");
    assert_eq!(vars[0].1, "D:\\cncodetest\\index.html");
}

#[test]
fn file_change_snapshots_capture_before_and_after_content() {
    let root = std::env::temp_dir().join(format!(
        "cn-codex-file-snapshot-test-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let target = root.join("index.html");
    std::fs::write(&target, "<title>AAA</title>").unwrap();

    let changes = vec![FileChange {
        path: target.to_string_lossy().to_string(),
        action: "modified".to_string(),
    }];
    let mut snapshot_map = BTreeMap::new();
    capture_before_file_snapshots(&mut snapshot_map, &changes, &root);
    std::fs::write(&target, "<title>BBB</title>").unwrap();
    capture_after_file_snapshots(&mut snapshot_map, &changes, &root);
    let snapshots = build_changed_file_snapshots(&changes, &snapshot_map, &root);

    assert_eq!(snapshots.len(), 1);
    assert_eq!(
        snapshots[0].before_content.as_deref(),
        Some("<title>AAA</title>")
    );
    assert_eq!(
        snapshots[0].after_content.as_deref(),
        Some("<title>BBB</title>")
    );

    std::fs::remove_dir_all(root).ok();
}

#[test]
fn file_change_snapshots_keep_before_when_file_deleted() {
    let root = std::env::temp_dir().join(format!(
        "cn-codex-file-snapshot-delete-test-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let target = root.join("remove-me.txt");
    std::fs::write(&target, "to be deleted").unwrap();

    let changes = vec![FileChange {
        path: target.to_string_lossy().to_string(),
        action: "deleted".to_string(),
    }];
    let mut snapshot_map = BTreeMap::new();
    capture_before_file_snapshots(&mut snapshot_map, &changes, &root);
    std::fs::remove_file(&target).unwrap();
    capture_after_file_snapshots(&mut snapshot_map, &changes, &root);
    let snapshots = build_changed_file_snapshots(&changes, &snapshot_map, &root);

    assert_eq!(snapshots.len(), 1);
    assert_eq!(
        snapshots[0].before_content.as_deref(),
        Some("to be deleted")
    );
    assert_eq!(snapshots[0].after_content, None);

    std::fs::remove_dir_all(root).ok();
}

#[test]
fn multimodal_user_content_keeps_images_as_data_urls() {
    let content = multimodal_user_content(
        "What is in this image?",
        &[
            UserAttachment {
                name: "screen.png".to_string(),
                mime_type: "image/png".to_string(),
                data_url: "data:image/png;base64,abc123".to_string(),
                size: 42,
            },
            UserAttachment {
                name: "notes.txt".to_string(),
                mime_type: "text/plain".to_string(),
                data_url: "data:text/plain;base64,aGVsbG8=".to_string(),
                size: 5,
            },
        ],
    );

    assert_eq!(content[0]["type"], "text");
    assert_eq!(content[1]["type"], "image_url");
    assert_eq!(
        content[1]["image_url"]["url"],
        "data:image/png;base64,abc123"
    );
    assert!(content[0]["text"].as_str().unwrap().contains("notes.txt"));
}

#[tokio::test]
async fn resolve_image_context_with_fallback_keeps_images_when_model_supports_vision() {
    let temp_dir = tempfile::tempdir().unwrap();
    let cwd = temp_dir.path().to_path_buf();
    let thread_store = Arc::new(ThreadStore::new(&cwd.join("codey")));
    let tool_executor = ToolExecutor::new(cwd.clone());
    let engine = AgentEngine::new(thread_store, tool_executor, cwd).unwrap();

    let config = ConfigToml {
        model_supports_vision: Some(true),
        ..Default::default()
    };
    let attachments = vec![UserAttachment {
        name: "image.png".to_string(),
        mime_type: "image/png".to_string(),
        data_url: "data:image/png;base64,abc123".to_string(),
        size: 12,
    }];

    let (processed, fallback_context) = engine
        .resolve_image_context_with_fallback(
            &config,
            "describe image",
            "text-model",
            &attachments,
        )
        .await;

    assert_eq!(processed.len(), 1);
    assert!(processed[0].mime_type.starts_with("image/"));
    assert!(fallback_context.is_none());
}

#[tokio::test]
async fn resolve_image_context_with_fallback_drops_images_without_fallback_config() {
    let temp_dir = tempfile::tempdir().unwrap();
    let cwd = temp_dir.path().to_path_buf();
    let thread_store = Arc::new(ThreadStore::new(&cwd.join("codey")));
    let tool_executor = ToolExecutor::new(cwd.clone());
    let engine = AgentEngine::new(thread_store, tool_executor, cwd).unwrap();

    let config = ConfigToml {
        model_supports_vision: Some(false),
        ..Default::default()
    };
    let attachments = vec![
        UserAttachment {
            name: "image.png".to_string(),
            mime_type: "image/png".to_string(),
            data_url: "data:image/png;base64,abc123".to_string(),
            size: 12,
        },
        UserAttachment {
            name: "readme.txt".to_string(),
            mime_type: "text/plain".to_string(),
            data_url: "data:text/plain;base64,aGVsbG8=".to_string(),
            size: 5,
        },
    ];

    let (processed, fallback_context) = engine
        .resolve_image_context_with_fallback(
            &config,
            "describe image",
            "text-model",
            &attachments,
        )
        .await;

    assert_eq!(processed.len(), 1);
    assert!(!processed[0].mime_type.starts_with("image/"));
    assert!(fallback_context.is_some());
    assert!(
        fallback_context
            .as_deref()
            .unwrap_or_default()
            .contains("未配置可用的视觉后补")
    );
}

#[tokio::test]
async fn resolve_image_context_with_local_ocr_fallback_keeps_non_image_attachments() {
    let temp_dir = tempfile::tempdir().unwrap();
    let cwd = temp_dir.path().to_path_buf();
    let thread_store = Arc::new(ThreadStore::new(&cwd.join("codey")));
    let tool_executor = ToolExecutor::new(cwd.clone());
    let engine = AgentEngine::new(thread_store, tool_executor, cwd).unwrap();

    let config = ConfigToml {
        model_supports_vision: Some(false),
        vision_fallback_kind: Some("local_ocr".to_string()),
        ..Default::default()
    };
    let attachments = vec![
        UserAttachment {
            name: "image.png".to_string(),
            mime_type: "image/png".to_string(),
            data_url: "data:image/png;base64,abc123".to_string(),
            size: 12,
        },
        UserAttachment {
            name: "notes.txt".to_string(),
            mime_type: "text/plain".to_string(),
            data_url: "data:text/plain;base64,aGVsbG8=".to_string(),
            size: 5,
        },
    ];

    let (processed, fallback_context) = engine
        .resolve_image_context_with_fallback(
            &config,
            "extract text",
            "text-model",
            &attachments,
        )
        .await;

    assert_eq!(processed.len(), 1);
    assert_eq!(processed[0].mime_type, "text/plain");
    assert!(fallback_context.is_some());
    assert!(
        fallback_context
            .as_deref()
            .unwrap_or_default()
            .contains("本地 OCR")
    );
}

#[test]
fn turn_budget_limited_uses_total_tokens() {
    let usage = TurnUsage {
        prompt_tokens: 700,
        completion_tokens: 300,
        total_tokens: 1_000,
        ..Default::default()
    };

    assert!(turn_budget_limited(Some(1_000), &usage));
    assert!(turn_budget_limited(Some(999), &usage));
    assert!(!turn_budget_limited(Some(1_001), &usage));
    assert!(!turn_budget_limited(None, &usage));
}

#[test]
fn text_expresses_intent_detects_common_unfinished_work_phrases() {
    assert!(text_expresses_intent("让我先检查相关代码"));
    assert!(text_expresses_intent("开始落地改动：先改 agent 循环"));
    assert!(text_expresses_intent("继续实现自动续跑逻辑"));
    assert!(text_expresses_intent(
        "I'll implement the auto-continue path now"
    ));
    assert!(text_expresses_intent(
        "I need to read the file and update it"
    ));
    assert!(!text_expresses_intent("已完成修复，验证通过。"));
    assert!(!text_expresses_intent("Fix is complete and verified."));
}

#[test]
fn text_contains_unapplied_patch_detects_patch_shaped_final_answers() {
    assert!(text_contains_unapplied_patch(
        "下面是完整修改：\n```diff\n-old\n+new\n```"
    ));
    assert!(text_contains_unapplied_patch(
        "*** Begin Patch\n*** Update File: src/app.rs\n*** End Patch"
    ));
    assert!(!text_contains_unapplied_patch(
        "apply_patch 已成功执行，测试通过。"
    ));
}

#[test]
fn explicit_patch_text_only_requests_bypass_the_edit_guard() {
    for request in [
        "只展示补丁，不要应用",
        "不要修改文件，只给补丁",
        "Show me the patch, but do not apply it",
        "Patch only",
    ] {
        assert!(user_requested_patch_text_only(request), "request={request}");
    }
    assert!(!user_requested_patch_text_only("请直接修改代码并运行测试"));
}

#[test]
fn repeated_read_only_tool_call_blocks_only_consecutive_identical_calls() {
    let read = ToolCallRequest {
        id: "read-1".to_string(),
        name: "read_file".to_string(),
        arguments: r#"{"path":"src/main.rs","line_offset":1}"#.to_string(),
    };
    let mut last_signature = None;

    assert!(!repeated_read_only_tool_call(&mut last_signature, &read));
    assert!(repeated_read_only_tool_call(&mut last_signature, &read));

    let patch = ToolCallRequest {
        id: "patch-1".to_string(),
        name: "apply_patch".to_string(),
        arguments: "*** Begin Patch\n*** End Patch".to_string(),
    };
    assert!(!repeated_read_only_tool_call(&mut last_signature, &patch));
    assert!(!repeated_read_only_tool_call(&mut last_signature, &read));
}

#[test]
fn repeated_read_only_tool_call_blocks_identical_code_reviews() {
    let review = ToolCallRequest {
        id: "review-1".to_string(),
        name: "code_review".to_string(),
        arguments: r#"{"base_ref":"HEAD","paths":["src/main.rs"]}"#.to_string(),
    };
    let mut last_signature = None;

    assert!(!repeated_read_only_tool_call(
        &mut last_signature,
        &review
    ));
    assert!(repeated_read_only_tool_call(
        &mut last_signature,
        &review
    ));
}

#[test]
fn repeated_read_only_tool_call_distinguishes_different_ranges() {
    let mut last_signature = None;
    let first = ToolCallRequest {
        id: "read-1".to_string(),
        name: "read_file".to_string(),
        arguments: r#"{"path":"src/main.rs","line_offset":1}"#.to_string(),
    };
    let second = ToolCallRequest {
        id: "read-2".to_string(),
        name: "read_file".to_string(),
        arguments: r#"{"path":"src/main.rs","line_offset":201}"#.to_string(),
    };

    assert!(!repeated_read_only_tool_call(&mut last_signature, &first));
    assert!(!repeated_read_only_tool_call(&mut last_signature, &second));
}

#[test]
fn repeated_read_only_tool_call_blocks_git_inspection_shell_pipeline() {
    let command = "git status --short | head -20; echo '---LAST COMMIT---'; git log -1 --pretty=format:\"%h %s %ad\" --date=short";

    for (tool_name, command_key) in [
        ("shell", "command"),
        ("shell_command", "command"),
        ("exec_command", "cmd"),
    ] {
        let call = ToolCallRequest {
            id: format!("{tool_name}-1"),
            name: tool_name.to_string(),
            arguments: serde_json::json!({ command_key: command }).to_string(),
        };
        let mut last_signature = None;

        assert!(is_read_only_shell_tool_call(&call), "tool={tool_name}");
        assert!(!repeated_read_only_tool_call(&mut last_signature, &call));
        assert!(repeated_read_only_tool_call(&mut last_signature, &call));
    }
}

#[test]
fn repeated_read_only_shell_call_allows_a_different_git_query() {
    let mut last_signature = None;
    let status = ToolCallRequest {
        id: "status-1".to_string(),
        name: "shell".to_string(),
        arguments: serde_json::json!({ "command": "git status --short" }).to_string(),
    };
    let log = ToolCallRequest {
        id: "log-1".to_string(),
        name: "shell".to_string(),
        arguments: serde_json::json!({ "command": "git log -1 --oneline" }).to_string(),
    };

    assert!(!repeated_read_only_tool_call(&mut last_signature, &status));
    assert!(!repeated_read_only_tool_call(&mut last_signature, &log));
    assert!(repeated_read_only_tool_call(&mut last_signature, &log));
}

#[test]
fn read_only_shell_detection_rejects_commands_with_side_effects() {
    for command in [
        "cargo test",
        "git commit -am 'checkpoint'",
        "git reset --hard HEAD~1",
        "git branch new-branch",
        "git diff --output=changes.patch",
        "git status --short > status.txt",
        "git status --short; Set-Content status.txt done",
        "git status --short | Tee-Object status.txt",
        "git status --short; $(Remove-Item status.txt)",
    ] {
        assert!(
            !is_read_only_shell_command(command),
            "unexpectedly classified as read-only: {command}"
        );
    }
}

#[test]
fn file_edit_resets_repeated_read_only_shell_detection() {
    let shell = ToolCallRequest {
        id: "status-1".to_string(),
        name: "shell".to_string(),
        arguments: serde_json::json!({ "command": "git status --short" }).to_string(),
    };
    let patch = ToolCallRequest {
        id: "patch-1".to_string(),
        name: "apply_patch".to_string(),
        arguments: "*** Begin Patch\n*** End Patch".to_string(),
    };
    let mut last_signature = None;

    assert!(!repeated_read_only_tool_call(&mut last_signature, &shell));
    assert!(repeated_read_only_tool_call(&mut last_signature, &shell));
    assert!(!repeated_read_only_tool_call(&mut last_signature, &patch));
    assert!(!repeated_read_only_tool_call(&mut last_signature, &shell));
}

#[test]
fn repeated_read_only_shell_stops_after_two_blocked_repeats() {
    let mut repeat_count = 0;

    assert!(!blocked_read_only_shell_repeat_should_stop(
        &mut repeat_count
    ));
    assert!(blocked_read_only_shell_repeat_should_stop(
        &mut repeat_count
    ));
    assert_eq!(repeat_count, 2);
}

#[test]
fn is_length_truncated_recognizes_provider_reasons() {
    assert!(is_length_truncated(Some("length")));
    assert!(is_length_truncated(Some("MAX_TOKENS")));
    assert!(is_length_truncated(Some("max_output_tokens")));
    assert!(!is_length_truncated(Some("stop")));
    assert!(!is_length_truncated(None));
}

#[test]
fn nonzero_turn_usage_keeps_call_count_when_tokens_are_zero() {
    let usage = TurnUsage {
        call_count: 2,
        ..Default::default()
    };
    let normalized =
        nonzero_turn_usage(&usage).expect("call_count should keep usage non-empty");
    assert_eq!(normalized.call_count, 2);
    assert_eq!(normalized.total_tokens, 0);
}

#[test]
fn normalize_tool_call_requests_fills_missing_ids() {
    let calls = normalize_tool_call_requests(vec![
        ToolCallRequest {
            id: String::new(),
            name: "apply_patch".to_string(),
            arguments: "*** Begin Patch\n*** End Patch".to_string(),
        },
        ToolCallRequest {
            id: "call-real".to_string(),
            name: "shell".to_string(),
            arguments: "{}".to_string(),
        },
    ]);

    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].id, "call_apply_patch_0");
    assert_eq!(calls[1].id, "call-real");
}

#[test]
fn normalize_tool_call_requests_preserves_invalid_calls_for_protocol_recovery() {
    let calls = normalize_tool_call_requests(vec![
        ToolCallRequest {
            id: "empty-shell".to_string(),
            name: "shell_command".to_string(),
            arguments: r#"{"command":""}"#.to_string(),
        },
        ToolCallRequest {
            id: "placeholder-shell".to_string(),
            name: "shell_command".to_string(),
            arguments: r##"{"command":"[ ] # try shell"}"##.to_string(),
        },
    ]);

    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].id, "empty-shell");
    assert_eq!(calls[1].id, "placeholder-shell");
}

#[test]
fn reorder_history_system_messages_for_model_moves_system_to_front() {
    let history = vec![
        test_thread_message("u1", "user", "hi"),
        test_thread_message("s1", "system", "note-a"),
        test_thread_message("a1", "assistant", "hello"),
        test_thread_message("s2", "system", "note-b"),
        test_thread_message("t1", "tool", "ok"),
    ];

    let reordered = reorder_history_system_messages_for_model(&history);
    let roles: Vec<&str> = reordered.iter().map(|msg| msg.role.as_str()).collect();
    assert_eq!(roles, vec!["system", "system", "user", "assistant", "tool"]);
    assert_eq!(reordered[0].id, "s1");
    assert_eq!(reordered[1].id, "s2");
}

#[test]
fn build_internal_messages_keeps_system_messages_before_non_system_roles() {
    let workspace_dir =
        std::env::temp_dir().join(format!("cn-codex-agent-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&workspace_dir).expect("create temp workspace");

    let thread_store = Arc::new(ThreadStore::new(&workspace_dir.join("codey")));
    let tool_executor = ToolExecutor::new(workspace_dir.clone());
    let engine =
        AgentEngine::new(thread_store, tool_executor, workspace_dir.clone()).expect("engine");
    let config = ConfigToml::default();
    let history = vec![
        test_thread_message("u1", "user", "hello"),
        test_thread_message("s1", "system", "runtime note"),
        test_thread_message("a1", "assistant", "reply"),
    ];

    let messages = engine.build_internal_messages(
        &config,
        &history,
        &workspace_dir,
        "chat",
        None,
        None,
        &[],
        None,
        None,
        None,
    );

    let first_non_system = messages
        .iter()
        .position(|msg| msg.role != "system")
        .expect("should contain non-system messages");
    assert!(
        messages[first_non_system..]
            .iter()
            .all(|msg| msg.role != "system")
    );
    assert!(messages.iter().any(|msg| {
        msg.role == "system"
            && matches!(
                msg.content.as_ref(),
                Some(serde_json::Value::String(content)) if content == "runtime note"
            )
    }));
}

#[test]
fn build_internal_messages_keeps_reasoning_on_tool_call_turns() {
    let workspace_dir =
        std::env::temp_dir().join(format!("cn-codex-agent-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&workspace_dir).expect("create temp workspace");

    let thread_store = Arc::new(ThreadStore::new(&workspace_dir.join("codey")));
    let tool_executor = ToolExecutor::new(workspace_dir.clone());
    let engine =
        AgentEngine::new(thread_store, tool_executor, workspace_dir.clone()).expect("engine");
    let config = ConfigToml::default();
    let mut assistant = test_thread_message("a1", "assistant", "");
    assistant.tool_calls = Some(vec![ToolCallInfo {
        id: "call-weather".to_string(),
        name: "get_weather".to_string(),
        arguments: r#"{"city":"Paris"}"#.to_string(),
        reasoning_content: Some("provider thinking".to_string()),
    }]);
    let mut tool = test_thread_message("t1", "tool", "sunny");
    tool.tool_call_id = Some("call-weather".to_string());
    tool.tool_name = Some("get_weather".to_string());

    let messages = engine.build_internal_messages(
        &config,
        &[assistant, tool],
        &workspace_dir,
        "chat",
        None,
        None,
        &[],
        None,
        None,
        None,
    );

    let assistant = messages
        .iter()
        .find(|message| message.role == "assistant" && message.tool_calls.is_some())
        .expect("assistant tool-call message");
    let call = assistant
        .tool_calls
        .as_ref()
        .and_then(|calls| calls.first())
        .expect("tool call");
    assert_eq!(call.reasoning_content.as_deref(), Some("provider thinking"));
}

#[test]
fn system_prompt_limits_tool_preamble_to_the_first_batch() {
    let workspace_dir =
        std::env::temp_dir().join(format!("cn-codex-agent-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&workspace_dir).expect("create temp workspace");

    let thread_store = Arc::new(ThreadStore::new(&workspace_dir.join("codey")));
    let tool_executor = ToolExecutor::new(workspace_dir.clone());
    let engine =
        AgentEngine::new(thread_store, tool_executor, workspace_dir.clone()).expect("engine");

    let prompt = engine.build_system_prompt(&ConfigToml::default(), &workspace_dir, "chat", None);

    assert!(prompt.contains("Before the first tool call in a user turn"));
    assert!(prompt.contains("do not restate or paraphrase the plan"));
    assert!(prompt.contains("Never repeat a previous progress update"));
    assert!(!prompt.contains("Before using any tools, always briefly explain"));
}

#[test]
fn repetitive_progress_updates_detect_exact_and_paraphrased_text() {
    let first = "数据库配置的存储和测试链路已经清楚了。我再核对状态读写、i18n 和 prompt 注入，然后按同样模式加上 SSH 多服务器配置。";
    let paraphrase = "数据库配置的存储和测试链路已经清楚了。我再核对 prompt 注入和命令注册细节，然后按同一套模式加上 SSH 多服务器配置。";

    assert!(progress_updates_are_repetitive(first, first));
    assert!(progress_updates_are_repetitive(first, paraphrase));
    assert!(!progress_updates_are_repetitive(
        first,
        "SSH 测试连接失败：当前构建缺少 russh feature。我会修正 Cargo 配置并重新编译。"
    ));
}

#[test]
fn sanitize_history_for_model_skips_orphan_tool_messages() {
    let history = vec![
        ThreadMessage {
            id: "tool-only".to_string(),
            role: "tool".to_string(),
            content: "result".to_string(),
            timestamp: 1,
            tool_call_id: Some("call-missing".to_string()),
            tool_name: Some("shell".to_string()),
            tool_calls: None,
            attachments: Vec::new(),
        },
        ThreadMessage {
            id: "assistant-call".to_string(),
            role: "assistant".to_string(),
            content: String::new(),
            timestamp: 2,
            tool_call_id: None,
            tool_name: None,
            tool_calls: Some(vec![ToolCallInfo {
                id: "call-ok".to_string(),
                name: "shell".to_string(),
                arguments: "{}".to_string(),
                reasoning_content: None,
            }]),
            attachments: Vec::new(),
        },
        ThreadMessage {
            id: "tool-ok".to_string(),
            role: "tool".to_string(),
            content: "done".to_string(),
            timestamp: 3,
            tool_call_id: Some("call-ok".to_string()),
            tool_name: Some("shell".to_string()),
            tool_calls: None,
            attachments: Vec::new(),
        },
    ];

    let sanitized = sanitize_history_for_model(&history);
    assert_eq!(sanitized.len(), 2);
    assert_eq!(sanitized[0].id, "assistant-call");
    assert_eq!(sanitized[1].id, "tool-ok");
}

#[test]
fn sanitize_history_for_model_recombines_preamble_with_its_tool_calls() {
    let history = vec![
        ThreadMessage {
            id: "assistant-preamble".to_string(),
            role: "assistant".to_string(),
            content: "I will inspect the relevant files.".to_string(),
            timestamp: 7,
            tool_call_id: None,
            tool_name: None,
            tool_calls: None,
            attachments: Vec::new(),
        },
        ThreadMessage {
            id: "assistant-call".to_string(),
            role: "assistant".to_string(),
            content: String::new(),
            timestamp: 7,
            tool_call_id: None,
            tool_name: None,
            tool_calls: Some(vec![ToolCallInfo {
                id: "call-read".to_string(),
                name: "read_file".to_string(),
                arguments: r#"{"path":"src/lib.rs"}"#.to_string(),
                reasoning_content: None,
            }]),
            attachments: Vec::new(),
        },
        ThreadMessage {
            id: "tool-result".to_string(),
            role: "tool".to_string(),
            content: "file contents".to_string(),
            timestamp: 8,
            tool_call_id: Some("call-read".to_string()),
            tool_name: Some("read_file".to_string()),
            tool_calls: None,
            attachments: Vec::new(),
        },
    ];

    let sanitized = sanitize_history_for_model(&history);

    assert_eq!(sanitized.len(), 2);
    assert_eq!(sanitized[0].id, "assistant-call");
    assert_eq!(sanitized[0].content, "I will inspect the relevant files.");
    assert_eq!(sanitized[0].tool_calls.as_ref().unwrap().len(), 1);
    assert_eq!(sanitized[1].id, "tool-result");
}

#[test]
fn sanitize_history_for_model_adds_aborted_result_for_dangling_call() {
    let history = vec![ThreadMessage {
        id: "assistant-call".to_string(),
        role: "assistant".to_string(),
        content: String::new(),
        timestamp: 7,
        tool_call_id: None,
        tool_name: None,
        tool_calls: Some(vec![ToolCallInfo {
            id: "call-interrupted".to_string(),
            name: "shell".to_string(),
            arguments: "{}".to_string(),
            reasoning_content: None,
        }]),
        attachments: Vec::new(),
    }];

    let sanitized = sanitize_history_for_model(&history);
    assert_eq!(sanitized.len(), 2);
    assert_eq!(sanitized[0].id, "assistant-call");
    assert_eq!(
        sanitized[1].tool_call_id.as_deref(),
        Some("call-interrupted")
    );
    assert_eq!(
        sanitized[1].content,
        "Tool execution aborted before a result was recorded."
    );
}

#[test]
fn sanitize_history_for_model_keeps_only_first_tool_result_per_call() {
    let history = vec![
        ThreadMessage {
            id: "assistant-call".to_string(),
            role: "assistant".to_string(),
            content: String::new(),
            timestamp: 1,
            tool_call_id: None,
            tool_name: None,
            tool_calls: Some(vec![ToolCallInfo {
                id: "call-1".to_string(),
                name: "shell".to_string(),
                arguments: "{}".to_string(),
                reasoning_content: None,
            }]),
            attachments: Vec::new(),
        },
        ThreadMessage {
            id: "tool-first".to_string(),
            role: "tool".to_string(),
            content: "first".to_string(),
            timestamp: 2,
            tool_call_id: Some("call-1".to_string()),
            tool_name: Some("shell".to_string()),
            tool_calls: None,
            attachments: Vec::new(),
        },
        ThreadMessage {
            id: "tool-duplicate".to_string(),
            role: "tool".to_string(),
            content: "duplicate".to_string(),
            timestamp: 3,
            tool_call_id: Some("call-1".to_string()),
            tool_name: Some("shell".to_string()),
            tool_calls: None,
            attachments: Vec::new(),
        },
    ];

    let sanitized = sanitize_history_for_model(&history);
    assert_eq!(sanitized.len(), 2);
    assert_eq!(sanitized[1].id, "tool-first");
}

#[test]
fn sanitize_history_for_model_remaps_reused_tool_call_ids_in_order() {
    let tool_call = |id: &str, arguments: &str| ToolCallInfo {
        id: id.to_string(),
        name: "apply_patch".to_string(),
        arguments: arguments.to_string(),
        reasoning_content: None,
    };
    let tool_result = |id: &str, content: &str, timestamp| ThreadMessage {
        id: format!("tool-{timestamp}"),
        role: "tool".to_string(),
        content: content.to_string(),
        timestamp,
        tool_call_id: Some(id.to_string()),
        tool_name: Some("apply_patch".to_string()),
        tool_calls: None,
        attachments: Vec::new(),
    };
    let assistant_call = |timestamp, arguments: &str| ThreadMessage {
        id: format!("assistant-{timestamp}"),
        role: "assistant".to_string(),
        content: String::new(),
        timestamp,
        tool_call_id: None,
        tool_name: None,
        tool_calls: Some(vec![tool_call("call_apply_patch_0", arguments)]),
        attachments: Vec::new(),
    };
    let history = vec![
        assistant_call(1, "first"),
        tool_result("call_apply_patch_0", "first result", 2),
        assistant_call(3, "second"),
        tool_result("call_apply_patch_0", "second result", 4),
    ];

    let sanitized = sanitize_history_for_model(&history);

    assert_eq!(sanitized.len(), 4);
    assert_eq!(
        sanitized[0].tool_calls.as_ref().unwrap()[0].id,
        "call_apply_patch_0"
    );
    assert_eq!(
        sanitized[1].tool_call_id.as_deref(),
        Some("call_apply_patch_0")
    );
    assert_eq!(
        sanitized[2].tool_calls.as_ref().unwrap()[0].id,
        "call_apply_patch_0__2"
    );
    assert_eq!(
        sanitized[3].tool_call_id.as_deref(),
        Some("call_apply_patch_0__2")
    );
    assert_eq!(sanitized[3].content, "second result");
}

#[test]
fn uniquify_tool_call_ids_preserves_every_result_mapping() {
    let mut issued = HashSet::from(["call_apply_patch_0".to_string()]);
    let calls = uniquify_tool_call_ids(
        vec![
            ToolCallRequest {
                id: "call_apply_patch_0".to_string(),
                name: "apply_patch".to_string(),
                arguments: "first".to_string(),
            },
            ToolCallRequest {
                id: "call_apply_patch_0".to_string(),
                name: "apply_patch".to_string(),
                arguments: "second".to_string(),
            },
        ],
        &mut issued,
    );

    assert_eq!(calls[0].id, "call_apply_patch_0__2");
    assert_eq!(calls[1].id, "call_apply_patch_0__3");
}

#[test]
fn patch_refresh_guard_matches_a_path_marked_after_a_stale_hunk_failure() {
    let changes = vec![FileChange {
        path: "src/i18n/zh-CN/common.json".to_string(),
        action: "modified".to_string(),
    }];
    let stale = HashSet::from(["src/i18n/zh-CN/common.json".to_string()]);

    assert_eq!(
        patch_paths_requiring_refresh_for(&changes, &stale),
        vec!["src/i18n/zh-CN/common.json".to_string()]
    );
    assert_eq!(
        read_file_path_from_tool_args(r#"{"path":"src/i18n/zh-CN/common.json"}"#),
        Some("src/i18n/zh-CN/common.json".to_string())
    );
}

#[test]
fn patch_refresh_guard_matches_provider_trailing_star_paths_after_read() {
    let arguments = serde_json::json!({
        "patch": "*** Begin Patch ***\n*** Update File: D:\\work\\BattleManager.gd ***\n@@\n-old\n+new\n*** End Patch ***"
    })
    .to_string();
    let changes = apply_patch_changes_from_args(&arguments);

    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].path, "D:/work/BattleManager.gd");

    let mut requiring_refresh = HashSet::from([changes[0].path.clone()]);
    let read_path =
        read_file_path_from_tool_args(r#"{"path":"D:\\work\\BattleManager.gd"}"#).unwrap();
    requiring_refresh.retain(|changed_path| !paths_match(changed_path, &read_path));

    assert!(requiring_refresh.is_empty());
    assert!(paths_match(
        "D:/work/BattleManager.gd ***",
        "D:\\work\\BattleManager.gd"
    ));
}

#[test]
fn apply_patch_parse_errors_are_not_recorded_as_successful_edits() {
    assert!(!tool_result_success(
        "apply_patch",
        "patch must start with *** Begin Patch"
    ));
    assert!(!tool_result_success(
        "apply_patch",
        "Error applying patch: failed to match hunk"
    ));
    assert!(tool_result_success(
        "apply_patch",
        "Success. Applied patch.\n- modified src/i18n/zh-CN/common.json"
    ));
    assert!(!tool_result_success(
        "write_file",
        "Error writing existing.md: access denied"
    ));
    assert!(tool_result_success(
        "write_file",
        "Successfully wrote 12 bytes to new.md"
    ));
}

#[test]
fn failed_file_edits_are_explicit_in_final_text() {
    let text = final_text_with_failed_file_edit_status("The requested change is complete.");

    assert!(text.starts_with("File modification status: failed."));
    assert!(text.contains("No apply_patch/write_file call wrote a file successfully"));
    assert!(text.contains("The requested change is complete."));
}

#[test]
fn failed_patch_blocks_write_file_fallback_for_existing_files() {
    let temp_dir = tempfile::tempdir().expect("should create temp dir");
    let existing_path = temp_dir.path().join("document.md");
    std::fs::write(&existing_path, "original content").expect("should create existing file");
    let existing_changes = vec![FileChange {
        path: existing_path.to_string_lossy().to_string(),
        action: "modified".to_string(),
    }];

    assert!(should_block_write_file_after_patch_failure(
        "write_file",
        true,
        &existing_changes,
        temp_dir.path(),
    ));
    assert!(!should_block_write_file_after_patch_failure(
        "write_file",
        false,
        &existing_changes,
        temp_dir.path(),
    ));

    let new_changes = vec![FileChange {
        path: "new-document.md".to_string(),
        action: "modified".to_string(),
    }];
    assert!(!should_block_write_file_after_patch_failure(
        "write_file",
        true,
        &new_changes,
        temp_dir.path(),
    ));
}

#[test]
fn stale_hunk_failure_requires_a_file_refresh_before_retry() {
    assert!(apply_patch_failure_requires_refresh(
        "Error applying patch: failed to match hunk in DESIGN.md"
    ));
    assert!(!apply_patch_failure_requires_refresh(
        "Error applying patch: patch must start with *** Begin Patch"
    ));
}

#[test]
fn repeated_failed_patch_limit_allows_one_correction_then_stops() {
    let mut duplicate_count = 0;

    assert!(!repeated_failed_patch_limit_reached(
        &mut duplicate_count,
        true,
        2,
    ));
    assert!(repeated_failed_patch_limit_reached(
        &mut duplicate_count,
        true,
        2,
    ));
    assert!(!repeated_failed_patch_limit_reached(
        &mut duplicate_count,
        false,
        2,
    ));
    assert_eq!(duplicate_count, 0);
}

#[test]
fn sanitize_history_for_model_removes_empty_tool_call_ids_from_assistant_and_tool() {
    let history = vec![
        ThreadMessage {
            id: "assistant-bad".to_string(),
            role: "assistant".to_string(),
            content: String::new(),
            timestamp: 1,
            tool_call_id: None,
            tool_name: None,
            tool_calls: Some(vec![ToolCallInfo {
                id: String::new(),
                name: "list_directory".to_string(),
                arguments: "{}".to_string(),
                reasoning_content: None,
            }]),
            attachments: Vec::new(),
        },
        ThreadMessage {
            id: "tool-bad".to_string(),
            role: "tool".to_string(),
            content: "files".to_string(),
            timestamp: 2,
            tool_call_id: Some(String::new()),
            tool_name: Some("list_directory".to_string()),
            tool_calls: None,
            attachments: Vec::new(),
        },
        ThreadMessage {
            id: "assistant-ok".to_string(),
            role: "assistant".to_string(),
            content: String::new(),
            timestamp: 3,
            tool_call_id: None,
            tool_name: None,
            tool_calls: Some(vec![ToolCallInfo {
                id: "call-ok".to_string(),
                name: "list_directory".to_string(),
                arguments: "{}".to_string(),
                reasoning_content: None,
            }]),
            attachments: Vec::new(),
        },
        ThreadMessage {
            id: "tool-ok".to_string(),
            role: "tool".to_string(),
            content: "files".to_string(),
            timestamp: 4,
            tool_call_id: Some("call-ok".to_string()),
            tool_name: Some("list_directory".to_string()),
            tool_calls: None,
            attachments: Vec::new(),
        },
    ];

    let sanitized = sanitize_history_for_model(&history);
    assert_eq!(sanitized.len(), 2);
    assert_eq!(sanitized[0].id, "assistant-ok");
    assert_eq!(sanitized[1].id, "tool-ok");
}

#[test]
fn apply_tool_result_sliding_window_keeps_recent_full_and_summarizes_older() {
    let mut history = Vec::new();
    for idx in 1..=8 {
        history.push(ThreadMessage {
            id: format!("assistant-{idx}"),
            role: "assistant".to_string(),
            content: String::new(),
            timestamp: idx * 2 - 1,
            tool_call_id: None,
            tool_name: None,
            tool_calls: Some(vec![ToolCallInfo {
                id: format!("call-{idx}"),
                name: "shell".to_string(),
                arguments: "{}".to_string(),
                reasoning_content: None,
            }]),
            attachments: Vec::new(),
        });
        history.push(ThreadMessage {
            id: format!("tool-{idx}"),
            role: "tool".to_string(),
            content: format!("FULL_RESULT_{idx}_{}", "x".repeat(1200)),
            timestamp: idx * 2,
            tool_call_id: Some(format!("call-{idx}")),
            tool_name: Some("shell".to_string()),
            tool_calls: None,
            attachments: Vec::new(),
        });
    }

    apply_tool_result_sliding_window(&mut history, 6, 10);

    let tool_messages: Vec<_> = history.iter().filter(|msg| msg.role == "tool").collect();
    assert_eq!(tool_messages.len(), 8);

    // Oldest 2 tool results should be summarized.
    assert!(
        tool_messages[0]
            .content
            .starts_with("[older tool result summarized]")
    );
    assert!(
        tool_messages[1]
            .content
            .starts_with("[older tool result summarized]")
    );
    assert!(tool_messages[0].content.contains("tool=shell"));
    assert!(!tool_messages[0].content.contains(&"x".repeat(1200)));

    // Most recent 6 tool results stay intact.
    for msg in &tool_messages[2..] {
        assert!(msg.content.starts_with("FULL_RESULT_"));
        assert!(!msg.content.starts_with("[older tool result summarized]"));
    }
}

#[test]
fn default_tool_result_window_covers_long_turns() {
    assert!(TOOL_RESULT_FULL_RETENTION >= 100);
    assert!(TOOL_RESULT_EXTENDED_RETENTION >= TOOL_RESULT_FULL_RETENTION);
    assert!(ROBOT_TOOL_RESULT_FULL_RETENTION < TOOL_RESULT_FULL_RETENTION);
    assert!(ROBOT_TOOL_RESULT_EXTENDED_RETENTION < TOOL_RESULT_EXTENDED_RETENTION);
    assert!(ROBOT_TOOL_RESULT_EXTENDED_RETENTION >= ROBOT_TOOL_RESULT_FULL_RETENTION);
    assert_eq!(
        tool_result_retention_for(
            "shell",
            "ok",
            ROBOT_TOOL_RESULT_FULL_RETENTION,
            ROBOT_TOOL_RESULT_EXTENDED_RETENTION,
        ),
        24
    );
    assert_eq!(
        tool_result_retention_for(
            "read_file",
            "source",
            ROBOT_TOOL_RESULT_FULL_RETENTION,
            ROBOT_TOOL_RESULT_EXTENDED_RETENTION,
        ),
        36
    );
}

#[test]
fn summarize_old_tool_result_is_idempotent_marker() {
    let summarized = summarize_old_tool_result("shell", &"a".repeat(2000), 800);
    assert!(is_already_summarized_tool_result(&summarized));
    assert!(summarized.contains("original_chars=2000"));
    assert!(summarized.contains("...[truncated]..."));
}

#[test]
fn apply_tool_result_sliding_window_extends_high_value_results() {
    let mut history = Vec::new();
    for idx in 1..=12 {
        let tool_name = if idx <= 4 {
            "read_file"
        } else if idx == 5 {
            "shell"
        } else {
            "list_directory"
        };
        let content = if idx == 5 {
            format!(
                "command failed with exit code 1\nerror: Permission denied\n{}",
                "y".repeat(900)
            )
        } else {
            format!("FULL_RESULT_{idx}_{}", "x".repeat(900))
        };

        history.push(ThreadMessage {
            id: format!("assistant-{idx}"),
            role: "assistant".to_string(),
            content: String::new(),
            timestamp: idx * 2 - 1,
            tool_call_id: None,
            tool_name: None,
            tool_calls: Some(vec![ToolCallInfo {
                id: format!("call-{idx}"),
                name: tool_name.to_string(),
                arguments: "{}".to_string(),
                reasoning_content: None,
            }]),
            attachments: Vec::new(),
        });
        history.push(ThreadMessage {
            id: format!("tool-{idx}"),
            role: "tool".to_string(),
            content,
            timestamp: idx * 2,
            tool_call_id: Some(format!("call-{idx}")),
            tool_name: Some(tool_name.to_string()),
            tool_calls: None,
            attachments: Vec::new(),
        });
    }

    apply_tool_result_sliding_window(&mut history, 6, 10);

    let tool_messages: Vec<_> = history.iter().filter(|msg| msg.role == "tool").collect();
    assert_eq!(tool_messages.len(), 12);

    // Ordinary list_directory older than the default window is summarized.
    // Positions: 0..3 read_file, 4 shell(failure), 5..11 list/read-like.
    // With extended retention=10, only results older than 10 are compressed
    // when high-value; default tools compress beyond 6.
    assert!(
        tool_messages[0]
            .content
            .starts_with("[older tool result summarized]"),
        "very old high-value result beyond extended window should summarize"
    );
    assert!(
        tool_messages[1]
            .content
            .starts_with("[older tool result summarized]"),
        "second-oldest high-value result beyond extended window should summarize"
    );
    // High-value results within extended window stay full.
    assert!(
        tool_messages[2].content.starts_with("FULL_RESULT_"),
        "read_file within extended window should remain full"
    );
    assert!(
        tool_messages[3].content.starts_with("FULL_RESULT_"),
        "read_file within extended window should remain full"
    );
    assert!(
        tool_messages[4].content.contains("exit code 1"),
        "failure signal should keep full content inside extended window"
    );
    assert!(
        !tool_messages[4]
            .content
            .starts_with("[older tool result summarized]")
    );

    // Ordinary tool just outside the default window is still compressed.
    assert!(
        tool_messages[5]
            .content
            .starts_with("[older tool result summarized]"),
        "ordinary list_directory beyond default window should summarize"
    );

    // Recent default-window results remain full.
    for msg in &tool_messages[6..] {
        assert!(!msg.content.starts_with("[older tool result summarized]"));
    }
}

#[test]
fn summarize_old_tool_result_preserves_critical_lines() {
    let mut body = String::new();
    body.push_str("start padding ");
    body.push_str(&"a".repeat(500));
    body.push_str("\nerror: compilation failed at src/agent.rs:120\n");
    body.push_str(&"b".repeat(500));
    body.push_str("\nexit code: 101\n");
    body.push_str(&"c".repeat(500));
    body.push_str("\nend padding");

    let summarized = summarize_old_tool_result("shell", &body, 800);
    assert!(is_already_summarized_tool_result(&summarized));
    assert!(summarized.contains("...[critical lines]..."));
    assert!(summarized.contains("error: compilation failed at src/agent.rs:120"));
    assert!(summarized.contains("exit code: 101"));
    assert!(summarized.contains("original_chars="));
    assert!(summarized.contains("...[truncated]..."));
}

#[test]
fn blocked_tool_call_output_surfaces_hook_reason_and_source() {
    let hook = HookRunResult {
        id: "run".to_string(),
        event: HOOK_COMMAND_EXEC.to_string(),
        source_type: "plugin".to_string(),
        source_name: "Safety".to_string(),
        command: "python hook.py".to_string(),
        status: "blocked".to_string(),
        exit_code: Some(0),
        stdout: String::new(),
        stderr: String::new(),
        error: None,
        duration_ms: 1,
        decision: Some("block".to_string()),
        reason: Some("do not run that".to_string()),
        additional_context: None,
        updated_input: None,
        invalid_output: None,
    };

    assert_eq!(
        blocked_tool_call_output("shell", &hook),
        "Tool call blocked by PreToolUse hook: do not run that. Tool: shell. Hook: python hook.py (Safety)"
    );
}

#[test]
fn append_post_tool_hook_feedback_adds_visible_section() {
    assert_eq!(
        append_post_tool_hook_feedback(
            "tool output".to_string(),
            vec![
                "Additional context from hook `a`: remember this".to_string(),
                "Feedback from hook `b`: review that".to_string(),
            ],
        ),
        "tool output\n\n[PostToolUse hook feedback]\nAdditional context from hook `a`: remember this\nFeedback from hook `b`: review that"
    );
}

#[test]
fn user_prompt_submit_hook_context_matches_codex_shape() {
    let context = user_prompt_submit_hook_context(
        "turn-1",
        "goal",
        Path::new("D:/work"),
        "gpt-test",
        "please continue",
    );

    assert_eq!(context["hookEventName"], "UserPromptSubmit");
    assert_eq!(context["turnId"], "turn-1");
    assert_eq!(context["mode"], "goal");
    assert_eq!(context["cwd"], "D:/work");
    assert_eq!(context["model"], "gpt-test");
    assert_eq!(context["permissionMode"], "default");
    assert_eq!(context["prompt"], "please continue");
}

#[test]
fn format_user_prompt_submit_hook_feedback_uses_blocked_or_context_heading() {
    assert_eq!(
        format_user_prompt_submit_hook_feedback(
            vec!["Feedback from hook `gate`: blocked".to_string()],
            true,
        ),
        "[UserPromptSubmit hook blocked prompt]\nFeedback from hook `gate`: blocked"
    );
    assert_eq!(
        format_user_prompt_submit_hook_feedback(
            vec!["Additional context from hook `ctx`: remember this".to_string()],
            false,
        ),
        "[UserPromptSubmit hook context]\nAdditional context from hook `ctx`: remember this"
    );
}

#[test]
fn stop_hook_continuation_message_uses_model_visible_feedback() {
    let hook = HookRunResult {
        id: "run".to_string(),
        event: HOOK_AGENT_END.to_string(),
        source_type: "config".to_string(),
        source_name: "config".to_string(),
        command: "python stop.py".to_string(),
        status: "blocked".to_string(),
        exit_code: Some(0),
        stdout: String::new(),
        stderr: String::new(),
        error: None,
        duration_ms: 1,
        decision: Some("block".to_string()),
        reason: Some("run tests before stopping".to_string()),
        additional_context: Some("quality gate".to_string()),
        updated_input: None,
        invalid_output: None,
    };

    assert_eq!(
        stop_hook_continuation_message(&[hook]).as_deref(),
        Some(
            "[Stop hook continuation]\nAdditional context from hook `python stop.py`: quality gate\nFeedback from hook `python stop.py`: run tests before stopping"
        )
    );
}

#[test]
fn stop_hook_context_matches_codex_stop_shape() {
    let context = stop_hook_context(
        "turn-1",
        "goal",
        Path::new("D:/work"),
        42,
        &[FileChange {
            path: "src/main.rs".to_string(),
            action: "modified".to_string(),
        }],
        &TurnUsage {
            prompt_tokens: 10,
            completion_tokens: 5,
            total_tokens: 15,
            ..Default::default()
        },
        Some(100),
        false,
        "gpt-test",
        true,
        "done",
    );

    assert_eq!(context["hookEventName"], "Stop");
    assert_eq!(context["turnId"], "turn-1");
    assert_eq!(context["mode"], "goal");
    assert_eq!(context["model"], "gpt-test");
    assert_eq!(context["stopHookActive"], true);
    assert_eq!(context["lastAssistantMessage"], "done");
    assert_eq!(context["changedFiles"][0]["path"], "src/main.rs");
    assert_eq!(context["usage"]["totalTokens"], 15);
}

#[test]
fn subagent_stop_hook_context_extracts_close_agent_result() {
    let call = ToolCallRequest {
        id: "call-1".to_string(),
        name: "close_agent".to_string(),
        arguments: r#"{"target":"agent-1"}"#.to_string(),
    };
    let context = subagent_stop_hook_context(
        "turn-1",
        "goal",
        Path::new("D:/work"),
        "gpt-test",
        &call,
        r#"{
          "target": "agent-1",
          "closed": true,
          "agent": {
            "id": "agent-1",
            "role": "tester",
            "status": "closed",
            "output": "child done"
          }
        }"#,
    );

    assert_eq!(context["hookEventName"], "SubagentStop");
    assert_eq!(context["turnId"], "turn-1");
    assert_eq!(context["mode"], "goal");
    assert_eq!(context["model"], "gpt-test");
    assert_eq!(context["agentId"], "agent-1");
    assert_eq!(context["agent_id"], "agent-1");
    assert_eq!(context["agentType"], "tester");
    assert_eq!(context["agent_type"], "tester");
    assert_eq!(context["lastAssistantMessage"], "child done");
    assert_eq!(context["closeResult"]["closed"], true);
}

#[test]
fn append_subagent_stop_hook_feedback_adds_visible_section() {
    assert_eq!(
        append_subagent_stop_hook_feedback(
            "close output".to_string(),
            vec!["Feedback from hook `child gate`: review child output".to_string()],
        ),
        "close output\n\n[SubagentStop hook feedback]\nFeedback from hook `child gate`: review child output"
    );
}

#[test]
fn plugin_apps_prompt_includes_codex_connector_guidance() {
    let root = std::env::temp_dir().join(format!(
        "cn-codex-agent-apps-prompt-test-{}",
        uuid::Uuid::new_v4()
    ));
    let plugin_root = root.join("plugins").join("demo");
    std::fs::create_dir_all(plugin_root.join(".codex-plugin")).unwrap();
    std::fs::write(
        plugin_root.join(".codex-plugin").join("plugin.json"),
        r#"{ "name": "demo", "interface": { "displayName": "Demo Apps" } }"#,
    )
    .unwrap();
    std::fs::write(
        plugin_root.join(".app.json"),
        r#"{"apps":{"calendar":{"id":"connector_calendar"}}}"#,
    )
    .unwrap();

    let prompt = render_plugin_apps_prompt_for_config_dir(&root);

    assert!(prompt.contains("## Apps (Connectors)"));
    assert!(prompt.contains("app://{connector_id}"));
    assert!(prompt.contains("`codex-apps` MCP server"));
    assert!(prompt.contains("lazy-load them through `tool_search`"));
    assert!(prompt.contains("`apps_list`"));
    assert!(prompt.contains("do not additionally call `mcp_list_resources`"));
    assert!(prompt.contains("mcp_list_resource_templates"));
    assert!(prompt.contains("connector `connector_calendar`"));

    std::fs::remove_dir_all(root).ok();
}

#[test]
fn smartbrain_runtime_prompt_includes_database_names_without_summary_injection() {
    let temp_dir = tempfile::tempdir().unwrap();
    let workspace_config_dir = temp_dir.path().join("codey");
    let usage_db = crate::usage::UsageDb::open(&workspace_config_dir.join("usage.db")).unwrap();
    usage_db
        .state_set(
            SMARTBRAIN_DB_SOURCES_STATE_KEY,
            r#"[{
              "name": "合同数据库",
              "dbType": "mysql",
              "enabled": true,
              "host": "10.136.0.134",
              "port": 3306,
              "databaseName": "psa_crm_pact_test",
              "username": "root",
              "password": "top-secret",
              "permissions": {
                "readSchema": true,
                "readData": true,
                "writeData": false
              }
            }]"#,
        )
        .unwrap();
    usage_db
        .state_set(
            SMARTBRAIN_DB_SETTINGS_STATE_KEY,
            r##"{
              "defaultRowLimit": 200,
              "defaultTimeoutSec": 15,
              "requireReadonlyReminder": true,
              "skipWhenNoPermission": true,
              "rulesMarkdown": "# 数据库安全规则\n- 只读优先"
            }"##,
        )
        .unwrap();

    let prompt = render_smartbrain_runtime_prompt(
        &workspace_config_dir,
        &SmartBrainConfig {
            enabled: true,
            inject_summary: false,
            ..SmartBrainConfig::default()
        },
    );

    assert!(prompt.contains("合同数据库"));
    assert!(prompt.contains("psa_crm_pact_test"));
    assert!(prompt.contains("不要再次向用户索要主机、端口、用户名、密码或完整连接串"));
    assert!(prompt.contains("密码已在配置中单独保存"));
    assert!(!prompt.contains("top-secret"));
}

#[test]
fn smartbrain_runtime_prompt_is_empty_when_dialog_knowledge_base_is_off() {
    let temp_dir = tempfile::tempdir().unwrap();
    let workspace_config_dir = temp_dir.path().join("codey");
    let experiences = workspace_config_dir
        .join("memories")
        .join("experiences");
    let knowledge = workspace_config_dir.join("memories").join("knowledge");
    std::fs::create_dir_all(&experiences).unwrap();
    std::fs::create_dir_all(&knowledge).unwrap();
    std::fs::write(
        experiences.join("experience_summary.md"),
        "prior experience that must not be injected while KB is off",
    )
    .unwrap();
    std::fs::write(
        knowledge.join("hierarchy.json"),
        r#"{"domains":{"backend":["api"]}}"#,
    )
    .unwrap();

    let prompt = render_smartbrain_runtime_prompt(
        &workspace_config_dir,
        &SmartBrainConfig {
            enabled: false,
            inject_summary: true,
            knowledge_enabled: true,
            ..SmartBrainConfig::default()
        },
    );

    assert!(
        prompt.is_empty(),
        "disabled chat KB must not advertise smartbrain_search: {prompt}"
    );
}

#[test]
fn smartbrain_runtime_prompt_omits_search_guidance_when_knowledge_disabled() {
    let temp_dir = tempfile::tempdir().unwrap();
    let workspace_config_dir = temp_dir.path().join("codey");
    let experiences = workspace_config_dir
        .join("memories")
        .join("experiences");
    let knowledge = workspace_config_dir.join("memories").join("knowledge");
    std::fs::create_dir_all(&experiences).unwrap();
    std::fs::create_dir_all(&knowledge).unwrap();
    std::fs::write(
        experiences.join("experience_summary.md"),
        "experience summary only",
    )
    .unwrap();
    std::fs::write(
        knowledge.join("hierarchy.json"),
        r#"{"domains":{"backend":["api"]}}"#,
    )
    .unwrap();

    let prompt = render_smartbrain_runtime_prompt(
        &workspace_config_dir,
        &SmartBrainConfig {
            enabled: true,
            inject_summary: true,
            knowledge_enabled: false,
            ..SmartBrainConfig::default()
        },
    );

    assert!(
        !prompt.contains("smartbrain_search"),
        "knowledge_enabled=false must not push search tool: {prompt}"
    );
}

#[test]
fn robot_runtime_prompt_includes_saved_robot_system_prompt() {
    let temp_dir = tempfile::tempdir().unwrap();
    let workspace_config_dir = temp_dir.path().join("codey");
    let config = crate::robot_loader::RobotConfig {
        name: "数据库机器人".to_string(),
        description: String::new(),
        icon: String::new(),
        skills: Vec::new(),
        plugin_skills: Vec::new(),
        workflow: vec!["查询合同数据库".to_string()],
        workflow_nodes: vec![crate::robot_loader::WorkflowNode {
            objective: "查询合同数据库".to_string(),
            skills: vec!["smartbrain-context-read".to_string()],
            plugin_skills: Vec::new(),
        }],
        system_prompt: "优先使用合同数据库回答问题。".to_string(),
        created_at: 0,
        updated_at: 0,
    };

    crate::robot_loader::save_robot(&workspace_config_dir, "db-bot", &config).unwrap();

    let prompt = render_robot_runtime_prompt(&workspace_config_dir, Some("db-bot"));
    assert!(prompt.contains("db-bot"));
    assert!(prompt.contains("优先使用合同数据库回答问题。"));
}

#[test]
fn active_thread_guard_rejects_overlapping_turns_and_releases_on_drop() {
    let temp_dir = tempfile::tempdir().unwrap();
    let workspace_dir = temp_dir.path().to_path_buf();
    let thread_store = Arc::new(ThreadStore::new(&workspace_dir.join("codey")));
    let tool_executor = ToolExecutor::new(workspace_dir.clone());
    let engine = AgentEngine::new(thread_store, tool_executor, workspace_dir).unwrap();

    let first = engine.claim_thread_turn("thread-1").unwrap();
    let overlapping = engine.claim_thread_turn("thread-1").unwrap_err();
    assert!(overlapping.to_string().contains("already running"));

    drop(first);
    assert!(engine.claim_thread_turn("thread-1").is_ok());
}

#[test]
fn robot_history_keeps_current_stage_only() {
    let history = vec![
        test_thread_message("seed", "user", "root objective"),
        test_thread_message(
            "old-output",
            "assistant",
            &"old stage details ".repeat(1000),
        ),
        test_thread_message("boundary", "system", "stage 2 started"),
        test_thread_message("current", "assistant", "current stage work"),
    ];
    let state = ThreadRobotState {
        robot_id: "bot".to_string(),
        current_node_index: 1,
        root_objective: "root objective".to_string(),
        runtime_nodes: vec!["stage 1".to_string(), "stage 2".to_string()],
        node_deliveries: vec![
            "Artifacts: notes.md\nDecisions: complete\nValidation: checked\nOpen items: none"
                .to_string(),
        ],
        completed: false,
        current_node_start_message_id: Some("boundary".to_string()),
    };

    let focused = build_robot_model_history(&history, &state);
    let ids = focused
        .iter()
        .map(|message| message.id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(ids, vec!["boundary", "current"]);
    assert!(
        estimate_robot_checkpoint_tokens(&focused) < estimate_robot_checkpoint_tokens(&history)
    );
}

#[tokio::test]
async fn robot_checkpoint_focuses_model_history_without_changing_transcript() {
    let temp_dir = tempfile::tempdir().unwrap();
    let thread_store = ThreadStore::new(&temp_dir.path().join("codey"));
    let thread = thread_store.create_thread(None).await.unwrap();
    thread_store
        .start_turn(&thread.id, None, None)
        .await
        .unwrap();
    for message in [
        test_thread_message("seed", "user", "root objective"),
        test_thread_message(
            "old-output",
            "assistant",
            &"old stage details ".repeat(1000),
        ),
        test_thread_message("boundary", "system", "stage 2 started"),
        test_thread_message("current", "assistant", "current stage work"),
    ] {
        thread_store.add_message(&thread.id, message).await.unwrap();
    }
    let state = ThreadRobotState {
        robot_id: "bot".to_string(),
        current_node_index: 1,
        root_objective: "root objective".to_string(),
        runtime_nodes: vec!["stage 1".to_string(), "stage 2".to_string()],
        node_deliveries: vec![
            "Artifacts: notes.md\nDecisions: complete\nValidation: checked\nOpen items: none"
                .to_string(),
        ],
        completed: false,
        current_node_start_message_id: Some("boundary".to_string()),
    };

    let estimated_tokens = checkpoint_robot_model_history(&thread_store, &thread.id, &state)
        .await
        .unwrap();
    let transcript = thread_store.get_thread_messages(&thread.id).await;
    let model_history = thread_store.get_model_history(&thread.id).await;

    assert_eq!(transcript.len(), 4);
    assert_eq!(model_history.len(), 2);
    assert_eq!(model_history[0].id, "boundary");
    assert_eq!(
        thread_store.get_thread_total_tokens(&thread.id).await,
        estimated_tokens
    );
}

#[test]
fn file_changes_from_apply_patch_tool_call_marks_move_destination() {
    let call = ToolCallRequest {
        id: "call-1".to_string(),
        name: "apply_patch".to_string(),
        arguments: serde_json::json!({
            "patch": "*** Begin Patch\n*** Update File: src/old.ts\n*** Move to: src/new.ts\n@@\n-old\n+new\n*** End Patch"
        })
        .to_string(),
    };

    assert_eq!(
        file_changes_from_tool_call(&call),
        vec![FileChange {
            path: "src/new.ts".to_string(),
            action: "renamed".to_string(),
        }]
    );
}

#[tokio::test]
async fn advance_robot_workflow_from_goal_completion_moves_to_next_node() {
    let temp_dir = tempfile::tempdir().unwrap();
    let workspace_dir = temp_dir.path().to_path_buf();
    let thread_store = ThreadStore::new(&workspace_dir.join("codey"));
    let thread = thread_store.create_thread(None).await.unwrap();
    thread_store
        .start_turn(&thread.id, Some("goal".to_string()), None)
        .await
        .unwrap();
    thread_store
        .set_thread_goal(
            &thread.id,
            "阶段 1：需求分析".to_string(),
            ThreadGoalStatus::Active,
            None,
        )
        .await
        .unwrap();
    let state = ThreadRobotState {
        robot_id: "bot".to_string(),
        current_node_index: 0,
        root_objective: "实现 lite 版".to_string(),
        runtime_nodes: vec![
            "阶段 1：需求分析".to_string(),
            "阶段 2：架构设计".to_string(),
        ],
        node_deliveries: Vec::new(),
        completed: false,
        current_node_start_message_id: None,
    };
    thread_store
        .set_thread_robot_state(&thread.id, state.clone())
        .await
        .unwrap();

    let orchestrator = RobotOrchestrator::new(&workspace_dir);
    let outcome = advance_robot_workflow_from_goal_completion(
        &thread_store,
        &orchestrator,
        &thread.id,
        state,
    )
    .await
    .unwrap();

    let advanced_state = match outcome {
        RobotGoalCompletionOutcome::Advanced(state) => state,
        RobotGoalCompletionOutcome::Completed(_) => panic!("expected workflow to advance"),
    };
    assert_eq!(advanced_state.current_node_index, 1);
    assert!(advanced_state.current_node_start_message_id.is_some());

    let stored_thread = thread_store.get_thread(&thread.id).await.unwrap();
    let stored_goal = stored_thread.goal.unwrap();
    let stored_robot = stored_thread.robot_state.unwrap();
    assert_eq!(stored_goal.objective, "阶段 2：架构设计");
    assert_eq!(stored_goal.status, ThreadGoalStatus::Active);
    assert_eq!(stored_robot.current_node_index, 1);
    assert_eq!(stored_robot.node_deliveries.len(), 1);
    assert!(
        stored_robot.node_deliveries[0]
            .contains("Node 1 completed without an explicit delivery summary.")
    );
    assert!(stored_robot.current_node_start_message_id.is_some());

    let messages = thread_store.get_thread_messages(&thread.id).await;
    assert!(messages.last().is_some_and(|message| {
        message.role == "system"
            && message
                .content
                .contains("Workflow node completed. Continue with node 2/2.")
    }));
}

#[tokio::test]
async fn advance_robot_workflow_from_goal_completion_finishes_last_node() {
    let temp_dir = tempfile::tempdir().unwrap();
    let workspace_dir = temp_dir.path().to_path_buf();
    let thread_store = ThreadStore::new(&workspace_dir.join("codey"));
    let thread = thread_store.create_thread(None).await.unwrap();
    thread_store
        .start_turn(&thread.id, Some("goal".to_string()), None)
        .await
        .unwrap();
    thread_store
        .set_thread_goal(
            &thread.id,
            "阶段 1：收尾".to_string(),
            ThreadGoalStatus::Active,
            None,
        )
        .await
        .unwrap();
    let state = ThreadRobotState {
        robot_id: "bot".to_string(),
        current_node_index: 0,
        root_objective: "完成整个工作流".to_string(),
        runtime_nodes: vec!["阶段 1：收尾".to_string()],
        node_deliveries: Vec::new(),
        completed: false,
        current_node_start_message_id: None,
    };
    thread_store
        .set_thread_robot_state(&thread.id, state.clone())
        .await
        .unwrap();

    let orchestrator = RobotOrchestrator::new(&workspace_dir);
    let outcome = advance_robot_workflow_from_goal_completion(
        &thread_store,
        &orchestrator,
        &thread.id,
        state,
    )
    .await
    .unwrap();

    assert!(matches!(outcome, RobotGoalCompletionOutcome::Completed(_)));
    let stored_thread = thread_store.get_thread(&thread.id).await.unwrap();
    let stored_goal = stored_thread.goal.unwrap();
    assert!(
        stored_thread
            .robot_state
            .is_some_and(|state| state.completed)
    );
    assert_eq!(stored_goal.status, ThreadGoalStatus::Complete);
}

#[test]
fn strip_robot_node_done_marker_removes_control_token() {
    let (cleaned, done) = crate::robot_orchestrator::strip_robot_node_done_marker(
        "Node completed. <workflow_node_done/> Moving to next stage.",
    );
    assert!(done);
    assert_eq!(cleaned, "Node completed.  Moving to next stage.");
}

#[test]
fn robot_node_prompts_include_progress_and_done_marker() {
    let nudge = crate::robot_orchestrator::build_robot_node_completion_nudge(1, 4);
    assert!(nudge.contains("2/4"));
    assert!(nudge.contains(crate::robot_orchestrator::ROBOT_NODE_DONE_SENTINEL));

    let advance = crate::robot_orchestrator::build_robot_node_advance_prompt(2, 4);
    assert!(advance.contains("3/4"));
    assert!(advance.contains(crate::robot_orchestrator::ROBOT_NODE_DONE_SENTINEL));
}
