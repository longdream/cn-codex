use super::*;

pub(crate) fn mcp_tool_namespace_and_name(full_name: &str) -> Option<(&str, &str)> {
    let rest = full_name.strip_prefix("mcp__")?;
    let split = rest.rfind("__")?;
    if split == 0 || split + 2 >= rest.len() {
        return None;
    }
    let namespace = &full_name[.."mcp__".len() + split];
    let local_name = &rest[split + 2..];
    Some((namespace, local_name))
}


pub(crate) fn coalesce_tool_search_loadable_tools(tools: Vec<serde_json::Value>) -> Vec<serde_json::Value> {
    let mut output = Vec::<serde_json::Value>::new();
    for tool in tools {
        let is_namespace =
            tool.get("type").and_then(serde_json::Value::as_str) == Some("namespace");
        if !is_namespace {
            output.push(tool);
            continue;
        }

        let name = tool
            .get("name")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_string();
        let incoming_tools = tool
            .get("tools")
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default();

        if let Some(existing) = output.iter_mut().find(|existing| {
            existing.get("type").and_then(serde_json::Value::as_str) == Some("namespace")
                && existing.get("name").and_then(serde_json::Value::as_str) == Some(name.as_str())
        }) {
            if let Some(existing_tools) = existing
                .get_mut("tools")
                .and_then(serde_json::Value::as_array_mut)
            {
                existing_tools.extend(incoming_tools);
            }
        } else {
            output.push(tool);
        }
    }
    output
}


pub(crate) fn mcp_direct_tool_spec(
    server_name: &str,
    tool_name: &str,
    tool: &serde_json::Value,
    alias: &str,
) -> serde_json::Value {
    let connector = mcp_connector_metadata(server_name, tool);
    let raw_description = tool
        .get("description")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let mut description_parts = Vec::new();
    if let Some(connector_name) = connector.connector_name.as_deref() {
        let connector_label = connector
            .connector_id
            .as_deref()
            .map(|connector_id| format!("{connector_name} ({connector_id})"))
            .unwrap_or_else(|| connector_name.to_string());
        description_parts.push(format!(
            "MCP app connector {connector_label} tool {server_name}:{tool_name}."
        ));
    } else {
        description_parts.push(format!("MCP tool {server_name}:{tool_name}."));
    }
    if let Some(namespace_description) = connector.namespace_description.as_deref() {
        description_parts.push(namespace_description.to_string());
    }
    if let Some(description) = raw_description {
        description_parts.push(description.to_string());
    }
    let description = description_parts.join(" ");
    let parameters = tool
        .get("inputSchema")
        .or_else(|| tool.get("input_schema"))
        .cloned()
        .filter(|value| value.is_object())
        .unwrap_or_else(default_mcp_input_schema);

    serde_json::json!({
        "type": "function",
        "function": {
            "name": alias,
            "description": description,
            "parameters": parameters
        }
    })
}


pub(crate) fn sorted_mcp_tool_specs(specs: &HashMap<String, serde_json::Value>) -> Vec<serde_json::Value> {
    let mut aliases = specs.keys().cloned().collect::<Vec<_>>();
    aliases.sort();
    aliases
        .into_iter()
        .filter_map(|alias| specs.get(&alias).cloned())
        .collect()
}


pub(crate) fn mcp_connector_metadata(server_name: &str, tool: &serde_json::Value) -> McpConnectorMetadata {
    if !trusted_codex_apps_server_name(server_name) {
        return McpConnectorMetadata::default();
    }

    McpConnectorMetadata {
        connector_id: mcp_tool_metadata_string(tool, &["connector_id", "connectorId"]),
        connector_name: mcp_tool_metadata_string(
            tool,
            &[
                "connector_name",
                "connectorName",
                "connector_display_name",
                "connectorDisplayName",
            ],
        ),
        namespace_description: mcp_tool_metadata_string(
            tool,
            &["connector_description", "connectorDescription"],
        ),
    }
}


pub(crate) fn trusted_codex_apps_server_name(server_name: &str) -> bool {
    matches!(server_name.trim(), "codex-apps" | "codex_apps")
}


pub(crate) fn mcp_tool_metadata_string(tool: &serde_json::Value, keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Some(value) = tool.get(*key).and_then(serde_json::Value::as_str) {
            let value = value.trim();
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }

    for meta_key in ["_meta", "meta"] {
        let Some(meta) = tool.get(meta_key).and_then(serde_json::Value::as_object) else {
            continue;
        };
        for key in keys {
            if let Some(value) = meta.get(*key).and_then(serde_json::Value::as_str) {
                let value = value.trim();
                if !value.is_empty() {
                    return Some(value.to_string());
                }
            }
        }
    }

    None
}


pub(crate) fn default_mcp_input_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {},
        "required": []
    })
}


pub(crate) fn mcp_direct_tool_name(
    server_name: &str,
    tool_name: &str,
    used_names: &mut BTreeSet<String>,
) -> String {
    let server_part = sanitize_tool_name_part(server_name, "server");
    let tool_part = sanitize_tool_name_part(tool_name, "tool");
    let base = format!("mcp__{server_part}__{tool_part}");
    let hash = stable_hash_hex(&format!("{server_name}\0{tool_name}"));
    let mut candidate = truncate_mcp_tool_name(&base, &hash);

    let mut counter = 2usize;
    while used_names.contains(&candidate) {
        let suffix = format!("_{counter}");
        let max_len = 64usize.saturating_sub(suffix.len());
        let prefix = candidate.chars().take(max_len).collect::<String>();
        candidate = format!("{prefix}{suffix}");
        counter += 1;
    }

    used_names.insert(candidate.clone());
    candidate
}


pub(crate) fn truncate_mcp_tool_name(base: &str, hash: &str) -> String {
    const MAX_TOOL_NAME_LEN: usize = 64;
    if base.len() <= MAX_TOOL_NAME_LEN {
        return base.to_string();
    }
    let suffix = format!("__{}", &hash[..8]);
    let max_prefix = MAX_TOOL_NAME_LEN.saturating_sub(suffix.len());
    let prefix = base.chars().take(max_prefix).collect::<String>();
    format!("{prefix}{suffix}")
}


pub(crate) fn sanitize_tool_name_part(value: &str, fallback: &str) -> String {
    let mut output = String::new();
    let mut last_was_underscore = false;
    for ch in value.chars() {
        let next = if ch.is_ascii_alphanumeric() { ch } else { '_' };
        if next == '_' {
            if last_was_underscore {
                continue;
            }
            last_was_underscore = true;
        } else {
            last_was_underscore = false;
        }
        output.push(next);
    }
    let trimmed = output.trim_matches('_');
    if trimmed.is_empty() {
        fallback.to_string()
    } else {
        trimmed.to_string()
    }
}


pub(crate) fn stable_hash_hex(value: &str) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in value.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}


pub(crate) async fn write_mcp_message(
    stdin: &mut tokio::process::ChildStdin,
    message: serde_json::Value,
) -> Result<(), String> {
    let mut line =
        serde_json::to_vec(&message).map_err(|e| format!("Failed to encode MCP message: {e}"))?;
    line.push(b'\n');
    stdin
        .write_all(&line)
        .await
        .map_err(|e| format!("Failed to write MCP message: {e}"))?;
    stdin
        .flush()
        .await
        .map_err(|e| format!("Failed to flush MCP message: {e}"))
}


pub(crate) async fn mcp_transport_error(
    server_name: &str,
    stderr: &Arc<Mutex<String>>,
    message: String,
) -> McpRequestError {
    let stderr_text = stderr.lock().await.clone();
    let message = if stderr_text.trim().is_empty() {
        message
    } else {
        format!(
            "{message}\n[stderr]\n{}",
            truncate_output(&stderr_text, 2000)
        )
    };
    McpRequestError::Transport(format!(
        "MCP server '{server_name}' transport failed: {message}"
    ))
}


pub(crate) fn collect_mcp_stderr(stderr: Option<tokio::process::ChildStderr>, buffer: Arc<Mutex<String>>) {
    tokio::spawn(async move {
        let Some(stderr) = stderr else {
            return;
        };
        let mut reader = BufReader::new(stderr);
        let mut line = String::new();
        loop {
            line.clear();
            let Ok(bytes) = reader.read_line(&mut line).await else {
                break;
            };
            if bytes == 0 {
                break;
            }
            let mut text = buffer.lock().await;
            text.push_str(&line);
            if text.len() > 8_000 {
                let keep_from = text.len().saturating_sub(8_000);
                let tail = text[keep_from..].to_string();
                *text = tail;
            }
        }
    });
}


pub(crate) async fn close_mcp_session(session: Arc<Mutex<McpSession>>) {
    let mut session = session.lock().await;
    let _ = session.child.kill().await;
    let _ = session.child.wait().await;
}


pub(crate) fn clear_mcp_sessions_async(sessions: Arc<Mutex<HashMap<String, Arc<Mutex<McpSession>>>>>) {
    let Ok(handle) = tokio::runtime::Handle::try_current() else {
        if let Ok(mut sessions) = sessions.try_lock() {
            sessions.clear();
        }
        return;
    };
    handle.spawn(async move {
        let sessions_to_close = {
            let mut sessions = sessions.lock().await;
            sessions
                .drain()
                .map(|(_, session)| session)
                .collect::<Vec<_>>()
        };
        for session in sessions_to_close {
            close_mcp_session(session).await;
        }
    });
}


pub(crate) fn clear_mcp_http_sessions_async(
    sessions: Arc<Mutex<HashMap<String, Arc<Mutex<McpHttpSession>>>>>,
) {
    let Ok(handle) = tokio::runtime::Handle::try_current() else {
        if let Ok(mut sessions) = sessions.try_lock() {
            sessions.clear();
        }
        return;
    };
    handle.spawn(async move {
        sessions.lock().await.clear();
    });
}


pub(crate) fn clear_mcp_sse_sessions_async(sessions: Arc<Mutex<HashMap<String, Arc<Mutex<McpSseSession>>>>>) {
    let Ok(handle) = tokio::runtime::Handle::try_current() else {
        if let Ok(mut sessions) = sessions.try_lock() {
            for (_, session) in sessions.drain() {
                if let Ok(session) = session.try_lock() {
                    session.worker.abort();
                }
            }
        }
        return;
    };
    handle.spawn(async move {
        let mut sessions = sessions.lock().await;
        for (_, session) in sessions.drain() {
            let session = session.lock().await;
            session.worker.abort();
        }
    });
}


pub(crate) fn parse_mcp_http_response_body(
    body: &str,
    content_type: &str,
) -> Result<Option<serde_json::Value>, String> {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    if content_type
        .to_ascii_lowercase()
        .contains("text/event-stream")
        || trimmed
            .lines()
            .any(|line| line.trim_start().starts_with("data:"))
    {
        return parse_mcp_sse_response_body(trimmed).map(Some);
    }

    serde_json::from_str::<serde_json::Value>(trimmed)
        .map(Some)
        .map_err(|error| format!("invalid JSON response: {error}"))
}


pub(crate) fn parse_mcp_sse_response_body(body: &str) -> Result<serde_json::Value, String> {
    let mut data_lines = Vec::new();
    for line in body.lines() {
        let line = line.trim_end();
        if line.is_empty() {
            if let Some(value) = parse_mcp_sse_data_lines(&data_lines)? {
                return Ok(value);
            }
            data_lines.clear();
            continue;
        }
        if let Some(data) = line.trim_start().strip_prefix("data:") {
            data_lines.push(data.trim_start().to_string());
        }
    }

    if let Some(value) = parse_mcp_sse_data_lines(&data_lines)? {
        return Ok(value);
    }
    Err("SSE response did not contain a JSON data event".to_string())
}


pub(crate) fn parse_mcp_sse_data_lines(lines: &[String]) -> Result<Option<serde_json::Value>, String> {
    if lines.is_empty() {
        return Ok(None);
    }
    let data = lines.join("\n");
    let trimmed = data.trim();
    if trimmed.is_empty() || trimmed == "[DONE]" {
        return Ok(None);
    }
    serde_json::from_str::<serde_json::Value>(trimmed)
        .map(Some)
        .map_err(|error| format!("invalid SSE JSON data: {error}"))
}


pub(crate) fn resolve_mcp_sse_endpoint_url(base_url: &str, endpoint: &str) -> Result<String, String> {
    let endpoint = endpoint.trim();
    if endpoint.is_empty() {
        return Err("SSE endpoint event was empty".to_string());
    }
    if endpoint.starts_with("http://") || endpoint.starts_with("https://") {
        return Ok(endpoint.to_string());
    }

    let base = reqwest::Url::parse(base_url)
        .map_err(|error| format!("invalid SSE base url '{base_url}': {error}"))?;
    if endpoint.starts_with('/') {
        let mut resolved = base;
        resolved.set_path(endpoint.split('?').next().unwrap_or(endpoint));
        if let Some((_, query)) = endpoint.split_once('?') {
            resolved.set_query(Some(query));
        } else {
            resolved.set_query(None);
        }
        return Ok(resolved.to_string());
    }

    base.join(endpoint)
        .map(|url| url.to_string())
        .map_err(|error| format!("invalid SSE endpoint '{endpoint}': {error}"))
}


pub(crate) fn response_matches_mcp_id(value: &serde_json::Value, expected_id: Option<i64>) -> bool {
    let Some(expected_id) = expected_id else {
        return false;
    };
    match value.get("id") {
        Some(serde_json::Value::Number(number)) => number
            .as_i64()
            .or_else(|| number.as_u64().map(|v| v as i64))
            .is_some_and(|id| id == expected_id),
        Some(serde_json::Value::String(text)) => {
            text.parse::<i64>().ok().is_some_and(|id| id == expected_id)
        }
        _ => false,
    }
}


pub(crate) async fn read_mcp_response<R>(
    reader: &mut BufReader<R>,
    expected_id: i64,
) -> Result<serde_json::Value, String>
where
    R: tokio::io::AsyncRead + Unpin,
{
    let mut line = String::new();
    loop {
        line.clear();
        let bytes = reader
            .read_line(&mut line)
            .await
            .map_err(|e| format!("Failed to read MCP response: {e}"))?;
        if bytes == 0 {
            return Err(format!(
                "MCP server closed stdout before response id {expected_id}"
            ));
        }

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) else {
            continue;
        };

        if value.get("id").and_then(serde_json::Value::as_i64) == Some(expected_id) {
            return Ok(value);
        }
    }
}


pub(crate) fn format_mcp_selection_result(
    results: Vec<(String, Result<serde_json::Value, String>)>,
    field: &str,
) -> (i32, String) {
    let mut has_error = false;
    let servers: Vec<_> = results
        .into_iter()
        .map(|(server, result)| match result {
            Ok(value) => serde_json::json!({
                "server": server,
                field: value,
            }),
            Err(error) => {
                has_error = true;
                serde_json::json!({
                    "server": server,
                    "error": error,
                })
            }
        })
        .collect();

    let text = serde_json::to_string_pretty(&serde_json::json!({ "servers": servers }))
        .unwrap_or_default();
    (if has_error { -1 } else { 0 }, text)
}


pub(crate) fn base_mcp_status_entry(server: &McpServerConfig) -> serde_json::Value {
    let mut env_keys = server.env.keys().cloned().collect::<Vec<_>>();
    env_keys.sort();
    let mut header_keys = server.headers.keys().cloned().collect::<Vec<_>>();
    header_keys.sort();
    serde_json::json!({
        "name": server.name,
        "transport": server.transport,
        "command": server.command,
        "args": server.args,
        "cwd": server.cwd,
        "url": server.url,
        "disabled": server.disabled,
        "envKeys": env_keys,
        "headerKeys": header_keys,
        "status": "configured",
    })
}


pub(crate) fn mcp_result_array_len(value: &serde_json::Value, field: &str) -> usize {
    value
        .get(field)
        .and_then(serde_json::Value::as_array)
        .map(Vec::len)
        .unwrap_or(0)
}


pub(crate) fn format_mcp_single_result(result: Result<serde_json::Value, String>) -> (i32, String) {
    match result {
        Ok(value) => (0, format_json_value(&value)),
        Err(error) => (-1, error),
    }
}


/// If MCP tool result is a MiniApp open_page envelope (or nested MCP content),
/// emit `miniapp-open-page` so the frontend can open the browser panel.
pub(crate) fn maybe_emit_miniapp_open_page(app_handle: &AppHandle, server: &str, text: &str) {
    let Some(payload) = extract_miniapp_open_page_payload(server, text) else {
        return;
    };
    let _ = app_handle.emit("miniapp-open-page", payload);
}


pub(crate) fn extract_miniapp_open_page_payload(server: &str, text: &str) -> Option<serde_json::Value> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    // format_mcp_single_result pretty-prints the raw MCP JSON-RPC result value.
    // Common shapes:
    // 1) { content:[{type:text,text:"{...envelope...}"}], structuredContent: {...} }
    // 2) direct envelope { ok, data:{url}, ui:{action:"open_page"} }
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) {
        if let Some(payload) = miniapp_open_page_from_value(server, &value) {
            return Some(payload);
        }
        // Walk content[].text JSON strings.
        if let Some(content) = value.get("content").and_then(|c| c.as_array()) {
            for item in content {
                if let Some(inner_text) = item.get("text").and_then(|t| t.as_str()) {
                    if let Ok(inner) = serde_json::from_str::<serde_json::Value>(inner_text) {
                        if let Some(payload) = miniapp_open_page_from_value(server, &inner) {
                            return Some(payload);
                        }
                    }
                }
            }
        }
        if let Some(structured) = value.get("structuredContent") {
            if let Some(payload) = miniapp_open_page_from_value(server, structured) {
                return Some(payload);
            }
        }
    }
    None
}


pub(crate) fn miniapp_open_page_from_value(
    server: &str,
    value: &serde_json::Value,
) -> Option<serde_json::Value> {
    let ui_action = value
        .pointer("/ui/action")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let url = value
        .pointer("/data/url")
        .and_then(|v| v.as_str())
        .or_else(|| value.get("url").and_then(|v| v.as_str()))
        .unwrap_or("")
        .trim();
    if url.is_empty() {
        return None;
    }
    // Prefer explicit open_page UI action; also accept when tool is open_page and URL looks local.
    let looks_local = url.starts_with("http://127.0.0.1") || url.starts_with("http://localhost");
    if ui_action != "open_page" && !looks_local {
        return None;
    }
    let page_id = value
        .pointer("/ui/pageId")
        .or_else(|| value.pointer("/data/pageId"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    Some(serde_json::json!({
        "server": server,
        "url": url,
        "pageId": page_id,
        "slug": server,
    }))
}


pub(crate) fn resolve_mcp_command_for_platform(command: &str) -> String {
    let trimmed = command.trim();
    #[cfg(target_os = "windows")]
    {
        if trimmed.eq_ignore_ascii_case("npx") {
            return "npx.cmd".to_string();
        }
        if trimmed.eq_ignore_ascii_case("npm") {
            return "npm.cmd".to_string();
        }
    }
    trimmed.to_string()
}


pub(crate) fn bundled_node_bin_dir(workspace_config_dir: &Path) -> Option<PathBuf> {
    let node_dir = workspace_config_dir.join("node");
    if node_dir.is_dir() {
        Some(node_dir)
    } else {
        None
    }
}


pub(crate) fn prepend_path_value(
    base_path: Option<std::ffi::OsString>,
    prepend_dir: &Path,
) -> std::ffi::OsString {
    let mut path_entries = vec![prepend_dir.to_path_buf()];
    if let Some(existing) = base_path {
        path_entries.extend(std::env::split_paths(&existing));
    }
    std::env::join_paths(path_entries).unwrap_or_else(|_| prepend_dir.as_os_str().to_os_string())
}


pub(crate) fn should_inject_bundled_node_runtime(program: &str) -> bool {
    let name = Path::new(program)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(program)
        .to_ascii_lowercase();
    matches!(
        name.as_str(),
        "node" | "node.exe" | "npm" | "npm.cmd" | "npx" | "npx.cmd"
    )
}


pub(crate) fn server_env_path_override(server: &McpServerConfig) -> Option<&str> {
    server.env.iter().find_map(|(key, value)| {
        if key.eq_ignore_ascii_case("PATH") {
            Some(value.as_str())
        } else {
            None
        }
    })
}


impl ToolExecutor {
    pub(crate) async fn exec_mcp_list_servers(
        &self,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
    ) -> AppResult<String> {
        self.emit_tool_start(app_handle, thread_id, call_id, "mcp_list_servers", ".");

        let mut servers: Vec<_> = self
            .mcp_servers
            .values()
            .map(|server| {
                let mut env_keys = server.env.keys().cloned().collect::<Vec<_>>();
                env_keys.sort();
                let mut header_keys = server.headers.keys().cloned().collect::<Vec<_>>();
                header_keys.sort();
                serde_json::json!({
                    "name": server.name,
                    "transport": server.transport,
                    "command": server.command,
                    "args": server.args,
                    "cwd": server.cwd,
                    "url": server.url,
                    "disabled": server.disabled,
                    "envKeys": env_keys,
                    "headerKeys": header_keys,
                })
            })
            .collect();
        servers.sort_by(|a, b| {
            a.get("name")
                .and_then(serde_json::Value::as_str)
                .cmp(&b.get("name").and_then(serde_json::Value::as_str))
        });

        let output = if servers.is_empty() {
            "No MCP servers configured in codey/config.toml".to_string()
        } else {
            serde_json::to_string_pretty(&serde_json::json!({ "servers": servers }))
                .unwrap_or_default()
        };
        self.emit_tool_end(
            app_handle,
            thread_id,
            call_id,
            "mcp_list_servers",
            0,
            &output,
        );
        Ok(output)
    }


    pub(crate) async fn exec_mcp_status(
        &self,
        arguments: &str,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
    ) -> AppResult<String> {
        #[derive(Deserialize, Default)]
        struct Args {
            #[serde(default)]
            server: Option<String>,
            #[serde(default)]
            probe: Option<bool>,
        }

        let args: Args = serde_json::from_str(arguments).unwrap_or_default();
        let display = args.server.as_deref().unwrap_or("all");
        self.emit_tool_start(app_handle, thread_id, call_id, "mcp_status", display);

        let mut servers = if let Some(server_name) = args.server.as_deref() {
            match self.mcp_servers.get(server_name) {
                Some(server) => vec![server.clone()],
                None => {
                    let msg = format!("MCP server not configured: {server_name}");
                    self.emit_tool_end(app_handle, thread_id, call_id, "mcp_status", -1, &msg);
                    return Ok(msg);
                }
            }
        } else {
            self.mcp_servers.values().cloned().collect::<Vec<_>>()
        };
        servers.sort_by(|a, b| a.name.cmp(&b.name));

        let probe = args.probe.unwrap_or(true);
        let mut entries = Vec::new();
        for server in servers {
            entries.push(self.mcp_status_entry(&server, probe).await);
        }
        let has_error = entries.iter().any(|entry| {
            entry
                .get("status")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|status| status == "error")
        });
        let output = serde_json::to_string_pretty(&serde_json::json!({
            "probe": probe,
            "servers": entries,
        }))
        .unwrap_or_default();
        self.emit_tool_end(
            app_handle,
            thread_id,
            call_id,
            "mcp_status",
            if has_error { -1 } else { 0 },
            &output,
        );
        Ok(output)
    }


    pub(crate) async fn mcp_status_entry(&self, server: &McpServerConfig, probe: bool) -> serde_json::Value {
        let mut entry = base_mcp_status_entry(server);
        entry["session"] = self.mcp_session_status(&server.name).await;
        if server.disabled {
            entry["status"] = serde_json::json!("disabled");
            return entry;
        }
        if !probe {
            entry["status"] = serde_json::json!("configured");
            return entry;
        }

        let probes = [
            ("tools", "tools/list", "tools"),
            ("resources", "resources/list", "resources"),
            (
                "resourceTemplates",
                "resources/templates/list",
                "resourceTemplates",
            ),
            ("prompts", "prompts/list", "prompts"),
        ];
        let mut probe_values = serde_json::Map::new();
        let mut has_error = false;
        for (label, method, field) in probes {
            let result = self
                .mcp_request(server, method, serde_json::json!({}))
                .await;
            match result {
                Ok(value) => {
                    probe_values.insert(
                        label.to_string(),
                        serde_json::json!({
                            "ok": true,
                            "count": mcp_result_array_len(&value, field),
                        }),
                    );
                }
                Err(error) => {
                    has_error = true;
                    probe_values.insert(
                        label.to_string(),
                        serde_json::json!({
                            "ok": false,
                            "error": error,
                        }),
                    );
                }
            }
        }
        entry["status"] = serde_json::json!(if has_error { "error" } else { "ok" });
        entry["probes"] = serde_json::Value::Object(probe_values);
        entry
    }


    pub(crate) async fn exec_mcp_list_tools(
        &self,
        arguments: &str,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
        turn_id: Option<&str>,
    ) -> AppResult<String> {
        #[derive(Deserialize, Default)]
        struct Args {
            #[serde(default)]
            server: Option<String>,
        }

        let args: Args = serde_json::from_str(arguments).unwrap_or_default();
        let display = args.server.as_deref().unwrap_or("all");
        self.emit_tool_start(app_handle, thread_id, call_id, "mcp_list_tools", display);
        let started = Instant::now();
        crate::agent::emit_and_broadcast(
            app_handle,
            "turn-loading",
            serde_json::json!({
                "threadId": thread_id,
                "turnId": turn_id,
                "callId": call_id,
                "kind": "mcp",
                "phase": "catalog",
                "status": "started",
                "server": display,
            }),
        );

        let output = self
            .mcp_request_for_selection(args.server.as_deref(), "tools/list", serde_json::json!({}))
            .await;
        let (exit_code, text) = format_mcp_selection_result(output, "tools");
        crate::agent::emit_and_broadcast(
            app_handle,
            "turn-loading",
            serde_json::json!({
                "threadId": thread_id,
                "turnId": turn_id,
                "callId": call_id,
                "kind": "mcp",
                "phase": "catalog",
                "status": if exit_code == 0 { "completed" } else { "failed" },
                "server": display,
                "durationMs": started.elapsed().as_millis() as u64,
                "error": if exit_code == 0 { serde_json::Value::Null } else { serde_json::json!(&text) },
            }),
        );
        self.emit_tool_end(
            app_handle,
            thread_id,
            call_id,
            "mcp_list_tools",
            exit_code,
            &text,
        );
        Ok(text)
    }


    pub(crate) async fn exec_mcp_call_tool(
        &self,
        arguments: &str,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
    ) -> AppResult<String> {
        #[derive(Deserialize)]
        struct Args {
            server: String,
            tool: String,
            #[serde(default)]
            arguments: Option<serde_json::Value>,
        }

        let args: Args = serde_json::from_str(arguments).map_err(|e| {
            crate::error::AppError::Custom(format!("Invalid mcp_call_tool args: {e}"))
        })?;
        let display = format!("{}:{}", args.server, args.tool);
        self.emit_tool_start(app_handle, thread_id, call_id, "mcp_call_tool", &display);

        let server = match self.mcp_server(&args.server) {
            Ok(server) => server,
            Err(msg) => {
                self.emit_tool_end(app_handle, thread_id, call_id, "mcp_call_tool", -1, &msg);
                return Ok(msg);
            }
        };
        let params = serde_json::json!({
            "name": args.tool,
            "arguments": args.arguments.unwrap_or_else(|| serde_json::json!({})),
        });
        let result = self.mcp_request(server, "tools/call", params).await;
        let (exit_code, text) = format_mcp_single_result(result);
        // MiniApp open_page bridge: host opens returned URL in browser panel.
        if exit_code == 0 && args.tool == "open_page" {
            maybe_emit_miniapp_open_page(app_handle, &args.server, &text);
        }
        self.emit_tool_end(
            app_handle,
            thread_id,
            call_id,
            "mcp_call_tool",
            exit_code,
            &text,
        );
        Ok(text)
    }


    pub(crate) async fn exec_mcp_direct_tool(
        &self,
        visible_tool_name: &str,
        server_name: &str,
        tool_name: &str,
        arguments: &str,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
    ) -> AppResult<String> {
        let display = format!("{server_name}:{tool_name}");
        self.emit_tool_start(app_handle, thread_id, call_id, visible_tool_name, &display);

        let server = match self.mcp_server(server_name) {
            Ok(server) => server,
            Err(msg) => {
                self.emit_tool_end(app_handle, thread_id, call_id, visible_tool_name, -1, &msg);
                return Ok(msg);
            }
        };

        let tool_arguments = if arguments.trim().is_empty() {
            serde_json::json!({})
        } else {
            match serde_json::from_str::<serde_json::Value>(arguments) {
                Ok(value) if value.is_object() => value,
                Ok(_) => {
                    let msg = format!(
                        "Invalid arguments for MCP tool {visible_tool_name}: expected a JSON object"
                    );
                    self.emit_tool_end(app_handle, thread_id, call_id, visible_tool_name, -1, &msg);
                    return Ok(msg);
                }
                Err(e) => {
                    let msg = format!("Invalid arguments for MCP tool {visible_tool_name}: {e}");
                    self.emit_tool_end(app_handle, thread_id, call_id, visible_tool_name, -1, &msg);
                    return Ok(msg);
                }
            }
        };

        let params = serde_json::json!({
            "name": tool_name,
            "arguments": tool_arguments,
        });
        let result = self.mcp_request(server, "tools/call", params).await;
        let (exit_code, text) = format_mcp_single_result(result);
        if exit_code == 0 && tool_name == "open_page" {
            maybe_emit_miniapp_open_page(app_handle, server_name, &text);
        }
        self.emit_tool_end(
            app_handle,
            thread_id,
            call_id,
            visible_tool_name,
            exit_code,
            &text,
        );
        Ok(text)
    }


    pub(crate) async fn exec_mcp_list_resources(
        &self,
        arguments: &str,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
    ) -> AppResult<String> {
        #[derive(Deserialize, Default)]
        struct Args {
            #[serde(default)]
            server: Option<String>,
        }

        let args: Args = serde_json::from_str(arguments).unwrap_or_default();
        let display = args.server.as_deref().unwrap_or("all");
        self.emit_tool_start(
            app_handle,
            thread_id,
            call_id,
            "mcp_list_resources",
            display,
        );

        let output = self
            .mcp_request_for_selection(
                args.server.as_deref(),
                "resources/list",
                serde_json::json!({}),
            )
            .await;
        let (exit_code, text) = format_mcp_selection_result(output, "resources");
        self.emit_tool_end(
            app_handle,
            thread_id,
            call_id,
            "mcp_list_resources",
            exit_code,
            &text,
        );
        Ok(text)
    }


    pub(crate) async fn exec_mcp_read_resource(
        &self,
        arguments: &str,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
    ) -> AppResult<String> {
        #[derive(Deserialize)]
        struct Args {
            server: String,
            uri: String,
        }

        let args: Args = serde_json::from_str(arguments).map_err(|e| {
            crate::error::AppError::Custom(format!("Invalid mcp_read_resource args: {e}"))
        })?;
        let display = format!("{}:{}", args.server, args.uri);
        self.emit_tool_start(
            app_handle,
            thread_id,
            call_id,
            "mcp_read_resource",
            &display,
        );

        let server = match self.mcp_server(&args.server) {
            Ok(server) => server,
            Err(msg) => {
                self.emit_tool_end(
                    app_handle,
                    thread_id,
                    call_id,
                    "mcp_read_resource",
                    -1,
                    &msg,
                );
                return Ok(msg);
            }
        };
        let result = self
            .mcp_request(
                server,
                "resources/read",
                serde_json::json!({ "uri": args.uri }),
            )
            .await;
        let (exit_code, text) = format_mcp_single_result(result);
        self.emit_tool_end(
            app_handle,
            thread_id,
            call_id,
            "mcp_read_resource",
            exit_code,
            &text,
        );
        Ok(text)
    }


    pub(crate) async fn exec_mcp_list_resource_templates(
        &self,
        arguments: &str,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
    ) -> AppResult<String> {
        #[derive(Deserialize, Default)]
        struct Args {
            #[serde(default)]
            server: Option<String>,
        }

        let args: Args = serde_json::from_str(arguments).unwrap_or_default();
        let display = args.server.as_deref().unwrap_or("all");
        self.emit_tool_start(
            app_handle,
            thread_id,
            call_id,
            "mcp_list_resource_templates",
            display,
        );

        let output = self
            .mcp_request_for_selection(
                args.server.as_deref(),
                "resources/templates/list",
                serde_json::json!({}),
            )
            .await;
        let (exit_code, text) = format_mcp_selection_result(output, "resourceTemplates");
        self.emit_tool_end(
            app_handle,
            thread_id,
            call_id,
            "mcp_list_resource_templates",
            exit_code,
            &text,
        );
        Ok(text)
    }


    pub(crate) async fn exec_mcp_list_prompts(
        &self,
        arguments: &str,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
    ) -> AppResult<String> {
        #[derive(Deserialize, Default)]
        struct Args {
            #[serde(default)]
            server: Option<String>,
        }

        let args: Args = serde_json::from_str(arguments).unwrap_or_default();
        let display = args.server.as_deref().unwrap_or("all");
        self.emit_tool_start(app_handle, thread_id, call_id, "mcp_list_prompts", display);

        let output = self
            .mcp_request_for_selection(
                args.server.as_deref(),
                "prompts/list",
                serde_json::json!({}),
            )
            .await;
        let (exit_code, text) = format_mcp_selection_result(output, "prompts");
        self.emit_tool_end(
            app_handle,
            thread_id,
            call_id,
            "mcp_list_prompts",
            exit_code,
            &text,
        );
        Ok(text)
    }


    pub(crate) async fn exec_mcp_get_prompt(
        &self,
        arguments: &str,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
    ) -> AppResult<String> {
        #[derive(Deserialize)]
        struct Args {
            server: String,
            prompt: String,
            #[serde(default)]
            arguments: Option<serde_json::Value>,
        }

        let args: Args = serde_json::from_str(arguments).map_err(|e| {
            crate::error::AppError::Custom(format!("Invalid mcp_get_prompt args: {e}"))
        })?;
        let display = format!("{}:{}", args.server, args.prompt);
        self.emit_tool_start(app_handle, thread_id, call_id, "mcp_get_prompt", &display);

        let server = match self.mcp_server(&args.server) {
            Ok(server) => server,
            Err(msg) => {
                self.emit_tool_end(app_handle, thread_id, call_id, "mcp_get_prompt", -1, &msg);
                return Ok(msg);
            }
        };
        let result = self
            .mcp_request(
                server,
                "prompts/get",
                serde_json::json!({
                    "name": args.prompt,
                    "arguments": args.arguments.unwrap_or_else(|| serde_json::json!({})),
                }),
            )
            .await;
        let (exit_code, text) = format_mcp_single_result(result);
        self.emit_tool_end(
            app_handle,
            thread_id,
            call_id,
            "mcp_get_prompt",
            exit_code,
            &text,
        );
        Ok(text)
    }


    pub(crate) async fn mcp_request_for_selection(
        &self,
        server: Option<&str>,
        method: &str,
        params: serde_json::Value,
    ) -> Vec<(String, Result<serde_json::Value, String>)> {
        if let Some(server_name) = server {
            return match self.mcp_server(server_name) {
                Ok(server) => {
                    vec![(
                        server.name.clone(),
                        self.mcp_request(server, method, params).await,
                    )]
                }
                Err(msg) => vec![(server_name.to_string(), Err(msg))],
            };
        }

        let mut servers = self.enabled_mcp_servers();
        servers.sort_by(|a, b| a.name.cmp(&b.name));
        if servers.is_empty() {
            return vec![(
                "all".to_string(),
                Err("No enabled MCP servers configured in codey/config.toml".to_string()),
            )];
        }

        let mut results = Vec::new();
        for server in servers {
            let name = server.name.clone();
            results.push((
                name,
                self.mcp_request(&server, method, params.clone()).await,
            ));
        }
        results
    }


    pub(crate) async fn discover_mcp_direct_tool_specs(&mut self) -> Vec<serde_json::Value> {
        if !self.mcp_discovery_needs_refresh() {
            return sorted_mcp_tool_specs(&self.mcp_tool_specs);
        }

        self.mcp_tool_aliases.clear();
        self.mcp_tool_specs.clear();
        let mut servers = self.enabled_mcp_servers();
        servers.sort_by(|a, b| a.name.cmp(&b.name));

        let mut used_names = BTreeSet::new();
        let mut specs = Vec::new();
        let discovery_timeout = Duration::from_secs(6);
        let requests = servers.iter().map(|server| async {
            let result = tokio::time::timeout(
                discovery_timeout,
                self.mcp_request(server, "tools/list", serde_json::json!({})),
            )
            .await
            .unwrap_or_else(|_| {
                Err(format!(
                    "MCP server '{}' catalog discovery timed out after {} seconds",
                    server.name,
                    discovery_timeout.as_secs()
                ))
            });
            (server.clone(), result)
        });
        let results = futures_util::future::join_all(requests).await;
        let mut failed_servers = Vec::new();

        for (server, result) in results {
            let result = match result {
                Ok(result) => result,
                Err(error) => {
                    warn!(
                        "MCP catalog discovery failed for '{}': {error}",
                        server.name
                    );
                    failed_servers.push(server.name.clone());
                    continue;
                }
            };
            let Some(tools) = result.get("tools").and_then(serde_json::Value::as_array) else {
                continue;
            };

            for tool in tools {
                let Some(tool_name) = tool.get("name").and_then(serde_json::Value::as_str) else {
                    continue;
                };
                let alias = mcp_direct_tool_name(&server.name, tool_name, &mut used_names);
                let connector = mcp_connector_metadata(&server.name, tool);
                self.mcp_tool_aliases.insert(
                    alias.clone(),
                    McpToolAlias {
                        server: server.name.clone(),
                        tool: tool_name.to_string(),
                        connector,
                    },
                );
                let spec = mcp_direct_tool_spec(&server.name, tool_name, tool, &alias);
                self.mcp_tool_specs.insert(alias, spec.clone());
                specs.push(spec);
            }
        }

        self.mcp_direct_tools_discovered = failed_servers.is_empty();
        self.mcp_discovery_retry_after = if failed_servers.is_empty() {
            None
        } else {
            warn!(
                "MCP catalog discovery failed for {}; keeping partial catalog and retrying after cooldown",
                failed_servers.join(", ")
            );
            Some(Instant::now() + Duration::from_secs(300))
        };
        specs
    }


    pub fn mcp_discovery_needs_refresh(&self) -> bool {
        if self.mcp_direct_tools_discovered {
            return false;
        }
        self.mcp_discovery_retry_after
            .is_none_or(|retry_after| Instant::now() >= retry_after)
    }


    pub(crate) async fn mcp_request(
        &self,
        server: &McpServerConfig,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        if server.disabled {
            return Err(format!("MCP server '{}' is disabled", server.name));
        }

        let timeout = std::time::Duration::from_secs(20);
        tokio::time::timeout(timeout, self.mcp_request_inner(server, method, params))
            .await
            .unwrap_or_else(|_| {
                Err(format!(
                    "MCP server '{}' timed out after 20 seconds",
                    server.name
                ))
            })
    }


    pub(crate) async fn mcp_request_inner(
        &self,
        server: &McpServerConfig,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        if server.is_http_transport() {
            self.mcp_http_request_with_session(server, method, params)
                .await
                .map_err(McpRequestError::message)
        } else if server.is_sse_transport() {
            self.mcp_sse_request_with_session(server, method, params)
                .await
                .map_err(McpRequestError::message)
        } else {
            self.mcp_request_with_session(server, method, params)
                .await
                .map_err(McpRequestError::message)
        }
    }


    pub(crate) async fn mcp_http_request_with_session(
        &self,
        server: &McpServerConfig,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, McpRequestError> {
        let (session, reused) = self.mcp_http_session(server).await?;
        let first = {
            let mut session = session.lock().await;
            self.request_mcp_http_session(&mut session, method, params.clone())
                .await
        };

        match first {
            Ok(value) => Ok(value),
            Err(error) if error.is_transport() && reused => {
                self.remove_mcp_http_session(&server.name).await;
                let (session, _) = self.mcp_http_session(server).await?;
                let retry = {
                    let mut session = session.lock().await;
                    self.request_mcp_http_session(&mut session, method, params)
                        .await
                };
                if retry
                    .as_ref()
                    .err()
                    .is_some_and(McpRequestError::is_transport)
                {
                    self.remove_mcp_http_session(&server.name).await;
                }
                retry
            }
            Err(error) => {
                if error.is_transport() {
                    self.remove_mcp_http_session(&server.name).await;
                }
                Err(error)
            }
        }
    }


    pub(crate) async fn request_mcp_http_session(
        &self,
        session: &mut McpHttpSession,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, McpRequestError> {
        let request_id = session.next_request_id;
        session.next_request_id += 1;
        let (response, session_id) = self
            .send_mcp_http_jsonrpc(
                &session.server,
                session.session_id.as_deref(),
                serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": request_id,
                    "method": method,
                    "params": params,
                }),
            )
            .await?;
        if session_id.is_some() {
            session.session_id = session_id;
        }
        session.request_count += 1;

        let response = response.ok_or_else(|| {
            McpRequestError::Transport(format!(
                "MCP HTTP server '{}' returned an empty response for request id {request_id}",
                session.server.name
            ))
        })?;
        if response.get("id").and_then(serde_json::Value::as_i64) != Some(request_id) {
            return Err(McpRequestError::Transport(format!(
                "MCP HTTP server '{}' returned response for unexpected id: {}",
                session.server.name,
                format_json_value(&response)
            )));
        }
        if let Some(error) = response.get("error") {
            return Err(McpRequestError::Rpc(format!(
                "MCP HTTP server '{}' returned error: {}",
                session.server.name,
                format_json_value(error)
            )));
        }
        Ok(response.get("result").cloned().unwrap_or(response))
    }


    pub(crate) async fn mcp_request_with_session(
        &self,
        server: &McpServerConfig,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, McpRequestError> {
        let (session, reused) = self.mcp_session(server).await?;
        let first = {
            let mut session = session.lock().await;
            session.request(method, params.clone()).await
        };

        match first {
            Ok(value) => Ok(value),
            Err(error) if error.is_transport() && reused => {
                self.remove_mcp_session(&server.name).await;
                let (session, _) = self.mcp_session(server).await?;
                let retry = {
                    let mut session = session.lock().await;
                    session.request(method, params).await
                };
                if retry
                    .as_ref()
                    .err()
                    .is_some_and(McpRequestError::is_transport)
                {
                    self.remove_mcp_session(&server.name).await;
                }
                retry
            }
            Err(error) => {
                if error.is_transport() {
                    self.remove_mcp_session(&server.name).await;
                }
                Err(error)
            }
        }
    }


    pub(crate) async fn mcp_session(
        &self,
        server: &McpServerConfig,
    ) -> Result<(Arc<Mutex<McpSession>>, bool), McpRequestError> {
        if let Some(existing) = self.mcp_sessions.lock().await.get(&server.name).cloned() {
            let matches_config = {
                let session = existing.lock().await;
                session.server == *server
            };
            if matches_config {
                return Ok((existing, true));
            }
            self.remove_mcp_session(&server.name).await;
        }

        let session = Arc::new(Mutex::new(self.start_mcp_session(server).await?));
        self.mcp_sessions
            .lock()
            .await
            .insert(server.name.clone(), session.clone());
        Ok((session, false))
    }


    pub(crate) async fn mcp_http_session(
        &self,
        server: &McpServerConfig,
    ) -> Result<(Arc<Mutex<McpHttpSession>>, bool), McpRequestError> {
        if let Some(existing) = self
            .mcp_http_sessions
            .lock()
            .await
            .get(&server.name)
            .cloned()
        {
            let matches_config = {
                let session = existing.lock().await;
                session.server == *server
            };
            if matches_config {
                return Ok((existing, true));
            }
            self.remove_mcp_http_session(&server.name).await;
        }

        let session = Arc::new(Mutex::new(self.start_mcp_http_session(server).await?));
        self.mcp_http_sessions
            .lock()
            .await
            .insert(server.name.clone(), session.clone());
        Ok((session, false))
    }


    pub(crate) async fn start_mcp_http_session(
        &self,
        server: &McpServerConfig,
    ) -> Result<McpHttpSession, McpRequestError> {
        let init_id = 1;
        let (response, session_id) = self
            .send_mcp_http_jsonrpc(
                server,
                None,
                serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": init_id,
                    "method": "initialize",
                    "params": {
                        "protocolVersion": "2024-11-05",
                        "capabilities": {},
                        "clientInfo": {
                            "name": "CN-Codex",
                            "version": "0.1.0"
                        }
                    }
                }),
            )
            .await?;

        let response = response.ok_or_else(|| {
            McpRequestError::Transport(format!(
                "MCP HTTP server '{}' returned an empty initialize response",
                server.name
            ))
        })?;
        if let Some(error) = response.get("error") {
            return Err(McpRequestError::Rpc(format!(
                "MCP HTTP server '{}' initialize failed: {}",
                server.name,
                format_json_value(error)
            )));
        }

        let mut session = McpHttpSession {
            server: server.clone(),
            session_id,
            next_request_id: 2,
            request_count: 0,
            initialized_at_ms: now_millis(),
        };

        let (_, session_id) = self
            .send_mcp_http_jsonrpc(
                server,
                session.session_id.as_deref(),
                serde_json::json!({
                    "jsonrpc": "2.0",
                    "method": "notifications/initialized",
                    "params": {}
                }),
            )
            .await?;
        if session_id.is_some() {
            session.session_id = session_id;
        }

        Ok(session)
    }


    pub(crate) async fn send_mcp_http_jsonrpc(
        &self,
        server: &McpServerConfig,
        session_id: Option<&str>,
        message: serde_json::Value,
    ) -> Result<(Option<serde_json::Value>, Option<String>), McpRequestError> {
        let Some(url) = server
            .url
            .as_deref()
            .map(str::trim)
            .filter(|url| !url.is_empty())
        else {
            return Err(McpRequestError::Transport(format!(
                "MCP HTTP server '{}' is missing url",
                server.name
            )));
        };

        let mut request = self
            .http
            .post(url)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .header(
                reqwest::header::ACCEPT,
                "application/json, text/event-stream",
            );
        if let Some(session_id) = session_id.filter(|value| !value.trim().is_empty()) {
            request = request.header("mcp-session-id", session_id);
        }
        for (key, value) in &server.headers {
            let Ok(name) = reqwest::header::HeaderName::from_bytes(key.as_bytes()) else {
                return Err(McpRequestError::Transport(format!(
                    "MCP HTTP server '{}' has invalid header name '{}'",
                    server.name, key
                )));
            };
            let Ok(value) = reqwest::header::HeaderValue::from_str(value) else {
                return Err(McpRequestError::Transport(format!(
                    "MCP HTTP server '{}' has invalid value for header '{}'",
                    server.name, key
                )));
            };
            request = request.header(name, value);
        }

        let response = request.json(&message).send().await.map_err(|error| {
            McpRequestError::Transport(format!(
                "MCP HTTP server '{}' request failed: {error}",
                server.name
            ))
        })?;
        let status = response.status();
        let response_session_id = response
            .headers()
            .get("mcp-session-id")
            .and_then(|value| value.to_str().ok())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string);
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_string();
        let body = response.text().await.map_err(|error| {
            McpRequestError::Transport(format!(
                "MCP HTTP server '{}' response read failed: {error}",
                server.name
            ))
        })?;

        if !status.is_success() {
            return Err(McpRequestError::Transport(format!(
                "MCP HTTP server '{}' returned HTTP {status}: {}",
                server.name,
                truncate_output(&body, 2000)
            )));
        }

        let parsed = parse_mcp_http_response_body(&body, &content_type).map_err(|message| {
            McpRequestError::Transport(format!(
                "MCP HTTP server '{}' response parse failed: {message}",
                server.name
            ))
        })?;
        Ok((parsed, response_session_id))
    }


    pub(crate) async fn mcp_sse_request_with_session(
        &self,
        server: &McpServerConfig,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, McpRequestError> {
        let (session, reused) = self.mcp_sse_session(server).await?;
        let first = {
            let mut session = session.lock().await;
            self.request_mcp_sse_session(&mut session, method, params.clone())
                .await
        };

        match first {
            Ok(value) => Ok(value),
            Err(error) if error.is_transport() && reused => {
                self.remove_mcp_sse_session(&server.name).await;
                let (session, _) = self.mcp_sse_session(server).await?;
                let retry = {
                    let mut session = session.lock().await;
                    self.request_mcp_sse_session(&mut session, method, params)
                        .await
                };
                if retry
                    .as_ref()
                    .err()
                    .is_some_and(McpRequestError::is_transport)
                {
                    self.remove_mcp_sse_session(&server.name).await;
                }
                retry
            }
            Err(error) => {
                if error.is_transport() {
                    self.remove_mcp_sse_session(&server.name).await;
                }
                Err(error)
            }
        }
    }


    pub(crate) async fn request_mcp_sse_session(
        &self,
        session: &mut McpSseSession,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, McpRequestError> {
        let request_id = session.next_request_id;
        session.next_request_id += 1;
        let message = serde_json::json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "method": method,
            "params": params,
        });
        let (response, _) = self
            .send_mcp_sse_jsonrpc(session, message, Some(request_id))
            .await?;
        let response = response.ok_or_else(|| {
            McpRequestError::Transport(format!(
                "MCP SSE server '{}' returned an empty response for {method}",
                session.server.name
            ))
        })?;
        if let Some(error) = response.get("error") {
            return Err(McpRequestError::Rpc(format!(
                "MCP SSE server '{}' error: {}",
                session.server.name,
                format_json_value(error)
            )));
        }
        session.request_count = session.request_count.saturating_add(1);
        Ok(response
            .get("result")
            .cloned()
            .unwrap_or(serde_json::Value::Null))
    }


    pub(crate) async fn mcp_sse_session(
        &self,
        server: &McpServerConfig,
    ) -> Result<(Arc<Mutex<McpSseSession>>, bool), McpRequestError> {
        if let Some(existing) = self
            .mcp_sse_sessions
            .lock()
            .await
            .get(&server.name)
            .cloned()
        {
            let matches_config = {
                let session = existing.lock().await;
                session.server == *server
            };
            if matches_config {
                return Ok((existing, true));
            }
            self.remove_mcp_sse_session(&server.name).await;
        }

        let session = Arc::new(Mutex::new(self.start_mcp_sse_session(server).await?));
        self.mcp_sse_sessions
            .lock()
            .await
            .insert(server.name.clone(), session.clone());
        Ok((session, false))
    }


    pub(crate) async fn start_mcp_sse_session(
        &self,
        server: &McpServerConfig,
    ) -> Result<McpSseSession, McpRequestError> {
        let mut session = self.open_mcp_sse_stream(server).await?;
        let init_id = session.next_request_id;
        session.next_request_id += 1;
        let (response, _) = self
            .send_mcp_sse_jsonrpc(
                &mut session,
                serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": init_id,
                    "method": "initialize",
                    "params": {
                        "protocolVersion": "2024-11-05",
                        "capabilities": {},
                        "clientInfo": {
                            "name": "CN-Codex",
                            "version": "0.1.0"
                        }
                    }
                }),
                Some(init_id),
            )
            .await?;
        let response = response.ok_or_else(|| {
            McpRequestError::Transport(format!(
                "MCP SSE server '{}' returned an empty initialize response",
                server.name
            ))
        })?;
        if let Some(error) = response.get("error") {
            return Err(McpRequestError::Rpc(format!(
                "MCP SSE server '{}' initialize failed: {}",
                server.name,
                format_json_value(error)
            )));
        }

        let _ = self
            .send_mcp_sse_jsonrpc(
                &mut session,
                serde_json::json!({
                    "jsonrpc": "2.0",
                    "method": "notifications/initialized",
                    "params": {}
                }),
                None,
            )
            .await?;

        session.initialized_at_ms = now_millis();
        Ok(session)
    }


    pub(crate) async fn open_mcp_sse_stream(
        &self,
        server: &McpServerConfig,
    ) -> Result<McpSseSession, McpRequestError> {
        let Some(url) = server
            .url
            .as_deref()
            .map(str::trim)
            .filter(|url| !url.is_empty())
        else {
            return Err(McpRequestError::Transport(format!(
                "MCP SSE server '{}' is missing url",
                server.name
            )));
        };

        let mut request = self
            .http
            .get(url)
            .header(reqwest::header::ACCEPT, "text/event-stream");
        for (key, value) in &server.headers {
            let Ok(name) = reqwest::header::HeaderName::from_bytes(key.as_bytes()) else {
                return Err(McpRequestError::Transport(format!(
                    "MCP SSE server '{}' has invalid header name '{}'",
                    server.name, key
                )));
            };
            let Ok(value) = reqwest::header::HeaderValue::from_str(value) else {
                return Err(McpRequestError::Transport(format!(
                    "MCP SSE server '{}' has invalid value for header '{}'",
                    server.name, key
                )));
            };
            request = request.header(name, value);
        }

        let response = request.send().await.map_err(|error| {
            McpRequestError::Transport(format!(
                "MCP SSE server '{}' connect failed: {error}",
                server.name
            ))
        })?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(McpRequestError::Transport(format!(
                "MCP SSE server '{}' returned HTTP {status}: {}",
                server.name,
                truncate_output(&body, 2000)
            )));
        }

        let (event_tx, event_rx) =
            tokio::sync::mpsc::unbounded_channel::<Result<serde_json::Value, String>>();
        let (endpoint_tx, endpoint_rx) = tokio::sync::oneshot::channel::<Result<String, String>>();
        let stream = response.bytes_stream();
        let base_url = url.to_string();
        let server_name = server.name.clone();
        let worker = tokio::spawn(async move {
            use futures_util::StreamExt;

            let mut endpoint_tx = Some(endpoint_tx);
            let mut buffer = String::new();
            let mut event_name = String::new();
            let mut data_lines: Vec<String> = Vec::new();
            let mut stream = stream;

            while let Some(chunk) = stream.next().await {
                let chunk = match chunk {
                    Ok(chunk) => chunk,
                    Err(error) => {
                        let message =
                            format!("MCP SSE server '{server_name}' stream read failed: {error}");
                        if let Some(tx) = endpoint_tx.take() {
                            let _ = tx.send(Err(message.clone()));
                        }
                        let _ = event_tx.send(Err(message));
                        return;
                    }
                };
                buffer.push_str(&String::from_utf8_lossy(&chunk));

                loop {
                    let Some(newline_at) = buffer.find('\n') else {
                        break;
                    };
                    let mut line = buffer[..newline_at].to_string();
                    buffer.drain(..=newline_at);
                    if line.ends_with('\r') {
                        line.pop();
                    }

                    if line.is_empty() {
                        let event = event_name.trim().to_ascii_lowercase();
                        let data = data_lines.join("\n");
                        event_name.clear();
                        data_lines.clear();
                        if data.trim().is_empty() {
                            continue;
                        }

                        if event == "endpoint" || event.is_empty() && data.trim().starts_with('/') {
                            if let Some(tx) = endpoint_tx.take() {
                                let endpoint =
                                    match resolve_mcp_sse_endpoint_url(&base_url, data.trim()) {
                                        Ok(endpoint) => Ok(endpoint),
                                        Err(error) => Err(error),
                                    };
                                let _ = tx.send(endpoint);
                            }
                            continue;
                        }

                        if event == "message" || event.is_empty() {
                            match serde_json::from_str::<serde_json::Value>(data.trim()) {
                                Ok(value) => {
                                    if event_tx.send(Ok(value)).is_err() {
                                        return;
                                    }
                                }
                                Err(error) => {
                                    let message = format!(
                                        "MCP SSE server '{server_name}' invalid message JSON: {error}"
                                    );
                                    let _ = event_tx.send(Err(message));
                                    return;
                                }
                            }
                        }
                        continue;
                    }

                    if let Some(rest) = line.strip_prefix("event:") {
                        event_name = rest.trim().to_string();
                        continue;
                    }
                    if let Some(rest) = line.strip_prefix("data:") {
                        data_lines.push(rest.trim_start().to_string());
                    }
                }
            }

            if let Some(tx) = endpoint_tx.take() {
                let _ = tx.send(Err(format!(
                    "MCP SSE server '{server_name}' closed before endpoint event"
                )));
            }
            let _ = event_tx.send(Err(format!("MCP SSE server '{server_name}' stream closed")));
        });

        let endpoint_url = match tokio::time::timeout(Duration::from_secs(10), endpoint_rx).await {
            Ok(Ok(Ok(endpoint))) => endpoint,
            Ok(Ok(Err(error))) => {
                worker.abort();
                return Err(McpRequestError::Transport(error));
            }
            Ok(Err(_)) => {
                worker.abort();
                return Err(McpRequestError::Transport(format!(
                    "MCP SSE server '{}' endpoint channel closed",
                    server.name
                )));
            }
            Err(_) => {
                worker.abort();
                return Err(McpRequestError::Transport(format!(
                    "MCP SSE server '{}' timed out waiting for endpoint event",
                    server.name
                )));
            }
        };

        Ok(McpSseSession {
            server: server.clone(),
            endpoint_url,
            event_rx,
            worker,
            next_request_id: 1,
            request_count: 0,
            initialized_at_ms: now_millis(),
        })
    }


    pub(crate) async fn send_mcp_sse_jsonrpc(
        &self,
        session: &mut McpSseSession,
        message: serde_json::Value,
        expected_id: Option<i64>,
    ) -> Result<(Option<serde_json::Value>, Option<String>), McpRequestError> {
        let mut request = self
            .http
            .post(&session.endpoint_url)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .header(
                reqwest::header::ACCEPT,
                "application/json, text/event-stream",
            );
        for (key, value) in &session.server.headers {
            let Ok(name) = reqwest::header::HeaderName::from_bytes(key.as_bytes()) else {
                return Err(McpRequestError::Transport(format!(
                    "MCP SSE server '{}' has invalid header name '{}'",
                    session.server.name, key
                )));
            };
            let Ok(value) = reqwest::header::HeaderValue::from_str(value) else {
                return Err(McpRequestError::Transport(format!(
                    "MCP SSE server '{}' has invalid value for header '{}'",
                    session.server.name, key
                )));
            };
            request = request.header(name, value);
        }

        let response = request.json(&message).send().await.map_err(|error| {
            McpRequestError::Transport(format!(
                "MCP SSE server '{}' request failed: {error}",
                session.server.name
            ))
        })?;
        let status = response.status();
        let response_session_id = response
            .headers()
            .get("mcp-session-id")
            .and_then(|value| value.to_str().ok())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string);
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_string();
        let body = response.text().await.map_err(|error| {
            McpRequestError::Transport(format!(
                "MCP SSE server '{}' response read failed: {error}",
                session.server.name
            ))
        })?;

        if !status.is_success() {
            return Err(McpRequestError::Transport(format!(
                "MCP SSE server '{}' returned HTTP {status}: {}",
                session.server.name,
                truncate_output(&body, 2000)
            )));
        }

        // Notifications may have empty accepted responses.
        if expected_id.is_none() {
            if body.trim().is_empty() {
                return Ok((None, response_session_id));
            }
            // Some servers still return a body; ignore it for notifications.
            return Ok((None, response_session_id));
        }

        if let Ok(Some(parsed)) = parse_mcp_http_response_body(&body, &content_type) {
            if response_matches_mcp_id(&parsed, expected_id) {
                return Ok((Some(parsed), response_session_id));
            }
        }

        // Classic SSE transport delivers JSON-RPC responses on the open GET stream.
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(McpRequestError::Transport(format!(
                    "MCP SSE server '{}' timed out waiting for response id {:?}",
                    session.server.name, expected_id
                )));
            }

            let event = {
                match tokio::time::timeout(remaining, session.event_rx.recv()).await {
                    Ok(Some(Ok(value))) => value,
                    Ok(Some(Err(error))) => {
                        return Err(McpRequestError::Transport(error));
                    }
                    Ok(None) => {
                        return Err(McpRequestError::Transport(format!(
                            "MCP SSE server '{}' stream closed while waiting for response",
                            session.server.name
                        )));
                    }
                    Err(_) => {
                        return Err(McpRequestError::Transport(format!(
                            "MCP SSE server '{}' timed out waiting for response id {:?}",
                            session.server.name, expected_id
                        )));
                    }
                }
            };

            if response_matches_mcp_id(&event, expected_id) {
                return Ok((Some(event), response_session_id));
            }
            // Ignore unrelated notifications / server requests for now.
        }
    }


    pub(crate) async fn start_mcp_session(
        &self,
        server: &McpServerConfig,
    ) -> Result<McpSession, McpRequestError> {
        let program = resolve_mcp_command_for_platform(&server.command);
        let mut command = Command::new(&program);
        command.envs(&server.env);
        if should_inject_bundled_node_runtime(&program) {
            if let Some(node_dir) = bundled_node_bin_dir(&self.workspace_config_dir) {
                let existing_path = server_env_path_override(server)
                    .map(std::ffi::OsString::from)
                    .or_else(|| std::env::var_os("PATH"));
                let merged_path = prepend_path_value(existing_path, &node_dir);
                command.env("PATH", &merged_path);
                #[cfg(target_os = "windows")]
                command.env("Path", merged_path);
            }
        }
        command
            .args(&server.args)
            .current_dir(resolve_command_cwd(&self.cwd, server.cwd.as_deref()))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        #[cfg(windows)]
        command.no_console();

        let mut child = command.spawn().map_err(|e| {
            McpRequestError::Transport(format!("Failed to start MCP server '{}': {e}", server.name))
        })?;
        let mut stdin = child.stdin.take().ok_or_else(|| {
            McpRequestError::Transport(format!("MCP server '{}' stdin unavailable", server.name))
        })?;
        let stdout = child.stdout.take().ok_or_else(|| {
            McpRequestError::Transport(format!("MCP server '{}' stdout unavailable", server.name))
        })?;
        let stderr = child.stderr.take();
        let stderr_buffer = Arc::new(Mutex::new(String::new()));
        collect_mcp_stderr(stderr, stderr_buffer.clone());

        let mut reader = BufReader::new(stdout);

        if let Err(error) = write_mcp_message(
            &mut stdin,
            serde_json::json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {
                    "protocolVersion": "2024-11-05",
                    "capabilities": {},
                    "clientInfo": {
                        "name": "CN-Codex",
                        "version": "0.1.0"
                    }
                }
            }),
        )
        .await
        {
            let _ = child.kill().await;
            let _ = child.wait().await;
            return Err(mcp_transport_error(&server.name, &stderr_buffer, error).await);
        }
        let init_response = match read_mcp_response(&mut reader, 1).await {
            Ok(response) => response,
            Err(error) => {
                let _ = child.kill().await;
                let _ = child.wait().await;
                return Err(mcp_transport_error(&server.name, &stderr_buffer, error).await);
            }
        };
        if let Some(error) = init_response.get("error") {
            let _ = child.kill().await;
            let _ = child.wait().await;
            return Err(McpRequestError::Rpc(format!(
                "MCP server '{}' initialize failed: {}",
                server.name,
                format_json_value(error)
            )));
        }

        if let Err(error) = write_mcp_message(
            &mut stdin,
            serde_json::json!({
                "jsonrpc": "2.0",
                "method": "notifications/initialized",
                "params": {}
            }),
        )
        .await
        {
            let _ = child.kill().await;
            let _ = child.wait().await;
            return Err(mcp_transport_error(&server.name, &stderr_buffer, error).await);
        }

        Ok(McpSession {
            server: server.clone(),
            stdin,
            reader,
            child,
            stderr: stderr_buffer,
            next_request_id: 2,
            request_count: 0,
            initialized_at_ms: now_millis(),
        })
    }


    pub(crate) async fn remove_mcp_session(&self, server_name: &str) {
        let session = self.mcp_sessions.lock().await.remove(server_name);
        if let Some(session) = session {
            close_mcp_session(session).await;
        }
    }


    pub(crate) async fn remove_mcp_http_session(&self, server_name: &str) {
        self.mcp_http_sessions.lock().await.remove(server_name);
    }


    pub(crate) async fn remove_mcp_sse_session(&self, server_name: &str) {
        if let Some(session) = self.mcp_sse_sessions.lock().await.remove(server_name) {
            let session = session.lock().await;
            session.worker.abort();
        }
    }


    pub(crate) async fn mcp_session_status(&self, server_name: &str) -> serde_json::Value {
        let session = self.mcp_sessions.lock().await.get(server_name).cloned();
        if let Some(session) = session {
            let session = session.lock().await;
            return serde_json::json!({
                "connected": true,
                "transport": "stdio",
                "requestCount": session.request_count,
                "initializedAtMs": session.initialized_at_ms,
            });
        }
        let session = self
            .mcp_http_sessions
            .lock()
            .await
            .get(server_name)
            .cloned();
        if let Some(session) = session {
            let session = session.lock().await;
            return serde_json::json!({
                "connected": true,
                "transport": "http",
                "requestCount": session.request_count,
                "initializedAtMs": session.initialized_at_ms,
                "sessionIdPresent": session.session_id.is_some(),
            });
        }
        let session = self.mcp_sse_sessions.lock().await.get(server_name).cloned();
        if let Some(session) = session {
            let session = session.lock().await;
            return serde_json::json!({
                "connected": true,
                "transport": "sse",
                "requestCount": session.request_count,
                "initializedAtMs": session.initialized_at_ms,
                "endpoint": session.endpoint_url,
            });
        }
        serde_json::json!({ "connected": false })
    }


    pub(crate) fn enabled_mcp_servers(&self) -> Vec<McpServerConfig> {
        self.mcp_servers
            .values()
            .filter(|server| !server.disabled)
            .cloned()
            .collect()
    }


    pub fn mcp_loading_snapshot(&self) -> (usize, usize, bool) {
        (
            self.enabled_mcp_servers().len(),
            self.mcp_tool_specs.len(),
            self.mcp_direct_tools_discovered,
        )
    }


    pub(crate) fn mcp_server(&self, name: &str) -> Result<&McpServerConfig, String> {
        self.mcp_servers
            .get(name)
            .ok_or_else(|| format!("MCP server not configured: {name}"))
            .and_then(|server| {
                if server.disabled {
                    Err(format!("MCP server '{name}' is disabled"))
                } else {
                    Ok(server)
                }
            })
    }

}
