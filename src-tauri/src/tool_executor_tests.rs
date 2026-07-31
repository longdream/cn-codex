use super::browser_support::*;
use super::code_review_support::*;
use super::file_op_support::*;
use super::image_gen_support::*;
use super::interaction_support::*;
use super::mcp_support::*;
use super::memory_support::*;
use super::patch_support::*;
use super::plugin_manage_support::*;
use super::shell_support::*;
use super::subagent_support::*;
use super::tool_search_support::*;
use super::web_search_support::*;
use super::*;
use crate::git_service::GitCommandOutput;

#[test]
fn existing_file_rewrite_rejects_omission_placeholders() {
    let existing = "complete document\n".repeat(20);
    let replacement = "header\n... (保留原有内容直到第949行) ...\nnew section\n";

    let error = validate_existing_file_rewrite(&existing, replacement).unwrap_err();
    assert!(error.contains("omission placeholder"));
}

#[test]
fn existing_file_rewrite_rejects_destructive_truncation() {
    let existing = "0123456789abcdef\n".repeat(400);
    let replacement = "short complete-looking replacement\n";

    let error = validate_existing_file_rewrite(&existing, replacement).unwrap_err();
    assert!(error.contains("would shrink"));
}

#[test]
fn existing_file_rewrite_allows_complete_similar_sized_content() {
    let existing = "old line\n".repeat(600);
    let replacement = "new line\n".repeat(600);

    assert!(validate_existing_file_rewrite(&existing, &replacement).is_ok());
}

#[test]
fn tool_specs_include_web_tools_only_when_enabled() {
    let temp_dir = tempfile::tempdir().expect("should create temp dir");
    let root = temp_dir.path().join("workspace");
    let config_dir = root.join("codey");
    std::fs::create_dir_all(&config_dir).expect("should create config dir");
    let executor = ToolExecutor::with_workspace_config_dir(root, config_dir);
    let disabled = executor.tool_specs(false);
    let enabled = executor.tool_specs(true);

    let disabled_names: Vec<_> = disabled
        .iter()
        .filter_map(|tool| tool.get("function")?.get("name")?.as_str())
        .collect();
    let enabled_names: Vec<_> = enabled
        .iter()
        .filter_map(|tool| tool.get("function")?.get("name")?.as_str())
        .collect();

    assert!(!disabled_names.contains(&"web_search"));
    assert!(!disabled_names.contains(&"web_fetch"));
    assert!(disabled_names.contains(&"memory_list"));
    assert!(disabled_names.contains(&"memory_read"));
    assert!(disabled_names.contains(&"memory_search"));
    assert!(disabled_names.contains(&"memory_write"));
    assert!(disabled_names.contains(&"memory_update"));
    assert!(disabled_names.contains(&"memory_forget"));
    assert!(disabled_names.contains(&"shell_command"));
    assert!(disabled_names.contains(&"exec_command"));
    assert!(disabled_names.contains(&"write_stdin"));
    assert!(disabled_names.contains(&"close_exec_session"));
    assert!(disabled_names.contains(&"tool_search"));
    assert!(disabled_names.contains(&"apps_list"));
    assert!(disabled_names.contains(&"list_available_plugins_to_install"));
    assert!(disabled_names.contains(&"request_plugin_install"));
    assert!(disabled_names.contains(&"plugin_manage"));
    assert!(disabled_names.contains(&"code_review"));
    assert!(disabled_names.contains(&"apply_patch"));
    assert!(disabled_names.contains(&"update_plan"));
    assert!(disabled_names.contains(&"request_user_input"));
    assert!(disabled_names.contains(&"request_permissions"));
    assert!(disabled_names.contains(&"view_image"));
    assert!(disabled_names.contains(&"ocr_image"));
    assert!(disabled_names.contains(&"image_generate"));
    assert!(disabled_names.contains(&"echarts_report"));
    assert!(disabled_names.contains(&"browser_run"));
    assert!(disabled_names.contains(&"spawn_agent"));
    assert!(disabled_names.contains(&"wait_agent"));
    assert!(disabled_names.contains(&"send_input"));
    assert!(disabled_names.contains(&"resume_agent"));
    assert!(disabled_names.contains(&"list_agents"));
    assert!(disabled_names.contains(&"close_agent"));
    assert!(disabled_names.contains(&"mcp_list_servers"));
    assert!(disabled_names.contains(&"mcp_status"));
    assert!(disabled_names.contains(&"mcp_list_tools"));
    assert!(disabled_names.contains(&"mcp_call_tool"));
    assert!(disabled_names.contains(&"mcp_list_resources"));
    assert!(disabled_names.contains(&"mcp_read_resource"));
    assert!(disabled_names.contains(&"mcp_list_resource_templates"));
    assert!(disabled_names.contains(&"mcp_list_prompts"));
    assert!(disabled_names.contains(&"mcp_get_prompt"));
    assert!(disabled_names.contains(&"mcp_manage"));
    assert!(disabled_names.contains(&"skill_manage"));
    assert!(enabled_names.contains(&"web_search"));
    assert!(enabled_names.contains(&"web_fetch"));
    assert!(enabled_names.contains(&"code_review"));
    assert!(enabled_names.contains(&"apps_list"));
    assert!(enabled_names.contains(&"list_available_plugins_to_install"));
    assert!(enabled_names.contains(&"request_plugin_install"));
    assert!(enabled_names.contains(&"plugin_manage"));
    assert!(enabled_names.contains(&"mcp_manage"));
    assert!(enabled_names.contains(&"skill_manage"));
    assert!(enabled_names.contains(&"request_user_input"));
    assert!(enabled_names.contains(&"request_permissions"));
    assert!(enabled_names.contains(&"memory_update"));
    assert!(enabled_names.contains(&"memory_forget"));
    assert!(enabled_names.contains(&"ocr_image"));
    assert!(enabled_names.contains(&"image_generate"));
    assert!(enabled_names.contains(&"echarts_report"));
    assert!(enabled_names.contains(&"browser_run"));
    assert!(enabled_names.contains(&"spawn_agent"));
    assert!(enabled_names.contains(&"send_input"));
    assert!(enabled_names.contains(&"resume_agent"));
    assert!(enabled_names.contains(&"close_agent"));
    assert!(enabled_names.contains(&"close_exec_session"));
    assert!(enabled_names.contains(&"mcp_status"));
}

#[tokio::test]
async fn layered_tool_specs_default_to_core_only_and_activate_via_search() {
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let root = temp_dir.path().join("workspace");
    let config_dir = root.join("codey");
    std::fs::create_dir_all(&config_dir).expect("config dir");

    let mut executor = ToolExecutor::with_workspace_config_dir(root, config_dir);
    executor.mcp_tool_specs.insert(
        "mcp__playwright__browser_navigate".to_string(),
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "mcp__playwright__browser_navigate",
                "description": "Navigate Playwright browser to a URL",
                "parameters": {
                    "type": "object",
                    "properties": { "url": { "type": "string" } },
                    "required": ["url"]
                }
            }
        }),
    );
    executor.mcp_tool_aliases.insert(
        "mcp__playwright__browser_navigate".to_string(),
        McpToolAlias {
            server: "playwright".to_string(),
            tool: "browser_navigate".to_string(),
            connector: McpConnectorMetadata::default(),
        },
    );
    executor.mcp_direct_tools_discovered = true;

    let default_specs = executor
        .tool_specs_for_turn(true, false, Some("thread-layered"))
        .await;
    let default_names: BTreeSet<_> = default_specs
        .iter()
        .filter_map(ToolExecutor::tool_spec_name)
        .map(str::to_string)
        .collect();

    assert!(default_names.contains("shell"));
    assert!(default_names.contains("read_file"));
    assert!(default_names.contains("tool_search"));
    assert!(default_names.contains("web_search"));
    assert!(!default_names.contains("memory_list"));
    assert!(!default_names.contains("spawn_agent"));
    assert!(!default_names.contains("mcp_list_tools"));
    assert!(!default_names.contains("mcp__playwright__browser_navigate"));
    assert!(
        default_names.len() <= 22,
        "core schema set should stay small, got {}",
        default_names.len()
    );

    executor
        .activate_tools_for_thread(
            "thread-layered",
            ["mcp__playwright__browser_navigate", "memory_list"],
        )
        .await;

    let activated_specs = executor
        .tool_specs_for_turn(true, false, Some("thread-layered"))
        .await;
    let activated_names: BTreeSet<_> = activated_specs
        .iter()
        .filter_map(ToolExecutor::tool_spec_name)
        .map(str::to_string)
        .collect();
    assert!(activated_names.contains("mcp__playwright__browser_navigate"));
    assert!(activated_names.contains("memory_list"));

    // Other threads remain core-only.
    let other_specs = executor
        .tool_specs_for_turn(true, false, Some("thread-other"))
        .await;
    let other_names: BTreeSet<_> = other_specs
        .iter()
        .filter_map(ToolExecutor::tool_spec_name)
        .map(str::to_string)
        .collect();
    assert!(!other_names.contains("mcp__playwright__browser_navigate"));
    assert!(!other_names.contains("memory_list"));
}

#[tokio::test]
async fn subagent_tools_attach_only_when_dialog_switch_enabled() {
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let root = temp_dir.path().join("workspace");
    let config_dir = root.join("codey");
    std::fs::create_dir_all(&config_dir).expect("config dir");

    let mut executor = ToolExecutor::with_workspace_config_dir(root, config_dir);
    let disabled = executor
        .tool_specs_for_turn(false, false, Some("thread-subagent"))
        .await;
    let disabled_names: BTreeSet<_> = disabled
        .iter()
        .filter_map(ToolExecutor::tool_spec_name)
        .map(str::to_string)
        .collect();
    assert!(!disabled_names.contains("spawn_agent"));
    assert!(!disabled_names.contains("wait_agent"));

    executor.set_subagent_enabled_override(Some(true));
    let enabled = executor
        .tool_specs_for_turn(false, false, Some("thread-subagent"))
        .await;
    let enabled_names: BTreeSet<_> = enabled
        .iter()
        .filter_map(ToolExecutor::tool_spec_name)
        .map(str::to_string)
        .collect();
    assert!(enabled_names.contains("spawn_agent"));
    assert!(enabled_names.contains("wait_agent"));
    assert!(enabled_names.contains("send_input"));
    assert!(enabled_names.contains("list_agents"));
    assert!(enabled_names.contains("close_agent"));
    assert!(enabled_names.contains("resume_agent"));
}

#[tokio::test]
async fn subagent_tools_stay_hidden_even_if_tool_search_activated_while_disabled() {
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let root = temp_dir.path().join("workspace");
    let config_dir = root.join("codey");
    std::fs::create_dir_all(&config_dir).expect("config dir");

    let mut executor = ToolExecutor::with_workspace_config_dir(root, config_dir);
    executor.set_subagent_enabled_override(Some(false));
    executor
        .activate_tools_for_thread(
            "thread-subagent-bypass",
            ["spawn_agent", "wait_agent", "list_agents"],
        )
        .await;

    let specs = executor
        .tool_specs_for_turn(false, false, Some("thread-subagent-bypass"))
        .await;
    let names: BTreeSet<_> = specs
        .iter()
        .filter_map(ToolExecutor::tool_spec_name)
        .map(str::to_string)
        .collect();
    assert!(!names.contains("spawn_agent"));
    assert!(!names.contains("wait_agent"));
    assert!(!names.contains("list_agents"));
}

#[test]
fn core_tool_name_set_is_intentionally_small() {
    assert!(ToolExecutor::is_core_tool_name("tool_search"));
    assert!(ToolExecutor::is_core_tool_name("apply_patch"));
    assert!(ToolExecutor::is_core_tool_name("mcp_manage"));
    assert!(ToolExecutor::is_core_tool_name("skill_manage"));
    assert!(!ToolExecutor::is_core_tool_name(
        "mcp__playwright__browser_click"
    ));
    assert!(!ToolExecutor::is_core_tool_name("memory_list"));
    assert!(!ToolExecutor::is_core_tool_name("spawn_agent"));
}

#[test]
fn build_mcp_server_config_value_supports_stdio_and_remote() {
    let stdio = build_mcp_server_config_value(&McpManageArgs {
        command: Some("npx".to_string()),
        args: Some(vec!["-y".to_string(), "godot-mcp".to_string()]),
        disabled: Some(false),
        ..Default::default()
    })
    .expect("stdio config");
    assert_eq!(stdio["command"], "npx");
    assert_eq!(stdio["args"], serde_json::json!(["-y", "godot-mcp"]));
    assert_eq!(stdio["disabled"], false);

    let remote = build_mcp_server_config_value(&McpManageArgs {
        url: Some("http://127.0.0.1:3000/sse".to_string()),
        ..Default::default()
    })
    .expect("remote config");
    assert_eq!(remote["url"], "http://127.0.0.1:3000/sse");
    assert_eq!(remote["type"], "sse");
}

#[test]
fn mcp_manage_helpers_persist_server_into_config_toml() {
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let config_path = temp_dir.path().join("config.toml");
    let mut config = ConfigToml::default();
    let server = build_mcp_server_config_value(&McpManageArgs {
        command: Some("npx".to_string()),
        args: Some(vec!["-y".to_string(), "godot-mcp".to_string()]),
        ..Default::default()
    })
    .expect("server config");
    config
        .apply_edit("mcp_servers.godot", &server)
        .expect("apply edit");
    config.save(&config_path).expect("save config");

    let reloaded = ConfigToml::load(&config_path).expect("reload config");
    let servers = reloaded.resolved_mcp_servers();
    let godot = servers.get("godot").expect("godot server");
    assert_eq!(godot.command, "npx");
    assert_eq!(godot.args, vec!["-y", "godot-mcp"]);
    assert!(!godot.disabled);
}

#[test]
fn skill_manage_helpers_write_skill_md() {
    sanitize_skill_manage_id("godot-helper").expect("valid id");
    assert!(sanitize_skill_manage_id("../evil").is_err());

    let content = build_skill_manage_content(
        &SkillManageArgs {
            name: Some("Godot Helper".to_string()),
            description: Some("Help with Godot MCP".to_string()),
            tags: Some(vec!["godot".to_string(), "mcp".to_string()]),
            ..Default::default()
        },
        "godot-helper",
    )
    .expect("content");
    assert!(content.contains("name: \"Godot Helper\""));
    assert!(content.contains("Help with Godot MCP"));

    let temp_dir = tempfile::tempdir().expect("temp dir");
    let skill_dir = temp_dir.path().join("godot-helper");
    std::fs::create_dir_all(&skill_dir).expect("skill dir");
    let skill_md = skill_dir.join("SKILL.md");
    std::fs::write(&skill_md, &content).expect("write skill");
    let listed = list_local_skills(temp_dir.path());
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0]["id"], "godot-helper");
    assert_eq!(listed[0]["name"], "Godot Helper");
}

#[tokio::test]
async fn tool_search_activates_non_core_tools_for_same_thread_only() {
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let root = temp_dir.path().join("workspace");
    let config_dir = root.join("codey");
    std::fs::create_dir_all(&config_dir).expect("config dir");

    let mut executor = ToolExecutor::with_workspace_config_dir(root, config_dir);
    executor.mcp_tool_specs.insert(
        "mcp__playwright__browser_navigate".to_string(),
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "mcp__playwright__browser_navigate",
                "description": "Navigate Playwright browser to a URL",
                "parameters": {
                    "type": "object",
                    "properties": { "url": { "type": "string" } },
                    "required": ["url"]
                }
            }
        }),
    );
    executor.mcp_tool_aliases.insert(
        "mcp__playwright__browser_navigate".to_string(),
        McpToolAlias {
            server: "playwright".to_string(),
            tool: "browser_navigate".to_string(),
            connector: McpConnectorMetadata::default(),
        },
    );
    executor.mcp_direct_tools_discovered = true;

    // Simulate what exec_tool_search does after ranking matches.
    let matches = search_tool_entries(executor.tool_search_entries(), "playwright navigate", 8);
    assert!(
        matches
            .iter()
            .any(|entry| entry.name == "mcp__playwright__browser_navigate"),
        "playwright tool should be discoverable via tool_search"
    );

    let activated_names: Vec<String> = matches
        .iter()
        .filter(|entry| entry.kind == "tool")
        .filter(|entry| !ToolExecutor::is_core_tool_name(&entry.name))
        .map(|entry| entry.name.clone())
        .collect();
    assert!(
        activated_names
            .iter()
            .any(|name| name == "mcp__playwright__browser_navigate")
    );
    executor
        .activate_tools_for_thread("thread-search", activated_names)
        .await;

    let activated = executor
        .tool_specs_for_turn(true, false, Some("thread-search"))
        .await;
    let activated_set: BTreeSet<_> = activated
        .iter()
        .filter_map(ToolExecutor::tool_spec_name)
        .map(str::to_string)
        .collect();
    assert!(activated_set.contains("mcp__playwright__browser_navigate"));

    let other = executor
        .tool_specs_for_turn(true, false, Some("thread-other"))
        .await;
    let other_set: BTreeSet<_> = other
        .iter()
        .filter_map(ToolExecutor::tool_spec_name)
        .map(str::to_string)
        .collect();
    assert!(!other_set.contains("mcp__playwright__browser_navigate"));
}

#[tokio::test]
async fn tool_search_hot_mounts_schemas_for_same_turn_next_iteration() {
    // Mirrors the agent loop: iteration N runs tool_search (activate),
    // iteration N+1 rebuilds tool_specs_for_turn and must include the schemas.
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let root = temp_dir.path().join("workspace");
    let config_dir = root.join("codey");
    std::fs::create_dir_all(&config_dir).expect("config dir");

    let mut executor = ToolExecutor::with_workspace_config_dir(root, config_dir);
    executor.mcp_tool_specs.insert(
        "mcp__playwright__browser_navigate".to_string(),
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "mcp__playwright__browser_navigate",
                "description": "Navigate Playwright browser to a URL",
                "parameters": {
                    "type": "object",
                    "properties": { "url": { "type": "string" } },
                    "required": ["url"]
                }
            }
        }),
    );
    executor.mcp_tool_aliases.insert(
        "mcp__playwright__browser_navigate".to_string(),
        McpToolAlias {
            server: "playwright".to_string(),
            tool: "browser_navigate".to_string(),
            connector: McpConnectorMetadata::default(),
        },
    );
    executor.mcp_direct_tools_discovered = true;

    let thread_id = "thread-same-turn";

    // Iteration N: default core-only schema set.
    let before = executor
        .tool_specs_for_turn(true, false, Some(thread_id))
        .await;
    let before_names: BTreeSet<_> = before
        .iter()
        .filter_map(ToolExecutor::tool_spec_name)
        .map(str::to_string)
        .collect();
    assert!(!before_names.contains("mcp__playwright__browser_navigate"));

    // Simulate tool_search activation mid-turn.
    let matches = search_tool_entries(executor.tool_search_entries(), "playwright navigate", 8);
    let activated_names: Vec<String> = matches
        .iter()
        .filter(|entry| entry.kind == "tool")
        .filter(|entry| !ToolExecutor::is_core_tool_name(&entry.name))
        .map(|entry| entry.name.clone())
        .collect();
    assert!(
        activated_names
            .iter()
            .any(|name| name == "mcp__playwright__browser_navigate")
    );
    executor
        .activate_tools_for_thread(thread_id, activated_names)
        .await;

    // Iteration N+1 (same user turn): rebuild tools before the next model call.
    let after = executor
        .tool_specs_for_turn(true, false, Some(thread_id))
        .await;
    let after_names: BTreeSet<_> = after
        .iter()
        .filter_map(ToolExecutor::tool_spec_name)
        .map(str::to_string)
        .collect();
    assert!(after_names.contains("mcp__playwright__browser_navigate"));
    assert!(after_names.contains("tool_search"));
    assert!(after_names.len() > before_names.len());
}

#[test]
fn smartbrain_search_tool_is_only_exposed_when_smartbrain_is_active() {
    let temp_dir = tempfile::tempdir().expect("should create temp dir");
    let root = temp_dir.path().join("workspace");
    let config_dir = root.join("codey");
    let memories_dir = config_dir.join("memories");
    std::fs::create_dir_all(&memories_dir).expect("should create memories dir");
    std::fs::write(memories_dir.join("smartbrain_index.json"), "{}")
        .expect("should create bm25 index file");

    std::fs::write(
        config_dir.join("config.toml"),
        "[smartbrain]\nenabled = false\n",
    )
    .expect("should write config");

    let mut executor =
        ToolExecutor::with_workspace_config_dir(root.clone(), config_dir.clone());
    let disabled_tools = executor.tool_specs(false);
    let disabled_names: Vec<_> = disabled_tools
        .iter()
        .filter_map(|tool| tool.get("function")?.get("name")?.as_str())
        .collect();
    assert!(!disabled_names.contains(&"smartbrain_search"));

    std::fs::write(
        config_dir.join("config.toml"),
        "[smartbrain]\nenabled = true\n",
    )
    .expect("should update config");

    let enabled_tools = executor.tool_specs(false);
    let enabled_names: Vec<_> = enabled_tools
        .iter()
        .filter_map(|tool| tool.get("function")?.get("name")?.as_str())
        .collect();
    assert!(enabled_names.contains(&"smartbrain_search"));
    assert!(
        enabled_names.contains(&"smartbrain_sql_query"),
        "smartbrain_sql_query must be exposed when Local Knowledge Base is enabled"
    );

    executor.set_smartbrain_enabled_override(Some(false));
    let overridden_specs = executor.tool_specs(false);
    let overridden_names: Vec<_> = overridden_specs
        .iter()
        .filter_map(|tool| tool.get("function")?.get("name")?.as_str())
        .collect();
    assert!(!overridden_names.contains(&"smartbrain_search"));
    assert!(!overridden_names.contains(&"smartbrain_sql_query"));
}

#[test]
fn image_generate_tool_is_hidden_when_disabled_in_settings() {
    let temp_dir = tempfile::tempdir().expect("should create temp dir");
    let root = temp_dir.path().join("workspace");
    let config_dir = root.join("codey");
    std::fs::create_dir_all(&config_dir).expect("should create config dir");

    std::fs::write(
        config_dir.join("config.toml"),
        "[image_generation]\nenabled = false\n",
    )
    .expect("should write config");

    let executor = ToolExecutor::with_workspace_config_dir(root.clone(), config_dir.clone());
    let disabled_tools = executor.tool_specs(false);
    let disabled_names: Vec<_> = disabled_tools
        .iter()
        .filter_map(|tool| tool.get("function")?.get("name")?.as_str())
        .collect();
    assert!(!disabled_names.contains(&"image_generate"));

    std::fs::write(
        config_dir.join("config.toml"),
        "[image_generation]\nenabled = true\n",
    )
    .expect("should update config");

    let enabled_tools = executor.tool_specs(false);
    let enabled_names: Vec<_> = enabled_tools
        .iter()
        .filter_map(|tool| tool.get("function")?.get("name")?.as_str())
        .collect();
    assert!(enabled_names.contains(&"image_generate"));
}

#[test]
fn browser_run_tool_spec_advertises_tab_actions() {
    let executor = ToolExecutor::new(PathBuf::from("."));
    let tools = executor.tool_specs(false);
    let browser_spec = tools
        .iter()
        .find(|tool| {
            tool.pointer("/function/name")
                .and_then(serde_json::Value::as_str)
                == Some("browser_run")
        })
        .expect("missing browser_run tool spec");
    let actions_description = browser_spec
        .pointer("/function/parameters/properties/actions/description")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();

    for action in [
        "set_viewport",
        "snapshot",
        "assets",
        "bundle_assets",
        "html",
        "list_tabs",
        "new_tab",
        "switch_tab",
        "close_tab",
    ] {
        assert!(
            actions_description.contains(action),
            "browser_run actions description should include {action}"
        );
    }
}

#[test]
fn tool_specs_require_non_empty_shell_script_strings() {
    let executor = ToolExecutor::new(PathBuf::from("."));
    let tools = executor.tool_specs(false);
    for name in ["shell", "shell_command"] {
        let spec = tools
            .iter()
            .find(|tool| {
                tool.pointer("/function/name")
                    .and_then(serde_json::Value::as_str)
                    == Some(name)
            })
            .unwrap_or_else(|| panic!("missing {name} tool spec"));
        assert_eq!(
            spec.pointer("/function/parameters/properties/command/type")
                .and_then(serde_json::Value::as_str),
            Some("string")
        );
        assert_eq!(
            spec.pointer("/function/parameters/properties/command/minLength")
                .and_then(serde_json::Value::as_u64),
            Some(1)
        );
        assert!(
            spec.pointer("/function/parameters/properties/workdir")
                .is_some()
        );
        assert!(
            spec.pointer("/function/parameters/properties/timeout_ms")
                .is_some()
        );
        assert!(
            spec.pointer("/function/parameters/properties/sandbox_permissions")
                .is_some()
        );
    }
}

#[test]
fn tool_specs_include_exec_session_tools() {
    let executor = ToolExecutor::new(PathBuf::from("."));
    let tools = executor.tool_specs(false);
    let exec = tools
        .iter()
        .find(|tool| {
            tool.pointer("/function/name")
                .and_then(serde_json::Value::as_str)
                == Some("exec_command")
        })
        .expect("missing exec_command tool spec");
    assert!(
        exec.pointer("/function/parameters/properties/cmd")
            .is_some()
    );
    assert!(
        exec.pointer("/function/parameters/properties/yield_time_ms")
            .is_some()
    );
    assert!(
        exec.pointer("/function/parameters/properties/sandbox_permissions")
            .is_some()
    );

    let write = tools
        .iter()
        .find(|tool| {
            tool.pointer("/function/name")
                .and_then(serde_json::Value::as_str)
                == Some("write_stdin")
        })
        .expect("missing write_stdin tool spec");
    assert!(
        write
            .pointer("/function/parameters/properties/session_id")
            .is_some()
    );
    assert!(
        write
            .pointer("/function/parameters/properties/chars")
            .is_some()
    );

    let close = tools
        .iter()
        .find(|tool| {
            tool.pointer("/function/name")
                .and_then(serde_json::Value::as_str)
                == Some("close_exec_session")
        })
        .expect("missing close_exec_session tool spec");
    assert!(
        close
            .pointer("/function/parameters/properties/session_id")
            .is_some()
    );
}

#[test]
fn apply_patch_tool_spec_requires_one_unambiguous_patch_field() {
    let executor = ToolExecutor::new(PathBuf::from("."));
    let tools = executor.tool_specs(false);
    let apply_patch = tools
        .iter()
        .find(|tool| {
            tool.pointer("/function/name")
                .and_then(serde_json::Value::as_str)
                == Some("apply_patch")
        })
        .expect("apply_patch tool spec");

    let description = apply_patch
        .pointer("/function/description")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    assert!(description.starts_with("Use apply_patch to edit files."));
    assert!(
        apply_patch
            .pointer("/function/parameters/properties/patch")
            .is_some()
    );
    assert!(
        apply_patch
            .pointer("/function/parameters/properties/command")
            .is_none()
    );
    assert_eq!(
        apply_patch.pointer("/function/parameters/required"),
        Some(&serde_json::json!(["patch"]))
    );
    assert_eq!(
        apply_patch.pointer("/function/parameters/additionalProperties"),
        Some(&serde_json::Value::Bool(false))
    );
}

#[test]
fn base_mcp_status_entry_exposes_env_keys_not_values() {
    let server = McpServerConfig {
        name: "docs".to_string(),
        transport: "stdio".to_string(),
        command: "node".to_string(),
        args: vec!["server.js".to_string()],
        env: HashMap::from([
            ("Z_TOKEN".to_string(), "secret-value".to_string()),
            ("A_KEY".to_string(), "also-secret".to_string()),
        ]),
        cwd: Some("tools/docs".to_string()),
        url: None,
        headers: HashMap::from([("Authorization".to_string(), "Bearer secret".to_string())]),
        disabled: false,
    };

    let entry = base_mcp_status_entry(&server);

    assert_eq!(
        entry.get("name").and_then(serde_json::Value::as_str),
        Some("docs")
    );
    assert_eq!(
        entry.get("envKeys"),
        Some(&serde_json::json!(["A_KEY", "Z_TOKEN"]))
    );
    assert_eq!(
        entry.get("headerKeys"),
        Some(&serde_json::json!(["Authorization"]))
    );
    assert!(!entry.to_string().contains("secret-value"));
    assert!(!entry.to_string().contains("also-secret"));
    assert!(!entry.to_string().contains("Bearer secret"));
}

#[test]
fn mcp_result_array_len_counts_expected_array_field() {
    let value = serde_json::json!({
        "tools": [
            { "name": "read" },
            { "name": "write" }
        ],
        "resources": "not-an-array"
    });

    assert_eq!(mcp_result_array_len(&value, "tools"), 2);
    assert_eq!(mcp_result_array_len(&value, "resources"), 0);
    assert_eq!(mcp_result_array_len(&value, "missing"), 0);
}

#[test]
fn apps_list_output_merges_plugin_apps_with_mcp_connector_tools() {
    let root =
        std::env::temp_dir().join(format!("cn-codex-apps-list-test-{}", uuid::Uuid::new_v4()));
    let config_dir = root.join("codey");
    let plugin_dir = config_dir.join("plugins").join("sites");
    std::fs::create_dir_all(plugin_dir.join(".codex-plugin")).unwrap();
    std::fs::write(
        plugin_dir.join(".codex-plugin").join("plugin.json"),
        r#"{"name":"sites","interface":{"displayName":"Sites"},"apps":"./.app.json"}"#,
    )
    .unwrap();
    std::fs::write(
        plugin_dir.join(".app.json"),
        r#"{"apps":{"sites":{"id":"connector_sites"}}}"#,
    )
    .unwrap();

    let aliases = HashMap::from([(
        "mcp__codex_apps__sites_create_project".to_string(),
        McpToolAlias {
            server: "codex-apps".to_string(),
            tool: "sites_create_project".to_string(),
            connector: McpConnectorMetadata {
                connector_id: Some("connector_sites".to_string()),
                connector_name: Some("Sites".to_string()),
                namespace_description: Some("Create and deploy hosted sites.".to_string()),
            },
        },
    )]);

    let output = format_apps_list_output(&config_dir, &aliases, None, true);
    let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
    assert_eq!(
        parsed
            .pointer("/summary/total")
            .and_then(serde_json::Value::as_u64),
        Some(1)
    );
    assert_eq!(
        parsed
            .pointer("/summary/accessible")
            .and_then(serde_json::Value::as_u64),
        Some(1)
    );
    assert_eq!(
        parsed
            .pointer("/apps/0/source")
            .and_then(serde_json::Value::as_str),
        Some("plugin+mcp")
    );
    assert_eq!(
        parsed
            .pointer("/apps/0/pluginApps/0/pluginDisplayName")
            .and_then(serde_json::Value::as_str),
        Some("Sites")
    );
    assert_eq!(
        parsed
            .pointer("/apps/0/tools/0/name")
            .and_then(serde_json::Value::as_str),
        Some("mcp__codex_apps__sites_create_project")
    );

    let filtered =
        format_apps_list_output(&config_dir, &aliases, Some("connector_missing"), true);
    let filtered: serde_json::Value = serde_json::from_str(&filtered).unwrap();
    assert_eq!(
        filtered
            .pointer("/summary/total")
            .and_then(serde_json::Value::as_u64),
        Some(0)
    );

    std::fs::remove_dir_all(root).ok();
}

#[test]
fn plugin_install_candidates_read_codex_cache_metadata() {
    let root = std::env::temp_dir().join(format!(
        "cn-codex-plugin-install-list-test-{}",
        uuid::Uuid::new_v4()
    ));
    let cache = root.join("cache");
    let config_dir = root.join("codey");
    let plugin_root = cache.join("openai-bundled").join("sites").join("1.0.0");
    std::fs::create_dir_all(plugin_root.join(".codex-plugin")).unwrap();
    std::fs::write(
        plugin_root.join(".codex-plugin").join("plugin.json"),
        r#"{
  "name": "sites",
  "version": "1.0.0",
  "description": "Create and deploy sites",
  "skills": "./skills",
  "mcpServers": "./mcp.json",
  "apps": "./.app.json"
}"#,
    )
    .unwrap();
    std::fs::create_dir_all(plugin_root.join("skills").join("sites-hosting")).unwrap();
    std::fs::write(
        plugin_root.join("mcp.json"),
        r#"{"mcpServers":{"sites-mcp":{"command":"node"}}}"#,
    )
    .unwrap();
    std::fs::write(
        plugin_root.join(".app.json"),
        r#"{"apps":{"sites":{"id":"connector_sites"}}}"#,
    )
    .unwrap();

    let candidates = plugin_install_candidates(&cache, &config_dir, Some("deploy"), true, 10);

    assert_eq!(candidates.len(), 1);
    let candidate = &candidates[0];
    assert_eq!(candidate.name, "sites");
    assert_eq!(candidate.version.as_deref(), Some("1.0.0"));
    assert!(candidate.id.ends_with("openai-bundled/sites/1.0.0"));
    assert!(candidate.has_skills);
    assert_eq!(candidate.mcp_server_names, vec!["sites-mcp".to_string()]);
    assert_eq!(
        candidate.app_connector_ids,
        vec!["connector_sites".to_string()]
    );
    assert!(!candidate.installed);

    let selected = select_plugin_install_candidate(&candidates, Some(&candidate.id), None)
        .expect("select by id");
    assert_eq!(selected.source, candidate.source);

    let _ = plugin_commands::import_plugin_root(
        Path::new(&selected.source),
        &config_dir.join("plugins"),
    )
    .expect("import plugin");
    let installed = plugin_install_candidates(&cache, &config_dir, Some("sites"), false, 10);
    assert!(installed.is_empty());

    std::fs::remove_dir_all(root).ok();
}

#[test]
fn tool_search_finds_builtin_skills_and_mcp_specs() {
    let root = std::env::temp_dir().join(format!(
        "cn-codex-tool-search-test-{}",
        uuid::Uuid::new_v4()
    ));
    let config_dir = root.join("codey");
    let skill_dir = config_dir.join("skills").join("browser");
    std::fs::create_dir_all(&skill_dir).unwrap();
    std::fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: Browser\ndescription: Control pages with WebView JS Injection\ntags: [\"browser\"]\n---\n",
    )
    .unwrap();
    let plugin_dir = root.join("codey").join("plugins").join("sites");
    std::fs::create_dir_all(plugin_dir.join(".codex-plugin")).unwrap();
    std::fs::write(
        plugin_dir.join(".codex-plugin").join("plugin.json"),
        r#"{"name":"sites","version":"1.0.0","description":"Build and host sites"}"#,
    )
    .unwrap();
    std::fs::write(
        plugin_dir.join(".app.json"),
        r#"{"apps":{"sites":{"id":"connector_sites"}}}"#,
    )
    .unwrap();

    let mut executor = ToolExecutor::with_workspace_config_dir(root.clone(), config_dir);
    executor.web_search_enabled = true;
    executor.mcp_tool_aliases.insert(
        "mcp__docs__search".to_string(),
        McpToolAlias {
            server: "docs".to_string(),
            tool: "search".to_string(),
            connector: McpConnectorMetadata::default(),
        },
    );
    executor.mcp_tool_specs.insert(
        "mcp__docs__search".to_string(),
        mcp_direct_tool_spec(
            "docs",
            "search",
            &serde_json::json!({
                "name": "search",
                "description": "Search project docs",
                "inputSchema": {
                    "type": "object",
                    "properties": { "query": { "type": "string" } },
                    "required": ["query"]
                }
            }),
            "mcp__docs__search",
        ),
    );
    executor.mcp_tool_aliases.insert(
        "mcp__docs__read".to_string(),
        McpToolAlias {
            server: "docs".to_string(),
            tool: "read".to_string(),
            connector: McpConnectorMetadata::default(),
        },
    );
    executor.mcp_tool_specs.insert(
        "mcp__docs__read".to_string(),
        mcp_direct_tool_spec(
            "docs",
            "read",
            &serde_json::json!({
                "name": "read",
                "description": "Read project docs",
                "inputSchema": {
                    "type": "object",
                    "properties": { "path": { "type": "string" } },
                    "required": ["path"]
                }
            }),
            "mcp__docs__read",
        ),
    );

    let browser_matches = search_tool_entries(executor.tool_search_entries(), "browser", 10);
    assert!(
        browser_matches
            .iter()
            .any(|entry| entry.name == "browser_run")
    );
    assert!(browser_matches.iter().any(|entry| {
        entry.kind == "skill" && entry.name == "Browser" && entry.path.is_some()
    }));

    let docs_matches = search_tool_entries(executor.tool_search_entries(), "project docs", 10);
    let mcp = docs_matches
        .iter()
        .find(|entry| entry.name == "mcp__docs__search")
        .expect("MCP direct tool should be searchable");
    assert_eq!(mcp.source, "mcp:docs");
    assert!(mcp.spec.is_some());
    let mcp_read = docs_matches
        .iter()
        .find(|entry| entry.name == "mcp__docs__read")
        .expect("second MCP direct tool should be searchable");

    executor.mcp_tool_aliases.insert(
        "mcp__codex_apps__sites_create_project".to_string(),
        McpToolAlias {
            server: "codex-apps".to_string(),
            tool: "sites_create_project".to_string(),
            connector: McpConnectorMetadata {
                connector_id: Some("connector_sites".to_string()),
                connector_name: Some("Sites".to_string()),
                namespace_description: Some("Create and deploy hosted sites.".to_string()),
            },
        },
    );
    executor.mcp_tool_specs.insert(
        "mcp__codex_apps__sites_create_project".to_string(),
        mcp_direct_tool_spec(
            "codex-apps",
            "sites_create_project",
            &serde_json::json!({
                "name": "sites_create_project",
                "description": "Create a hosted site project",
                "connector_id": "connector_sites",
                "connector_name": "Sites",
                "connector_description": "Create and deploy hosted sites.",
                "inputSchema": {
                    "type": "object",
                    "properties": { "name": { "type": "string" } },
                    "required": ["name"]
                }
            }),
            "mcp__codex_apps__sites_create_project",
        ),
    );
    let app_tool_matches =
        search_tool_entries(executor.tool_search_entries(), "hosted sites", 10);
    let app_tool = app_tool_matches
        .iter()
        .find(|entry| entry.name == "mcp__codex_apps__sites_create_project")
        .expect("MCP app connector tool should be searchable");
    assert_eq!(
        app_tool.metadata.get("connectorId").map(String::as_str),
        Some("connector_sites")
    );
    assert_eq!(
        app_tool.metadata.get("connectorName").map(String::as_str),
        Some("Sites")
    );
    assert!(
        app_tool
            .usage
            .as_deref()
            .unwrap_or_default()
            .contains("Sites app connector")
    );

    let output = format_tool_search_output("project docs", vec![mcp.clone(), mcp_read.clone()]);
    assert!(output.contains("\"mcp__docs__search\""));
    assert!(output.contains("\"spec\""));
    let output_json: serde_json::Value = serde_json::from_str(&output).unwrap();
    assert_eq!(
        output_json
            .pointer("/tools/0/type")
            .and_then(serde_json::Value::as_str),
        Some("namespace")
    );
    assert_eq!(
        output_json
            .pointer("/tools/0/name")
            .and_then(serde_json::Value::as_str),
        Some("mcp__docs")
    );
    let tool_names = output_json
        .pointer("/tools/0/tools")
        .and_then(serde_json::Value::as_array)
        .unwrap()
        .iter()
        .filter_map(|tool| tool.get("name").and_then(serde_json::Value::as_str))
        .collect::<Vec<_>>();
    assert_eq!(tool_names, vec!["search", "read"]);

    let app_matches = search_tool_entries(executor.tool_search_entries(), "sites hosting", 10);
    let app = app_matches
        .iter()
        .find(|entry| entry.kind == "app" && entry.name == "sites: sites")
        .expect("plugin app connector should be searchable");
    assert_eq!(
        app.metadata.get("connectorId").map(String::as_str),
        Some("connector_sites")
    );
    assert!(
        app.usage
            .as_deref()
            .unwrap_or_default()
            .contains("MCP tools")
    );

    let app_output = format_tool_search_output("sites hosting", vec![app.clone()]);
    assert!(app_output.contains("\"type\": \"app\""));
    assert!(app_output.contains("\"connectorId\": \"connector_sites\""));

    let agent_matches =
        search_tool_entries(executor.tool_search_entries(), "spawn delegated agents", 10);
    assert!(
        agent_matches
            .iter()
            .any(|entry| entry.name == "spawn_agent")
    );
    assert!(
        agent_matches
            .iter()
            .any(|entry| entry.name == "close_agent")
    );

    let send_input_matches = search_tool_entries(
        executor.tool_search_entries(),
        "send message existing agent",
        10,
    );
    assert!(
        send_input_matches
            .iter()
            .any(|entry| entry.name == "send_input")
    );

    let resume_matches =
        search_tool_entries(executor.tool_search_entries(), "resume closed agent", 10);
    assert!(
        resume_matches
            .iter()
            .any(|entry| entry.name == "resume_agent")
    );

    let image_matches =
        search_tool_entries(executor.tool_search_entries(), "generate image", 10);
    assert!(
        image_matches
            .iter()
            .any(|entry| entry.name == "image_generate")
    );

    let review_matches = search_tool_entries(executor.tool_search_entries(), "review diff", 10);
    assert!(
        review_matches
            .iter()
            .any(|entry| entry.name == "code_review")
    );

    let close_exec_matches =
        search_tool_entries(executor.tool_search_entries(), "close exec session", 10);
    assert!(
        close_exec_matches
            .iter()
            .any(|entry| entry.name == "close_exec_session")
    );

    let plugin_manage_matches =
        search_tool_entries(executor.tool_search_entries(), "disable local plugin", 10);
    assert!(
        plugin_manage_matches
            .iter()
            .any(|entry| entry.name == "plugin_manage")
    );

    std::fs::remove_dir_all(root).ok();
}

#[test]
fn tool_search_uses_schema_text_for_bm25_ranking() {
    let fetch_spec = serde_json::json!({
        "type": "function",
        "function": {
            "name": "web_fetch",
            "description": "Fetch a remote page.",
            "parameters": {
                "type": "object",
                "properties": {
                    "url": {
                        "type": "string",
                        "description": "Readable page address to retrieve."
                    },
                    "max_chars": {
                        "type": "integer",
                        "description": "Maximum readable text characters."
                    }
                },
                "required": ["url"]
            }
        }
    });
    let review_spec = serde_json::json!({
        "type": "function",
        "function": {
            "name": "code_review",
            "description": "Review the current git diff.",
            "parameters": {
                "type": "object",
                "properties": {
                    "base_ref": { "type": "string" }
                }
            }
        }
    });
    let fetch = tool_search_entry_from_function_spec(&fetch_spec, "tool", "built-in", None)
        .expect("fetch spec should become searchable");
    let review = tool_search_entry_from_function_spec(&review_spec, "tool", "built-in", None)
        .expect("review spec should become searchable");

    let matches = search_tool_entries(vec![review, fetch], "readable page address", 10);

    assert_eq!(
        matches.first().map(|entry| entry.name.as_str()),
        Some("web_fetch")
    );
}

#[test]
fn set_mcp_servers_preserves_cache_when_config_is_unchanged() {
    let root =
        std::env::temp_dir().join(format!("cn-codex-mcp-cache-test-{}", uuid::Uuid::new_v4()));
    let config_dir = root.join("codey");
    let mut executor = ToolExecutor::with_workspace_config_dir(root.clone(), config_dir);
    let mut servers = HashMap::new();
    servers.insert(
        "docs".to_string(),
        McpServerConfig {
            name: "docs".to_string(),
            transport: "stdio".to_string(),
            command: "docs-mcp".to_string(),
            args: vec!["--stdio".to_string()],
            env: HashMap::new(),
            cwd: None,
            url: None,
            headers: HashMap::new(),
            disabled: false,
        },
    );

    executor.set_mcp_servers(servers.clone());
    executor.mcp_tool_aliases.insert(
        "mcp__docs__search".to_string(),
        McpToolAlias {
            server: "docs".to_string(),
            tool: "search".to_string(),
            connector: McpConnectorMetadata::default(),
        },
    );
    executor.mcp_tool_specs.insert(
        "mcp__docs__search".to_string(),
        mcp_direct_tool_spec(
            "docs",
            "search",
            &serde_json::json!({ "name": "search" }),
            "mcp__docs__search",
        ),
    );
    executor.mcp_direct_tools_discovered = true;

    executor.set_mcp_servers(servers.clone());

    assert!(executor.mcp_direct_tools_discovered);
    assert!(executor.mcp_tool_aliases.contains_key("mcp__docs__search"));
    assert!(executor.mcp_tool_specs.contains_key("mcp__docs__search"));

    let mut changed = servers;
    changed
        .get_mut("docs")
        .unwrap()
        .args
        .push("--changed".to_string());
    executor.set_mcp_servers(changed);

    assert!(!executor.mcp_direct_tools_discovered);
    assert!(executor.mcp_tool_aliases.is_empty());
    assert!(executor.mcp_tool_specs.is_empty());

    std::fs::remove_dir_all(root).ok();
}

#[test]
fn mcp_discovery_cooldown_skips_repeated_turn_loading() {
    let root = std::env::temp_dir().join(format!(
        "cn-codex-mcp-cooldown-test-{}",
        uuid::Uuid::new_v4()
    ));
    let mut executor =
        ToolExecutor::with_workspace_config_dir(root.clone(), root.join("codey"));

    assert!(executor.mcp_discovery_needs_refresh());

    executor.mcp_discovery_retry_after = Some(Instant::now() + Duration::from_secs(300));
    assert!(!executor.mcp_discovery_needs_refresh());

    executor.mcp_discovery_retry_after = Some(Instant::now() - Duration::from_secs(1));
    assert!(executor.mcp_discovery_needs_refresh());

    executor.mcp_direct_tools_discovered = true;
    assert!(!executor.mcp_discovery_needs_refresh());
}

#[tokio::test]
async fn initial_turn_defers_mcp_connection_until_tool_search_activation() {
    let root = std::env::temp_dir().join(format!(
        "cn-codex-mcp-lazy-turn-test-{}",
        uuid::Uuid::new_v4()
    ));
    let mut executor =
        ToolExecutor::with_workspace_config_dir(root.clone(), root.join("codey"));
    executor.set_mcp_servers(HashMap::from([(
        "playwright".to_string(),
        McpServerConfig {
            name: "playwright".to_string(),
            transport: "stdio".to_string(),
            command: "this-command-must-not-run".to_string(),
            args: Vec::new(),
            env: HashMap::new(),
            cwd: None,
            url: None,
            headers: HashMap::new(),
            disabled: false,
        },
    )]));

    let tools = executor
        .tool_specs_for_turn(false, false, Some("thread-1"))
        .await;
    let tool_names = tools
        .iter()
        .filter_map(|tool| {
            tool.pointer("/function/name")
                .and_then(serde_json::Value::as_str)
        })
        .collect::<Vec<_>>();

    assert!(!tool_names.contains(&"mcp_list_tools"));
    assert!(!tool_names.contains(&"mcp_call_tool"));
    assert!(executor.mcp_tool_specs.is_empty());
    assert!(!executor.mcp_direct_tools_discovered);
    assert!(executor.tool_search_entries().iter().any(|entry| {
        entry.kind == "mcp_server"
            && entry.name == "MCP server: playwright"
            && entry.source == "mcp:playwright"
    }));
}

#[tokio::test]
async fn mcp_request_reuses_stdio_session_for_same_server() {
    if Command::new("node")
        .arg("--version")
        .output()
        .await
        .is_err()
    {
        return;
    }

    let root = std::env::temp_dir().join(format!(
        "cn-codex-mcp-session-test-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let script = root.join("fake-mcp.cjs");
    let starts = root.join("starts.txt");
    std::fs::write(
        &script,
        r#"
const fs = require("fs");
const readline = require("readline");
fs.appendFileSync(process.argv[2], "start\n");
const rl = readline.createInterface({ input: process.stdin });
rl.on("line", (line) => {
  const msg = JSON.parse(line);
  if (msg.method === "notifications/initialized") return;
  if (msg.method === "initialize") {
console.log(JSON.stringify({
  jsonrpc: "2.0",
  id: msg.id,
  result: {
    protocolVersion: "2024-11-05",
    capabilities: {},
    serverInfo: { name: "fake", version: "1.0.0" }
  }
}));
return;
  }
  if (msg.method === "tools/list") {
console.log(JSON.stringify({
  jsonrpc: "2.0",
  id: msg.id,
  result: {
    tools: [
      {
        name: "ping",
        description: "Ping",
        inputSchema: { type: "object", properties: {} }
      }
    ]
  }
}));
return;
  }
  console.log(JSON.stringify({ jsonrpc: "2.0", id: msg.id, result: { ok: true } }));
});
"#,
    )
    .unwrap();

    let config_dir = root.join("codey");
    let mut executor = ToolExecutor::with_workspace_config_dir(root.clone(), config_dir);
    let server = McpServerConfig {
        name: "fake".to_string(),
        transport: "stdio".to_string(),
        command: "node".to_string(),
        args: vec![
            script.to_string_lossy().to_string(),
            starts.to_string_lossy().to_string(),
        ],
        env: HashMap::new(),
        cwd: Some(root.to_string_lossy().to_string()),
        url: None,
        headers: HashMap::new(),
        disabled: false,
    };
    let mut servers = HashMap::new();
    servers.insert(server.name.clone(), server.clone());
    executor.set_mcp_servers(servers);

    let first = executor
        .mcp_request(&server, "tools/list", serde_json::json!({}))
        .await
        .expect("first tools/list should succeed");
    let second = executor
        .mcp_request(&server, "tools/list", serde_json::json!({}))
        .await
        .expect("second tools/list should reuse session");
    let status = executor.mcp_session_status("fake").await;
    executor.remove_mcp_session("fake").await;

    assert_eq!(
        first
            .pointer("/tools/0/name")
            .and_then(serde_json::Value::as_str),
        Some("ping")
    );
    assert_eq!(
        second
            .pointer("/tools/0/name")
            .and_then(serde_json::Value::as_str),
        Some("ping")
    );
    assert_eq!(
        status.get("connected").and_then(serde_json::Value::as_bool),
        Some(true)
    );
    assert_eq!(
        status
            .get("requestCount")
            .and_then(serde_json::Value::as_u64),
        Some(2)
    );
    let start_count = std::fs::read_to_string(&starts).unwrap().lines().count();
    assert_eq!(start_count, 1);

    std::fs::remove_dir_all(root).ok();
}

#[tokio::test]
async fn mcp_request_supports_http_jsonrpc_servers() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind fake MCP HTTP server");
    let addr = listener.local_addr().expect("fake server addr");
    let seen = Arc::new(Mutex::new(Vec::<serde_json::Value>::new()));
    let seen_headers = Arc::new(Mutex::new(Vec::<serde_json::Value>::new()));
    let server_seen = seen.clone();
    let server_seen_headers = seen_headers.clone();
    let server_task = tokio::spawn(async move {
        for _ in 0..3 {
            let (mut stream, _) = listener.accept().await.expect("accept request");
            let (request, headers) = read_test_http_json_request(&mut stream).await;
            let method = request
                .get("method")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_string();
            server_seen.lock().await.push(request.clone());
            server_seen_headers.lock().await.push(headers);

            match method.as_str() {
                "initialize" => {
                    write_test_http_response(
                        &mut stream,
                        200,
                        Some("session-123"),
                        Some(serde_json::json!({
                            "jsonrpc": "2.0",
                            "id": request.get("id").cloned().unwrap_or_default(),
                            "result": {
                                "protocolVersion": "2024-11-05",
                                "capabilities": {},
                                "serverInfo": { "name": "fake-http", "version": "1.0.0" }
                            }
                        })),
                        false,
                    )
                    .await;
                }
                "notifications/initialized" => {
                    write_test_http_response(
                        &mut stream,
                        202,
                        Some("session-123"),
                        None,
                        false,
                    )
                    .await;
                }
                "tools/list" => {
                    write_test_http_response(
                        &mut stream,
                        200,
                        Some("session-123"),
                        Some(serde_json::json!({
                            "jsonrpc": "2.0",
                            "id": request.get("id").cloned().unwrap_or_default(),
                            "result": {
                                "tools": [
                                    {
                                        "name": "ping",
                                        "description": "Ping over HTTP",
                                        "inputSchema": { "type": "object", "properties": {} }
                                    }
                                ]
                            }
                        })),
                        false,
                    )
                    .await;
                }
                other => panic!("unexpected MCP HTTP method {other}"),
            }
        }
    });

    let root =
        std::env::temp_dir().join(format!("cn-codex-mcp-http-test-{}", uuid::Uuid::new_v4()));
    let config_dir = root.join("codey");
    let executor = ToolExecutor::with_workspace_config_dir(root.clone(), config_dir);
    let server = McpServerConfig {
        name: "remote".to_string(),
        transport: "http".to_string(),
        command: String::new(),
        args: Vec::new(),
        env: HashMap::new(),
        cwd: None,
        url: Some(format!("http://{addr}/mcp")),
        headers: HashMap::from([("X-Test-Token".to_string(), "secret".to_string())]),
        disabled: false,
    };

    let result = executor
        .mcp_request(&server, "tools/list", serde_json::json!({}))
        .await
        .expect("HTTP tools/list should succeed");
    let status = executor.mcp_session_status("remote").await;

    assert_eq!(
        result
            .pointer("/tools/0/name")
            .and_then(serde_json::Value::as_str),
        Some("ping")
    );
    assert_eq!(
        status.get("connected").and_then(serde_json::Value::as_bool),
        Some(true)
    );
    assert_eq!(
        status.get("transport").and_then(serde_json::Value::as_str),
        Some("http")
    );
    assert_eq!(
        status
            .get("requestCount")
            .and_then(serde_json::Value::as_u64),
        Some(1)
    );
    assert_eq!(
        status
            .get("sessionIdPresent")
            .and_then(serde_json::Value::as_bool),
        Some(true)
    );

    let methods = seen
        .lock()
        .await
        .iter()
        .filter_map(|request| request.get("method").and_then(serde_json::Value::as_str))
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    assert_eq!(
        methods,
        vec![
            "initialize".to_string(),
            "notifications/initialized".to_string(),
            "tools/list".to_string()
        ]
    );
    let headers = seen_headers.lock().await;
    assert!(headers.iter().all(|header| {
        header
            .get("x-test-token")
            .and_then(serde_json::Value::as_str)
            == Some("secret")
    }));
    assert_eq!(
        headers
            .get(1)
            .and_then(|header| header.get("mcp-session-id"))
            .and_then(serde_json::Value::as_str),
        Some("session-123")
    );
    assert_eq!(
        headers
            .get(2)
            .and_then(|header| header.get("mcp-session-id"))
            .and_then(serde_json::Value::as_str),
        Some("session-123")
    );

    server_task.await.expect("fake server task");
    std::fs::remove_dir_all(root).ok();
}

#[tokio::test]
async fn mcp_request_supports_sse_servers() {
    use tokio::io::AsyncWriteExt;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind fake MCP SSE server");
    let addr = listener.local_addr().expect("fake server addr");
    let seen = Arc::new(Mutex::new(Vec::<String>::new()));
    let server_seen = seen.clone();
    let server_task = tokio::spawn(async move {
        // GET /sse
        let (mut stream, _) = listener.accept().await.expect("accept sse");
        let mut buf = vec![0u8; 2048];
        let n = stream.read(&mut buf).await.expect("read sse get");
        let req = String::from_utf8_lossy(&buf[..n]).to_string();
        assert!(req.starts_with("GET /sse"), "unexpected request: {req}");
        server_seen.lock().await.push("GET /sse".to_string());

        let endpoint = format!("/messages?sessionId=sse-session-1");
        let body = format!("event: endpoint\ndata: {endpoint}\n\n");
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: keep-alive\r\nTransfer-Encoding: chunked\r\n\r\n{:x}\r\n{}\r\n",
            body.len(),
            body
        );
        stream
            .write_all(response.as_bytes())
            .await
            .expect("write sse open");

        // Keep connection open and later write initialize + tools/list responses.
        let mut stream_sse = stream;

        // POST initialize
        let (mut stream_post, _) = listener.accept().await.expect("accept initialize");
        let (request, _) = read_test_http_json_request(&mut stream_post).await;
        assert_eq!(
            request.get("method").and_then(serde_json::Value::as_str),
            Some("initialize")
        );
        server_seen.lock().await.push("initialize".to_string());
        stream_post
            .write_all(b"HTTP/1.1 202 Accepted\r\nContent-Length: 0\r\n\r\n")
            .await
            .expect("ack initialize");
        let init_id = request.get("id").cloned().unwrap_or(serde_json::json!(1));
        let init_event = format!(
            "event: message\ndata: {}\n\n",
            serde_json::json!({
                "jsonrpc": "2.0",
                "id": init_id,
                "result": {
                    "protocolVersion": "2024-11-05",
                    "capabilities": {},
                    "serverInfo": { "name": "fake-sse", "version": "0.0.1" }
                }
            })
        );
        stream_sse
            .write_all(format!("{:x}\r\n{}\r\n", init_event.len(), init_event).as_bytes())
            .await
            .expect("write initialize event");

        // POST notifications/initialized
        let (mut stream_post, _) = listener.accept().await.expect("accept initialized");
        let (request, _) = read_test_http_json_request(&mut stream_post).await;
        assert_eq!(
            request.get("method").and_then(serde_json::Value::as_str),
            Some("notifications/initialized")
        );
        server_seen
            .lock()
            .await
            .push("notifications/initialized".to_string());
        stream_post
            .write_all(b"HTTP/1.1 202 Accepted\r\nContent-Length: 0\r\n\r\n")
            .await
            .expect("ack initialized");

        // POST tools/list
        let (mut stream_post, _) = listener.accept().await.expect("accept tools/list");
        let (request, _) = read_test_http_json_request(&mut stream_post).await;
        assert_eq!(
            request.get("method").and_then(serde_json::Value::as_str),
            Some("tools/list")
        );
        server_seen.lock().await.push("tools/list".to_string());
        stream_post
            .write_all(b"HTTP/1.1 202 Accepted\r\nContent-Length: 0\r\n\r\n")
            .await
            .expect("ack tools/list");
        let list_id = request.get("id").cloned().unwrap_or(serde_json::json!(2));
        let list_event = format!(
            "event: message\ndata: {}\n\n",
            serde_json::json!({
                "jsonrpc": "2.0",
                "id": list_id,
                "result": {
                    "tools": [{
                        "name": "ping",
                        "description": "ping",
                        "inputSchema": { "type": "object", "properties": {} }
                    }]
                }
            })
        );
        stream_sse
            .write_all(format!("{:x}\r\n{}\r\n", list_event.len(), list_event).as_bytes())
            .await
            .expect("write tools/list event");
    });

    let root =
        std::env::temp_dir().join(format!("cn-codex-mcp-sse-test-{}", uuid::Uuid::new_v4()));
    let config_dir = root.join("codey");
    let executor = ToolExecutor::with_workspace_config_dir(root.clone(), config_dir);
    let server = McpServerConfig {
        name: "remote_sse".to_string(),
        transport: "sse".to_string(),
        command: String::new(),
        args: Vec::new(),
        env: HashMap::new(),
        cwd: None,
        url: Some(format!("http://{addr}/sse")),
        headers: HashMap::new(),
        disabled: false,
    };

    let result = executor
        .mcp_request(&server, "tools/list", serde_json::json!({}))
        .await
        .expect("SSE tools/list should succeed");
    let status = executor.mcp_session_status("remote_sse").await;

    assert_eq!(
        result
            .pointer("/tools/0/name")
            .and_then(serde_json::Value::as_str),
        Some("ping")
    );
    assert_eq!(
        status.get("connected").and_then(serde_json::Value::as_bool),
        Some(true)
    );
    assert_eq!(
        status.get("transport").and_then(serde_json::Value::as_str),
        Some("sse")
    );
    assert_eq!(
        status
            .get("requestCount")
            .and_then(serde_json::Value::as_u64),
        Some(1)
    );
    assert_eq!(
        status.get("endpoint").and_then(serde_json::Value::as_str),
        Some(format!("http://{addr}/messages?sessionId=sse-session-1").as_str())
    );

    let methods = seen.lock().await.clone();
    assert_eq!(
        methods,
        vec![
            "GET /sse".to_string(),
            "initialize".to_string(),
            "notifications/initialized".to_string(),
            "tools/list".to_string()
        ]
    );

    server_task.await.expect("fake sse server task");
    std::fs::remove_dir_all(root).ok();
}

async fn read_test_http_json_request(
    stream: &mut tokio::net::TcpStream,
) -> (serde_json::Value, serde_json::Value) {
    let mut bytes = Vec::new();
    let header_end = loop {
        let mut chunk = [0u8; 1024];
        let read = stream.read(&mut chunk).await.expect("read request");
        assert!(read > 0, "client closed connection before headers");
        bytes.extend_from_slice(&chunk[..read]);
        if let Some(index) = find_header_end(&bytes) {
            break index;
        }
    };

    let headers_text = String::from_utf8_lossy(&bytes[..header_end]).to_string();
    let content_length = headers_text
        .lines()
        .find_map(|line| {
            let (key, value) = line.split_once(':')?;
            key.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().ok())
                .flatten()
        })
        .unwrap_or(0);
    let body_start = header_end + 4;
    while bytes.len() < body_start + content_length {
        let mut chunk = [0u8; 1024];
        let read = stream.read(&mut chunk).await.expect("read body");
        assert!(read > 0, "client closed connection before body");
        bytes.extend_from_slice(&chunk[..read]);
    }

    let body = &bytes[body_start..body_start + content_length];
    let request = serde_json::from_slice::<serde_json::Value>(body).expect("JSON body");
    let mut headers = serde_json::Map::new();
    for line in headers_text.lines().skip(1) {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        headers.insert(
            key.trim().to_ascii_lowercase(),
            serde_json::Value::String(value.trim().to_string()),
        );
    }
    (request, serde_json::Value::Object(headers))
}

async fn write_test_http_response(
    stream: &mut tokio::net::TcpStream,
    status: u16,
    session_id: Option<&str>,
    body: Option<serde_json::Value>,
    sse: bool,
) {
    let body = match (body, sse) {
        (Some(body), true) => format!("event: message\ndata: {}\n\n", body),
        (Some(body), false) => serde_json::to_string(&body).expect("response JSON"),
        (None, _) => String::new(),
    };
    let reason = match status {
        200 => "OK",
        202 => "Accepted",
        _ => "Status",
    };
    let content_type = if sse {
        "text/event-stream"
    } else {
        "application/json"
    };
    let session_header = session_id
        .map(|id| format!("Mcp-Session-Id: {id}\r\n"))
        .unwrap_or_default();
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\n{session_header}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.as_bytes().len()
    );
    stream
        .write_all(response.as_bytes())
        .await
        .expect("write response");
}

fn find_header_end(bytes: &[u8]) -> Option<usize> {
    bytes.windows(4).position(|window| window == b"\r\n\r\n")
}

#[test]
fn resolve_subagent_cwd_requires_existing_directory() {
    let root = std::env::temp_dir().join(format!(
        "cn-codex-subagent-cwd-test-{}",
        uuid::Uuid::new_v4()
    ));
    let child = root.join("child");
    std::fs::create_dir_all(&child).unwrap();

    assert_eq!(
        resolve_subagent_cwd(&root, None).unwrap(),
        root.canonicalize().unwrap()
    );
    assert_eq!(
        resolve_subagent_cwd(&root, Some("child")).unwrap(),
        child.canonicalize().unwrap()
    );
    assert!(resolve_subagent_cwd(&root, Some("missing")).is_err());

    std::fs::remove_dir_all(root).ok();
}

#[tokio::test]
async fn wait_for_subagents_reports_success_without_missing_flag() {
    let id = "agent-test".to_string();
    let mut map = HashMap::new();
    map.insert(
        id.clone(),
        SubagentRecord {
            id: id.clone(),
            role: "reviewer".to_string(),
            status: "completed".to_string(),
            prompt: "review".to_string(),
            cwd: ".".to_string(),
            command: "codex exec review".to_string(),
            process_id: None,
            started_at_ms: 10,
            completed_at_ms: Some(20),
            duration_ms: Some(10),
            exit_code: Some(0),
            output: Some("done".to_string()),
            error: None,
            input_history: Vec::new(),
            last_input_at_ms: None,
        },
    );

    let result = wait_for_subagents(Arc::new(Mutex::new(map)), vec![id], 0).await;
    assert!(!result.has_missing);
    assert!(!result.has_failed);
    assert!(result.output.contains("\"completed\": true"));
}

#[test]
fn load_subagent_records_marks_running_records_interrupted() {
    let root = std::env::temp_dir().join(format!(
        "cn-codex-subagent-load-test-{}",
        uuid::Uuid::new_v4()
    ));
    let config_dir = root.join("codey");
    std::fs::create_dir_all(config_dir.join("subagents")).unwrap();
    std::fs::write(
        subagent_state_path(&config_dir),
        r#"[
  {
"id": "agent-running",
"role": "tester",
"status": "running",
"prompt": "test",
"cwd": ".",
"command": "codex exec test",
"processId": 123,
"startedAtMs": 10
  }
]"#,
    )
    .unwrap();

    let records = load_subagent_records(&config_dir);
    let record = records.get("agent-running").unwrap();
    assert_eq!(record.status, "interrupted");
    assert_eq!(record.process_id, None);
    assert!(
        record
            .error
            .as_deref()
            .unwrap_or_default()
            .contains("running")
    );

    std::fs::remove_dir_all(root).ok();
}

#[tokio::test]
async fn persist_subagent_records_writes_state_file() {
    let root = std::env::temp_dir().join(format!(
        "cn-codex-subagent-persist-test-{}",
        uuid::Uuid::new_v4()
    ));
    let config_dir = root.join("codey");
    let id = "agent-persist-test".to_string();
    let mut map = HashMap::new();
    map.insert(
        id.clone(),
        SubagentRecord {
            id: id.clone(),
            role: "tester".to_string(),
            status: "completed".to_string(),
            prompt: "test".to_string(),
            cwd: ".".to_string(),
            command: "codex exec test".to_string(),
            process_id: None,
            started_at_ms: 10,
            completed_at_ms: Some(20),
            duration_ms: Some(10),
            exit_code: Some(0),
            output: Some("done".to_string()),
            error: None,
            input_history: Vec::new(),
            last_input_at_ms: None,
        },
    );
    let subagents = Arc::new(Mutex::new(map));

    persist_subagent_records(&config_dir, &subagents).await;

    let state = std::fs::read_to_string(subagent_state_path(&config_dir)).unwrap();
    assert!(state.contains("agent-persist-test"));
    assert!(state.contains("completed"));

    std::fs::remove_dir_all(root).ok();
}

#[test]
fn browser_run_display_uses_url_and_action_count() {
    let payload = serde_json::json!({
        "url": "http://localhost:1420",
        "actions": [
            { "type": "screenshot" },
            { "type": "text", "selector": "body" }
        ]
    });
    assert_eq!(
        browser_run_display(&payload),
        "http://localhost:1420 (2 actions)"
    );

    let empty = serde_json::json!({});
    assert_eq!(browser_run_display(&empty), "browser");
}

#[test]
fn browser_run_prefers_visible_browser_unless_disabled() {
    assert!(browser_run_use_visible_browser(&serde_json::json!({})));
    assert!(!browser_run_use_visible_browser(&serde_json::json!({
        "use_visible_browser": false
    })));
    assert!(!browser_run_use_visible_browser(&serde_json::json!({
        "useVisibleBrowser": false
    })));
}

#[test]
fn browser_run_initial_url_uses_url_or_first_goto() {
    assert_eq!(
        browser_run_initial_url(&serde_json::json!({
            "url": " http://localhost:1420 "
        })),
        Some("http://localhost:1420".to_string())
    );
    assert_eq!(
        browser_run_initial_url(&serde_json::json!({
            "actions": [
                { "type": "screenshot" },
                { "type": "goto", "url": "https://example.com" }
            ]
        })),
        Some("https://example.com".to_string())
    );
    assert_eq!(browser_run_initial_url(&serde_json::json!({})), None);
}

#[test]
fn classify_browser_run_error_maps_selector_timeout() {
    let (code, hint) = classify_browser_run_error(
        "Action 0 (wait_for_selector) failed: Timeout waiting for selector: #game-board",
    );
    assert_eq!(code, "SELECTOR_TIMEOUT");
    assert!(hint.contains("选择器"));
}

#[test]
fn classify_browser_run_error_maps_cdp_unavailable() {
    let (code, hint) = classify_browser_run_error(
        "WEBVIEW_CDP_UNAVAILABLE: cannot connect to http://127.0.0.1:9242",
    );
    assert_eq!(code, "WEBVIEW_CDP_UNAVAILABLE");
    assert!(hint.contains("CDP"));
}

#[test]
fn extract_browser_output_json_parses_mixed_stdout_and_stderr() {
    let mixed = "{\n  \"actions\": []\n}\n[stderr]\nwarning";
    let parsed = extract_browser_output_json(mixed).expect("expected browser json");
    assert_eq!(
        parsed
            .get("actions")
            .and_then(serde_json::Value::as_array)
            .map(Vec::len),
        Some(0)
    );
}

#[test]
fn mcp_direct_tool_name_sanitizes_and_caps_length() {
    let mut used = BTreeSet::new();
    let name = mcp_direct_tool_name("docs server", "read-file", &mut used);
    assert_eq!(name, "mcp__docs_server__read_file");

    let long = mcp_direct_tool_name(
        "server-with-a-very-very-long-name",
        "tool-with-a-very-very-long-name-and-extra-suffix",
        &mut used,
    );
    assert!(long.starts_with("mcp__server_with_a_very_very_long_name__tool_with"));
    assert!(long.len() <= 64);
}

#[test]
fn mcp_direct_tool_spec_uses_input_schema() {
    let spec = mcp_direct_tool_spec(
        "docs",
        "search",
        &serde_json::json!({
            "name": "search",
            "description": "Search docs",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": { "type": "string" }
                },
                "required": ["query"]
            }
        }),
        "mcp__docs__search",
    );

    assert_eq!(
        spec.get("function")
            .and_then(|function| function.get("name"))
            .and_then(serde_json::Value::as_str),
        Some("mcp__docs__search")
    );
    assert_eq!(
        spec.pointer("/function/parameters/required/0")
            .and_then(serde_json::Value::as_str),
        Some("query")
    );
}

#[test]
fn mcp_connector_metadata_is_trusted_only_for_codex_apps() {
    let tool = serde_json::json!({
        "name": "gmail_send",
        "description": "Send a message",
        "connector_id": "connector_gmail",
        "connector_name": "Gmail",
        "connector_description": "Tools for Gmail."
    });

    let trusted = mcp_connector_metadata("codex-apps", &tool);
    assert_eq!(trusted.connector_id.as_deref(), Some("connector_gmail"));
    assert_eq!(trusted.connector_name.as_deref(), Some("Gmail"));
    assert_eq!(
        trusted.namespace_description.as_deref(),
        Some("Tools for Gmail.")
    );

    let untrusted = mcp_connector_metadata("custom-server", &tool);
    assert_eq!(untrusted, McpConnectorMetadata::default());

    let trusted_spec = mcp_direct_tool_spec("codex-apps", "gmail_send", &tool, "mcp__x");
    assert!(
        trusted_spec
            .pointer("/function/description")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .contains("MCP app connector Gmail (connector_gmail)")
    );

    let untrusted_spec = mcp_direct_tool_spec("custom-server", "gmail_send", &tool, "mcp__x");
    let description = untrusted_spec
        .pointer("/function/description")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    assert!(description.contains("MCP tool custom-server:gmail_send."));
    assert!(!description.contains("connector_gmail"));
}

#[test]
fn code_review_analyzer_flags_risks_and_missing_tests() {
    let diff = r#"diff --git a/src/app.rs b/src/app.rs
index 1111111..2222222 100644
--- a/src/app.rs
+++ b/src/app.rs
@@ -10,0 +11,3 @@
+const API_KEY: &str = "sk_live_1234567890abcdef";
+let value = maybe.unwrap();
+todo!();
diff --git a/src/ui.tsx b/src/ui.tsx
index 1111111..2222222 100644
--- a/src/ui.tsx
+++ b/src/ui.tsx
@@ -3,0 +4,2 @@
+console.log("debug");
+<div dangerouslySetInnerHTML={{ __html: html }} />
"#;
    let numstat = "3\t0\tsrc/app.rs\n2\t0\tsrc/ui.tsx\n";
    let diff_check = GitCommandOutput {
        exit_code: 0,
        stdout: String::new(),
        stderr: String::new(),
    };

    let summary = analyze_code_review_diff(diff, numstat, &diff_check, &[], false);

    assert_eq!(summary.files_changed, 2);
    assert_eq!(summary.additions, 5);
    assert!(
        summary
            .findings
            .iter()
            .any(|finding| finding.priority == "P1" && finding.title.contains("Secret"))
    );
    assert!(summary.findings.iter().any(|finding| {
        finding
            .title
            .contains("Source changed without matching test")
    }));
    assert!(
        summary
            .findings
            .iter()
            .any(|finding| finding.title.contains("Risky dynamic"))
    );
    assert!(
        summary
            .findings
            .iter()
            .any(|finding| finding.title.contains("New Rust panic"))
    );
}

#[test]
fn image_generation_helpers_build_request_and_paths() {
    assert_eq!(
        image_generation_api_url(Some("https://example.test/v1/"), None),
        "https://example.test/v1/images/generations"
    );
    assert_eq!(
        image_generation_api_url(Some("https://example.test/v1/images/generations/"), None),
        "https://example.test/v1/images/generations"
    );
    assert_eq!(
        image_generation_api_url(None, Some("https://settings.test/v1/")),
        "https://settings.test/v1/images/generations"
    );
    assert_eq!(
        image_generation_model(Some("tool-override"), Some("settings-default")),
        "tool-override"
    );
    assert_eq!(
        image_generation_model(None, Some("settings-default")),
        "settings-default"
    );
    assert_eq!(
        image_generation_api_key(Some("sk-settings")),
        Some("sk-settings".to_string())
    );

    let body = image_generation_request_body(
        "draw a window",
        "gpt-image-1",
        Some("auto"),
        Some("high"),
        None,
        Some(3),
    );
    assert_eq!(
        body.pointer("/model").and_then(serde_json::Value::as_str),
        Some("gpt-image-1")
    );
    assert_eq!(
        body.pointer("/size").and_then(serde_json::Value::as_str),
        Some("auto")
    );
    assert_eq!(
        body.pointer("/quality").and_then(serde_json::Value::as_str),
        Some("high")
    );
    assert_eq!(
        body.pointer("/n").and_then(serde_json::Value::as_u64),
        Some(3)
    );
    assert_eq!(
        body.pointer("/response_format")
            .and_then(serde_json::Value::as_str),
        Some("b64_json")
    );
    assert!(body.pointer("/background").is_none());
    assert_eq!(image_generation_count(Some(0)), 1);
    assert_eq!(image_generation_count(Some(99)), 10);
    assert_eq!(
        ToolExecutor::resolve_generated_image_url(
            "/v1/files/image/example.png",
            "https://rayplus.site/v1/images/generations",
        )
        .unwrap(),
        "https://rayplus.site/v1/files/image/example.png"
    );
    assert_eq!(
        ToolExecutor::resolve_generated_image_url(
            "https://cdn.example.test/image.png",
            "https://rayplus.site/v1/images/generations",
        )
        .unwrap(),
        "https://cdn.example.test/image.png"
    );
    assert!(
        ToolExecutor::resolve_generated_image_url(
            "",
            "https://rayplus.site/v1/images/generations"
        )
        .is_err()
    );

    assert_eq!(
        decode_image_base64("data:image/png;base64, aGk=\n").unwrap(),
        b"hi"
    );

    let root = PathBuf::from("C:/workspace/app");
    let config_dir = root.join("codey");
    let default_path = resolve_image_generate_output_path(
        &root,
        &config_dir,
        None,
        "call-1",
        "draw a window",
        "png",
    )
    .unwrap();
    assert!(default_path.starts_with(config_dir.join("images").join("generated")));
    assert_eq!(
        default_path.extension().and_then(|value| value.to_str()),
        Some("png")
    );

    assert_eq!(
        resolve_image_generate_output_path(
            &root,
            &config_dir,
            Some("assets/generated/window"),
            "call-1",
            "draw",
            "webp",
        )
        .unwrap(),
        root.join("assets").join("generated").join("window.webp")
    );
    assert!(
        resolve_image_generate_output_path(
            &root,
            &config_dir,
            Some("../window.png"),
            "call-1",
            "draw",
            "png",
        )
        .is_err()
    );

    assert_eq!(
        resolve_image_generate_output_path_for_index(
            &root,
            &config_dir,
            Some("assets/generated/window"),
            "call-1",
            "draw",
            "png",
            0,
            2,
        )
        .unwrap(),
        root.join("assets").join("generated").join("window-1.png")
    );
    assert_eq!(
        resolve_image_generate_output_path_for_index(
            &root,
            &config_dir,
            Some("assets/generated/window"),
            "call-1",
            "draw",
            "png",
            1,
            2,
        )
        .unwrap(),
        root.join("assets").join("generated").join("window-2.png")
    );
}

#[test]
fn inspect_image_bytes_reads_png_dimensions() {
    let mut png = b"\x89PNG\r\n\x1A\n\0\0\0\rIHDR".to_vec();
    png.extend_from_slice(&320u32.to_be_bytes());
    png.extend_from_slice(&180u32.to_be_bytes());
    png.extend_from_slice(&[8, 6, 0, 0, 0]);

    assert_eq!(
        inspect_image_bytes(&png).unwrap(),
        ImageInfo {
            format: "PNG",
            mime: "image/png",
            width: 320,
            height: 180,
        }
    );
}

#[test]
fn format_plan_update_validates_and_renders_plan() {
    let output = format_plan_update(
        Some("Working through parity gaps"),
        &[
            PlanItemArg {
                step: "Audit missing tools".to_string(),
                status: "completed".to_string(),
            },
            PlanItemArg {
                step: "Add plan tool".to_string(),
                status: "in_progress".to_string(),
            },
            PlanItemArg {
                step: "Run tests".to_string(),
                status: "pending".to_string(),
            },
        ],
    )
    .unwrap();

    assert!(output.contains("Plan updated: Working through parity gaps"));
    assert!(output.contains("- [completed] Audit missing tools"));
    assert!(output.contains("- [in_progress] Add plan tool"));
}

#[test]
fn format_plan_update_rejects_multiple_in_progress_items() {
    let err = format_plan_update(
        None,
        &[
            PlanItemArg {
                step: "One".to_string(),
                status: "in_progress".to_string(),
            },
            PlanItemArg {
                step: "Two".to_string(),
                status: "in_progress".to_string(),
            },
        ],
    )
    .unwrap_err();

    assert!(err.contains("only one plan item"));
}

#[test]
fn format_echarts_report_outputs_reusable_echarts_block() {
    let output = format_echarts_report(&EchartsReportArgs {
        title: Some("Quarterly revenue".to_string()),
        chart_type: Some("bar".to_string()),
        option: serde_json::json!({
            "xAxis": { "type": "category", "data": ["Q1", "Q2"] },
            "yAxis": { "type": "value" },
            "series": [{ "type": "bar", "data": [120, 180] }]
        }),
        notes: Some("Revenue in ten-thousands".to_string()),
    })
    .unwrap();

    assert!(output.contains("ECharts report ready: Quarterly revenue"));
    assert!(output.contains("Chart type: bar"));
    assert!(output.contains("```echarts"));
    assert!(output.contains("\"series\""));
}

#[test]
fn format_echarts_report_rejects_non_object_option() {
    let err = format_echarts_report(&EchartsReportArgs {
        title: None,
        chart_type: None,
        option: serde_json::json!(["invalid"]),
        notes: None,
    })
    .unwrap_err();

    assert!(err.contains("option must be a JSON object"));
}

#[test]
fn request_user_input_validation_limits_questions_and_options() {
    let valid = RequestUserInputArgs {
        questions: vec![RequestUserInputQuestion {
            id: "choice".to_string(),
            header: "Choice".to_string(),
            question: "Which path should I take?".to_string(),
            options: vec![
                RequestUserInputQuestionOption {
                    label: "Fast".to_string(),
                    description: "Move quickly.".to_string(),
                },
                RequestUserInputQuestionOption {
                    label: "Careful".to_string(),
                    description: "Spend more time verifying.".to_string(),
                },
            ],
        }],
    };
    assert!(validate_request_user_input_args(&valid).is_ok());

    let empty = RequestUserInputArgs { questions: vec![] };
    assert!(
        validate_request_user_input_args(&empty)
            .unwrap_err()
            .contains("at least one")
    );

    let too_many = RequestUserInputArgs {
        questions: vec![
            valid.questions[0].clone(),
            valid.questions[0].clone(),
            valid.questions[0].clone(),
            valid.questions[0].clone(),
        ],
    };
    assert!(
        validate_request_user_input_args(&too_many)
            .unwrap_err()
            .contains("at most three")
    );
}

#[test]
fn request_permissions_validation_requires_known_non_empty_profile() {
    let valid = RequestPermissionsArgs {
        environment_id: None,
        reason: Some("Need to fetch dependencies".to_string()),
        permissions: serde_json::json!({
            "network": { "enabled": true }
        }),
    };
    assert!(validate_request_permissions_args(&valid).is_ok());

    let empty = RequestPermissionsArgs {
        environment_id: None,
        reason: None,
        permissions: serde_json::json!({}),
    };
    assert!(
        validate_request_permissions_args(&empty)
            .unwrap_err()
            .contains("at least one")
    );

    let unknown = RequestPermissionsArgs {
        environment_id: None,
        reason: None,
        permissions: serde_json::json!({ "camera": true }),
    };
    assert!(
        validate_request_permissions_args(&unknown)
            .unwrap_err()
            .contains("network or file_system")
    );
}

#[test]
fn permission_profile_covers_only_granted_subsets() {
    let granted = serde_json::json!({
        "network": { "enabled": true },
        "file_system": {
            "read": ["D:/workspace", "D:/cache"],
            "write": ["D:/workspace/out"]
        }
    });

    assert!(permission_profile_covers(
        &granted,
        &serde_json::json!({
            "network": { "enabled": true },
            "file_system": { "read": ["D:/cache"] }
        })
    ));
    assert!(!permission_profile_covers(
        &granted,
        &serde_json::json!({
            "network": { "enabled": false }
        })
    ));
    assert!(!permission_profile_covers(
        &granted,
        &serde_json::json!({
            "file_system": { "write": ["D:/secret"] }
        })
    ));
}

#[test]
fn granted_permissions_are_extracted_from_approval_result() {
    let result = serde_json::json!({
        "permissions": { "network": { "enabled": true } },
        "scope": "turn",
        "strict_auto_review": false
    });
    assert_eq!(
        granted_permissions_from_result(&result),
        Some(serde_json::json!({ "network": { "enabled": true } }))
    );
    assert!(
        granted_permissions_from_result(&serde_json::json!({ "permissions": {} })).is_none()
    );
}

#[tokio::test]
async fn executor_reuses_granted_additional_permissions() {
    let executor = ToolExecutor::new(PathBuf::from("."));
    let granted = serde_json::json!({
        "network": { "enabled": true },
        "file_system": { "read": ["D:/workspace"] }
    });
    executor.remember_permission_grant(granted).await;

    assert!(
        executor
            .additional_permissions_preapproved(
                Some("with_additional_permissions"),
                Some(&serde_json::json!({
                    "network": { "enabled": true }
                })),
            )
            .await
    );
    assert!(
        !executor
            .additional_permissions_preapproved(
                Some("with_additional_permissions"),
                Some(&serde_json::json!({
                    "file_system": { "write": ["D:/workspace"] }
                })),
            )
            .await
    );
    assert!(
        !executor
            .additional_permissions_preapproved(
                Some("require_escalated"),
                Some(&serde_json::json!({
                    "network": { "enabled": true }
                })),
            )
            .await
    );
}

#[test]
fn inspect_image_bytes_reads_gif_jpeg_and_webp_dimensions() {
    let mut gif = b"GIF89a".to_vec();
    gif.extend_from_slice(&64u16.to_le_bytes());
    gif.extend_from_slice(&32u16.to_le_bytes());
    assert_eq!(inspect_image_bytes(&gif).unwrap().width, 64);
    assert_eq!(inspect_image_bytes(&gif).unwrap().height, 32);

    let jpeg = vec![
        0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x04, 0x00, 0x00, 0xFF, 0xC0, 0x00, 0x11, 0x08, 0x00,
        0x0A, 0x00, 0x14, 0x03, 0x01, 0x11, 0x00, 0x02, 0x11, 0x00, 0x03, 0x11, 0x00, 0xFF,
        0xD9,
    ];
    let jpeg_info = inspect_image_bytes(&jpeg).unwrap();
    assert_eq!(jpeg_info.format, "JPEG");
    assert_eq!(jpeg_info.width, 20);
    assert_eq!(jpeg_info.height, 10);

    let mut webp = b"RIFF".to_vec();
    webp.extend_from_slice(&22u32.to_le_bytes());
    webp.extend_from_slice(b"WEBPVP8X");
    webp.extend_from_slice(&10u32.to_le_bytes());
    webp.extend_from_slice(&[0, 0, 0, 0]);
    webp.extend_from_slice(&[127, 2, 0]);
    webp.extend_from_slice(&[223, 0, 0]);
    let webp_info = inspect_image_bytes(&webp).unwrap();
    assert_eq!(webp_info.format, "WebP");
    assert_eq!(webp_info.width, 640);
    assert_eq!(webp_info.height, 224);
}

#[test]
fn resolve_view_image_path_accepts_absolute_and_rejects_parent_relative_paths() {
    let root = PathBuf::from("C:/workspace/app");
    assert_eq!(
        resolve_view_image_path(&root, "assets/pic.png").unwrap(),
        root.join("assets").join("pic.png")
    );
    assert_eq!(
        resolve_view_image_path(&root, "D:/images/pic.png").unwrap(),
        PathBuf::from("D:/images/pic.png")
    );
    assert!(resolve_view_image_path(&root, "../pic.png").is_err());
}

#[test]
fn extract_patch_argument_accepts_raw_patch_text_and_json_aliases() {
    let raw = r#"*** Begin Patch
*** Add File: src/raw.txt
+raw
*** End Patch"#;
    assert_eq!(extract_patch_argument(raw).unwrap(), raw);

    let wrapped_patch = serde_json::json!({ "patch": raw }).to_string();
    assert_eq!(extract_patch_argument(&wrapped_patch).unwrap(), raw);

    let wrapped_command = serde_json::json!({ "command": raw }).to_string();
    assert_eq!(extract_patch_argument(&wrapped_command).unwrap(), raw);

    let err = extract_patch_argument(r#"{"body":"missing"}"#).unwrap_err();
    assert!(err.contains("raw patch text"));
    assert!(err.contains("patch"));
    assert!(err.contains("command"));
}

#[test]
fn apply_patch_adds_updates_and_deletes_files() {
    let root = std::env::temp_dir().join(format!(
        "cn-codex-apply-patch-test-{}",
        uuid::Uuid::new_v4()
    ));
    let src = root.join("src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("app.txt"), "one\ntwo\n").unwrap();
    std::fs::write(src.join("remove.txt"), "remove me\n").unwrap();

    let patch = r#"*** Begin Patch
*** Update File: src/app.txt
@@
 one
-two
+three
*** Add File: src/new.txt
+hello
+world
*** Delete File: src/remove.txt
*** End Patch"#;

    let report = apply_patch_to_workspace(&root, patch).unwrap();

    assert_eq!(
        std::fs::read_to_string(src.join("app.txt")).unwrap(),
        "one\nthree\n"
    );
    assert_eq!(
        std::fs::read_to_string(src.join("new.txt")).unwrap(),
        "hello\nworld\n"
    );
    assert!(!src.join("remove.txt").exists());
    assert_eq!(
        report.changes,
        vec![
            ApplyPatchReportChange {
                path: "src/app.txt".to_string(),
                action: "modified",
                move_to: None,
            },
            ApplyPatchReportChange {
                path: "src/new.txt".to_string(),
                action: "created",
                move_to: None,
            },
            ApplyPatchReportChange {
                path: "src/remove.txt".to_string(),
                action: "deleted",
                move_to: None,
            },
        ]
    );

    std::fs::remove_dir_all(root).ok();
}

#[test]
fn apply_patch_progress_changes_describe_file_actions() {
    let patch = r#"*** Begin Patch
*** Add File: ./src/new.txt
+hello
*** Update File: src/app.txt
@@
-old
+new
*** Update File: src/old-name.txt
*** Move to: src/new-name.txt
@@
-old
+new
*** Delete File: src/remove.txt
*** End Patch"#;

    let actions = parse_patch_actions(patch).unwrap();
    assert_eq!(
        apply_patch_progress_changes(&actions),
        vec![
            ApplyPatchProgressChange {
                path: "src/new.txt".to_string(),
                action: "created",
                move_to: None,
                additions: 1,
                deletions: 0,
            },
            ApplyPatchProgressChange {
                path: "src/app.txt".to_string(),
                action: "modified",
                move_to: None,
                additions: 1,
                deletions: 1,
            },
            ApplyPatchProgressChange {
                path: "src/old-name.txt".to_string(),
                action: "renamed",
                move_to: Some("src/new-name.txt".to_string()),
                additions: 1,
                deletions: 1,
            },
            ApplyPatchProgressChange {
                path: "src/remove.txt".to_string(),
                action: "deleted",
                move_to: None,
                additions: 0,
                deletions: 0,
            },
        ]
    );
}

#[test]
fn apply_patch_updates_and_moves_file() {
    let root = std::env::temp_dir().join(format!(
        "cn-codex-apply-patch-move-test-{}",
        uuid::Uuid::new_v4()
    ));
    let src = root.join("src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("old.txt"), "alpha\nbeta\n").unwrap();

    let patch = r#"*** Begin Patch
*** Update File: src/old.txt
*** Move to: src/new.txt
@@
 alpha
-beta
+gamma
*** End Patch"#;

    let report = apply_patch_to_workspace(&root, patch).unwrap();

    assert!(!src.join("old.txt").exists());
    assert_eq!(
        std::fs::read_to_string(src.join("new.txt")).unwrap(),
        "alpha\ngamma\n"
    );
    assert_eq!(
        report.changes,
        vec![ApplyPatchReportChange {
            path: "src/old.txt".to_string(),
            action: "renamed",
            move_to: Some("src/new.txt".to_string()),
        }]
    );

    std::fs::remove_dir_all(root).ok();
}

#[test]
fn apply_patch_rejects_unsafe_paths() {
    let root = std::env::temp_dir().join(format!(
        "cn-codex-apply-patch-unsafe-test-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let patch = r#"*** Begin Patch
*** Add File: ../secret.txt
+nope
*** End Patch"#;

    let err = apply_patch_to_workspace(&root, patch).unwrap_err();

    assert!(err.contains("must not contain '..'"));
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn apply_patch_accepts_absolute_paths_inside_workspace() {
    let root = std::env::temp_dir().join(format!(
        "cn-codex-apply-patch-absolute-test-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let target = root.join("index.html");
    std::fs::write(&target, "<title>ABCDE</title>\n").unwrap();
    let patch = format!(
        "*** Begin Patch\n*** Update File: {}\n@@\n-<title>ABCDE</title>\n+<title>时尚代码</title>\n*** End Patch",
        target.display()
    );

    apply_patch_to_workspace(&root, &patch).unwrap();

    assert_eq!(
        std::fs::read_to_string(&target).unwrap(),
        "<title>时尚代码</title>\n"
    );
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn apply_patch_rejects_absolute_paths_outside_workspace() {
    let workspace = std::env::temp_dir().join(format!(
        "cn-codex-apply-patch-workspace-test-{}",
        uuid::Uuid::new_v4()
    ));
    let outside = std::env::temp_dir().join(format!(
        "cn-codex-apply-patch-outside-test-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&workspace).unwrap();
    let patch = format!(
        "*** Begin Patch\n*** Add File: {}\n+outside\n*** End Patch",
        outside.join("file.txt").display()
    );

    let err = apply_patch_to_workspace(&workspace, &patch).unwrap_err();

    assert!(err.contains("must be inside the workspace"));
    assert!(!outside.join("file.txt").exists());
    std::fs::remove_dir_all(workspace).ok();
}

#[test]
fn apply_patch_accepts_unified_diff_file_headers() {
    let root = std::env::temp_dir().join(format!(
        "cn-codex-apply-patch-unified-test-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("index.html"), "<title>ABCDE</title>\n").unwrap();
    let patch = r#"*** Begin Patch
*** Update File: index.html
--- a/index.html
+++ b/index.html
@@ -1 +1 @@
-<title>ABCDE</title>
+<title>时尚代码</title>
*** End Patch"#;

    apply_patch_to_workspace(&root, patch).unwrap();

    assert_eq!(
        std::fs::read_to_string(root.join("index.html")).unwrap(),
        "<title>时尚代码</title>\n"
    );
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn apply_patch_preflights_all_actions_before_writing() {
    let root = std::env::temp_dir().join(format!(
        "cn-codex-apply-patch-preflight-test-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("first.txt"), "before\n").unwrap();
    std::fs::write(root.join("exists.txt"), "keep\n").unwrap();

    let patch = r#"*** Begin Patch
*** Update File: first.txt
@@
-before
+after
*** Add File: exists.txt
+replacement
*** End Patch"#;

    let err = apply_patch_to_workspace(&root, patch).unwrap_err();

    assert!(err.contains("file already exists"));
    assert_eq!(
        std::fs::read_to_string(root.join("first.txt")).unwrap(),
        "before\n"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("exists.txt")).unwrap(),
        "keep\n"
    );
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn format_mcp_selection_result_preserves_partial_errors() {
    let (exit_code, output) = format_mcp_selection_result(
        vec![
            (
                "ok".to_string(),
                Ok(serde_json::json!({ "tools": [{ "name": "read" }] })),
            ),
            ("bad".to_string(), Err("failed".to_string())),
        ],
        "tools",
    );

    assert_eq!(exit_code, -1);
    assert!(output.contains("\"server\": \"ok\""));
    assert!(output.contains("\"server\": \"bad\""));
    assert!(output.contains("failed"));
}

#[test]
fn resolve_command_cwd_handles_relative_and_absolute_paths() {
    let base = PathBuf::from("C:/workspace/app");
    assert_eq!(resolve_command_cwd(&base, None), base);
    assert_eq!(
        resolve_command_cwd(Path::new("C:/workspace/app"), Some("tools/mcp")),
        PathBuf::from("C:/workspace/app").join("tools/mcp")
    );
    assert_eq!(
        resolve_command_cwd(Path::new("C:/workspace/app"), Some("D:/mcp")),
        PathBuf::from("D:/mcp")
    );
}

#[test]
fn resolve_mcp_command_for_platform_maps_windows_node_wrappers() {
    if cfg!(target_os = "windows") {
        assert_eq!(resolve_mcp_command_for_platform("npx"), "npx.cmd");
        assert_eq!(resolve_mcp_command_for_platform("npm"), "npm.cmd");
    } else {
        assert_eq!(resolve_mcp_command_for_platform("npx"), "npx");
        assert_eq!(resolve_mcp_command_for_platform("npm"), "npm");
    }
    assert_eq!(resolve_mcp_command_for_platform("python"), "python");
}

#[test]
fn should_inject_bundled_node_runtime_for_node_wrappers() {
    assert!(should_inject_bundled_node_runtime("npx"));
    assert!(should_inject_bundled_node_runtime("npx.cmd"));
    assert!(should_inject_bundled_node_runtime("npm"));
    assert!(should_inject_bundled_node_runtime("C:/bin/node.exe"));
    assert!(!should_inject_bundled_node_runtime("python"));
}

#[test]
fn prepend_path_value_puts_bundled_dir_first() {
    let bundled = PathBuf::from("node-runtime");
    let existing = std::env::join_paths([PathBuf::from("bin-a"), PathBuf::from("bin-b")])
        .expect("failed to build PATH fixture");
    let merged = prepend_path_value(Some(existing), &bundled);
    let entries: Vec<PathBuf> = std::env::split_paths(&merged).collect();
    assert_eq!(entries.first(), Some(&bundled));
}

#[test]
fn shell_args_accept_codex_style_script_and_legacy_argv() {
    let script: ShellArgs = serde_json::from_str(
        r#"{
            "command": "Get-ChildItem -Force",
            "workdir": "D:/workspace/app",
            "timeout_ms": 5000,
            "login": false
        }"#,
    )
    .unwrap();
    assert_eq!(
        shell_command_display(&script.command),
        "Get-ChildItem -Force"
    );
    assert_eq!(script.workdir.as_deref(), Some("D:/workspace/app"));
    assert_eq!(script.timeout_ms, Some(5000));
    assert_eq!(script.login, Some(false));

    let argv: ShellArgs = serde_json::from_str(
        r#"{
            "command": ["pnpm", "test", "--", "--run"]
        }"#,
    )
    .unwrap();
    assert_eq!(shell_command_display(&argv.command), "pnpm test -- --run");

    let alias_timeout: ShellArgs = serde_json::from_str(
        r#"{
            "command": "echo alias",
            "block_until_ms": 45000
        }"#,
    )
    .unwrap();
    assert_eq!(alias_timeout.timeout_ms, None);
    assert_eq!(alias_timeout.block_until_ms, Some(45000));
    assert_eq!(resolve_shell_timeout_ms(&alias_timeout).unwrap(), 45000);
}

#[test]
fn powershell_prefix_configures_console_pipeline_and_file_cmdlet_utf8() {
    let script =
        inject_powershell_utf8_prefix("Get-Content index.html -Raw | Set-Content index.html");

    assert!(script.starts_with(POWERSHELL_UTF8_PREFIX_MARKER));
    assert!(script.contains("[Console]::InputEncoding"));
    assert!(script.contains("[Console]::OutputEncoding"));
    assert!(script.contains("$OutputEncoding"));
    assert!(script.contains("$PSDefaultParameterValues['*:Encoding'] = 'utf8'"));
}

#[test]
fn powershell_prefix_is_not_added_twice() {
    let script = inject_powershell_utf8_prefix("Write-Output '中文'");
    assert_eq!(inject_powershell_utf8_prefix(&script), script);
}

#[test]
fn shell_file_editing_guard_blocks_encoding_risk_commands() {
    for command in [
        "Get-Content index.html -Raw | Set-Content index.html -Encoding UTF8",
        "$text | Out-File index.html",
        "Path('index.html').write_text(text)",
        "sed -i 's/old/new/' index.html",
    ] {
        let error = shell_file_editing_violation(command).expect(command);
        assert!(error.contains("Use apply_patch"));
        assert!(error.contains("encoding"));
    }

    assert!(shell_file_editing_violation("cargo test --lib").is_none());
    assert!(shell_file_editing_violation("Get-Content index.html -Raw").is_none());
}

#[test]
fn powershell_validation_rejects_missing_pipeline_input_and_bad_current_item() {
    for command in [
        "[ ] | Select-Object Name",
        "[x] | Select-Object -First 30 LineNumber,Filename,Line",
        "[ ] # try shell",
        "] # try shell",
        "  | Where-Object { $_.Name -match 'skill' }",
    ] {
        let error = powershell_command_validation_error(command).expect(command);
        assert!(error.contains("complete executable script"));
        assert!(error.contains("input-producing command"));
    }

    let error = powershell_command_validation_error(
        "Get-ChildItem | Where-Object { $*.FullName -match 'skill' }",
    )
    .expect("bad current-item variable");
    assert!(error.contains("Use '$_'"));
}

#[test]
fn powershell_validation_accepts_complete_pipelines() {
    for command in [
        "$items | Select-Object Name",
        "Get-ChildItem | Where-Object { $_.FullName -match 'skill' }",
        "cargo test --lib",
    ] {
        assert!(
            powershell_command_validation_error(command).is_none(),
            "{command}"
        );
    }
}

#[test]
fn shell_timeout_contract_enforces_explicit_bounds() {
    let mut args = ShellArgs {
        command: ShellCommandArg::Script("echo ok".to_string()),
        workdir: None,
        timeout_ms: Some(999),
        block_until_ms: None,
        login: None,
        sandbox_permissions: None,
        justification: None,
        prefix_rule: None,
        additional_permissions: None,
    };
    assert!(
        resolve_shell_timeout_ms(&args)
            .unwrap_err()
            .contains("at least")
    );

    args.timeout_ms = Some(3_600_001);
    assert!(
        resolve_shell_timeout_ms(&args)
            .unwrap_err()
            .contains("exceeds")
    );

    args.timeout_ms = None;
    args.block_until_ms = Some(60_000);
    assert_eq!(resolve_shell_timeout_ms(&args).unwrap(), 60_000);
}

#[test]
fn decode_command_output_bytes_supports_gbk_cp1252_and_utf8() {
    let utf8 = "路径 C:/workspace/result.json";
    assert_eq!(decode_command_output_bytes(utf8.as_bytes()), utf8);

    let (gbk_bytes, _, _) = encoding_rs::GBK.encode("中文输出");
    assert_eq!(decode_command_output_bytes(&gbk_bytes), "中文输出");

    let (cp1252_bytes, _, _) = WINDOWS_1252.encode("“quoted” test");
    let decoded = decode_command_output_bytes(&cp1252_bytes);
    assert!(decoded.contains("quoted"));
    assert!(decoded.contains('“'));
    assert!(decoded.contains('”'));
}

#[test]
fn normalize_windows_path_helpers_strip_verbatim_prefix() {
    assert_eq!(
        normalize_windows_verbatim_prefix(r#"\\?\C:\work\out\result.json"#),
        r#"C:\work\out\result.json"#
    );
    assert_eq!(
        normalize_windows_verbatim_prefix(r#"\\?\UNC\server\share\result.json"#),
        r#"\\server\share\result.json"#
    );
}

#[test]
fn truncate_shell_output_keeps_priority_lines() {
    let mut source = String::new();
    source.push_str(&"x".repeat(3500));
    source.push_str("\nartifact root: C:\\work\\artifact\\run-1\n");
    source.push_str("result_extract.json: C:\\work\\artifact\\run-1\\result_extract.json\n");
    source.push_str(&"y".repeat(3500));

    let truncated = truncate_shell_output(&source, 1200);
    assert!(truncated.contains("priority lines"));
    assert!(truncated.contains("result_extract.json"));
}

#[test]
fn shell_permission_args_validate_additional_permissions() {
    let default_with_extra = ShellArgs {
        command: ShellCommandArg::Script("echo ok".to_string()),
        workdir: None,
        timeout_ms: None,
        block_until_ms: None,
        login: None,
        sandbox_permissions: Some("use_default".to_string()),
        justification: None,
        prefix_rule: None,
        additional_permissions: Some(serde_json::json!({ "network": { "enabled": true } })),
    };
    assert!(
        validate_shell_permission_args(&default_with_extra)
            .unwrap_err()
            .contains("with_additional_permissions")
    );

    let missing_profile = ShellArgs {
        sandbox_permissions: Some("withAdditionalPermissions".to_string()),
        additional_permissions: None,
        ..default_with_extra.clone()
    };
    assert!(
        validate_shell_permission_args(&missing_profile)
            .unwrap_err()
            .contains("requires additional_permissions")
    );

    let valid = ShellArgs {
        sandbox_permissions: Some("with_additional_permissions".to_string()),
        additional_permissions: Some(serde_json::json!({
            "file_system": { "read": ["D:/workspace/app"] }
        })),
        ..default_with_extra
    };
    assert!(validate_shell_permission_args(&valid).is_ok());
    assert!(shell_requires_permission_approval(&valid));
}

#[test]
fn exec_command_args_parse_codex_style_fields() {
    let args: ExecCommandArgs = serde_json::from_str(
        r#"{
            "cmd": "pnpm test -- --run",
            "workdir": "D:/workspace/app",
            "yield_time_ms": 750,
            "max_output_tokens": 1200,
            "sandbox_permissions": "requireEscalated",
            "justification": "Need to run local tests"
        }"#,
    )
    .unwrap();

    assert_eq!(args.cmd, "pnpm test -- --run");
    assert_eq!(args.workdir.as_deref(), Some("D:/workspace/app"));
    assert_eq!(args.yield_time_ms, Some(750));
    assert_eq!(args.max_output_tokens, Some(1200));
    assert!(exec_requires_permission_approval(&args));
    assert!(validate_exec_permission_args(&args).is_ok());
}

#[tokio::test]
async fn exec_session_snapshot_returns_incremental_output_and_exit_code() {
    let record = ExecSessionRecord {
        id: 7,
        process_id: None,
        command: "echo hi".to_string(),
        cwd: "D:/workspace/app".to_string(),
        started_at_ms: now_millis(),
        output: Arc::new(Mutex::new("hello\n".to_string())),
        cursor: Arc::new(Mutex::new(0)),
        exit_code: Arc::new(Mutex::new(None)),
        stdin: Arc::new(Mutex::new(None)),
    };

    let first = exec_session_snapshot(&record, Some(100)).await;
    assert_eq!(
        first.get("session_id").and_then(serde_json::Value::as_u64),
        Some(7)
    );
    assert_eq!(
        first.get("output").and_then(serde_json::Value::as_str),
        Some("hello\n")
    );

    record.output.lock().await.push_str("done\n");
    *record.exit_code.lock().await = Some(0);
    let second = exec_session_snapshot(&record, Some(100)).await;
    assert_eq!(
        second.get("exit_code").and_then(serde_json::Value::as_i64),
        Some(0)
    );
    assert_eq!(
        second.get("output").and_then(serde_json::Value::as_str),
        Some("done\n")
    );
}

#[tokio::test]
async fn close_exec_session_record_marks_finished_session_closed() {
    let record = ExecSessionRecord {
        id: 9,
        process_id: None,
        command: "echo done".to_string(),
        cwd: "D:/workspace/app".to_string(),
        started_at_ms: now_millis(),
        output: Arc::new(Mutex::new("done\n".to_string())),
        cursor: Arc::new(Mutex::new(0)),
        exit_code: Arc::new(Mutex::new(Some(0))),
        stdin: Arc::new(Mutex::new(None)),
    };

    let closed = close_exec_session_record(record).await;

    assert_eq!(
        closed.get("session_id").and_then(serde_json::Value::as_u64),
        Some(9)
    );
    assert_eq!(
        closed.get("closed").and_then(serde_json::Value::as_bool),
        Some(true)
    );
    assert_eq!(
        closed
            .get("was_running")
            .and_then(serde_json::Value::as_bool),
        Some(false)
    );
    assert_eq!(
        closed
            .get("previous_exit_code")
            .and_then(serde_json::Value::as_i64),
        Some(0)
    );
    assert!(
        closed
            .get("output")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .contains("already finished")
    );
}

#[tokio::test]
async fn close_exec_session_record_stops_running_process() {
    let mut command = if cfg!(windows) {
        let mut command = Command::new("cmd");
        command.args(["/C", "ping -n 30 127.0.0.1 > NUL"]);
        command
    } else {
        let mut command = Command::new("sleep");
        command.arg("30");
        command
    };
    let mut child = match command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(child) => child,
        Err(_) => return,
    };
    let process_id = child.id();
    let record = ExecSessionRecord {
        id: 11,
        process_id,
        command: "long-running".to_string(),
        cwd: "D:/workspace/app".to_string(),
        started_at_ms: now_millis(),
        output: Arc::new(Mutex::new(String::new())),
        cursor: Arc::new(Mutex::new(0)),
        exit_code: Arc::new(Mutex::new(None)),
        stdin: Arc::new(Mutex::new(None)),
    };

    let closed = close_exec_session_record(record).await;
    let wait_result = tokio::time::timeout(Duration::from_secs(5), child.wait()).await;

    assert!(
        wait_result.is_ok(),
        "close_exec_session should stop the running process"
    );
    assert_eq!(
        closed.get("closed").and_then(serde_json::Value::as_bool),
        Some(true)
    );
    assert_eq!(
        closed
            .get("was_running")
            .and_then(serde_json::Value::as_bool),
        Some(true)
    );
    assert_eq!(
        closed.get("exit_code").and_then(serde_json::Value::as_i64),
        Some(130)
    );
}

#[test]
fn resolve_memory_path_rejects_escape_paths() {
    let root = PathBuf::from("C:/tmp/cn-codex-memories");

    assert_eq!(
        resolve_memory_path(&root, "project/notes.md").unwrap(),
        root.join("project").join("notes.md")
    );
    assert!(resolve_memory_path(&root, "../secret.md").is_err());
    assert!(resolve_memory_path(&root, "C:/secret.md").is_err());
    assert!(resolve_memory_path(&root, "/secret.md").is_err());
}

#[test]
fn search_memory_files_finds_markdown_matches() {
    let root =
        std::env::temp_dir().join(format!("cn-codex-memory-test-{}", uuid::Uuid::new_v4()));
    let nested = root.join("projects");
    std::fs::create_dir_all(&nested).unwrap();
    std::fs::write(
        nested.join("notes.md"),
        "First line\nRemember the lighthouse project\nOther line\n",
    )
    .unwrap();
    std::fs::write(nested.join("binary.bin"), "lighthouse").unwrap();

    let result = search_memory_files(&root, &root, "LIGHTHOUSE", false, 1, 0, 10).unwrap();

    assert_eq!(
        result.matches,
        vec![MemorySearchMatch {
            path: "projects/notes.md".to_string(),
            line_number: 2,
            line: "Remember the lighthouse project".to_string(),
            before: vec!["First line".to_string()],
            after: vec!["Other line".to_string()],
        }]
    );
    assert_eq!(result.total_matches, 1);
    assert_eq!(result.next_cursor, None);

    let json = format_memory_search_output("LIGHTHOUSE", &result, MemoryOutputFormat::Json);
    assert!(json.contains("\"totalMatches\": 1"));
    assert!(json.contains("\"before\""));

    std::fs::remove_dir_all(root).ok();
}

#[test]
fn html_to_text_strips_script_style_and_decodes_entities() {
    let html = r#"
        <html>
          <head><title>Example &amp; Docs</title><style>.x{display:none}</style></head>
          <body><h1>Hello&nbsp;world</h1><script>alert(1)</script><p>Rust &lt;3</p></body>
        </html>
    "#;

    assert_eq!(extract_html_title(html).as_deref(), Some("Example & Docs"));
    let text = html_to_text(html);
    assert!(text.contains("Hello world"));
    assert!(text.contains("Rust <3"));
    assert!(!text.contains("alert"));
    assert!(!text.contains("display:none"));
}

#[test]
fn format_duckduckgo_results_flattens_related_topics() {
    let response = DuckDuckGoResponse {
        related_topics: vec![DuckDuckGoTopic {
            topics: vec![DuckDuckGoTopic {
                text: "CN-Codex - A local coding app".to_string(),
                first_url: "https://example.com/cn-codex".to_string(),
                ..Default::default()
            }],
            ..Default::default()
        }],
        ..Default::default()
    };

    let formatted = format_duckduckgo_results("cn codex", response, 5);

    assert!(formatted.contains("CN-Codex"));
    assert!(formatted.contains("https://example.com/cn-codex"));
    assert!(formatted.contains("A local coding app"));
}

#[test]
fn web_search_provider_order_prefers_bing_before_duckduckgo() {
    assert_eq!(
        web_search_provider_order(),
        [
            WebSearchProvider::BingBrowser,
            WebSearchProvider::DuckDuckGoApi,
            WebSearchProvider::DuckDuckGoBrowser
        ]
    );
}

#[test]
fn web_search_output_has_results_detects_empty_marker() {
    assert!(web_search_output_has_results(
        "Web search results for \"cn codex\":\n1. Result"
    ));
    assert!(!web_search_output_has_results(
        "No web search results found for: cn codex (tried Bing and DuckDuckGo)"
    ));
}

#[test]
fn render_chunk_context_bridge_includes_neighbor_overlap_windows() {
    let root = std::env::temp_dir().join(format!(
        "cn-codex-smartbrain-context-{}",
        uuid::Uuid::new_v4()
    ));
    let docs_dir = root
        .join("codey")
        .join("memories")
        .join("knowledge")
        .join("docs");
    std::fs::create_dir_all(&docs_dir).unwrap();

    let prev_content = (1..=40)
        .map(|idx| format!("prev-{idx}"))
        .collect::<Vec<_>>()
        .join("\n");
    let current_content = (1..=40)
        .map(|idx| format!("current-{idx}"))
        .collect::<Vec<_>>()
        .join("\n");
    let next_content = (1..=40)
        .map(|idx| format!("next-{idx}"))
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(
        docs_dir.join(crate::smartbrain::knowledge::chunk_file_name("doc", 1)),
        &prev_content,
    )
    .unwrap();
    std::fs::write(
        docs_dir.join(crate::smartbrain::knowledge::chunk_file_name("doc", 2)),
        &current_content,
    )
    .unwrap();
    std::fs::write(
        docs_dir.join(crate::smartbrain::knowledge::chunk_file_name("doc", 3)),
        &next_content,
    )
    .unwrap();

    let executor = ToolExecutor::new(root.clone());
    let result = crate::smartbrain::search::SmartBrainSearchResult {
        doc_id: "know:doc::chunk:0002".to_string(),
        source_type: "knowledge".to_string(),
        file_path: "knowledge/docs/doc__chunk_0002.md".to_string(),
        title: "Doc chunk".to_string(),
        score: 1.0,
        tags: Vec::new(),
        concept_type: None,
        domain: None,
        source_group: None,
        relative_path: None,
        source_file: None,
        parent_doc_id: Some("doc".to_string()),
        chunk_index: Some(2),
        chunk_total: Some(3),
        is_chunk: true,
    };

    let block = executor
        .render_chunk_context_bridge(&result, 30)
        .expect("chunk context bridge should be generated");

    assert!(block.contains("overlap >= 30 lines"));
    assert!(block.contains("Prev chunk 1/3"));
    assert!(block.contains("Current chunk 2/3"));
    assert!(block.contains("next chunk 3/3"));
    assert!(block.contains("prev-11"));
    assert!(!block.contains("prev-10"));
    assert!(block.contains("next-30"));
    assert!(!block.contains("next-31"));
    assert!(block.contains("current-1"));
    assert!(block.contains("current-40"));

    std::fs::remove_dir_all(root).ok();
}

#[test]
fn encode_query_component_handles_spaces_and_unicode() {
    assert_eq!(encode_query_component("cn codex"), "cn%20codex");
    assert_eq!(encode_query_component("网页"), "%E7%BD%91%E9%A1%B5");
}

#[test]
fn format_read_file_output_returns_full_file_without_range() {
    let content = "alpha\nbeta\ngamma\n";
    let output = format_read_file_output("src/demo.rs", content, None, None, None, None);
    assert!(output.contains("File: src/demo.rs"));
    assert!(output.contains("Lines: 1-3 / 3"));
    assert!(output.contains("1|alpha"));
    assert!(output.contains("2|beta"));
    assert!(output.contains("3|gamma"));
    assert!(!output.contains("Next line_offset:"));
}

#[test]
fn format_read_file_output_supports_numbered_window_and_end_line() {
    let content = (1..=12)
        .map(|idx| format!("line-{idx}"))
        .collect::<Vec<_>>()
        .join("\n");

    let output =
        format_read_file_output("src/demo.rs", &content, Some(4), None, Some(6), Some(true));

    assert!(output.contains("File: src/demo.rs"));
    assert!(output.contains("Lines: 4-6 / 12"));
    assert!(output.contains("Next line_offset: 7"));
    assert!(output.contains("4|line-4"));
    assert!(output.contains("5|line-5"));
    assert!(output.contains("6|line-6"));
    assert!(!output.contains("3|line-3"));
    assert!(!output.contains("7|line-7"));
}

#[test]
fn format_read_file_output_rejects_invalid_end_line() {
    let output =
        format_read_file_output("src/demo.rs", "a\nb\nc\n", Some(3), None, Some(1), None);
    assert!(output.contains("end_line 1 must be >= line_offset 3"));
}

#[test]
fn format_read_file_output_defaults_to_paginated_window() {
    let content = (1..=250)
        .map(|idx| format!("line-{idx}"))
        .collect::<Vec<_>>()
        .join("\n");

    let output = format_read_file_output("src/demo.rs", &content, None, None, None, None);
    assert!(output.contains("Lines: 1-200 / 250"));
    assert!(output.contains("Next line_offset: 201"));
    assert!(output.contains("1|line-1"));
    assert!(output.contains("200|line-200"));
    assert!(!output.contains("201|line-201"));
}

#[test]
fn format_read_file_output_hard_caps_max_lines() {
    let content = (1..=500)
        .map(|idx| format!("line-{idx}"))
        .collect::<Vec<_>>()
        .join("\n");

    let output =
        format_read_file_output("src/demo.rs", &content, Some(1), Some(1000), None, None);
    assert!(output.contains("Lines: 1-400 / 500"));
    assert!(output.contains("Next line_offset: 401"));
    assert!(!output.contains("401|line-401"));
}

#[test]
fn read_file_tool_spec_exposes_range_parameters() {
    let temp_dir = tempfile::tempdir().expect("should create temp dir");
    let root = temp_dir.path().join("workspace");
    let config_dir = root.join("codey");
    std::fs::create_dir_all(&config_dir).expect("should create config dir");
    let executor = ToolExecutor::with_workspace_config_dir(root, config_dir);
    let tools = executor.tool_specs(false);
    let read_file = tools
        .iter()
        .find(|tool| {
            tool.get("function")
                .and_then(|function| function.get("name"))
                .and_then(serde_json::Value::as_str)
                == Some("read_file")
        })
        .expect("read_file tool spec");

    let properties = read_file
        .pointer("/function/parameters/properties")
        .expect("read_file properties");
    assert!(properties.get("line_offset").is_some());
    assert!(properties.get("max_lines").is_some());
    assert!(properties.get("end_line").is_some());
    assert!(properties.get("show_line_numbers").is_some());

    let description = read_file
        .pointer("/function/description")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    assert!(description.contains("line_offset"));
    assert!(description.contains("Prefer this over shell"));

    let max_lines = properties
        .pointer("/max_lines/maximum")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or_default();
    assert_eq!(max_lines, 400);
    assert!(
        description.contains("Defaults to a numbered page")
            || description.contains("numbered page")
    );
}
