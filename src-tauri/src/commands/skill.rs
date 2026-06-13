use serde::Serialize;
use std::path::PathBuf;
use tauri::State;

use crate::error::{AppError, AppResult};
use crate::state::AppState;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillSummary {
    pub id: String,
    pub name: String,
    pub description: String,
    pub tags: Vec<String>,
    pub path: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillDetail {
    pub id: String,
    pub name: String,
    pub description: String,
    pub tags: Vec<String>,
    pub path: String,
    pub content: String,
    pub enabled: bool,
}

fn get_skills_dir(state: &AppState) -> PathBuf {
    state.workspace_config_dir.join("skills")
}

fn parse_skill_frontmatter(content: &str) -> (String, String, Vec<String>) {
    let mut name = String::new();
    let mut description = String::new();
    let mut tags = Vec::new();

    if content.starts_with("---") {
        if let Some(end) = content[3..].find("---") {
            let front = &content[3..3 + end];
            for line in front.lines() {
                let line = line.trim();
                if let Some(val) = line.strip_prefix("name:") {
                    name = val.trim().trim_matches('"').to_string();
                } else if let Some(val) = line.strip_prefix("description:") {
                    description = val.trim().trim_matches('"').to_string();
                } else if let Some(val) = line.strip_prefix("tags:") {
                    let val = val.trim();
                    if val.starts_with('[') {
                        tags = val
                            .trim_matches(|c| c == '[' || c == ']')
                            .split(',')
                            .map(|s| s.trim().trim_matches('"').to_string())
                            .filter(|s| !s.is_empty())
                            .collect();
                    }
                }
            }
        }
    }

    (name, description, tags)
}

#[tauri::command]
pub async fn skill_list(state: State<'_, AppState>) -> AppResult<Vec<SkillSummary>> {
    let skills_dir = get_skills_dir(&state);

    if !skills_dir.exists() {
        return Ok(vec![]);
    }

    let disabled_skills = state.config_manager.read()?.disabled_skills.clone();

    let mut skills = Vec::new();
    let entries = std::fs::read_dir(&skills_dir)
        .map_err(|e| AppError::Custom(format!("Failed to read skills dir: {e}")))?;

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let skill_md = path.join("SKILL.md");
        if !skill_md.exists() {
            continue;
        }

        let id = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();

        let content = std::fs::read_to_string(&skill_md).unwrap_or_default();
        let (name, description, tags) = parse_skill_frontmatter(&content);

        skills.push(SkillSummary {
            name: if name.is_empty() { id.clone() } else { name },
            description,
            tags,
            path: skill_md.to_string_lossy().to_string(),
            enabled: !disabled_skills.contains(&id),
            id,
        });
    }

    skills.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(skills)
}

#[tauri::command]
pub async fn skill_read(state: State<'_, AppState>, skill_id: String) -> AppResult<SkillDetail> {
    let skills_dir = get_skills_dir(&state);
    let skill_md = skills_dir.join(&skill_id).join("SKILL.md");

    if !skill_md.exists() {
        return Err(AppError::Custom(format!("Skill not found: {skill_id}")));
    }

    let content = std::fs::read_to_string(&skill_md)
        .map_err(|e| AppError::Custom(format!("Failed to read skill: {e}")))?;

    let (name, description, tags) = parse_skill_frontmatter(&content);

    let enabled = !state
        .config_manager
        .read()?
        .disabled_skills
        .contains(&skill_id);

    Ok(SkillDetail {
        name: if name.is_empty() {
            skill_id.clone()
        } else {
            name
        },
        description,
        tags,
        path: skill_md.to_string_lossy().to_string(),
        id: skill_id,
        content,
        enabled,
    })
}

#[tauri::command]
pub async fn skill_set_enabled(
    state: State<'_, AppState>,
    skill_id: String,
    enabled: bool,
) -> AppResult<()> {
    let mut config = state.config_manager.read()?;
    if enabled {
        config.disabled_skills.retain(|id| id != &skill_id);
    } else if !config.disabled_skills.contains(&skill_id) {
        config.disabled_skills.push(skill_id);
    }
    config.save(&state.config_path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_full_frontmatter() {
        let content = r#"---
name: "Browser Harness"
description: "Automate browser interactions"
tags: ["browser", "automation", "testing"]
---
# Some content
"#;
        let (name, desc, tags) = parse_skill_frontmatter(content);
        assert_eq!(name, "Browser Harness");
        assert_eq!(desc, "Automate browser interactions");
        assert_eq!(tags, vec!["browser", "automation", "testing"]);
    }

    #[test]
    fn parse_no_frontmatter() {
        let content = "# Just a heading\nSome text";
        let (name, desc, tags) = parse_skill_frontmatter(content);
        assert!(name.is_empty());
        assert!(desc.is_empty());
        assert!(tags.is_empty());
    }

    #[test]
    fn parse_partial_frontmatter_name_only() {
        let content = "---\nname: My Skill\n---\nBody";
        let (name, desc, tags) = parse_skill_frontmatter(content);
        assert_eq!(name, "My Skill");
        assert!(desc.is_empty());
        assert!(tags.is_empty());
    }

    #[test]
    fn parse_empty_tags_array() {
        let content = "---\nname: Test\ntags: []\n---\n";
        let (name, _desc, tags) = parse_skill_frontmatter(content);
        assert_eq!(name, "Test");
        assert!(tags.is_empty());
    }

    #[test]
    fn parse_quoted_values() {
        let content = "---\nname: \"Quoted Name\"\ndescription: \"A quoted desc\"\n---\n";
        let (name, desc, _tags) = parse_skill_frontmatter(content);
        assert_eq!(name, "Quoted Name");
        assert_eq!(desc, "A quoted desc");
    }
}
