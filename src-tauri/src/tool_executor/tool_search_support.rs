use super::*;

pub(crate) fn tool_search_entry_from_function_spec(
    spec: &serde_json::Value,
    kind: &str,
    source: &str,
    path: Option<String>,
) -> Option<ToolSearchEntry> {
    let function = spec.get("function")?;
    let name = function.get("name")?.as_str()?.trim();
    if name.is_empty() {
        return None;
    }
    let description = function
        .get("description")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();

    Some(ToolSearchEntry {
        kind: kind.to_string(),
        name: name.to_string(),
        description,
        source: source.to_string(),
        path,
        spec: Some(spec.clone()),
        metadata: BTreeMap::new(),
        usage: Some("Call this function tool directly by name.".to_string()),
    })
}


/// Prefer short relative skill paths in tool_search results.
pub(crate) fn skill_search_path(workspace_config_dir: &Path, skill_md: &Path) -> String {
    let workspace_root = workspace_config_dir
        .parent()
        .unwrap_or(workspace_config_dir);
    skill_md
        .strip_prefix(workspace_root)
        .or_else(|_| skill_md.strip_prefix(workspace_config_dir))
        .map(|relative| relative.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| skill_md.to_string_lossy().replace('\\', "/"))
}


pub(crate) fn local_skill_search_entries(workspace_config_dir: &Path) -> Vec<ToolSearchEntry> {
    let skills_dir = workspace_config_dir.join("skills");
    let Ok(entries) = std::fs::read_dir(&skills_dir) else {
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
        let display_name = if name.is_empty() { id } else { name };
        let mut description_parts = Vec::new();
        if !description.is_empty() {
            description_parts.push(description);
        }
        if !tags.is_empty() {
            description_parts.push(format!("tags: {}", tags.join(", ")));
        }
        skills.push(ToolSearchEntry {
            kind: "skill".to_string(),
            name: display_name,
            description: description_parts.join(" | "),
            source: "codey/skills".to_string(),
            path: Some(skill_search_path(workspace_config_dir, &skill_md)),
            spec: None,
            metadata: BTreeMap::new(),
            usage: Some("Read this skill's SKILL.md before applying it.".to_string()),
        });
    }
    skills.sort_by(|left, right| left.name.cmp(&right.name));
    skills
}


pub(crate) fn plugin_skill_search_entries(workspace_config_dir: &Path) -> Vec<ToolSearchEntry> {
    let mut entries = plugin_loader::list_plugin_skill_prompt_entries(workspace_config_dir)
        .into_iter()
        .map(|skill| ToolSearchEntry {
            kind: "skill".to_string(),
            name: format!("{}: {}", skill.plugin_display_name, skill.skill_name),
            description: skill.description,
            source: format!("plugin:{}", skill.plugin_id),
            path: Some(skill_search_path(workspace_config_dir, &skill.path)),
            spec: None,
            metadata: BTreeMap::new(),
            usage: Some("Read this plugin skill's SKILL.md before applying it.".to_string()),
        })
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| left.name.cmp(&right.name));
    entries
}


pub(crate) fn plugin_app_search_entries(workspace_config_dir: &Path) -> Vec<ToolSearchEntry> {
    let mut entries = plugin_loader::list_plugin_app_prompt_entries(workspace_config_dir)
        .into_iter()
        .map(|app| {
            let mut metadata = BTreeMap::new();
            metadata.insert("pluginId".to_string(), app.plugin_id.clone());
            metadata.insert(
                "pluginDisplayName".to_string(),
                app.plugin_display_name.clone(),
            );
            metadata.insert("appKey".to_string(), app.app_key.clone());
            metadata.insert("connectorId".to_string(), app.connector_id.clone());

            ToolSearchEntry {
                kind: "app".to_string(),
                name: format!("{}: {}", app.plugin_display_name, app.app_key),
                description: format!(
                    "Plugin app connector `{}` with connector id `{}`.",
                    app.app_key, app.connector_id
                ),
                source: format!("plugin:{}", app.plugin_id),
                path: None,
                spec: None,
                metadata,
                usage: Some(
                    "Use this app connector only through matching MCP tools/resources when they are exposed; do not invent app actions or data."
                        .to_string(),
                ),
            }
        })
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| left.name.cmp(&right.name));
    entries
}


pub(crate) fn parse_tool_search_skill_frontmatter(content: &str) -> (String, String, Vec<String>) {
    let mut name = String::new();
    let mut description = String::new();
    let mut tags = Vec::new();

    if !content.starts_with("---") {
        return (name, description, tags);
    }
    let Some(end) = content[3..].find("---") else {
        return (name, description, tags);
    };

    let frontmatter = &content[3..3 + end];
    for line in frontmatter.lines() {
        let line = line.trim();
        if let Some(val) = line.strip_prefix("name:") {
            name = val.trim().trim_matches('"').to_string();
        } else if let Some(val) = line.strip_prefix("description:") {
            description = val.trim().trim_matches('"').to_string();
        } else if let Some(val) = line.strip_prefix("tags:") {
            let val = val.trim();
            if val.starts_with('[') {
                tags = val
                    .trim_matches(|ch| ch == '[' || ch == ']')
                    .split(',')
                    .map(|tag| tag.trim().trim_matches('"').to_string())
                    .filter(|tag| !tag.is_empty())
                    .collect();
            }
        }
    }

    (name, description, tags)
}


pub(crate) fn search_tool_entries(
    entries: Vec<ToolSearchEntry>,
    query: &str,
    limit: usize,
) -> Vec<ToolSearchEntry> {
    let query = query.trim();
    if query.is_empty() {
        return Vec::new();
    }
    let show_all = matches!(
        query.to_ascii_lowercase().as_str(),
        "*" | "all" | "list all" | "tools"
    );

    if show_all {
        let mut entries = entries;
        entries.sort_by(|left, right| {
            left.kind
                .cmp(&right.kind)
                .then_with(|| left.name.cmp(&right.name))
        });
        entries.truncate(limit);
        return entries;
    }

    let documents = entries
        .iter()
        .map(tool_search_document_text)
        .map(|text| tokenize_tool_search_query(&text))
        .collect::<Vec<_>>();
    let query_tokens = unique_tool_search_tokens(tokenize_tool_search_query(query));
    if query_tokens.is_empty() {
        return Vec::new();
    }
    let inverse_document_frequencies = tool_search_inverse_document_frequencies(&documents);
    let average_document_length = if documents.is_empty() {
        0.0
    } else {
        documents
            .iter()
            .map(|document| document.len() as f64)
            .sum::<f64>()
            / documents.len() as f64
    };

    let mut scored = entries
        .into_iter()
        .enumerate()
        .filter_map(|(index, entry)| {
            let score = tool_search_score(
                query,
                &query_tokens,
                documents.get(index).map(Vec::as_slice).unwrap_or_default(),
                average_document_length,
                &inverse_document_frequencies,
                &entry,
            );
            (score > 0.0).then_some((score, entry))
        })
        .collect::<Vec<_>>();
    scored.sort_by(|(left_score, left), (right_score, right)| {
        right_score
            .total_cmp(left_score)
            .then_with(|| left.kind.cmp(&right.kind))
            .then_with(|| left.name.cmp(&right.name))
    });

    scored
        .into_iter()
        .take(limit)
        .map(|(_, entry)| entry)
        .collect()
}


pub(crate) fn tool_search_document_text(entry: &ToolSearchEntry) -> String {
    let mut parts = Vec::new();
    push_tool_search_part(&mut parts, &entry.kind);
    push_tool_search_part(&mut parts, &entry.name);
    push_tool_search_part(&mut parts, &entry.name.replace('_', " "));
    push_tool_search_part(&mut parts, &entry.description);
    push_tool_search_part(&mut parts, &entry.source);
    if let Some(path) = entry.path.as_deref() {
        push_tool_search_part(&mut parts, path);
    }
    if let Some(usage) = entry.usage.as_deref() {
        push_tool_search_part(&mut parts, usage);
    }
    for (key, value) in &entry.metadata {
        push_tool_search_part(&mut parts, key);
        push_tool_search_part(&mut parts, value);
    }
    if let Some(spec) = &entry.spec {
        append_tool_spec_search_text(spec, &mut parts);
    }
    parts.join(" ")
}


pub(crate) fn append_tool_spec_search_text(spec: &serde_json::Value, parts: &mut Vec<String>) {
    let Some(function) = spec.get("function") else {
        return;
    };
    if let Some(name) = function.get("name").and_then(serde_json::Value::as_str) {
        push_tool_search_part(parts, name);
        push_tool_search_part(parts, &name.replace('_', " "));
    }
    if let Some(description) = function
        .get("description")
        .and_then(serde_json::Value::as_str)
    {
        push_tool_search_part(parts, description);
    }
    if let Some(parameters) = function.get("parameters") {
        append_json_schema_search_text(parameters, parts);
    }
}


pub(crate) fn append_json_schema_search_text(schema: &serde_json::Value, parts: &mut Vec<String>) {
    let Some(object) = schema.as_object() else {
        return;
    };

    for key in ["title", "description", "$comment"] {
        if let Some(value) = object.get(key).and_then(serde_json::Value::as_str) {
            push_tool_search_part(parts, value);
        }
    }

    if let Some(properties) = object
        .get("properties")
        .and_then(serde_json::Value::as_object)
    {
        for (name, property_schema) in properties {
            push_tool_search_part(parts, name);
            push_tool_search_part(parts, &name.replace('_', " "));
            append_json_schema_search_text(property_schema, parts);
        }
    }

    if let Some(required) = object.get("required").and_then(serde_json::Value::as_array) {
        for item in required {
            if let Some(name) = item.as_str() {
                push_tool_search_part(parts, name);
            }
        }
    }

    if let Some(enum_values) = object.get("enum").and_then(serde_json::Value::as_array) {
        for item in enum_values {
            if let Some(value) = item.as_str() {
                push_tool_search_part(parts, value);
            }
        }
    }

    for key in ["items", "additionalProperties"] {
        if let Some(value) = object.get(key) {
            append_json_schema_search_text(value, parts);
        }
    }

    for key in ["anyOf", "oneOf", "allOf"] {
        if let Some(variants) = object.get(key).and_then(serde_json::Value::as_array) {
            for variant in variants {
                append_json_schema_search_text(variant, parts);
            }
        }
    }
}


pub(crate) fn push_tool_search_part(parts: &mut Vec<String>, part: &str) {
    let part = part.trim();
    if !part.is_empty() {
        parts.push(part.to_string());
    }
}


pub(crate) fn tool_search_score(
    raw_query: &str,
    query_tokens: &[String],
    document_tokens: &[String],
    average_document_length: f64,
    inverse_document_frequencies: &HashMap<String, f64>,
    entry: &ToolSearchEntry,
) -> f64 {
    let mut score = tool_search_bm25_score(
        query_tokens,
        document_tokens,
        average_document_length,
        inverse_document_frequencies,
    );

    score += tool_search_exact_match_boost(raw_query, entry);
    score
}


pub(crate) fn tool_search_bm25_score(
    query_tokens: &[String],
    document_tokens: &[String],
    average_document_length: f64,
    inverse_document_frequencies: &HashMap<String, f64>,
) -> f64 {
    if document_tokens.is_empty() || average_document_length <= 0.0 {
        return 0.0;
    }

    let mut frequencies: HashMap<&str, usize> = HashMap::new();
    for token in document_tokens {
        *frequencies.entry(token.as_str()).or_insert(0) += 1;
    }

    const K1: f64 = 1.5;
    const B: f64 = 0.75;
    let document_length = document_tokens.len() as f64;
    let length_norm = K1 * (1.0 - B + B * document_length / average_document_length);

    query_tokens.iter().fold(0.0, |score, token| {
        let Some(term_frequency) = frequencies.get(token.as_str()).copied() else {
            return score;
        };
        let Some(idf) = inverse_document_frequencies.get(token) else {
            return score;
        };
        let term_frequency = term_frequency as f64;
        score + idf * (term_frequency * (K1 + 1.0)) / (term_frequency + length_norm)
    })
}


pub(crate) fn tool_search_inverse_document_frequencies(documents: &[Vec<String>]) -> HashMap<String, f64> {
    let document_count = documents.len() as f64;
    let mut document_frequencies: HashMap<String, usize> = HashMap::new();
    for document in documents {
        let mut seen = BTreeSet::new();
        for token in document {
            if seen.insert(token) {
                *document_frequencies.entry(token.clone()).or_insert(0) += 1;
            }
        }
    }

    document_frequencies
        .into_iter()
        .map(|(token, frequency)| {
            let frequency = frequency as f64;
            let idf = (1.0 + (document_count - frequency + 0.5) / (frequency + 0.5)).ln();
            (token, idf)
        })
        .collect()
}


pub(crate) fn tool_search_exact_match_boost(query: &str, entry: &ToolSearchEntry) -> f64 {
    let metadata_text = entry
        .metadata
        .iter()
        .map(|(key, value)| format!("{key} {value}"))
        .collect::<Vec<_>>()
        .join(" ");
    let haystack = format!(
        "{} {} {} {} {} {}",
        entry.kind,
        entry.name,
        entry.description,
        entry.source,
        entry.path.as_deref().unwrap_or_default(),
        metadata_text
    )
    .to_ascii_lowercase();
    let name = entry.name.to_ascii_lowercase();
    let query_lower = normalize_tool_search_phrase(query);
    let tokens = tokenize_tool_search_query(&query_lower);
    if tokens.is_empty() {
        return 0.0;
    }

    let mut score = 0.0;
    if name == query_lower {
        score += 8.0;
    }
    if name.contains(&query_lower) {
        score += 3.5;
    }
    if haystack.contains(&query_lower) {
        score += 2.0;
    }
    for token in tokens {
        if name.split(['_', '-', ' ', ':']).any(|part| part == token) {
            score += 1.6;
        } else if name.contains(&token) {
            score += 1.0;
        }
        if entry.description.to_ascii_lowercase().contains(&token) {
            score += 0.7;
        }
        if entry.source.to_ascii_lowercase().contains(&token) {
            score += 0.35;
        }
        if metadata_text.to_ascii_lowercase().contains(&token) {
            score += 0.45;
        }
        if entry
            .path
            .as_deref()
            .unwrap_or_default()
            .to_ascii_lowercase()
            .contains(&token)
        {
            score += 0.25;
        }
    }
    score
}


pub(crate) fn tokenize_tool_search_query(query: &str) -> Vec<String> {
    expand_tool_search_identifiers(query)
        .split(|ch: char| !ch.is_alphanumeric())
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(|part| part.to_ascii_lowercase())
        .collect()
}


pub(crate) fn unique_tool_search_tokens(tokens: Vec<String>) -> Vec<String> {
    let mut seen = BTreeSet::new();
    tokens
        .into_iter()
        .filter(|token| seen.insert(token.clone()))
        .collect()
}


pub(crate) fn normalize_tool_search_phrase(value: &str) -> String {
    tokenize_tool_search_query(value).join(" ")
}


pub(crate) fn expand_tool_search_identifiers(value: &str) -> String {
    let mut output = String::with_capacity(value.len() + 8);
    let mut previous: Option<char> = None;
    for ch in value.chars() {
        if matches!(
            ch,
            '_' | '-' | ':' | '/' | '\\' | '.' | '(' | ')' | '[' | ']'
        ) {
            output.push(' ');
            previous = None;
            continue;
        }

        if let Some(prev) = previous {
            if (ch.is_ascii_uppercase() && (prev.is_ascii_lowercase() || prev.is_ascii_digit()))
                || (ch.is_ascii_digit() && prev.is_ascii_alphabetic())
                || (ch.is_ascii_alphabetic() && prev.is_ascii_digit())
            {
                output.push(' ');
            }
        }
        output.push(ch);
        previous = Some(ch);
    }
    output
}


pub(crate) fn format_tool_search_output(query: &str, matches: Vec<ToolSearchEntry>) -> String {
    let mut output_matches = Vec::new();
    let mut loadable_tools = Vec::new();
    for entry in &matches {
        let mut item = BTreeMap::new();
        item.insert("type".to_string(), serde_json::json!(entry.kind.clone()));
        item.insert("name".to_string(), serde_json::json!(entry.name.clone()));
        item.insert(
            "description".to_string(),
            serde_json::json!(entry.description.clone()),
        );
        item.insert(
            "source".to_string(),
            serde_json::json!(entry.source.clone()),
        );
        if !entry.metadata.is_empty() {
            item.insert(
                "metadata".to_string(),
                serde_json::json!(entry.metadata.clone()),
            );
        }
        if let Some(usage) = entry.usage.clone() {
            item.insert("usage".to_string(), serde_json::json!(usage));
        }
        if let Some(path) = entry.path.clone() {
            item.insert("path".to_string(), serde_json::json!(path));
        }
        if let Some(spec) = entry.spec.clone() {
            item.insert("spec".to_string(), spec.clone());
            if let Some(tool) = tool_search_loadable_tool(entry, &spec) {
                loadable_tools.push(tool);
            }
        }
        output_matches.push(serde_json::Value::Object(item.into_iter().collect()));
    }

    serde_json::to_string_pretty(&serde_json::json!({
        "query": query,
        "matches": output_matches,
        "tools": coalesce_tool_search_loadable_tools(loadable_tools),
    }))
    .unwrap_or_default()
}


pub(crate) fn tool_search_loadable_tool(
    entry: &ToolSearchEntry,
    spec: &serde_json::Value,
) -> Option<serde_json::Value> {
    let function = spec.get("function")?;
    let full_name = function.get("name")?.as_str()?;
    let response_tool = response_tool_from_function_spec(spec)?;

    let Some((namespace, local_name)) = mcp_tool_namespace_and_name(full_name) else {
        return Some(response_tool);
    };
    let mut local_tool = response_tool;
    local_tool["name"] = serde_json::Value::String(local_name.to_string());
    local_tool["defer_loading"] = serde_json::Value::Bool(true);

    let description = entry
        .metadata
        .get("namespaceDescription")
        .cloned()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| format!("Tools in the {namespace} namespace."));

    Some(serde_json::json!({
        "type": "namespace",
        "name": namespace,
        "description": description,
        "tools": [local_tool],
    }))
}


pub(crate) fn response_tool_from_function_spec(spec: &serde_json::Value) -> Option<serde_json::Value> {
    let function = spec.get("function")?;
    let name = function.get("name")?.as_str()?;
    let mut response_tool = serde_json::json!({
        "type": "function",
        "name": name,
        "description": function
            .get("description")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default(),
        "strict": false,
        "parameters": function
            .get("parameters")
            .cloned()
            .unwrap_or_else(|| serde_json::json!({})),
    });
    if let Some(defer_loading) = spec
        .get("defer_loading")
        .and_then(serde_json::Value::as_bool)
    {
        response_tool["defer_loading"] = serde_json::Value::Bool(defer_loading);
    }
    Some(response_tool)
}


impl ToolExecutor {
    pub(crate) fn tool_search_entries(&self) -> Vec<ToolSearchEntry> {
        let mut entries = Vec::new();
        for spec in self.tool_specs(self.web_search_enabled) {
            let Some(entry) = tool_search_entry_from_function_spec(&spec, "tool", "built-in", None)
            else {
                continue;
            };
            if entry.name == "tool_search" {
                continue;
            }
            entries.push(entry);
        }

        let mut mcp_aliases = self.mcp_tool_specs.keys().cloned().collect::<Vec<_>>();
        mcp_aliases.sort();
        for alias in mcp_aliases {
            let Some(spec) = self.mcp_tool_specs.get(&alias) else {
                continue;
            };
            let source = self
                .mcp_tool_aliases
                .get(&alias)
                .map(|entry| format!("mcp:{}", entry.server))
                .unwrap_or_else(|| "mcp".to_string());
            if let Some(mut entry) =
                tool_search_entry_from_function_spec(spec, "tool", &source, None)
            {
                if let Some(alias_info) = self.mcp_tool_aliases.get(&alias) {
                    entry
                        .metadata
                        .insert("server".to_string(), alias_info.server.clone());
                    entry
                        .metadata
                        .insert("tool".to_string(), alias_info.tool.clone());
                    if let Some(connector_id) = alias_info.connector.connector_id.clone() {
                        entry
                            .metadata
                            .insert("connectorId".to_string(), connector_id);
                    }
                    if let Some(connector_name) = alias_info.connector.connector_name.clone() {
                        entry
                            .metadata
                            .insert("connectorName".to_string(), connector_name.clone());
                        entry.usage = Some(format!(
                            "Call this function tool directly by name. This MCP tool belongs to the {connector_name} app connector."
                        ));
                    }
                    if let Some(description) = alias_info.connector.namespace_description.clone() {
                        entry
                            .metadata
                            .insert("namespaceDescription".to_string(), description);
                    }
                }
                entries.push(entry);
            }
        }

        // A configured server name is enough for tool discovery. Do not call
        // tools/list here: this offline index lets the model activate the
        // generic MCP tools through tool_search without starting any server.
        let mut configured_servers = self.enabled_mcp_servers();
        configured_servers.sort_by(|left, right| left.name.cmp(&right.name));
        for server in configured_servers {
            let mut metadata = BTreeMap::new();
            metadata.insert("server".to_string(), server.name.clone());
            metadata.insert("transport".to_string(), server.transport.clone());
            entries.push(ToolSearchEntry {
                kind: "mcp_server".to_string(),
                name: format!("MCP server: {}", server.name),
                description: format!(
                    "Configured MCP server '{}'. Activate it only when the request needs its capability; then list tools for this server.",
                    server.name
                ),
                source: format!("mcp:{}", server.name),
                path: None,
                spec: None,
                metadata,
                usage: Some(format!(
                    "The next model call can use mcp_list_tools or mcp_call_tool with server=\"{}\". This server is not connected until one of those tools is called.",
                    server.name
                )),
            });
        }

        entries.extend(local_skill_search_entries(&self.workspace_config_dir));
        entries.extend(plugin_skill_search_entries(&self.workspace_config_dir));
        entries.extend(plugin_app_search_entries(&self.workspace_config_dir));
        entries
    }


    pub(crate) async fn exec_tool_search(
        &self,
        arguments: &str,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
        turn_id: Option<&str>,
    ) -> AppResult<String> {
        let args: ToolSearchArgs = match serde_json::from_str(arguments) {
            Ok(args) => args,
            Err(e) => {
                let msg = format!("Invalid tool_search args: {e}");
                self.emit_tool_start(app_handle, thread_id, call_id, "tool_search", "invalid");
                self.emit_tool_end(app_handle, thread_id, call_id, "tool_search", -1, &msg);
                return Ok(msg);
            }
        };
        let query = args.query.trim();
        self.emit_tool_start(app_handle, thread_id, call_id, "tool_search", query);

        if query.is_empty() {
            let msg = "tool_search query must not be empty".to_string();
            self.emit_tool_end(app_handle, thread_id, call_id, "tool_search", -1, &msg);
            return Ok(msg);
        }

        let limit = args.limit.unwrap_or(8).clamp(1, 50);
        let matches = search_tool_entries(self.tool_search_entries(), query, limit);
        let mut activated_names = BTreeSet::new();
        for entry in &matches {
            if entry.kind == "tool"
                && !Self::is_core_tool_name(&entry.name)
                && self.subagent_tools_allowed(&entry.name)
            {
                activated_names.insert(entry.name.clone());
            }
            if entry.kind == "mcp_server" {
                activated_names.insert("mcp_list_tools".to_string());
                activated_names.insert("mcp_call_tool".to_string());
            }
        }
        let activated_names: Vec<String> = activated_names.into_iter().collect();
        let activated_count = activated_names.len();
        if activated_count > 0 {
            self.activate_tools_for_thread(thread_id, activated_names.iter().cloned())
                .await;
        }
        let skill_match_count = matches.iter().filter(|entry| entry.kind == "skill").count();
        let mcp_match_count = matches
            .iter()
            .filter(|entry| entry.source.starts_with("mcp:"))
            .count();
        crate::agent::emit_and_broadcast(
            app_handle,
            "turn-loading",
            serde_json::json!({
                "threadId": thread_id,
                "turnId": turn_id,
                "callId": call_id,
                "kind": if mcp_match_count > 0 { "mcp" } else { "skill" },
                "phase": "activation",
                "status": "completed",
                "query": query,
                "skillMatchCount": skill_match_count,
                "mcpMatchCount": mcp_match_count,
                "activatedCount": activated_count,
                "activatedTools": activated_names.clone(),
            }),
        );
        let output = format_tool_search_output(query, matches);
        let output = truncate_output(&output, TOOL_OUTPUT_SEARCH_MAX_CHARS);
        let output = if activated_count > 0 {
            format!(
                "{output}\n\n[activated {activated_count} non-core tool schema(s) for the next model call in this turn: {}]",
                activated_names.join(", ")
            )
        } else {
            output
        };
        self.emit_tool_end(app_handle, thread_id, call_id, "tool_search", 0, &output);
        Ok(output)
    }

}
