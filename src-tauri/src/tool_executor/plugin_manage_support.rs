use super::*;

pub(crate) fn sanitize_mcp_server_name(name: &str) -> Result<(), String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("MCP server name must not be empty".to_string());
    }
    if name.contains('/')
        || name.contains('\\')
        || name.contains("..")
        || name.contains('[')
        || name.contains(']')
        || name.contains('"')
        || name.contains('\'')
        || name.contains(' ')
        || Path::new(name).components().count() != 1
    {
        return Err(format!(
            "Invalid MCP server name '{name}'. Use a single token without path separators or spaces."
        ));
    }
    Ok(())
}


pub(crate) fn sanitize_skill_manage_id(skill_id: &str) -> Result<(), String> {
    let skill_id = skill_id.trim();
    if skill_id.is_empty()
        || skill_id.contains('/')
        || skill_id.contains('\\')
        || skill_id.contains("..")
        || Path::new(skill_id).components().count() != 1
    {
        return Err(format!(
            "Invalid skill_id '{skill_id}'. Use a single directory name without path separators."
        ));
    }
    if !skill_id
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
    {
        return Err(format!(
            "Invalid skill_id '{skill_id}'. Use lowercase letters, digits, hyphen, or underscore."
        ));
    }
    Ok(())
}


pub(crate) fn string_map_to_json(map: &BTreeMap<String, String>) -> serde_json::Value {
    let mut object = serde_json::Map::new();
    for (key, value) in map {
        object.insert(key.clone(), serde_json::Value::String(value.clone()));
    }
    serde_json::Value::Object(object)
}


pub(crate) fn toml_table_to_json_object(table: &toml::Table) -> serde_json::Value {
    serde_json::to_value(table).unwrap_or_else(|_| serde_json::json!({}))
}


pub(crate) fn build_mcp_server_config_value(args: &McpManageArgs) -> Result<serde_json::Value, String> {
    let mut object = match &args.config {
        Some(serde_json::Value::Object(map)) => map.clone(),
        Some(_) => return Err("mcp_manage config must be an object".to_string()),
        None => serde_json::Map::new(),
    };

    if let Some(command) = args
        .command
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        object.insert(
            "command".to_string(),
            serde_json::Value::String(command.to_string()),
        );
    }
    if let Some(args_list) = &args.args {
        object.insert(
            "args".to_string(),
            serde_json::Value::Array(
                args_list
                    .iter()
                    .map(|item| serde_json::Value::String(item.clone()))
                    .collect(),
            ),
        );
    }
    if let Some(cwd) = args
        .cwd
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        object.insert(
            "cwd".to_string(),
            serde_json::Value::String(cwd.to_string()),
        );
    }
    if let Some(env) = &args.env {
        object.insert("env".to_string(), string_map_to_json(env));
    }
    if let Some(url) = args
        .url
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        object.insert(
            "url".to_string(),
            serde_json::Value::String(url.to_string()),
        );
    }
    if let Some(transport) = args
        .r#type
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        object.insert(
            "type".to_string(),
            serde_json::Value::String(transport.to_string()),
        );
    }
    if let Some(headers) = &args.headers {
        object.insert("headers".to_string(), string_map_to_json(headers));
    }
    if let Some(disabled) = args.disabled {
        object.insert("disabled".to_string(), serde_json::Value::Bool(disabled));
    }

    object.remove("name");
    object.remove("description");
    object.remove("isActive");
    object.remove("is_active");
    object.remove("enabled");

    let command = object
        .get("command")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .unwrap_or_default();
    let url = object
        .get("url")
        .or_else(|| object.get("server_url"))
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .unwrap_or_default();
    if command.is_empty() && url.is_empty() {
        return Err(
            "mcp_manage install requires either command (stdio) or url (http/sse)".to_string(),
        );
    }

    if !url.is_empty() {
        if object
            .get("type")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .is_none()
        {
            let inferred = if url.to_ascii_lowercase().contains("/sse") {
                "sse"
            } else {
                "http"
            };
            object.insert(
                "type".to_string(),
                serde_json::Value::String(inferred.to_string()),
            );
        }
    }

    if !object.contains_key("disabled") {
        object.insert("disabled".to_string(), serde_json::Value::Bool(false));
    }

    Ok(serde_json::Value::Object(object))
}


pub(crate) fn build_skill_manage_content(args: &SkillManageArgs, skill_id: &str) -> Result<String, String> {
    if let Some(content) = args
        .content
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Ok(content.to_string());
    }

    let name = args
        .name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(skill_id);
    let description = args
        .description
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("Installed via skill_manage");
    let tags = args.tags.clone().unwrap_or_default();
    let tags_literal = if tags.is_empty() {
        "[]".to_string()
    } else {
        let rendered = tags
            .iter()
            .map(|tag| format!("\"{}\"", tag.replace('"', "\\\"")))
            .collect::<Vec<_>>()
            .join(", ");
        format!("[{rendered}]")
    };

    Ok(format!(
        "---\nname: \"{}\"\ndescription: \"{}\"\ntags: {}\n---\n\n# {}\n\n{}\n",
        name.replace('"', "\\\""),
        description.replace('"', "\\\""),
        tags_literal,
        name,
        description
    ))
}


pub(crate) fn list_local_skills(skills_dir: &Path) -> Vec<serde_json::Value> {
    let Ok(entries) = std::fs::read_dir(skills_dir) else {
        return Vec::new();
    };
    let mut skills = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let skill_md = path.join("SKILL.md");
        if !skill_md.is_file() {
            continue;
        }
        let id = path
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| "skill".to_string());
        let content = std::fs::read_to_string(&skill_md).unwrap_or_default();
        let (name, description, tags) = parse_tool_search_skill_frontmatter(&content);
        skills.push(serde_json::json!({
            "id": id,
            "name": if name.is_empty() { id.clone() } else { name },
            "description": description,
            "tags": tags,
            "path": skill_md.to_string_lossy(),
        }));
    }
    skills.sort_by(|left, right| {
        left.get("id")
            .and_then(serde_json::Value::as_str)
            .cmp(&right.get("id").and_then(serde_json::Value::as_str))
    });
    skills
}


pub(crate) fn emit_mcp_servers_changed(app_handle: &AppHandle, name: &str, action: &str, config_path: &str) {
    let payload = serde_json::json!({
        "name": name,
        "action": action,
        "configPath": config_path,
        "source": "mcp_manage",
    });
    let _ = app_handle.emit("mcp-servers-changed", payload.clone());
    crate::mobile_server::broadcast("mcp-servers-changed", payload);
}


pub(crate) fn emit_skills_changed(app_handle: &AppHandle, skill_id: &str, action: &str) {
    let payload = serde_json::json!({
        "skillId": skill_id,
        "action": action,
        "source": "skill_manage",
    });
    let _ = app_handle.emit("skills-changed", payload.clone());
    crate::mobile_server::broadcast("skills-changed", payload);
}


pub(crate) fn resolve_plugin_cache_source_dir(raw_source_dir: Option<&str>) -> Result<PathBuf, String> {
    if let Some(source_dir) = raw_source_dir
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let path = PathBuf::from(source_dir);
        if path.is_dir() {
            return Ok(path);
        }
        return Err(format!("Codex plugin cache not found: {}", path.display()));
    }

    let path = plugin_commands::default_codex_plugin_cache_dir()
        .ok_or_else(|| "Could not locate the Codex plugin cache".to_string())?;
    if path.is_dir() {
        Ok(path)
    } else {
        Err(format!("Codex plugin cache not found: {}", path.display()))
    }
}


pub(crate) fn plugin_install_candidates(
    source_dir: &Path,
    workspace_config_dir: &Path,
    query: Option<&str>,
    include_installed: bool,
    limit: usize,
) -> Vec<PluginInstallCandidate> {
    let query = query
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_ascii_lowercase());
    let mut candidates = plugin_commands::discover_plugin_roots(source_dir)
        .into_iter()
        .filter_map(|root| {
            plugin_install_candidate_from_root(source_dir, workspace_config_dir, &root)
        })
        .filter(|candidate| include_installed || !candidate.installed)
        .filter(|candidate| {
            query
                .as_deref()
                .is_none_or(|query| plugin_install_candidate_matches(candidate, query))
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then(left.version.cmp(&right.version))
            .then(left.id.cmp(&right.id))
    });
    candidates.truncate(limit);
    candidates
}


pub(crate) fn plugin_install_candidate_from_root(
    source_dir: &Path,
    workspace_config_dir: &Path,
    root: &Path,
) -> Option<PluginInstallCandidate> {
    let manifest_path = root.join(".codex-plugin").join("plugin.json");
    let manifest = read_json_file(&manifest_path)?;
    let name = manifest
        .get("name")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())?
        .to_string();
    let version = manifest
        .get("version")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);
    let description = manifest
        .get("description")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);
    let destination = workspace_config_dir
        .join("plugins")
        .join(plugin_commands::sanitize_plugin_destination_name(&name));
    let id = plugin_candidate_id(source_dir, root, &name, version.as_deref());
    let mcp_server_names = plugin_candidate_mcp_server_names(root, &manifest);
    let app_connector_ids = plugin_candidate_app_connector_ids(root, &manifest);
    let has_skills = plugin_candidate_has_skills(root, &manifest);

    Some(PluginInstallCandidate {
        id,
        name,
        version,
        description,
        source: root.to_string_lossy().to_string(),
        destination: destination.to_string_lossy().to_string(),
        installed: destination.is_dir(),
        has_skills,
        mcp_server_names,
        app_connector_ids,
    })
}


pub(crate) fn plugin_candidate_id(
    source_dir: &Path,
    root: &Path,
    name: &str,
    version: Option<&str>,
) -> String {
    if let Ok(relative) = root.strip_prefix(source_dir)
        && let Some(relative) = relative.to_str()
    {
        let relative = relative.replace('\\', "/");
        if !relative.is_empty() {
            return format!("local-cache:{relative}");
        }
    }
    match version {
        Some(version) => format!("local-cache:{}@{}", name, version),
        None => format!("local-cache:{name}"),
    }
}


pub(crate) fn plugin_install_candidate_matches(candidate: &PluginInstallCandidate, query: &str) -> bool {
    let haystack = format!(
        "{} {} {} {} {} {} {} {}",
        candidate.id,
        candidate.name,
        candidate.version.as_deref().unwrap_or_default(),
        candidate.description.as_deref().unwrap_or_default(),
        candidate.source,
        candidate.destination,
        candidate.mcp_server_names.join(" "),
        candidate.app_connector_ids.join(" "),
    )
    .to_ascii_lowercase();
    haystack.contains(query)
}


pub(crate) fn select_plugin_install_candidate(
    candidates: &[PluginInstallCandidate],
    tool_id: Option<&str>,
    name: Option<&str>,
) -> Result<PluginInstallCandidate, String> {
    if let Some(tool_id) = tool_id.map(str::trim).filter(|value| !value.is_empty()) {
        return candidates
            .iter()
            .find(|candidate| candidate.id == tool_id || candidate.source == tool_id)
            .cloned()
            .ok_or_else(|| {
                format!(
                    "tool_id must match one of the candidates returned by list_available_plugins_to_install: {tool_id}"
                )
            });
    }

    let Some(name) = name.map(str::trim).filter(|value| !value.is_empty()) else {
        return Err(
            "request_plugin_install requires tool_id or name. Call list_available_plugins_to_install first."
                .to_string(),
        );
    };
    let name_lower = name.to_ascii_lowercase();
    let mut matches = candidates
        .iter()
        .filter(|candidate| candidate.name.eq_ignore_ascii_case(name))
        .cloned()
        .collect::<Vec<_>>();
    if matches.is_empty() {
        matches = candidates
            .iter()
            .filter(|candidate| candidate.name.to_ascii_lowercase().contains(&name_lower))
            .cloned()
            .collect::<Vec<_>>();
    }
    match matches.len() {
        0 => Err(format!("No plugin candidate matched name: {name}")),
        1 => Ok(matches.remove(0)),
        _ => Err(format!(
            "Plugin name matched multiple candidates. Use tool_id from list_available_plugins_to_install: {}",
            matches
                .iter()
                .map(|candidate| candidate.id.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}


pub(crate) fn plugin_candidate_has_skills(root: &Path, manifest: &serde_json::Value) -> bool {
    if root.join("skills").is_dir() {
        return true;
    }
    manifest
        .get("skills")
        .and_then(serde_json::Value::as_str)
        .and_then(|value| resolve_plugin_manifest_path(root, value).ok())
        .is_some_and(|path| path.is_dir())
}


pub(crate) fn plugin_candidate_mcp_server_names(root: &Path, manifest: &serde_json::Value) -> Vec<String> {
    let mut names = BTreeSet::new();
    if let Some(value) = manifest
        .get("mcpServers")
        .or_else(|| manifest.get("mcp_servers"))
    {
        collect_mcp_server_names(root, value, &mut names);
    } else {
        let default_path = root.join(".mcp.json");
        if let Some(value) = read_json_file(&default_path) {
            collect_mcp_server_names(root, &value, &mut names);
        }
    }
    names.into_iter().collect()
}


pub(crate) fn collect_mcp_server_names(root: &Path, value: &serde_json::Value, names: &mut BTreeSet<String>) {
    if let Some(path) = value.as_str() {
        if let Ok(path) = resolve_plugin_manifest_path(root, path)
            && let Some(value) = read_json_file(&path)
        {
            collect_mcp_server_names(root, &value, names);
        }
        return;
    }
    let Some(object) = value.as_object() else {
        return;
    };
    let server_map = object
        .get("mcpServers")
        .or_else(|| object.get("mcp_servers"))
        .and_then(serde_json::Value::as_object)
        .unwrap_or(object);
    for name in server_map.keys() {
        names.insert(name.clone());
    }
}


pub(crate) fn plugin_candidate_app_connector_ids(root: &Path, manifest: &serde_json::Value) -> Vec<String> {
    let mut ids = BTreeSet::new();
    if let Some(value) = manifest.get("apps") {
        collect_app_connector_ids(root, value, &mut ids);
    } else {
        let default_path = root.join(".app.json");
        if let Some(value) = read_json_file(&default_path) {
            collect_app_connector_ids(root, &value, &mut ids);
        }
    }
    ids.into_iter().collect()
}


pub(crate) fn collect_app_connector_ids(root: &Path, value: &serde_json::Value, ids: &mut BTreeSet<String>) {
    if let Some(path) = value.as_str() {
        if let Ok(path) = resolve_plugin_manifest_path(root, path)
            && let Some(value) = read_json_file(&path)
        {
            collect_app_connector_ids(root, &value, ids);
        }
        return;
    }
    let Some(object) = value.as_object() else {
        return;
    };
    let apps_map = object
        .get("apps")
        .and_then(serde_json::Value::as_object)
        .unwrap_or(object);
    for value in apps_map.values() {
        let connector_id = value
            .get("id")
            .and_then(serde_json::Value::as_str)
            .or_else(|| value.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty());
        if let Some(connector_id) = connector_id {
            ids.insert(connector_id.to_string());
        }
    }
}


pub(crate) fn resolve_plugin_manifest_path(root: &Path, raw_path: &str) -> Result<PathBuf, String> {
    let Some(relative_path) = raw_path.trim().strip_prefix("./") else {
        return Err("plugin manifest path must start with ./".to_string());
    };
    if relative_path.is_empty() {
        return Err("plugin manifest path must not be empty".to_string());
    }

    let mut normalized = PathBuf::new();
    for component in Path::new(relative_path).components() {
        match component {
            Component::Normal(part) => normalized.push(part),
            Component::CurDir => {}
            Component::ParentDir => {
                return Err("plugin manifest path must not contain '..'".to_string());
            }
            _ => return Err("plugin manifest path must stay inside plugin root".to_string()),
        }
    }
    Ok(root.join(normalized))
}


pub(crate) fn read_json_file(path: &Path) -> Option<serde_json::Value> {
    let content = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}


pub(crate) fn format_apps_list_output(
    workspace_config_dir: &Path,
    mcp_tool_aliases: &HashMap<String, McpToolAlias>,
    connector_filter: Option<&str>,
    include_tools: bool,
) -> String {
    let apps = app_connector_list_entries(
        workspace_config_dir,
        mcp_tool_aliases,
        connector_filter,
        include_tools,
    );
    let accessible = apps.iter().filter(|app| app.accessible).count();
    let declared_by_plugins = apps
        .iter()
        .filter(|app| !app.plugin_apps.is_empty())
        .count();
    let mcp_tool_count: usize = apps.iter().map(|app| app.tools.len()).sum();
    serde_json::to_string_pretty(&serde_json::json!({
        "connectorId": connector_filter,
        "summary": {
            "total": apps.len(),
            "accessible": accessible,
            "declaredByPlugins": declared_by_plugins,
            "mcpTools": mcp_tool_count,
        },
        "apps": apps,
    }))
    .unwrap_or_default()
}


pub(crate) fn app_connector_list_entries(
    workspace_config_dir: &Path,
    mcp_tool_aliases: &HashMap<String, McpToolAlias>,
    connector_filter: Option<&str>,
    include_tools: bool,
) -> Vec<AppConnectorListEntry> {
    let mut entries = BTreeMap::<String, AppConnectorListEntry>::new();

    for app in plugin_loader::list_plugin_app_prompt_entries(workspace_config_dir) {
        if connector_filter.is_some_and(|filter| filter != app.connector_id) {
            continue;
        }
        let connector_id = app.connector_id.clone();
        let entry = entries
            .entry(connector_id.clone())
            .or_insert_with(|| AppConnectorListEntry {
                connector_id: connector_id.clone(),
                connector_name: None,
                accessible: false,
                source: "plugin".to_string(),
                install_url: app_connector_install_url(&app.app_key, &connector_id),
                plugin_apps: Vec::new(),
                tools: Vec::new(),
            });
        entry.plugin_apps.push(AppConnectorPluginSource {
            plugin_id: app.plugin_id,
            plugin_display_name: app.plugin_display_name,
            app_key: app.app_key,
        });
    }

    let mut aliases = mcp_tool_aliases.iter().collect::<Vec<_>>();
    aliases.sort_by(|left, right| left.0.cmp(right.0));
    for (alias, info) in aliases {
        let Some(connector_id) = info.connector.connector_id.as_deref() else {
            continue;
        };
        if connector_filter.is_some_and(|filter| filter != connector_id) {
            continue;
        }

        let entry =
            entries
                .entry(connector_id.to_string())
                .or_insert_with(|| AppConnectorListEntry {
                    connector_id: connector_id.to_string(),
                    connector_name: info.connector.connector_name.clone(),
                    accessible: false,
                    source: "mcp".to_string(),
                    install_url: app_connector_install_url(
                        info.connector
                            .connector_name
                            .as_deref()
                            .unwrap_or(connector_id),
                        connector_id,
                    ),
                    plugin_apps: Vec::new(),
                    tools: Vec::new(),
                });
        if entry.connector_name.is_none() {
            entry.connector_name = info.connector.connector_name.clone();
        }
        entry.accessible = true;
        if include_tools {
            entry.tools.push(AppConnectorToolEntry {
                name: alias.clone(),
                server: info.server.clone(),
                tool: info.tool.clone(),
                connector_name: info.connector.connector_name.clone(),
                namespace_description: info.connector.namespace_description.clone(),
            });
        }
    }

    let mut apps = entries.into_values().collect::<Vec<_>>();
    for app in &mut apps {
        app.plugin_apps.sort();
        app.plugin_apps.dedup();
        app.tools.sort();
        app.tools.dedup();
        app.source = match (app.plugin_apps.is_empty(), app.accessible) {
            (false, true) => "plugin+mcp".to_string(),
            (false, false) => "plugin".to_string(),
            (true, true) => "mcp".to_string(),
            (true, false) => "unknown".to_string(),
        };
    }
    apps.sort_by(|left, right| {
        left.accessible
            .cmp(&right.accessible)
            .reverse()
            .then(
                left.connector_name
                    .as_deref()
                    .unwrap_or("")
                    .cmp(right.connector_name.as_deref().unwrap_or("")),
            )
            .then(left.connector_id.cmp(&right.connector_id))
    });
    apps
}


pub(crate) fn app_connector_install_url(name: &str, connector_id: &str) -> String {
    let slug = connector_slug(name);
    format!("https://chatgpt.com/apps/{slug}/{connector_id}")
}


pub(crate) fn connector_slug(name: &str) -> String {
    let mut output = String::new();
    let mut last_dash = false;
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            output.push(ch.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash {
            output.push('-');
            last_dash = true;
        }
    }
    let output = output.trim_matches('-').to_string();
    if output.is_empty() {
        "app".to_string()
    } else {
        output
    }
}


impl ToolExecutor {
    pub(crate) async fn exec_apps_list(
        &self,
        arguments: &str,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
    ) -> AppResult<String> {
        let args: AppsListArgs = match serde_json::from_str(arguments) {
            Ok(args) => args,
            Err(e) => {
                let msg = format!("Invalid apps_list args: {e}");
                self.emit_tool_start(app_handle, thread_id, call_id, "apps_list", "invalid");
                self.emit_tool_end(app_handle, thread_id, call_id, "apps_list", -1, &msg);
                return Ok(msg);
            }
        };
        let connector_filter = args
            .connector_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let display = connector_filter.unwrap_or("all");
        self.emit_tool_start(app_handle, thread_id, call_id, "apps_list", display);

        let output = format_apps_list_output(
            &self.workspace_config_dir,
            &self.mcp_tool_aliases,
            connector_filter,
            args.include_tools.unwrap_or(true),
        );
        let output = truncate_output(&output, TOOL_OUTPUT_SEARCH_MAX_CHARS);
        self.emit_tool_end(app_handle, thread_id, call_id, "apps_list", 0, &output);
        Ok(output)
    }


    pub(crate) async fn exec_list_available_plugins_to_install(
        &self,
        arguments: &str,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
    ) -> AppResult<String> {
        let args: ListAvailablePluginsArgs = match serde_json::from_str(arguments) {
            Ok(args) => args,
            Err(e) => {
                let msg = format!("Invalid list_available_plugins_to_install args: {e}");
                self.emit_tool_start(
                    app_handle,
                    thread_id,
                    call_id,
                    "list_available_plugins_to_install",
                    "invalid",
                );
                self.emit_tool_end(
                    app_handle,
                    thread_id,
                    call_id,
                    "list_available_plugins_to_install",
                    -1,
                    &msg,
                );
                return Ok(msg);
            }
        };
        let display = args
            .query
            .as_deref()
            .map(str::trim)
            .filter(|query| !query.is_empty())
            .unwrap_or("all");
        self.emit_tool_start(
            app_handle,
            thread_id,
            call_id,
            "list_available_plugins_to_install",
            display,
        );

        let source_dir = match resolve_plugin_cache_source_dir(args.source_dir.as_deref()) {
            Ok(path) => path,
            Err(msg) => {
                self.emit_tool_end(
                    app_handle,
                    thread_id,
                    call_id,
                    "list_available_plugins_to_install",
                    -1,
                    &msg,
                );
                return Ok(msg);
            }
        };
        let candidates = plugin_install_candidates(
            &source_dir,
            &self.workspace_config_dir,
            args.query.as_deref(),
            args.include_installed.unwrap_or(true),
            args.limit.unwrap_or(50).clamp(1, 100),
        );
        let output = serde_json::to_string_pretty(&serde_json::json!({
            "sourceDir": source_dir,
            "tools": candidates,
        }))
        .unwrap_or_default();
        let output = truncate_output(&output, TOOL_OUTPUT_SEARCH_MAX_CHARS);
        self.emit_tool_end(
            app_handle,
            thread_id,
            call_id,
            "list_available_plugins_to_install",
            0,
            &output,
        );
        Ok(output)
    }


    pub(crate) async fn exec_request_plugin_install(
        &self,
        arguments: &str,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
    ) -> AppResult<String> {
        let args: RequestPluginInstallArgs = match serde_json::from_str(arguments) {
            Ok(args) => args,
            Err(e) => {
                let msg = format!("Invalid request_plugin_install args: {e}");
                self.emit_tool_start(
                    app_handle,
                    thread_id,
                    call_id,
                    "request_plugin_install",
                    "invalid",
                );
                self.emit_tool_end(
                    app_handle,
                    thread_id,
                    call_id,
                    "request_plugin_install",
                    -1,
                    &msg,
                );
                return Ok(msg);
            }
        };
        let display = args
            .tool_id
            .as_deref()
            .or(args.name.as_deref())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("plugin");
        self.emit_tool_start(
            app_handle,
            thread_id,
            call_id,
            "request_plugin_install",
            display,
        );

        if args
            .tool_type
            .as_deref()
            .is_some_and(|tool_type| tool_type != "plugin")
        {
            let msg =
                "request_plugin_install currently supports only tool_type=\"plugin\"".to_string();
            self.emit_tool_end(
                app_handle,
                thread_id,
                call_id,
                "request_plugin_install",
                -1,
                &msg,
            );
            return Ok(msg);
        }
        if args
            .action_type
            .as_deref()
            .is_some_and(|action_type| action_type != "install")
        {
            let msg = "request_plugin_install currently supports only action_type=\"install\""
                .to_string();
            self.emit_tool_end(
                app_handle,
                thread_id,
                call_id,
                "request_plugin_install",
                -1,
                &msg,
            );
            return Ok(msg);
        }

        let source_dir = match resolve_plugin_cache_source_dir(args.source_dir.as_deref()) {
            Ok(path) => path,
            Err(msg) => {
                self.emit_tool_end(
                    app_handle,
                    thread_id,
                    call_id,
                    "request_plugin_install",
                    -1,
                    &msg,
                );
                return Ok(msg);
            }
        };

        let candidates =
            plugin_install_candidates(&source_dir, &self.workspace_config_dir, None, true, 200);
        let selection = select_plugin_install_candidate(
            &candidates,
            args.tool_id.as_deref(),
            args.name.as_deref(),
        );
        let candidate = match selection {
            Ok(candidate) => candidate,
            Err(msg) => {
                self.emit_tool_end(
                    app_handle,
                    thread_id,
                    call_id,
                    "request_plugin_install",
                    -1,
                    &msg,
                );
                return Ok(msg);
            }
        };

        let import_result = plugin_commands::import_plugin_root(
            Path::new(&candidate.source),
            &self.workspace_config_dir.join("plugins"),
        );
        let output = match import_result {
            Ok(item) => serde_json::to_string_pretty(&serde_json::json!({
                "completed": true,
                "userConfirmed": true,
                "toolType": "plugin",
                "actionType": "install",
                "toolId": candidate.id,
                "toolName": candidate.name,
                "suggestReason": args.suggest_reason.unwrap_or_default(),
                "imported": item,
            }))
            .unwrap_or_default(),
            Err(error) => {
                let msg = format!("Failed to import plugin '{}': {error}", candidate.name);
                self.emit_tool_end(
                    app_handle,
                    thread_id,
                    call_id,
                    "request_plugin_install",
                    -1,
                    &msg,
                );
                return Ok(msg);
            }
        };
        self.emit_tool_end(
            app_handle,
            thread_id,
            call_id,
            "request_plugin_install",
            0,
            &output,
        );
        Ok(output)
    }


    pub(crate) async fn exec_plugin_manage(
        &self,
        arguments: &str,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
    ) -> AppResult<String> {
        let args: PluginManageArgs = match serde_json::from_str(arguments) {
            Ok(args) => args,
            Err(e) => {
                let msg = format!("Invalid plugin_manage args: {e}");
                self.emit_tool_start(app_handle, thread_id, call_id, "plugin_manage", "invalid");
                self.emit_tool_end(app_handle, thread_id, call_id, "plugin_manage", -1, &msg);
                return Ok(msg);
            }
        };
        let action = args
            .action
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("list");
        let plugin_id = args
            .plugin_id
            .as_deref()
            .or(args.id.as_deref())
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let display = plugin_id.unwrap_or(action);
        self.emit_tool_start(app_handle, thread_id, call_id, "plugin_manage", display);

        let output = match action {
            "list" => serde_json::to_string_pretty(&serde_json::json!({
                "plugins": plugin_loader::list_plugins(&self.workspace_config_dir),
            }))
            .unwrap_or_default(),
            "enable" | "disable" => {
                let Some(plugin_id) = plugin_id else {
                    let msg = format!("plugin_manage action '{action}' requires plugin_id");
                    self.emit_tool_end(app_handle, thread_id, call_id, "plugin_manage", -1, &msg);
                    return Ok(msg);
                };
                match plugin_loader::set_plugin_enabled(
                    &self.workspace_config_dir,
                    plugin_id,
                    action == "enable",
                ) {
                    Ok(plugin) => serde_json::to_string_pretty(&serde_json::json!({
                        "completed": true,
                        "action": action,
                        "plugin": plugin,
                    }))
                    .unwrap_or_default(),
                    Err(msg) => {
                        self.emit_tool_end(
                            app_handle,
                            thread_id,
                            call_id,
                            "plugin_manage",
                            -1,
                            &msg,
                        );
                        return Ok(msg);
                    }
                }
            }
            "uninstall" => {
                let Some(plugin_id) = plugin_id else {
                    let msg = "plugin_manage action 'uninstall' requires plugin_id".to_string();
                    self.emit_tool_end(app_handle, thread_id, call_id, "plugin_manage", -1, &msg);
                    return Ok(msg);
                };
                let removed_path = self
                    .workspace_config_dir
                    .join("plugins")
                    .join(plugin_id)
                    .to_string_lossy()
                    .to_string();
                match plugin_loader::uninstall_plugin(&self.workspace_config_dir, plugin_id) {
                    Ok(()) => serde_json::to_string_pretty(&serde_json::json!({
                        "completed": true,
                        "action": "uninstall",
                        "pluginId": plugin_id,
                        "removedPath": removed_path,
                    }))
                    .unwrap_or_default(),
                    Err(msg) => {
                        self.emit_tool_end(
                            app_handle,
                            thread_id,
                            call_id,
                            "plugin_manage",
                            -1,
                            &msg,
                        );
                        return Ok(msg);
                    }
                }
            }
            other => {
                let msg = format!(
                    "plugin_manage action must be one of list, enable, disable, or uninstall: {other}"
                );
                self.emit_tool_end(app_handle, thread_id, call_id, "plugin_manage", -1, &msg);
                return Ok(msg);
            }
        };

        let output = truncate_output(&output, TOOL_OUTPUT_SEARCH_MAX_CHARS);
        self.emit_tool_end(app_handle, thread_id, call_id, "plugin_manage", 0, &output);
        Ok(output)
    }


    pub(crate) async fn exec_mcp_manage(
        &self,
        arguments: &str,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
    ) -> AppResult<String> {
        let args: McpManageArgs = match serde_json::from_str(arguments) {
            Ok(args) => args,
            Err(e) => {
                let msg = format!("Invalid mcp_manage args: {e}");
                self.emit_tool_start(app_handle, thread_id, call_id, "mcp_manage", "invalid");
                self.emit_tool_end(app_handle, thread_id, call_id, "mcp_manage", -1, &msg);
                return Ok(msg);
            }
        };

        let action = args
            .action
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("list");
        let server_name = args
            .name
            .as_deref()
            .or(args.server.as_deref())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        let display = server_name.as_deref().unwrap_or(action);
        self.emit_tool_start(app_handle, thread_id, call_id, "mcp_manage", display);

        let config_path = self.workspace_config_dir.join("config.toml");
        let mut config = match ConfigToml::load(&config_path) {
            Ok(config) => config,
            Err(error) => {
                let msg = format!("Failed to load codey/config.toml: {error}");
                self.emit_tool_end(app_handle, thread_id, call_id, "mcp_manage", -1, &msg);
                return Ok(msg);
            }
        };

        let output = match action {
            "list" => {
                let mut servers: Vec<_> = config
                    .resolved_mcp_servers()
                    .into_values()
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
                serde_json::to_string_pretty(&serde_json::json!({
                    "servers": servers,
                    "configPath": config_path.to_string_lossy(),
                    "source": "codey/config.toml",
                }))
                .unwrap_or_default()
            }
            "install" | "add" => {
                let Some(name) = server_name.as_deref() else {
                    let msg = "mcp_manage install requires name".to_string();
                    self.emit_tool_end(app_handle, thread_id, call_id, "mcp_manage", -1, &msg);
                    return Ok(msg);
                };
                if let Err(error) = sanitize_mcp_server_name(name) {
                    self.emit_tool_end(app_handle, thread_id, call_id, "mcp_manage", -1, &error);
                    return Ok(error);
                }
                let overwrite = args.overwrite.unwrap_or(false);
                if config.mcp_servers.contains_key(name) && !overwrite {
                    let msg = format!(
                        "MCP server '{name}' already exists in codey/config.toml. Pass overwrite=true to replace it."
                    );
                    self.emit_tool_end(app_handle, thread_id, call_id, "mcp_manage", -1, &msg);
                    return Ok(msg);
                }

                let server_value = match build_mcp_server_config_value(&args) {
                    Ok(value) => value,
                    Err(error) => {
                        self.emit_tool_end(
                            app_handle,
                            thread_id,
                            call_id,
                            "mcp_manage",
                            -1,
                            &error,
                        );
                        return Ok(error);
                    }
                };

                if let Err(error) = config.apply_edit(&format!("mcp_servers.{name}"), &server_value)
                {
                    let msg = format!("Failed to install MCP server '{name}': {error}");
                    self.emit_tool_end(app_handle, thread_id, call_id, "mcp_manage", -1, &msg);
                    return Ok(msg);
                }
                if let Err(error) = config.save(&config_path) {
                    let msg = format!("Failed to save codey/config.toml: {error}");
                    self.emit_tool_end(app_handle, thread_id, call_id, "mcp_manage", -1, &msg);
                    return Ok(msg);
                }

                emit_mcp_servers_changed(
                    app_handle,
                    name,
                    action,
                    config_path.to_string_lossy().as_ref(),
                );
                serde_json::to_string_pretty(&serde_json::json!({
                    "completed": true,
                    "action": action,
                    "name": name,
                    "server": server_value,
                    "configPath": config_path.to_string_lossy(),
                    "uiVisible": true,
                    "message": format!("MCP server '{name}' installed into codey/config.toml and Settings > Integration can now show it."),
                }))
                .unwrap_or_default()
            }
            "enable" | "disable" => {
                let Some(name) = server_name.as_deref() else {
                    let msg = format!("mcp_manage action '{action}' requires name");
                    self.emit_tool_end(app_handle, thread_id, call_id, "mcp_manage", -1, &msg);
                    return Ok(msg);
                };
                let Some(existing) = config.mcp_servers.get(name).cloned() else {
                    let msg = format!("MCP server not found in codey/config.toml: {name}");
                    self.emit_tool_end(app_handle, thread_id, call_id, "mcp_manage", -1, &msg);
                    return Ok(msg);
                };
                let mut table = match existing.as_table().cloned() {
                    Some(table) => table,
                    None => {
                        let msg = format!("MCP server '{name}' config is invalid");
                        self.emit_tool_end(app_handle, thread_id, call_id, "mcp_manage", -1, &msg);
                        return Ok(msg);
                    }
                };
                table.remove("isActive");
                table.remove("is_active");
                table.remove("enabled");
                table.insert(
                    "disabled".to_string(),
                    toml::Value::Boolean(action == "disable"),
                );
                let next = toml_table_to_json_object(&table);
                if let Err(error) = config.apply_edit(&format!("mcp_servers.{name}"), &next) {
                    let msg = format!("Failed to {action} MCP server '{name}': {error}");
                    self.emit_tool_end(app_handle, thread_id, call_id, "mcp_manage", -1, &msg);
                    return Ok(msg);
                }
                if let Err(error) = config.save(&config_path) {
                    let msg = format!("Failed to save codey/config.toml: {error}");
                    self.emit_tool_end(app_handle, thread_id, call_id, "mcp_manage", -1, &msg);
                    return Ok(msg);
                }
                emit_mcp_servers_changed(
                    app_handle,
                    name,
                    action,
                    config_path.to_string_lossy().as_ref(),
                );
                serde_json::to_string_pretty(&serde_json::json!({
                    "completed": true,
                    "action": action,
                    "name": name,
                    "disabled": action == "disable",
                    "configPath": config_path.to_string_lossy(),
                }))
                .unwrap_or_default()
            }
            "uninstall" | "remove" => {
                let Some(name) = server_name.as_deref() else {
                    let msg = "mcp_manage uninstall requires name".to_string();
                    self.emit_tool_end(app_handle, thread_id, call_id, "mcp_manage", -1, &msg);
                    return Ok(msg);
                };
                if !config.mcp_servers.contains_key(name) {
                    let msg = format!("MCP server not found in codey/config.toml: {name}");
                    self.emit_tool_end(app_handle, thread_id, call_id, "mcp_manage", -1, &msg);
                    return Ok(msg);
                }
                if let Err(error) =
                    config.apply_edit(&format!("mcp_servers.{name}"), &serde_json::Value::Null)
                {
                    let msg = format!("Failed to uninstall MCP server '{name}': {error}");
                    self.emit_tool_end(app_handle, thread_id, call_id, "mcp_manage", -1, &msg);
                    return Ok(msg);
                }
                if let Err(error) = config.save(&config_path) {
                    let msg = format!("Failed to save codey/config.toml: {error}");
                    self.emit_tool_end(app_handle, thread_id, call_id, "mcp_manage", -1, &msg);
                    return Ok(msg);
                }
                emit_mcp_servers_changed(
                    app_handle,
                    name,
                    "uninstall",
                    config_path.to_string_lossy().as_ref(),
                );
                serde_json::to_string_pretty(&serde_json::json!({
                    "completed": true,
                    "action": "uninstall",
                    "name": name,
                    "configPath": config_path.to_string_lossy(),
                }))
                .unwrap_or_default()
            }
            other => {
                let msg = format!(
                    "mcp_manage action must be one of list, install, add, enable, disable, uninstall, or remove: {other}"
                );
                self.emit_tool_end(app_handle, thread_id, call_id, "mcp_manage", -1, &msg);
                return Ok(msg);
            }
        };

        let output = truncate_output(&output, TOOL_OUTPUT_SEARCH_MAX_CHARS);
        self.emit_tool_end(app_handle, thread_id, call_id, "mcp_manage", 0, &output);
        Ok(output)
    }


    pub(crate) async fn exec_skill_manage(
        &self,
        arguments: &str,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
    ) -> AppResult<String> {
        let args: SkillManageArgs = match serde_json::from_str(arguments) {
            Ok(args) => args,
            Err(e) => {
                let msg = format!("Invalid skill_manage args: {e}");
                self.emit_tool_start(app_handle, thread_id, call_id, "skill_manage", "invalid");
                self.emit_tool_end(app_handle, thread_id, call_id, "skill_manage", -1, &msg);
                return Ok(msg);
            }
        };

        let action = args
            .action
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("list");
        let skill_id = args
            .skill_id
            .as_deref()
            .or(args.id.as_deref())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        let display = skill_id.as_deref().unwrap_or(action);
        self.emit_tool_start(app_handle, thread_id, call_id, "skill_manage", display);

        let skills_dir = self.workspace_config_dir.join("skills");

        let output = match action {
            "list" => {
                let skills = list_local_skills(&skills_dir);
                serde_json::to_string_pretty(&serde_json::json!({
                    "skills": skills,
                    "skillsDir": skills_dir.to_string_lossy(),
                }))
                .unwrap_or_default()
            }
            "install" | "create" | "update" => {
                let Some(skill_id) = skill_id.as_deref() else {
                    let msg = format!("skill_manage action '{action}' requires skill_id");
                    self.emit_tool_end(app_handle, thread_id, call_id, "skill_manage", -1, &msg);
                    return Ok(msg);
                };
                if let Err(error) = sanitize_skill_manage_id(skill_id) {
                    self.emit_tool_end(app_handle, thread_id, call_id, "skill_manage", -1, &error);
                    return Ok(error);
                }

                let skill_dir = skills_dir.join(skill_id);
                let skill_md = skill_dir.join("SKILL.md");
                let exists = skill_md.is_file();
                let overwrite = args.overwrite.unwrap_or(matches!(action, "update"));
                if exists && !overwrite && matches!(action, "install" | "create") {
                    let msg = format!(
                        "Skill '{skill_id}' already exists. Pass overwrite=true or use action=update."
                    );
                    self.emit_tool_end(app_handle, thread_id, call_id, "skill_manage", -1, &msg);
                    return Ok(msg);
                }
                if !exists && action == "update" {
                    let msg = format!(
                        "Skill '{skill_id}' does not exist. Use action=install to create it."
                    );
                    self.emit_tool_end(app_handle, thread_id, call_id, "skill_manage", -1, &msg);
                    return Ok(msg);
                }

                let content = match build_skill_manage_content(&args, skill_id) {
                    Ok(content) => content,
                    Err(error) => {
                        self.emit_tool_end(
                            app_handle,
                            thread_id,
                            call_id,
                            "skill_manage",
                            -1,
                            &error,
                        );
                        return Ok(error);
                    }
                };
                if content.chars().count() > MAX_SKILL_MANAGE_CONTENT_CHARS {
                    let msg = format!(
                        "Skill content too large (max {MAX_SKILL_MANAGE_CONTENT_CHARS} characters)"
                    );
                    self.emit_tool_end(app_handle, thread_id, call_id, "skill_manage", -1, &msg);
                    return Ok(msg);
                }

                if let Err(error) = std::fs::create_dir_all(&skill_dir) {
                    let msg = format!("Failed to create skill directory: {error}");
                    self.emit_tool_end(app_handle, thread_id, call_id, "skill_manage", -1, &msg);
                    return Ok(msg);
                }
                if let Err(error) = std::fs::write(&skill_md, &content) {
                    let msg = format!("Failed to write SKILL.md: {error}");
                    self.emit_tool_end(app_handle, thread_id, call_id, "skill_manage", -1, &msg);
                    return Ok(msg);
                }

                let (name, description, tags) = parse_tool_search_skill_frontmatter(&content);
                emit_skills_changed(app_handle, skill_id, action);
                serde_json::to_string_pretty(&serde_json::json!({
                    "completed": true,
                    "action": action,
                    "skillId": skill_id,
                    "name": if name.is_empty() { skill_id.to_string() } else { name },
                    "description": description,
                    "tags": tags,
                    "path": skill_md.to_string_lossy(),
                    "uiVisible": true,
                    "message": format!("Skill '{skill_id}' saved under codey/skills and Settings > Skills can now show it."),
                }))
                .unwrap_or_default()
            }
            "uninstall" | "remove" => {
                let Some(skill_id) = skill_id.as_deref() else {
                    let msg = "skill_manage uninstall requires skill_id".to_string();
                    self.emit_tool_end(app_handle, thread_id, call_id, "skill_manage", -1, &msg);
                    return Ok(msg);
                };
                if let Err(error) = sanitize_skill_manage_id(skill_id) {
                    self.emit_tool_end(app_handle, thread_id, call_id, "skill_manage", -1, &error);
                    return Ok(error);
                }
                let skill_dir = skills_dir.join(skill_id);
                if !skill_dir.exists() {
                    let msg = format!("Skill not found: {skill_id}");
                    self.emit_tool_end(app_handle, thread_id, call_id, "skill_manage", -1, &msg);
                    return Ok(msg);
                }
                if let Err(error) = std::fs::remove_dir_all(&skill_dir) {
                    let msg = format!("Failed to uninstall skill '{skill_id}': {error}");
                    self.emit_tool_end(app_handle, thread_id, call_id, "skill_manage", -1, &msg);
                    return Ok(msg);
                }
                emit_skills_changed(app_handle, skill_id, "uninstall");
                serde_json::to_string_pretty(&serde_json::json!({
                    "completed": true,
                    "action": "uninstall",
                    "skillId": skill_id,
                    "removedPath": skill_dir.to_string_lossy(),
                }))
                .unwrap_or_default()
            }
            other => {
                let msg = format!(
                    "skill_manage action must be one of list, install, create, update, uninstall, or remove: {other}"
                );
                self.emit_tool_end(app_handle, thread_id, call_id, "skill_manage", -1, &msg);
                return Ok(msg);
            }
        };

        let output = truncate_output(&output, TOOL_OUTPUT_SEARCH_MAX_CHARS);
        self.emit_tool_end(app_handle, thread_id, call_id, "skill_manage", 0, &output);
        Ok(output)
    }


    pub(crate) async fn exec_robot_save(
        &self,
        arguments: &str,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
    ) -> AppResult<String> {
        #[derive(Deserialize)]
        struct RobotSaveArgs {
            id: Option<String>,
            config: Option<serde_json::Value>,
        }

        let args: RobotSaveArgs = match serde_json::from_str(arguments) {
            Ok(a) => a,
            Err(e) => {
                let msg = format!("Invalid robot_save args: {e}");
                self.emit_tool_start(app_handle, thread_id, call_id, "robot_save", "invalid");
                self.emit_tool_end(app_handle, thread_id, call_id, "robot_save", -1, &msg);
                return Ok(msg);
            }
        };

        let robot_id = args
            .id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("unnamed-robot");

        self.emit_tool_start(app_handle, thread_id, call_id, "robot_save", robot_id);

        let Some(config_value) = args.config else {
            let msg = "robot_save requires a config object".to_string();
            self.emit_tool_end(app_handle, thread_id, call_id, "robot_save", -1, &msg);
            return Ok(msg);
        };

        let mut config: crate::robot_loader::RobotConfig =
            match serde_json::from_value(config_value) {
                Ok(c) => c,
                Err(e) => {
                    let msg = format!("Invalid robot config: {e}");
                    self.emit_tool_end(app_handle, thread_id, call_id, "robot_save", -1, &msg);
                    return Ok(msg);
                }
            };

        // 强约束校验：workflow 必须可归一化为节点，且每个节点都要绑定至少一个 skill。
        // 这里做服务端兜底，即使模型未严格遵循 schema，也能及时返回清晰错误。
        let normalized_nodes = config.normalized_workflow_nodes();
        if normalized_nodes.is_empty() {
            let msg =
                "Invalid robot config: workflowNodes must contain at least one node".to_string();
            self.emit_tool_end(app_handle, thread_id, call_id, "robot_save", -1, &msg);
            return Ok(msg);
        }
        if let Some((idx, _)) = normalized_nodes
            .iter()
            .enumerate()
            .find(|(_, node)| node.skills.is_empty() && node.plugin_skills.is_empty())
        {
            let msg = format!(
                "Invalid robot config: workflowNodes[{idx}] must assign at least one local or plugin skill"
            );
            self.emit_tool_end(app_handle, thread_id, call_id, "robot_save", -1, &msg);
            return Ok(msg);
        }

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        if config.created_at == 0 {
            config.created_at = now;
        }
        config.updated_at = now;

        match crate::robot_loader::save_robot(&self.workspace_config_dir, robot_id, &config) {
            Ok(detail) => {
                let output = serde_json::to_string_pretty(&serde_json::json!({
                    "status": "created",
                    "robotId": detail.id,
                    "name": detail.config.name,
                    "path": detail.path,
                    "skillsCount": detail.config.all_local_skills().len() + detail.config.all_plugin_skills().len(),
                    "workflowSteps": detail.config.normalized_workflow_nodes().len(),
                }))
                .unwrap_or_default();

                app_handle
                    .emit(
                        "robot-created",
                        serde_json::json!({
                            "threadId": thread_id,
                            "robotId": detail.id,
                            "name": detail.config.name,
                        }),
                    )
                    .ok();

                self.emit_tool_end(app_handle, thread_id, call_id, "robot_save", 0, &output);
                Ok(output)
            }
            Err(msg) => {
                self.emit_tool_end(app_handle, thread_id, call_id, "robot_save", -1, &msg);
                Ok(msg)
            }
        }
    }

}
