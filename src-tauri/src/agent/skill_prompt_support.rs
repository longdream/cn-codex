use super::*;

pub(crate) fn render_plugin_apps_prompt_for_config_dir(config_dir: &Path) -> String {
    let mut apps = plugin_loader::list_plugin_app_prompt_entries(config_dir)
        .into_iter()
        .map(|app| {
            format!(
                "- {}: app `{}` connector `{}` (plugin `{}`)",
                app.plugin_display_name, app.app_key, app.connector_id, app.plugin_id
            )
        })
        .collect::<Vec<_>>();

    if apps.is_empty() {
        return String::new();
    }

    apps.sort();
    apps.dedup();
    let mut body = String::from(
        "\n\n## Apps (Connectors)\n\
         Apps (Connectors) can be explicitly triggered in user messages in the format `[$app-name](app://{connector_id})`. Apps can also be implicitly triggered when the context suggests using an available app.\n\
         An app is equivalent to a set of MCP tools within the `codex-apps` MCP server.\n\
         Installed app tools are not attached by default; lazy-load them through `tool_search` before calling. Use `apps_list` only after activating it via `tool_search` to inspect installed connector IDs and currently exposed trusted codex-apps MCP tools.\n\
         For apps, prefer `tool_search` then the matching MCP tools; do not additionally call `mcp_list_resources` or `mcp_list_resource_templates` to discover app capabilities, and do not invent app data or actions that are not exposed by tools.\n\
         Available plugin app connectors:\n",
    );
    let mut total_chars = body.chars().count();
    let mut omitted = 0usize;
    for line in apps {
        let next_chars = total_chars
            .saturating_add(line.chars().count())
            .saturating_add(1);
        if next_chars > 4_000 {
            omitted = omitted.saturating_add(1);
            continue;
        }
        body.push_str(&line);
        body.push('\n');
        total_chars = next_chars;
    }
    if omitted > 0 {
        body.push_str(&format!(
            "- {omitted} additional app connectors omitted from this bounded list.\n"
        ));
    }
    body
}


pub(crate) fn parse_skill_prompt_frontmatter(content: &str) -> (String, String) {
    let mut name = String::new();
    let mut description = String::new();

    if !content.starts_with("---") {
        return (name, description);
    }

    let Some(end) = content[3..].find("---") else {
        return (name, description);
    };

    let frontmatter = &content[3..3 + end];
    for line in frontmatter.lines() {
        let line = line.trim();
        if let Some(val) = line.strip_prefix("name:") {
            name = val.trim().trim_matches('"').to_string();
        } else if let Some(val) = line.strip_prefix("description:") {
            description = val.trim().trim_matches('"').to_string();
        }
    }

    (name, description)
}


/// Prefer workspace-relative skill paths in the system prompt to keep catalog tokens small.
pub(crate) fn skill_prompt_path(cwd: &Path, skill_md: &Path) -> String {
    skill_md
        .strip_prefix(cwd)
        .map(|relative| relative.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| skill_md.to_string_lossy().replace('\\', "/"))
}


/// Skills withheld from the always-on "Available skills" prompt catalog.
/// They stay discoverable via `tool_search` and load only when the user
/// explicitly selects them, avoiding automatic activation at session start.
const DEFERRED_SKILLS: &[&str] = &["using-superpowers"];


pub(crate) fn is_deferred_skill(name: &str) -> bool {
    DEFERRED_SKILLS
        .iter()
        .any(|deferred| name.trim().eq_ignore_ascii_case(deferred))
}


/// Prefer high-frequency coding skills in the always-on prompt catalog.
/// Remaining skills stay discoverable via `tool_search`.
pub(crate) fn skill_prompt_priority_score(name: &str, description: &str, source: &str) -> i32 {
    let haystack = format!("{name} {description}").to_ascii_lowercase();
    let mut score = match source {
        "local" => 20,
        "plugin" => 10,
        "workflow" => 5,
        _ => 0,
    };

    const BOOSTS: &[(&str, i32)] = &[
        ("brainstorming", 70),
        ("writing-plans", 70),
        ("executing-plans", 65),
        ("test-driven-development", 65),
        ("systematic-debugging", 65),
        ("verification-before-completion", 60),
        ("requesting-code-review", 55),
        ("receiving-code-review", 55),
        ("code-review", 50),
        ("browser", 45),
        ("documents", 40),
        ("presentations", 35),
        ("spreadsheets", 35),
        ("sites-building", 30),
        ("sites-hosting", 30),
        ("computer-use", 25),
        ("smartbrain", 25),
    ];
    for (needle, boost) in BOOSTS {
        if haystack.contains(needle) {
            score += boost;
        }
    }
    score
}


pub(crate) fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}


