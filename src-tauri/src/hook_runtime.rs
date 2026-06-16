use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use serde::Serialize;
use serde_json::Value as JsonValue;
use tauri::{AppHandle, Emitter};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;
use tracing::{info, warn};

#[cfg(windows)]
trait CommandNoConsole {
    fn no_console(&mut self) -> &mut Self;
}

#[cfg(windows)]
impl CommandNoConsole for Command {
    fn no_console(&mut self) -> &mut Self {
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        self.creation_flags(CREATE_NO_WINDOW)
    }
}

use crate::config_system::ConfigToml;
use crate::plugin_loader;

pub const HOOK_AGENT_START: &str = "on-agent-start";
pub const HOOK_USER_PROMPT_SUBMIT: &str = "on-user-prompt-submit";
pub const HOOK_AGENT_END: &str = "on-agent-end";
pub const HOOK_FILE_CHANGE: &str = "on-file-change";
pub const HOOK_COMMAND_EXEC: &str = "on-command-exec";
pub const HOOK_POST_TOOL_USE: &str = "on-post-tool-use";
pub const HOOK_SUBAGENT_STOP: &str = "on-subagent-stop";

const DEFAULT_HOOK_TIMEOUT_MS: u64 = 10_000;
const MAX_HOOK_TIMEOUT_MS: u64 = 120_000;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HookListItem {
    pub id: String,
    pub event: String,
    pub source_type: String,
    pub source_name: String,
    pub source_path: String,
    pub command: String,
    pub enabled: bool,
    pub matcher: Option<String>,
}

#[derive(Debug, Clone)]
struct HookDefinition {
    id: String,
    event: String,
    source_type: String,
    source_name: String,
    source_path: String,
    command: String,
    cwd: Option<PathBuf>,
    env: HashMap<String, String>,
    timeout_ms: u64,
    disabled: bool,
    matcher: Option<String>,
    plugin_root: Option<PathBuf>,
}

#[derive(Debug, Clone)]
struct HookSource {
    source_type: String,
    source_name: String,
    source_path: String,
    base_dir: PathBuf,
    plugin_root: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HookRunResult {
    pub id: String,
    pub event: String,
    pub source_type: String,
    pub source_name: String,
    pub command: String,
    pub status: String,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub error: Option<String>,
    pub duration_ms: u64,
    pub decision: Option<String>,
    pub reason: Option<String>,
    pub additional_context: Option<String>,
    pub updated_input: Option<JsonValue>,
    pub invalid_output: Option<String>,
}

#[derive(Debug, Clone)]
pub struct HookRuntime {
    hooks: Vec<HookDefinition>,
}

impl HookRuntime {
    pub fn load(config: &ConfigToml, workspace_config_dir: &Path) -> Self {
        Self {
            hooks: collect_hook_definitions(config, workspace_config_dir),
        }
    }

    pub fn list(&self) -> Vec<HookListItem> {
        hook_list_items(&self.hooks)
    }

    pub fn is_empty(&self) -> bool {
        self.hooks.iter().all(|hook| hook.disabled)
    }

    pub async fn run_event(
        &self,
        app_handle: &AppHandle,
        thread_id: &str,
        event: &str,
        cwd: &Path,
        context: JsonValue,
    ) -> Vec<HookRunResult> {
        let matching_hooks = self
            .hooks
            .iter()
            .filter(|hook| hook.event == event)
            .filter(|hook| !hook.disabled)
            .filter(|hook| hook_matches_context(hook, &context))
            .cloned()
            .collect::<Vec<_>>();

        let mut results = Vec::new();
        for hook in matching_hooks {
            results.push(run_hook(app_handle, thread_id, cwd, &hook, &context).await);
        }

        results
    }
}

pub fn list_hooks(config: &ConfigToml, workspace_config_dir: &Path) -> Vec<HookListItem> {
    hook_list_items(&collect_hook_definitions(config, workspace_config_dir))
}

pub fn first_blocking_hook_result(results: &[HookRunResult]) -> Option<&HookRunResult> {
    results
        .iter()
        .find(|result| result.decision.as_deref() == Some("block"))
}

pub fn latest_hook_updated_input(results: &[HookRunResult]) -> Option<JsonValue> {
    results
        .iter()
        .rev()
        .find_map(|result| result.updated_input.clone())
}

pub fn hook_feedback_for_model(results: &[HookRunResult]) -> Vec<String> {
    let mut feedback = Vec::new();
    for result in results {
        if let Some(context) = result
            .additional_context
            .as_deref()
            .and_then(trimmed_non_empty)
        {
            feedback.push(format!(
                "Additional context from hook `{}`: {context}",
                result.command
            ));
        }
        if matches!(
            result.decision.as_deref(),
            Some("block" | "stop" | "feedback")
        ) && let Some(reason) = result.reason.as_deref().and_then(trimmed_non_empty)
        {
            feedback.push(format!("Feedback from hook `{}`: {reason}", result.command));
        }
    }
    feedback
}

fn hook_list_items(hooks: &[HookDefinition]) -> Vec<HookListItem> {
    hooks
        .iter()
        .map(|hook| HookListItem {
            id: hook.id.clone(),
            event: hook.event.clone(),
            source_type: hook.source_type.clone(),
            source_name: hook.source_name.clone(),
            source_path: hook.source_path.clone(),
            command: hook.command.clone(),
            enabled: !hook.disabled,
            matcher: hook.matcher.clone(),
        })
        .collect()
}

fn collect_hook_definitions(
    config: &ConfigToml,
    workspace_config_dir: &Path,
) -> Vec<HookDefinition> {
    let mut hooks = Vec::new();
    collect_config_hooks(config, workspace_config_dir, &mut hooks);
    collect_hooks_json(workspace_config_dir, &mut hooks);
    collect_plugin_hooks(workspace_config_dir, &mut hooks);
    hooks.sort_by(|left, right| left.id.cmp(&right.id));
    hooks
}

fn collect_config_hooks(
    config: &ConfigToml,
    workspace_config_dir: &Path,
    hooks: &mut Vec<HookDefinition>,
) {
    if config.hooks.is_empty() {
        return;
    }

    let Some(value) = serde_json::to_value(&config.hooks).ok() else {
        return;
    };
    let source = HookSource {
        source_type: "config".to_string(),
        source_name: "codey/config.toml".to_string(),
        source_path: workspace_config_dir
            .join("config.toml")
            .to_string_lossy()
            .to_string(),
        base_dir: workspace_config_dir
            .parent()
            .unwrap_or(workspace_config_dir)
            .to_path_buf(),
        plugin_root: None,
    };

    collect_hooks_from_value(&value, &source, hooks);
}

fn collect_hooks_json(workspace_config_dir: &Path, hooks: &mut Vec<HookDefinition>) {
    let hooks_path = workspace_config_dir.join("hooks.json");
    if !hooks_path.is_file() {
        return;
    }

    let Ok(content) = std::fs::read_to_string(&hooks_path) else {
        return;
    };
    let Ok(value) = serde_json::from_str::<JsonValue>(&content) else {
        warn!("Failed to parse hooks config {}", hooks_path.display());
        return;
    };
    let source = HookSource {
        source_type: "hooksJson".to_string(),
        source_name: "codey/hooks.json".to_string(),
        source_path: hooks_path.to_string_lossy().to_string(),
        base_dir: workspace_config_dir
            .parent()
            .unwrap_or(workspace_config_dir)
            .to_path_buf(),
        plugin_root: None,
    };

    collect_hooks_from_value(&value, &source, hooks);
}

fn collect_plugin_hooks(workspace_config_dir: &Path, hooks: &mut Vec<HookDefinition>) {
    let plugins_dir = workspace_config_dir.join("plugins");
    let Ok(entries) = std::fs::read_dir(&plugins_dir) else {
        return;
    };

    let mut plugin_roots = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .filter(|path| !plugin_loader::is_plugin_disabled_root(path))
        .collect::<Vec<_>>();
    plugin_roots.sort();

    for plugin_root in plugin_roots {
        collect_plugin_root_hooks(&plugin_root, hooks);
    }
}

fn collect_plugin_root_hooks(plugin_root: &Path, hooks: &mut Vec<HookDefinition>) {
    let Some(manifest_path) = find_plugin_manifest_path(plugin_root) else {
        return;
    };
    let Ok(content) = std::fs::read_to_string(&manifest_path) else {
        return;
    };
    let Ok(manifest) = serde_json::from_str::<JsonValue>(&content) else {
        return;
    };

    let plugin_id = plugin_root
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "plugin".to_string());
    let plugin_name = manifest
        .get("interface")
        .and_then(|interface| {
            interface
                .get("displayName")
                .or_else(|| interface.get("display_name"))
                .and_then(JsonValue::as_str)
        })
        .or_else(|| manifest.get("name").and_then(JsonValue::as_str))
        .unwrap_or(&plugin_id)
        .trim()
        .to_string();

    match manifest.get("hooks") {
        Some(JsonValue::String(path)) => {
            collect_plugin_hooks_file(plugin_root, &plugin_name, path, hooks);
        }
        Some(JsonValue::Array(items)) => {
            for (index, item) in items.iter().enumerate() {
                if let Some(path) = item.as_str() {
                    collect_plugin_hooks_file(plugin_root, &plugin_name, path, hooks);
                } else if item.is_object() {
                    let source = HookSource {
                        source_type: "plugin".to_string(),
                        source_name: plugin_name.clone(),
                        source_path: format!("{}#hooks[{index}]", manifest_path.display()),
                        base_dir: plugin_root.to_path_buf(),
                        plugin_root: Some(plugin_root.to_path_buf()),
                    };
                    collect_hooks_from_value(item, &source, hooks);
                }
            }
        }
        Some(value) if value.is_object() => {
            let source = HookSource {
                source_type: "plugin".to_string(),
                source_name: plugin_name.clone(),
                source_path: format!("{}#hooks[0]", manifest_path.display()),
                base_dir: plugin_root.to_path_buf(),
                plugin_root: Some(plugin_root.to_path_buf()),
            };
            collect_hooks_from_value(value, &source, hooks);
        }
        Some(_) => {}
        None => {
            let default_path = plugin_root.join("hooks").join("hooks.json");
            if default_path.is_file() {
                collect_hooks_file(
                    &default_path,
                    HookSource {
                        source_type: "plugin".to_string(),
                        source_name: plugin_name,
                        source_path: default_path.to_string_lossy().to_string(),
                        base_dir: plugin_root.to_path_buf(),
                        plugin_root: Some(plugin_root.to_path_buf()),
                    },
                    hooks,
                );
            }
        }
    }
}

fn collect_plugin_hooks_file(
    plugin_root: &Path,
    plugin_name: &str,
    raw_path: &str,
    hooks: &mut Vec<HookDefinition>,
) {
    let Ok(path) = resolve_manifest_relative_path(plugin_root, raw_path) else {
        return;
    };
    if !path.is_file() {
        return;
    }

    collect_hooks_file(
        &path,
        HookSource {
            source_type: "plugin".to_string(),
            source_name: plugin_name.to_string(),
            source_path: path.to_string_lossy().to_string(),
            base_dir: plugin_root.to_path_buf(),
            plugin_root: Some(plugin_root.to_path_buf()),
        },
        hooks,
    );
}

fn collect_hooks_file(path: &Path, source: HookSource, hooks: &mut Vec<HookDefinition>) {
    let Ok(content) = std::fs::read_to_string(path) else {
        return;
    };
    let Ok(value) = serde_json::from_str::<JsonValue>(&content) else {
        warn!("Failed to parse hooks config {}", path.display());
        return;
    };
    collect_hooks_from_value(&value, &source, hooks);
}

fn collect_hooks_from_value(
    value: &JsonValue,
    source: &HookSource,
    hooks: &mut Vec<HookDefinition>,
) {
    let hooks_value = value.get("hooks").unwrap_or(value);
    let Some(map) = hooks_value.as_object() else {
        return;
    };

    for (raw_event, raw_config) in map {
        let Some(event) = normalize_hook_event(raw_event) else {
            continue;
        };
        collect_event_hooks(&event, raw_config, source, hooks);
    }
}

fn collect_event_hooks(
    event: &str,
    raw_config: &JsonValue,
    source: &HookSource,
    hooks: &mut Vec<HookDefinition>,
) {
    match raw_config {
        JsonValue::String(_) => push_hook_definition(event, raw_config, None, source, hooks),
        JsonValue::Array(items) => {
            for item in items {
                collect_event_hook_item(event, item, source, hooks);
            }
        }
        JsonValue::Object(map) => {
            if map.contains_key("command") || map.contains_key("commandWindows") {
                push_hook_definition(event, raw_config, None, source, hooks);
            } else if let Some(inner_hooks) = map.get("hooks") {
                let matcher = map
                    .get("matcher")
                    .and_then(JsonValue::as_str)
                    .map(ToString::to_string);
                match inner_hooks {
                    JsonValue::Array(items) => {
                        for item in items {
                            push_hook_definition(event, item, matcher.clone(), source, hooks);
                        }
                    }
                    other => push_hook_definition(event, other, matcher, source, hooks),
                }
            }
        }
        _ => {}
    }
}

fn collect_event_hook_item(
    event: &str,
    item: &JsonValue,
    source: &HookSource,
    hooks: &mut Vec<HookDefinition>,
) {
    let matcher = item
        .get("matcher")
        .and_then(JsonValue::as_str)
        .map(ToString::to_string);

    if let Some(inner_hooks) = item.get("hooks") {
        match inner_hooks {
            JsonValue::Array(items) => {
                for hook in items {
                    push_hook_definition(event, hook, matcher.clone(), source, hooks);
                }
            }
            other => push_hook_definition(event, other, matcher, source, hooks),
        }
    } else {
        push_hook_definition(event, item, matcher, source, hooks);
    }
}

fn push_hook_definition(
    event: &str,
    raw: &JsonValue,
    matcher: Option<String>,
    source: &HookSource,
    hooks: &mut Vec<HookDefinition>,
) {
    let Some(parsed) = parse_hook_command(raw, source) else {
        return;
    };

    let index = hooks.len();
    let id = format!(
        "{}:{}:{}:{}",
        source.source_type, source.source_name, event, index
    );
    hooks.push(HookDefinition {
        id,
        event: event.to_string(),
        source_type: source.source_type.clone(),
        source_name: source.source_name.clone(),
        source_path: source.source_path.clone(),
        command: parsed.command,
        cwd: parsed.cwd,
        env: parsed.env,
        timeout_ms: parsed.timeout_ms,
        disabled: parsed.disabled,
        matcher,
        plugin_root: source.plugin_root.clone(),
    });
}

#[derive(Debug)]
struct ParsedHookCommand {
    command: String,
    cwd: Option<PathBuf>,
    env: HashMap<String, String>,
    timeout_ms: u64,
    disabled: bool,
}

fn parse_hook_command(raw: &JsonValue, source: &HookSource) -> Option<ParsedHookCommand> {
    match raw {
        JsonValue::String(command) => Some(ParsedHookCommand {
            command: expand_command_vars(command.trim(), source),
            cwd: None,
            env: HashMap::new(),
            timeout_ms: DEFAULT_HOOK_TIMEOUT_MS,
            disabled: command.trim().is_empty(),
        }),
        JsonValue::Object(map) => {
            if map
                .get("type")
                .and_then(JsonValue::as_str)
                .is_some_and(|hook_type| hook_type != "command")
            {
                return None;
            }

            let command = if cfg!(target_os = "windows") {
                map.get("commandWindows")
                    .or_else(|| map.get("command_windows"))
                    .and_then(JsonValue::as_str)
                    .or_else(|| map.get("command").and_then(JsonValue::as_str))
            } else {
                map.get("command").and_then(JsonValue::as_str)
            }?
            .trim()
            .to_string();

            let enabled = map
                .get("enabled")
                .and_then(JsonValue::as_bool)
                .unwrap_or(true);
            let disabled = map
                .get("disabled")
                .and_then(JsonValue::as_bool)
                .unwrap_or(false)
                || !enabled
                || command.is_empty();
            let cwd = map
                .get("cwd")
                .and_then(JsonValue::as_str)
                .and_then(|cwd| resolve_hook_cwd(cwd, &source.base_dir));
            let env = map
                .get("env")
                .and_then(JsonValue::as_object)
                .map(|env| {
                    env.iter()
                        .filter_map(|(key, value)| {
                            value
                                .as_str()
                                .map(|val| (key.clone(), expand_command_vars(val, source)))
                        })
                        .collect()
                })
                .unwrap_or_default();
            let timeout_ms = map
                .get("timeoutMs")
                .or_else(|| map.get("timeout_ms"))
                .or_else(|| map.get("timeout"))
                .and_then(JsonValue::as_u64)
                .map(normalize_timeout_ms)
                .unwrap_or(DEFAULT_HOOK_TIMEOUT_MS);

            Some(ParsedHookCommand {
                command: expand_command_vars(&command, source),
                cwd,
                env,
                timeout_ms,
                disabled,
            })
        }
        _ => None,
    }
}

fn normalize_timeout_ms(value: u64) -> u64 {
    let value = if value < 1_000 { value * 1_000 } else { value };
    value.clamp(1_000, MAX_HOOK_TIMEOUT_MS)
}

fn resolve_hook_cwd(raw: &str, base_dir: &Path) -> Option<PathBuf> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }

    let path = Path::new(raw);
    if path.is_absolute() {
        return Some(path.to_path_buf());
    }

    Some(base_dir.join(path))
}

fn expand_command_vars(input: &str, source: &HookSource) -> String {
    let mut output = input.to_string();
    if let Some(plugin_root) = &source.plugin_root {
        let plugin_root = plugin_root.to_string_lossy();
        output = output.replace("${CODEX_PLUGIN_ROOT}", &plugin_root);
        output = output.replace("$CODEX_PLUGIN_ROOT", &plugin_root);
    }
    output
}

fn normalize_hook_event(raw: &str) -> Option<String> {
    let normalized = raw.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "on-agent-start" | "sessionstart" | "session_start" => Some(HOOK_AGENT_START.to_string()),
        "on-user-prompt-submit" | "userpromptsubmit" | "user_prompt_submit" => {
            Some(HOOK_USER_PROMPT_SUBMIT.to_string())
        }
        "on-agent-end" | "stop" | "sessionend" | "session_end" => Some(HOOK_AGENT_END.to_string()),
        "on-subagent-stop" | "subagentstop" | "subagent_stop" => {
            Some(HOOK_SUBAGENT_STOP.to_string())
        }
        "on-file-change" => Some(HOOK_FILE_CHANGE.to_string()),
        "on-post-tool-use" | "posttooluse" | "post_tool_use" => {
            Some(HOOK_POST_TOOL_USE.to_string())
        }
        "on-command-exec" | "pretooluse" | "pre_tool_use" | "permissionrequest"
        | "permission_request" => Some(HOOK_COMMAND_EXEC.to_string()),
        _ => None,
    }
}

fn hook_matches_context(hook: &HookDefinition, context: &JsonValue) -> bool {
    let Some(matcher) = hook
        .matcher
        .as_deref()
        .map(str::trim)
        .filter(|m| !m.is_empty())
    else {
        return true;
    };

    let matcher = matcher.to_ascii_lowercase();
    if matcher == "*" {
        return true;
    }

    if hook.event == HOOK_SUBAGENT_STOP {
        let agent_type = context
            .get("agentType")
            .or_else(|| context.get("agent_type"))
            .or_else(|| context.get("role"))
            .and_then(JsonValue::as_str)
            .unwrap_or_default()
            .to_ascii_lowercase();
        return agent_type.is_empty() || agent_type == matcher;
    }

    if hook.event != HOOK_COMMAND_EXEC && hook.event != HOOK_POST_TOOL_USE {
        return true;
    }

    let tool_name = context
        .get("toolName")
        .or_else(|| context.get("tool"))
        .and_then(JsonValue::as_str)
        .unwrap_or_default()
        .to_ascii_lowercase();
    if tool_name.is_empty() {
        return true;
    }

    matcher == tool_name
        || (tool_name == "shell" && matcher == "bash")
        || matcher == "command"
        || matcher == "shell"
}

async fn run_hook(
    app_handle: &AppHandle,
    thread_id: &str,
    cwd: &Path,
    hook: &HookDefinition,
    context: &JsonValue,
) -> HookRunResult {
    let run_id = uuid::Uuid::new_v4().to_string();
    let started_at = now_millis();
    let started = std::time::Instant::now();
    let run_cwd = hook.cwd.as_deref().unwrap_or(cwd);
    info!("Running hook {} ({})", hook.id, hook.event);

    app_handle
        .emit(
            "hook-started",
            serde_json::json!({
                "threadId": thread_id,
                "run": {
                    "id": run_id,
                    "hookId": &hook.id,
                    "event": &hook.event,
                    "sourceType": &hook.source_type,
                    "sourceName": &hook.source_name,
                    "sourcePath": &hook.source_path,
                    "command": &hook.command,
                    "startedAt": started_at,
                }
            }),
        )
        .ok();

    let mut result = execute_hook_command(run_cwd, hook, context).await;
    result.id = run_id.clone();
    result.duration_ms = started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;

    app_handle
        .emit(
            "hook-completed",
            serde_json::json!({
                "threadId": thread_id,
                "run": {
                    "id": run_id,
                    "hookId": &hook.id,
                    "event": &hook.event,
                    "sourceType": &hook.source_type,
                    "sourceName": &hook.source_name,
                    "sourcePath": &hook.source_path,
                    "command": &hook.command,
                    "status": &result.status,
                    "exitCode": result.exit_code,
                    "stdout": truncate_for_event(&result.stdout),
                    "stderr": truncate_for_event(&result.stderr),
                    "error": &result.error,
                    "decision": &result.decision,
                    "reason": &result.reason,
                    "additionalContext": &result.additional_context,
                    "updatedInput": &result.updated_input,
                    "invalidOutput": &result.invalid_output,
                    "startedAt": started_at,
                    "completedAt": now_millis(),
                    "durationMs": result.duration_ms,
                }
            }),
        )
        .ok();

    result
}

async fn execute_hook_command(
    cwd: &Path,
    hook: &HookDefinition,
    context: &JsonValue,
) -> HookRunResult {
    let (program, args) = shell_command(&hook.command);
    let mut command = Command::new(program);
    command
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("CN_CODEX_HOOK_EVENT", &hook.event)
        .env("CN_CODEX_HOOK_ID", &hook.id)
        .env("CN_CODEX_HOOK_SOURCE", &hook.source_name);

    if let Some(plugin_root) = &hook.plugin_root {
        command.env("CODEX_PLUGIN_ROOT", plugin_root);
    }

    for (key, value) in &hook.env {
        command.env(key, value);
    }

    #[cfg(windows)]
    command.no_console();

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(err) => {
            return hook_result(
                hook,
                "error",
                None,
                "",
                "",
                Some(format!("Failed to spawn hook: {err}")),
                0,
            );
        }
    };

    if let Some(mut stdin) = child.stdin.take() {
        let payload = serde_json::json!({
            "event": &hook.event,
            "hookId": &hook.id,
            "sourceType": &hook.source_type,
            "sourceName": &hook.source_name,
            "sourcePath": &hook.source_path,
            "cwd": cwd,
            "context": context,
        });
        let _ = stdin.write_all(payload.to_string().as_bytes()).await;
    }

    let child_stdout = child.stdout.take();
    let child_stderr = child.stderr.take();
    let stdout_handle = tokio::spawn(async move {
        let mut buf = Vec::new();
        if let Some(mut out) = child_stdout {
            let _ = out.read_to_end(&mut buf).await;
        }
        buf
    });
    let stderr_handle = tokio::spawn(async move {
        let mut buf = Vec::new();
        if let Some(mut err) = child_stderr {
            let _ = err.read_to_end(&mut buf).await;
        }
        buf
    });

    match tokio::time::timeout(Duration::from_millis(hook.timeout_ms), child.wait()).await {
        Ok(Ok(status)) => {
            let stdout =
                String::from_utf8_lossy(&stdout_handle.await.unwrap_or_default()).to_string();
            let stderr =
                String::from_utf8_lossy(&stderr_handle.await.unwrap_or_default()).to_string();
            let exit_code = status.code().unwrap_or(-1);
            let mut result = hook_result(
                hook,
                if exit_code == 0 { "success" } else { "failed" },
                Some(exit_code),
                &stdout,
                &stderr,
                None,
                0,
            );
            apply_hook_output_effects(&mut result);
            result
        }
        Ok(Err(err)) => {
            stdout_handle.abort();
            stderr_handle.abort();
            hook_result(
                hook,
                "error",
                None,
                "",
                "",
                Some(format!("Failed to wait for hook: {err}")),
                0,
            )
        }
        Err(_) => {
            let _ = child.kill().await;
            stdout_handle.abort();
            stderr_handle.abort();
            hook_result(
                hook,
                "timeout",
                Some(124),
                "",
                "",
                Some(format!("Hook timed out after {} ms", hook.timeout_ms)),
                0,
            )
        }
    }
}

fn hook_result(
    hook: &HookDefinition,
    status: &str,
    exit_code: Option<i32>,
    stdout: &str,
    stderr: &str,
    error: Option<String>,
    duration_ms: u64,
) -> HookRunResult {
    HookRunResult {
        id: String::new(),
        event: hook.event.clone(),
        source_type: hook.source_type.clone(),
        source_name: hook.source_name.clone(),
        command: hook.command.clone(),
        status: status.to_string(),
        exit_code,
        stdout: stdout.to_string(),
        stderr: stderr.to_string(),
        error,
        duration_ms,
        decision: None,
        reason: None,
        additional_context: None,
        updated_input: None,
        invalid_output: None,
    }
}

fn apply_hook_output_effects(result: &mut HookRunResult) {
    if result.event == HOOK_AGENT_END || result.event == HOOK_SUBAGENT_STOP {
        apply_stop_output_effects(result);
        return;
    }

    if result.event == HOOK_USER_PROMPT_SUBMIT {
        apply_user_prompt_submit_output_effects(result);
        return;
    }

    if result.event == HOOK_POST_TOOL_USE {
        apply_post_tool_use_output_effects(result);
        return;
    }

    if result.event == HOOK_COMMAND_EXEC {
        apply_pre_tool_use_output_effects(result);
    }
}

fn apply_pre_tool_use_output_effects(result: &mut HookRunResult) {
    if result.exit_code == Some(2) {
        if let Some(reason) = trimmed_non_empty(&result.stderr) {
            result.status = "blocked".to_string();
            result.decision = Some("block".to_string());
            result.reason = Some(reason);
        } else {
            result.status = "failed".to_string();
            result.invalid_output = Some(
                "PreToolUse hook exited with code 2 but did not write a blocking reason to stderr"
                    .to_string(),
            );
        }
        return;
    }

    if result.exit_code != Some(0) {
        return;
    }

    match parse_pre_tool_use_output(&result.stdout) {
        Some(effect) => {
            if let Some(invalid_output) = effect.invalid_output {
                result.status = "failed".to_string();
                result.invalid_output = Some(invalid_output);
                return;
            }
            result.additional_context = effect.additional_context;
            if let Some(reason) = effect.block_reason {
                result.status = "blocked".to_string();
                result.decision = Some("block".to_string());
                result.reason = Some(reason);
            } else if let Some(updated_input) = effect.updated_input {
                result.decision = Some("allow".to_string());
                result.updated_input = Some(updated_input);
            }
        }
        None if looks_like_json(&result.stdout) => {
            result.status = "failed".to_string();
            result.invalid_output =
                Some("hook returned invalid pre-tool-use JSON output".to_string());
        }
        None => {}
    }
}

fn apply_post_tool_use_output_effects(result: &mut HookRunResult) {
    if result.exit_code == Some(2) {
        if let Some(reason) = trimmed_non_empty(&result.stderr) {
            result.status = "success".to_string();
            result.decision = Some("feedback".to_string());
            result.reason = Some(reason);
        } else {
            result.status = "failed".to_string();
            result.invalid_output = Some(
                "PostToolUse hook exited with code 2 but did not write feedback to stderr"
                    .to_string(),
            );
        }
        return;
    }

    if result.exit_code != Some(0) {
        return;
    }

    match parse_post_tool_use_output(&result.stdout) {
        Some(effect) => {
            if let Some(invalid_output) = effect.invalid_output {
                result.status = "failed".to_string();
                result.invalid_output = Some(invalid_output);
                return;
            }
            result.additional_context = effect.additional_context;
            if let Some(reason) = effect.stop_reason {
                result.status = "stopped".to_string();
                result.decision = Some("stop".to_string());
                result.reason = Some(reason);
            } else if let Some(reason) = effect.block_reason {
                result.status = "blocked".to_string();
                result.decision = Some("block".to_string());
                result.reason = Some(reason);
            }
        }
        None if looks_like_json(&result.stdout) => {
            result.status = "failed".to_string();
            result.invalid_output =
                Some("hook returned invalid post-tool-use JSON output".to_string());
        }
        None => {}
    }
}

fn apply_stop_output_effects(result: &mut HookRunResult) {
    let event_label = if result.event == HOOK_SUBAGENT_STOP {
        "SubagentStop"
    } else {
        "Stop"
    };

    if result.exit_code == Some(2) {
        if let Some(reason) = trimmed_non_empty(&result.stderr) {
            result.status = "blocked".to_string();
            result.decision = Some("block".to_string());
            result.reason = Some(reason);
        } else {
            result.status = "failed".to_string();
            result.invalid_output = Some(format!(
                "{event_label} hook exited with code 2 but did not write a continuation prompt to stderr"
            ));
        }
        return;
    }

    if result.exit_code != Some(0) {
        return;
    }

    match parse_stop_output(&result.stdout, event_label) {
        Some(effect) => {
            if let Some(invalid_output) = effect.invalid_output {
                result.status = "failed".to_string();
                result.invalid_output = Some(invalid_output);
                return;
            }
            result.additional_context = effect.additional_context;
            if let Some(reason) = effect.stop_reason {
                result.status = "stopped".to_string();
                result.decision = Some("stop".to_string());
                result.reason = Some(reason);
            } else if let Some(reason) = effect.block_reason {
                result.status = "blocked".to_string();
                result.decision = Some("block".to_string());
                result.reason = Some(reason);
            }
        }
        None if looks_like_json(&result.stdout) => {
            result.status = "failed".to_string();
            result.invalid_output = Some(if result.event == HOOK_SUBAGENT_STOP {
                "hook returned invalid subagent stop hook JSON output".to_string()
            } else {
                "hook returned invalid stop hook JSON output".to_string()
            });
        }
        None => {}
    }
}

fn apply_user_prompt_submit_output_effects(result: &mut HookRunResult) {
    if result.exit_code == Some(2) {
        if let Some(reason) = trimmed_non_empty(&result.stderr) {
            result.status = "blocked".to_string();
            result.decision = Some("block".to_string());
            result.reason = Some(reason);
        } else {
            result.status = "failed".to_string();
            result.invalid_output = Some(
                "UserPromptSubmit hook exited with code 2 but did not write a blocking reason to stderr"
                    .to_string(),
            );
        }
        return;
    }

    if result.exit_code != Some(0) {
        return;
    }

    match parse_user_prompt_submit_output(&result.stdout) {
        Some(effect) => {
            if let Some(invalid_output) = effect.invalid_output {
                result.status = "failed".to_string();
                result.invalid_output = Some(invalid_output);
                return;
            }
            result.additional_context = effect.additional_context;
            if let Some(reason) = effect.stop_reason {
                result.status = "stopped".to_string();
                result.decision = Some("stop".to_string());
                result.reason = Some(reason);
            } else if let Some(reason) = effect.block_reason {
                result.status = "blocked".to_string();
                result.decision = Some("block".to_string());
                result.reason = Some(reason);
            }
        }
        None if looks_like_json(&result.stdout) => {
            result.status = "failed".to_string();
            result.invalid_output =
                Some("hook returned invalid user prompt submit JSON output".to_string());
        }
        None => {
            result.additional_context = trimmed_non_empty(&result.stdout);
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
struct PreToolUseOutputEffect {
    block_reason: Option<String>,
    additional_context: Option<String>,
    updated_input: Option<JsonValue>,
    invalid_output: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
struct PostToolUseOutputEffect {
    block_reason: Option<String>,
    stop_reason: Option<String>,
    additional_context: Option<String>,
    invalid_output: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
struct StopOutputEffect {
    block_reason: Option<String>,
    stop_reason: Option<String>,
    additional_context: Option<String>,
    invalid_output: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
struct UserPromptSubmitOutputEffect {
    block_reason: Option<String>,
    stop_reason: Option<String>,
    additional_context: Option<String>,
    invalid_output: Option<String>,
}

fn parse_pre_tool_use_output(stdout: &str) -> Option<PreToolUseOutputEffect> {
    let value: JsonValue = serde_json::from_str(stdout.trim()).ok()?;
    let object = value.as_object()?;
    let hook_specific = object
        .get("hookSpecificOutput")
        .or_else(|| object.get("hook_specific_output"));
    let hook_specific_object = hook_specific.and_then(JsonValue::as_object);

    let universal_invalid = unsupported_pre_tool_use_universal(&value);
    let additional_context = hook_specific_object
        .and_then(|output| {
            output
                .get("additionalContext")
                .or_else(|| output.get("additional_context"))
                .and_then(JsonValue::as_str)
        })
        .and_then(trimmed_non_empty);

    if let Some(invalid_output) = universal_invalid {
        return Some(PreToolUseOutputEffect {
            block_reason: None,
            additional_context,
            updated_input: None,
            invalid_output: Some(invalid_output),
        });
    }

    let permission_decision = hook_specific_object
        .and_then(|output| {
            output
                .get("permissionDecision")
                .or_else(|| output.get("permission_decision"))
                .and_then(JsonValue::as_str)
        })
        .map(|decision| decision.to_ascii_lowercase());
    let permission_reason = hook_specific_object
        .and_then(|output| {
            output
                .get("permissionDecisionReason")
                .or_else(|| output.get("permission_decision_reason"))
                .and_then(JsonValue::as_str)
        })
        .and_then(trimmed_non_empty);
    let updated_input = hook_specific_object.and_then(|output| {
        output
            .get("updatedInput")
            .or_else(|| output.get("updated_input"))
            .cloned()
    });
    let uses_hook_specific_decision =
        permission_decision.is_some() || permission_reason.is_some() || updated_input.is_some();

    if uses_hook_specific_decision {
        return Some(match permission_decision.as_deref() {
            Some("deny") => {
                if let Some(reason) = permission_reason {
                    PreToolUseOutputEffect {
                        block_reason: Some(reason),
                        additional_context,
                        updated_input: None,
                        invalid_output: None,
                    }
                } else {
                    PreToolUseOutputEffect {
                        block_reason: None,
                        additional_context,
                        updated_input: None,
                        invalid_output: Some(
                            "PreToolUse hook returned permissionDecision:deny without a non-empty permissionDecisionReason"
                                .to_string(),
                        ),
                    }
                }
            }
            Some("allow") => {
                if let Some(updated_input) = updated_input {
                    PreToolUseOutputEffect {
                        block_reason: None,
                        additional_context,
                        updated_input: Some(updated_input),
                        invalid_output: None,
                    }
                } else {
                    PreToolUseOutputEffect {
                        block_reason: None,
                        additional_context,
                        updated_input: None,
                        invalid_output: Some(
                            "PreToolUse hook returned unsupported permissionDecision:allow"
                                .to_string(),
                        ),
                    }
                }
            }
            Some("ask") => PreToolUseOutputEffect {
                block_reason: None,
                additional_context,
                updated_input: None,
                invalid_output: Some(
                    "PreToolUse hook returned unsupported permissionDecision:ask".to_string(),
                ),
            },
            Some(other) => PreToolUseOutputEffect {
                block_reason: None,
                additional_context,
                updated_input: None,
                invalid_output: Some(format!(
                    "PreToolUse hook returned unsupported permissionDecision:{other}"
                )),
            },
            None if updated_input.is_some() => PreToolUseOutputEffect {
                block_reason: None,
                additional_context,
                updated_input: None,
                invalid_output: Some(
                    "PreToolUse hook returned updatedInput without permissionDecision:allow"
                        .to_string(),
                ),
            },
            None => PreToolUseOutputEffect {
                block_reason: None,
                additional_context,
                updated_input: None,
                invalid_output: Some(
                    "PreToolUse hook returned permissionDecisionReason without permissionDecision"
                        .to_string(),
                ),
            },
        });
    }

    let decision = object
        .get("decision")
        .and_then(JsonValue::as_str)
        .map(|decision| decision.to_ascii_lowercase());
    let reason = object
        .get("reason")
        .and_then(JsonValue::as_str)
        .and_then(trimmed_non_empty);

    match decision.as_deref() {
        Some("block") => Some(PreToolUseOutputEffect {
            block_reason: reason.clone(),
            additional_context,
            updated_input: None,
            invalid_output: if reason.is_some() {
                None
            } else {
                Some(
                    "PreToolUse hook returned decision:block without a non-empty reason"
                        .to_string(),
                )
            },
        }),
        Some("approve") => Some(PreToolUseOutputEffect {
            block_reason: None,
            additional_context,
            updated_input: None,
            invalid_output: Some(
                "PreToolUse hook returned unsupported decision:approve".to_string(),
            ),
        }),
        Some(other) => Some(PreToolUseOutputEffect {
            block_reason: None,
            additional_context,
            updated_input: None,
            invalid_output: Some(format!(
                "PreToolUse hook returned unsupported decision:{other}"
            )),
        }),
        None if object.contains_key("reason") => Some(PreToolUseOutputEffect {
            block_reason: None,
            additional_context,
            updated_input: None,
            invalid_output: Some("PreToolUse hook returned reason without decision".to_string()),
        }),
        None => Some(PreToolUseOutputEffect {
            block_reason: None,
            additional_context,
            updated_input: None,
            invalid_output: None,
        }),
    }
}

fn parse_post_tool_use_output(stdout: &str) -> Option<PostToolUseOutputEffect> {
    let value: JsonValue = serde_json::from_str(stdout.trim()).ok()?;
    let object = value.as_object()?;
    let hook_specific = object
        .get("hookSpecificOutput")
        .or_else(|| object.get("hook_specific_output"));
    let hook_specific_object = hook_specific.and_then(JsonValue::as_object);
    let additional_context = hook_specific_object
        .and_then(|output| {
            output
                .get("additionalContext")
                .or_else(|| output.get("additional_context"))
                .and_then(JsonValue::as_str)
        })
        .and_then(trimmed_non_empty);

    if let Some(invalid_output) = unsupported_post_tool_use_universal(&value) {
        return Some(PostToolUseOutputEffect {
            block_reason: None,
            stop_reason: None,
            additional_context,
            invalid_output: Some(invalid_output),
        });
    }

    if hook_specific_object.is_some_and(|output| {
        output
            .get("updatedMCPToolOutput")
            .or_else(|| output.get("updated_mcp_tool_output"))
            .is_some()
    }) {
        return Some(PostToolUseOutputEffect {
            block_reason: None,
            stop_reason: None,
            additional_context,
            invalid_output: Some(
                "PostToolUse hook returned unsupported updatedMCPToolOutput".to_string(),
            ),
        });
    }

    let continue_processing = object
        .get("continue")
        .or_else(|| object.get("continueProcessing"))
        .or_else(|| object.get("continue_processing"))
        .and_then(JsonValue::as_bool)
        .unwrap_or(true);
    if !continue_processing {
        let stop_reason = object
            .get("stopReason")
            .or_else(|| object.get("stop_reason"))
            .and_then(JsonValue::as_str)
            .and_then(trimmed_non_empty)
            .or_else(|| {
                object
                    .get("reason")
                    .and_then(JsonValue::as_str)
                    .and_then(trimmed_non_empty)
            })
            .unwrap_or_else(|| "PostToolUse hook stopped execution".to_string());
        return Some(PostToolUseOutputEffect {
            block_reason: None,
            stop_reason: Some(stop_reason),
            additional_context,
            invalid_output: None,
        });
    }

    let decision = object
        .get("decision")
        .and_then(JsonValue::as_str)
        .map(|decision| decision.to_ascii_lowercase());
    let reason = object
        .get("reason")
        .and_then(JsonValue::as_str)
        .and_then(trimmed_non_empty);

    match decision.as_deref() {
        Some("block") => Some(PostToolUseOutputEffect {
            block_reason: reason.clone(),
            stop_reason: None,
            additional_context,
            invalid_output: if reason.is_some() {
                None
            } else {
                Some(
                    "PostToolUse hook returned decision:block without a non-empty reason"
                        .to_string(),
                )
            },
        }),
        Some(other) => Some(PostToolUseOutputEffect {
            block_reason: None,
            stop_reason: None,
            additional_context,
            invalid_output: Some(format!(
                "PostToolUse hook returned unsupported decision:{other}"
            )),
        }),
        None if object.contains_key("reason") => Some(PostToolUseOutputEffect {
            block_reason: None,
            stop_reason: None,
            additional_context,
            invalid_output: Some("PostToolUse hook returned reason without decision".to_string()),
        }),
        None => Some(PostToolUseOutputEffect {
            block_reason: None,
            stop_reason: None,
            additional_context,
            invalid_output: None,
        }),
    }
}

fn parse_user_prompt_submit_output(stdout: &str) -> Option<UserPromptSubmitOutputEffect> {
    let value: JsonValue = serde_json::from_str(stdout.trim()).ok()?;
    let object = value.as_object()?;
    let hook_specific = object
        .get("hookSpecificOutput")
        .or_else(|| object.get("hook_specific_output"));
    let hook_specific_object = hook_specific.and_then(JsonValue::as_object);
    let additional_context = hook_specific_object
        .and_then(|output| {
            output
                .get("additionalContext")
                .or_else(|| output.get("additional_context"))
                .and_then(JsonValue::as_str)
        })
        .and_then(trimmed_non_empty);

    let continue_processing = object
        .get("continue")
        .or_else(|| object.get("continueProcessing"))
        .or_else(|| object.get("continue_processing"))
        .and_then(JsonValue::as_bool)
        .unwrap_or(true);
    if !continue_processing {
        let stop_reason = object
            .get("stopReason")
            .or_else(|| object.get("stop_reason"))
            .and_then(JsonValue::as_str)
            .and_then(trimmed_non_empty)
            .or_else(|| {
                object
                    .get("reason")
                    .and_then(JsonValue::as_str)
                    .and_then(trimmed_non_empty)
            })
            .unwrap_or_else(|| "UserPromptSubmit hook stopped prompt processing".to_string());
        return Some(UserPromptSubmitOutputEffect {
            block_reason: None,
            stop_reason: Some(stop_reason),
            additional_context,
            invalid_output: None,
        });
    }

    let decision = object
        .get("decision")
        .and_then(JsonValue::as_str)
        .map(|decision| decision.to_ascii_lowercase());
    let reason = object
        .get("reason")
        .and_then(JsonValue::as_str)
        .and_then(trimmed_non_empty);

    match decision.as_deref() {
        Some("block") => Some(UserPromptSubmitOutputEffect {
            block_reason: reason.clone(),
            stop_reason: None,
            additional_context,
            invalid_output: if reason.is_some() {
                None
            } else {
                Some(
                    "UserPromptSubmit hook returned decision:block without a non-empty reason"
                        .to_string(),
                )
            },
        }),
        Some(other) => Some(UserPromptSubmitOutputEffect {
            block_reason: None,
            stop_reason: None,
            additional_context,
            invalid_output: Some(format!(
                "UserPromptSubmit hook returned unsupported decision:{other}"
            )),
        }),
        None if object.contains_key("reason") => Some(UserPromptSubmitOutputEffect {
            block_reason: None,
            stop_reason: None,
            additional_context,
            invalid_output: Some(
                "UserPromptSubmit hook returned reason without decision".to_string(),
            ),
        }),
        None => Some(UserPromptSubmitOutputEffect {
            block_reason: None,
            stop_reason: None,
            additional_context,
            invalid_output: None,
        }),
    }
}

fn parse_stop_output(stdout: &str, event_label: &str) -> Option<StopOutputEffect> {
    let value: JsonValue = serde_json::from_str(stdout.trim()).ok()?;
    let object = value.as_object()?;
    let additional_context = object
        .get("systemMessage")
        .or_else(|| object.get("system_message"))
        .and_then(JsonValue::as_str)
        .and_then(trimmed_non_empty);

    let continue_processing = object
        .get("continue")
        .or_else(|| object.get("continueProcessing"))
        .or_else(|| object.get("continue_processing"))
        .and_then(JsonValue::as_bool)
        .unwrap_or(true);
    if !continue_processing {
        let stop_reason = object
            .get("stopReason")
            .or_else(|| object.get("stop_reason"))
            .and_then(JsonValue::as_str)
            .and_then(trimmed_non_empty)
            .or_else(|| {
                object
                    .get("reason")
                    .and_then(JsonValue::as_str)
                    .and_then(trimmed_non_empty)
            })
            .unwrap_or_else(|| format!("{event_label} hook stopped execution"));
        return Some(StopOutputEffect {
            block_reason: None,
            stop_reason: Some(stop_reason),
            additional_context,
            invalid_output: None,
        });
    }

    let decision = object
        .get("decision")
        .and_then(JsonValue::as_str)
        .map(|decision| decision.to_ascii_lowercase());
    let reason = object
        .get("reason")
        .and_then(JsonValue::as_str)
        .and_then(trimmed_non_empty);

    match decision.as_deref() {
        Some("block") => Some(StopOutputEffect {
            block_reason: reason.clone(),
            stop_reason: None,
            additional_context,
            invalid_output: if reason.is_some() {
                None
            } else {
                Some(format!(
                    "{event_label} hook returned decision:block without a non-empty reason"
                ))
            },
        }),
        Some(other) => Some(StopOutputEffect {
            block_reason: None,
            stop_reason: None,
            additional_context,
            invalid_output: Some(format!(
                "{event_label} hook returned unsupported decision:{other}"
            )),
        }),
        None if object.contains_key("reason") => Some(StopOutputEffect {
            block_reason: None,
            stop_reason: None,
            additional_context,
            invalid_output: Some(format!(
                "{event_label} hook returned reason without decision"
            )),
        }),
        None => Some(StopOutputEffect {
            block_reason: None,
            stop_reason: None,
            additional_context,
            invalid_output: None,
        }),
    }
}

fn unsupported_pre_tool_use_universal(value: &JsonValue) -> Option<String> {
    if value
        .get("continue")
        .or_else(|| value.get("continueProcessing"))
        .or_else(|| value.get("continue_processing"))
        .and_then(JsonValue::as_bool)
        == Some(false)
    {
        Some("PreToolUse hook returned unsupported continue:false".to_string())
    } else if value
        .get("stopReason")
        .or_else(|| value.get("stop_reason"))
        .is_some()
    {
        Some("PreToolUse hook returned unsupported stopReason".to_string())
    } else if value
        .get("suppressOutput")
        .or_else(|| value.get("suppress_output"))
        .and_then(JsonValue::as_bool)
        == Some(true)
    {
        Some("PreToolUse hook returned unsupported suppressOutput".to_string())
    } else {
        None
    }
}

fn unsupported_post_tool_use_universal(value: &JsonValue) -> Option<String> {
    if value
        .get("suppressOutput")
        .or_else(|| value.get("suppress_output"))
        .and_then(JsonValue::as_bool)
        == Some(true)
    {
        Some("PostToolUse hook returned unsupported suppressOutput".to_string())
    } else {
        None
    }
}

fn looks_like_json(stdout: &str) -> bool {
    let trimmed = stdout.trim_start();
    trimmed.starts_with('{') || trimmed.starts_with('[')
}

fn trimmed_non_empty(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn shell_command(command: &str) -> (&'static str, Vec<String>) {
    if cfg!(target_os = "windows") {
        ("cmd", vec!["/C".to_string(), command.to_string()])
    } else {
        ("sh", vec!["-c".to_string(), command.to_string()])
    }
}

fn truncate_for_event(value: &str) -> String {
    const MAX_CHARS: usize = 4_000;
    if value.chars().count() <= MAX_CHARS {
        return value.to_string();
    }

    let prefix: String = value.chars().take(2_000).collect();
    let suffix: String = value
        .chars()
        .rev()
        .take(1_500)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!(
        "{prefix}\n\n... [{} chars truncated] ...\n\n{suffix}",
        value.len()
    )
}

fn find_plugin_manifest_path(plugin_root: &Path) -> Option<PathBuf> {
    [".codex-plugin/plugin.json", ".claude-plugin/plugin.json"]
        .iter()
        .map(|relative| plugin_root.join(relative))
        .find(|path| path.is_file())
}

fn resolve_manifest_relative_path(plugin_root: &Path, raw_path: &str) -> Result<PathBuf, String> {
    let Some(relative_path) = raw_path.strip_prefix("./") else {
        return Err("path must start with ./".to_string());
    };

    if relative_path.is_empty() {
        return Err("path must not be empty".to_string());
    }

    let mut normalized = PathBuf::new();
    for component in Path::new(relative_path).components() {
        match component {
            Component::Normal(part) => normalized.push(part),
            Component::ParentDir => return Err("path must not contain ..".to_string()),
            _ => return Err("path must stay within plugin root".to_string()),
        }
    }

    Ok(plugin_root.join(normalized))
}

fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unique_temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "cn-codex-hook-test-{name}-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    fn write_file(path: &Path, content: &str) {
        std::fs::create_dir_all(path.parent().expect("file parent")).expect("create parent");
        std::fs::write(path, content).expect("write file");
    }

    #[test]
    fn config_hooks_support_simple_and_official_shapes() {
        let root = unique_temp_dir("config-shapes");
        let config: ConfigToml = toml::from_str(
            r#"
            [hooks.on-agent-start]
            command = "echo start"

            [[hooks.PreToolUse]]
            matcher = "Bash"
            hooks = [{ type = "command", command = "echo pre" }]
            "#,
        )
        .unwrap();

        let hooks = list_hooks(&config, &root);

        assert_eq!(hooks.len(), 2);
        assert!(hooks.iter().any(|hook| {
            hook.event == HOOK_AGENT_START && hook.command == "echo start" && hook.enabled
        }));
        assert!(hooks.iter().any(|hook| {
            hook.event == HOOK_COMMAND_EXEC
                && hook.command == "echo pre"
                && hook.matcher.as_deref() == Some("Bash")
        }));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn hooks_json_supports_codex_hooks_file_shape() {
        let root = unique_temp_dir("hooks-json");
        write_file(
            &root.join("hooks.json"),
            r#"{
  "hooks": {
    "SessionStart": [
      {
        "matcher": "startup",
        "hooks": [{ "type": "command", "command": "echo start" }]
      }
    ],
    "UserPromptSubmit": [{ "hooks": [{ "type": "command", "command": "echo prompt" }] }],
    "Stop": [{ "hooks": [{ "type": "command", "command": "echo stop" }] }],
    "SubagentStop": [{ "matcher": "tester", "hooks": [{ "type": "command", "command": "echo child-stop" }] }]
  }
}"#,
        );

        let hooks = list_hooks(&ConfigToml::default(), &root);

        assert!(hooks.iter().any(|hook| hook.event == HOOK_AGENT_START));
        assert!(
            hooks
                .iter()
                .any(|hook| hook.event == HOOK_USER_PROMPT_SUBMIT)
        );
        assert!(hooks.iter().any(|hook| hook.event == HOOK_AGENT_END));
        assert!(hooks.iter().any(|hook| {
            hook.event == HOOK_SUBAGENT_STOP && hook.matcher.as_deref() == Some("tester")
        }));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn post_tool_use_event_is_distinct_from_file_change() {
        assert_eq!(
            normalize_hook_event("PostToolUse").as_deref(),
            Some(HOOK_POST_TOOL_USE)
        );
        assert_eq!(
            normalize_hook_event("on-file-change").as_deref(),
            Some(HOOK_FILE_CHANGE)
        );
    }

    #[test]
    fn plugin_hooks_discover_default_and_manifest_path() {
        let root = unique_temp_dir("plugins");
        let default_plugin = root.join("plugins/default");
        write_file(
            &default_plugin.join(".codex-plugin/plugin.json"),
            r#"{ "name": "default-plugin" }"#,
        );
        write_file(
            &default_plugin.join("hooks/hooks.json"),
            r#"{"hooks":{"SessionStart":[{"hooks":[{"type":"command","command":"echo default"}]}]}}"#,
        );

        let custom_plugin = root.join("plugins/custom");
        write_file(
            &custom_plugin.join(".codex-plugin/plugin.json"),
            r#"{ "name": "custom-plugin", "hooks": "./hooks/one.json" }"#,
        );
        write_file(
            &custom_plugin.join("hooks/hooks.json"),
            r#"{"hooks":{"SessionStart":[{"hooks":[{"type":"command","command":"echo ignored"}]}]}}"#,
        );
        write_file(
            &custom_plugin.join("hooks/one.json"),
            r#"{"hooks":{"Stop":[{"hooks":[{"type":"command","command":"echo custom"}]}]}}"#,
        );

        let hooks = list_hooks(&ConfigToml::default(), &root);

        assert!(hooks.iter().any(|hook| hook.command == "echo default"));
        assert!(hooks.iter().any(|hook| hook.command == "echo custom"));
        assert!(!hooks.iter().any(|hook| hook.command == "echo ignored"));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn command_hooks_match_bash_alias_for_shell_tool() {
        let hook = HookDefinition {
            id: "id".to_string(),
            event: HOOK_COMMAND_EXEC.to_string(),
            source_type: "config".to_string(),
            source_name: "config".to_string(),
            source_path: "config".to_string(),
            command: "echo hi".to_string(),
            cwd: None,
            env: HashMap::new(),
            timeout_ms: DEFAULT_HOOK_TIMEOUT_MS,
            disabled: false,
            matcher: Some("Bash".to_string()),
            plugin_root: None,
        };

        assert!(hook_matches_context(
            &hook,
            &serde_json::json!({ "toolName": "shell" })
        ));
        assert!(!hook_matches_context(
            &hook,
            &serde_json::json!({ "toolName": "read_file" })
        ));
    }

    #[test]
    fn subagent_stop_hooks_match_agent_type() {
        let hook = HookDefinition {
            id: "id".to_string(),
            event: HOOK_SUBAGENT_STOP.to_string(),
            source_type: "config".to_string(),
            source_name: "config".to_string(),
            source_path: "config".to_string(),
            command: "echo hi".to_string(),
            cwd: None,
            env: HashMap::new(),
            timeout_ms: DEFAULT_HOOK_TIMEOUT_MS,
            disabled: false,
            matcher: Some("tester".to_string()),
            plugin_root: None,
        };

        assert!(hook_matches_context(
            &hook,
            &serde_json::json!({ "agentType": "tester" })
        ));
        assert!(!hook_matches_context(
            &hook,
            &serde_json::json!({ "agentType": "reviewer" })
        ));
    }

    #[test]
    fn pre_tool_use_permission_deny_blocks_tool_execution() {
        let mut result = hook_run_result(
            Some(0),
            r#"{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"deny","permissionDecisionReason":"blocked by policy","additionalContext":"remember why"}}"#,
            "",
        );

        apply_hook_output_effects(&mut result);

        assert_eq!(result.status, "blocked");
        assert_eq!(result.decision.as_deref(), Some("block"));
        assert_eq!(result.reason.as_deref(), Some("blocked by policy"));
        assert_eq!(result.additional_context.as_deref(), Some("remember why"));
        assert!(result.updated_input.is_none());
    }

    #[test]
    fn pre_tool_use_permission_allow_can_update_input() {
        let mut result = hook_run_result(
            Some(0),
            r#"{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"allow","updatedInput":{"command":"echo rewritten"}}}"#,
            "",
        );

        apply_hook_output_effects(&mut result);

        assert_eq!(result.status, "success");
        assert_eq!(result.decision.as_deref(), Some("allow"));
        assert_eq!(
            result.updated_input,
            Some(serde_json::json!({ "command": "echo rewritten" }))
        );
    }

    #[test]
    fn pre_tool_use_legacy_block_and_exit_code_two_block() {
        let mut legacy = hook_run_result(
            Some(0),
            r#"{"decision":"block","reason":"legacy block"}"#,
            "",
        );
        apply_hook_output_effects(&mut legacy);
        assert_eq!(legacy.status, "blocked");
        assert_eq!(legacy.reason.as_deref(), Some("legacy block"));

        let mut exit_code_two = hook_run_result(Some(2), "", "stderr block reason\n");
        apply_hook_output_effects(&mut exit_code_two);
        assert_eq!(exit_code_two.status, "blocked");
        assert_eq!(exit_code_two.reason.as_deref(), Some("stderr block reason"));
    }

    #[test]
    fn pre_tool_use_invalid_json_like_output_fails_open() {
        let mut result = hook_run_result(Some(0), "{\"decision\":\n", "");

        apply_hook_output_effects(&mut result);

        assert_eq!(result.status, "failed");
        assert_eq!(
            result.invalid_output.as_deref(),
            Some("hook returned invalid pre-tool-use JSON output")
        );
        assert!(result.decision.is_none());
    }

    #[test]
    fn user_prompt_submit_context_and_block_are_actionable() {
        let mut context = user_prompt_hook_run_result(
            Some(0),
            r#"{"hookSpecificOutput":{"hookEventName":"UserPromptSubmit","additionalContext":"remember this prompt"}}"#,
            "",
        );
        apply_hook_output_effects(&mut context);
        assert_eq!(context.status, "success");
        assert_eq!(
            context.additional_context.as_deref(),
            Some("remember this prompt")
        );

        let mut plain_context = user_prompt_hook_run_result(Some(0), "plain context", "");
        apply_hook_output_effects(&mut plain_context);
        assert_eq!(
            plain_context.additional_context.as_deref(),
            Some("plain context")
        );

        let mut blocked = user_prompt_hook_run_result(
            Some(0),
            r#"{"decision":"block","reason":"blocked by policy"}"#,
            "",
        );
        apply_hook_output_effects(&mut blocked);
        assert_eq!(blocked.status, "blocked");
        assert_eq!(blocked.decision.as_deref(), Some("block"));
        assert_eq!(blocked.reason.as_deref(), Some("blocked by policy"));
    }

    #[test]
    fn user_prompt_submit_stop_and_invalid_output_are_actionable() {
        let mut stopped = user_prompt_hook_run_result(
            Some(0),
            r#"{"continue":false,"stopReason":"pause prompt"}"#,
            "",
        );
        apply_hook_output_effects(&mut stopped);
        assert_eq!(stopped.status, "stopped");
        assert_eq!(stopped.decision.as_deref(), Some("stop"));
        assert_eq!(stopped.reason.as_deref(), Some("pause prompt"));

        let mut exit_code_two = user_prompt_hook_run_result(Some(2), "", "stderr block");
        apply_hook_output_effects(&mut exit_code_two);
        assert_eq!(exit_code_two.status, "blocked");
        assert_eq!(exit_code_two.decision.as_deref(), Some("block"));
        assert_eq!(exit_code_two.reason.as_deref(), Some("stderr block"));

        let mut missing_reason =
            user_prompt_hook_run_result(Some(0), r#"{"decision":"block"}"#, "");
        apply_hook_output_effects(&mut missing_reason);
        assert_eq!(missing_reason.status, "failed");
        assert_eq!(
            missing_reason.invalid_output.as_deref(),
            Some("UserPromptSubmit hook returned decision:block without a non-empty reason")
        );

        let mut invalid_json = user_prompt_hook_run_result(Some(0), "{\"decision\":\n", "");
        apply_hook_output_effects(&mut invalid_json);
        assert_eq!(invalid_json.status, "failed");
        assert_eq!(
            invalid_json.invalid_output.as_deref(),
            Some("hook returned invalid user prompt submit JSON output")
        );
    }

    #[test]
    fn post_tool_use_block_and_context_surface_feedback() {
        let mut result = post_hook_run_result(
            Some(0),
            r#"{"decision":"block","reason":"review this output","hookSpecificOutput":{"hookEventName":"PostToolUse","additionalContext":"remember output note"}}"#,
            "",
        );

        apply_hook_output_effects(&mut result);

        assert_eq!(result.status, "blocked");
        assert_eq!(result.decision.as_deref(), Some("block"));
        assert_eq!(result.reason.as_deref(), Some("review this output"));
        assert_eq!(
            result.additional_context.as_deref(),
            Some("remember output note")
        );
        assert_eq!(
            hook_feedback_for_model(&[result]),
            vec![
                "Additional context from hook `echo hook`: remember output note".to_string(),
                "Feedback from hook `echo hook`: review this output".to_string(),
            ]
        );
    }

    #[test]
    fn post_tool_use_continue_false_and_exit_two_surface_feedback() {
        let mut stopped = post_hook_run_result(
            Some(0),
            r#"{"continue":false,"stopReason":"stop after this tool"}"#,
            "",
        );
        apply_hook_output_effects(&mut stopped);
        assert_eq!(stopped.status, "stopped");
        assert_eq!(stopped.decision.as_deref(), Some("stop"));
        assert_eq!(stopped.reason.as_deref(), Some("stop after this tool"));

        let mut feedback = post_hook_run_result(Some(2), "", "stderr feedback");
        apply_hook_output_effects(&mut feedback);
        assert_eq!(feedback.status, "success");
        assert_eq!(feedback.decision.as_deref(), Some("feedback"));
        assert_eq!(feedback.reason.as_deref(), Some("stderr feedback"));
    }

    #[test]
    fn post_tool_use_invalid_or_unsupported_output_fails_open() {
        let mut unsupported = post_hook_run_result(
            Some(0),
            r#"{"hookSpecificOutput":{"hookEventName":"PostToolUse","updatedMCPToolOutput":{"ok":true}}}"#,
            "",
        );
        apply_hook_output_effects(&mut unsupported);
        assert_eq!(unsupported.status, "failed");
        assert_eq!(
            unsupported.invalid_output.as_deref(),
            Some("PostToolUse hook returned unsupported updatedMCPToolOutput")
        );

        let mut invalid = post_hook_run_result(Some(0), "{\"decision\":\n", "");
        apply_hook_output_effects(&mut invalid);
        assert_eq!(invalid.status, "failed");
        assert_eq!(
            invalid.invalid_output.as_deref(),
            Some("hook returned invalid post-tool-use JSON output")
        );
    }

    #[test]
    fn stop_hook_block_stop_and_exit_two_are_actionable() {
        let mut blocked = stop_hook_run_result(
            Some(0),
            r#"{"decision":"block","reason":"run the missing test","systemMessage":"review gate"}"#,
            "",
        );
        apply_hook_output_effects(&mut blocked);
        assert_eq!(blocked.status, "blocked");
        assert_eq!(blocked.decision.as_deref(), Some("block"));
        assert_eq!(blocked.reason.as_deref(), Some("run the missing test"));
        assert_eq!(blocked.additional_context.as_deref(), Some("review gate"));

        let mut stopped =
            stop_hook_run_result(Some(0), r#"{"continue":false,"stopReason":"all done"}"#, "");
        apply_hook_output_effects(&mut stopped);
        assert_eq!(stopped.status, "stopped");
        assert_eq!(stopped.decision.as_deref(), Some("stop"));
        assert_eq!(stopped.reason.as_deref(), Some("all done"));

        let mut exit_code_two = stop_hook_run_result(Some(2), "", "continue with lint");
        apply_hook_output_effects(&mut exit_code_two);
        assert_eq!(exit_code_two.status, "blocked");
        assert_eq!(exit_code_two.decision.as_deref(), Some("block"));
        assert_eq!(exit_code_two.reason.as_deref(), Some("continue with lint"));
    }

    #[test]
    fn stop_hook_invalid_output_fails_open() {
        let mut missing_reason = stop_hook_run_result(Some(0), r#"{"decision":"block"}"#, "");
        apply_hook_output_effects(&mut missing_reason);
        assert_eq!(missing_reason.status, "failed");
        assert_eq!(
            missing_reason.invalid_output.as_deref(),
            Some("Stop hook returned decision:block without a non-empty reason")
        );

        let mut invalid_json = stop_hook_run_result(Some(0), "{\"decision\":\n", "");
        apply_hook_output_effects(&mut invalid_json);
        assert_eq!(invalid_json.status, "failed");
        assert_eq!(
            invalid_json.invalid_output.as_deref(),
            Some("hook returned invalid stop hook JSON output")
        );

        let mut exit_code_two = stop_hook_run_result(Some(2), "", "  ");
        apply_hook_output_effects(&mut exit_code_two);
        assert_eq!(exit_code_two.status, "failed");
        assert_eq!(
            exit_code_two.invalid_output.as_deref(),
            Some("Stop hook exited with code 2 but did not write a continuation prompt to stderr")
        );
    }

    #[test]
    fn subagent_stop_output_uses_subagent_labels() {
        let mut missing_reason =
            subagent_stop_hook_run_result(Some(0), r#"{"decision":"block"}"#, "");
        apply_hook_output_effects(&mut missing_reason);
        assert_eq!(missing_reason.status, "failed");
        assert_eq!(
            missing_reason.invalid_output.as_deref(),
            Some("SubagentStop hook returned decision:block without a non-empty reason")
        );

        let mut exit_code_two = subagent_stop_hook_run_result(Some(2), "", "continue child");
        apply_hook_output_effects(&mut exit_code_two);
        assert_eq!(exit_code_two.status, "blocked");
        assert_eq!(exit_code_two.decision.as_deref(), Some("block"));
        assert_eq!(exit_code_two.reason.as_deref(), Some("continue child"));
    }

    fn hook_run_result(exit_code: Option<i32>, stdout: &str, stderr: &str) -> HookRunResult {
        HookRunResult {
            id: "run".to_string(),
            event: HOOK_COMMAND_EXEC.to_string(),
            source_type: "config".to_string(),
            source_name: "config".to_string(),
            command: "echo hook".to_string(),
            status: if exit_code == Some(0) {
                "success".to_string()
            } else {
                "failed".to_string()
            },
            exit_code,
            stdout: stdout.to_string(),
            stderr: stderr.to_string(),
            error: None,
            duration_ms: 0,
            decision: None,
            reason: None,
            additional_context: None,
            updated_input: None,
            invalid_output: None,
        }
    }

    fn post_hook_run_result(exit_code: Option<i32>, stdout: &str, stderr: &str) -> HookRunResult {
        HookRunResult {
            event: HOOK_POST_TOOL_USE.to_string(),
            ..hook_run_result(exit_code, stdout, stderr)
        }
    }

    fn user_prompt_hook_run_result(
        exit_code: Option<i32>,
        stdout: &str,
        stderr: &str,
    ) -> HookRunResult {
        HookRunResult {
            event: HOOK_USER_PROMPT_SUBMIT.to_string(),
            ..hook_run_result(exit_code, stdout, stderr)
        }
    }

    fn stop_hook_run_result(exit_code: Option<i32>, stdout: &str, stderr: &str) -> HookRunResult {
        HookRunResult {
            event: HOOK_AGENT_END.to_string(),
            ..hook_run_result(exit_code, stdout, stderr)
        }
    }

    fn subagent_stop_hook_run_result(
        exit_code: Option<i32>,
        stdout: &str,
        stderr: &str,
    ) -> HookRunResult {
        HookRunResult {
            event: HOOK_SUBAGENT_STOP.to_string(),
            ..hook_run_result(exit_code, stdout, stderr)
        }
    }
}
