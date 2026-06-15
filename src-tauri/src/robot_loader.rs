use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};

use crate::plugin_loader;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RobotConfig {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub icon: String,
    #[serde(default)]
    pub skills: Vec<String>,
    #[serde(default)]
    pub plugin_skills: Vec<PluginSkillRef>,
    #[serde(default)]
    pub workflow: Vec<String>,
    #[serde(default)]
    pub system_prompt: String,
    #[serde(default)]
    pub created_at: i64,
    #[serde(default)]
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginSkillRef {
    pub plugin_id: String,
    pub skill_id: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RobotSummary {
    pub id: String,
    pub name: String,
    pub description: String,
    pub icon: String,
    pub skills_count: usize,
    pub plugin_skills_count: usize,
    pub workflow_steps: usize,
    pub path: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RobotDetail {
    pub id: String,
    #[serde(flatten)]
    pub config: RobotConfig,
    pub path: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AvailableSkillEntry {
    pub id: String,
    pub name: String,
    pub description: String,
    pub source: String,
    pub plugin_id: Option<String>,
}

fn robots_dir(workspace_config_dir: &Path) -> PathBuf {
    workspace_config_dir.join("robots")
}

fn is_safe_robot_id(robot_id: &str) -> bool {
    if robot_id.trim().is_empty() {
        return false;
    }
    let mut components = Path::new(robot_id).components();
    matches!(components.next(), Some(Component::Normal(_))) && components.next().is_none()
}

pub fn list_robots(workspace_config_dir: &Path) -> Vec<RobotSummary> {
    let dir = robots_dir(workspace_config_dir);
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };

    let mut robots: Vec<RobotSummary> = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            if !path.is_dir() {
                return None;
            }
            let config_file = path.join("robot.json");
            if !config_file.is_file() {
                return None;
            }
            let id = path.file_name()?.to_string_lossy().to_string();
            let content = std::fs::read_to_string(&config_file).ok()?;
            let config: RobotConfig = serde_json::from_str(&content).ok()?;
            Some(RobotSummary {
                id,
                name: config.name,
                description: config.description,
                icon: config.icon,
                skills_count: config.skills.len(),
                plugin_skills_count: config.plugin_skills.len(),
                workflow_steps: config.workflow.len(),
                path: config_file.to_string_lossy().to_string(),
            })
        })
        .collect();

    robots.sort_by(|a, b| a.id.cmp(&b.id));
    robots
}

pub fn read_robot(workspace_config_dir: &Path, robot_id: &str) -> Option<RobotDetail> {
    if !is_safe_robot_id(robot_id) {
        return None;
    }
    let config_file = robots_dir(workspace_config_dir)
        .join(robot_id)
        .join("robot.json");
    if !config_file.is_file() {
        return None;
    }
    let content = std::fs::read_to_string(&config_file).ok()?;
    let config: RobotConfig = serde_json::from_str(&content).ok()?;
    Some(RobotDetail {
        id: robot_id.to_string(),
        path: config_file.to_string_lossy().to_string(),
        config,
    })
}

pub fn save_robot(
    workspace_config_dir: &Path,
    robot_id: &str,
    config: &RobotConfig,
) -> Result<RobotDetail, String> {
    if !is_safe_robot_id(robot_id) {
        return Err(format!("Invalid robot id: {robot_id}"));
    }
    let robot_dir = robots_dir(workspace_config_dir).join(robot_id);
    std::fs::create_dir_all(&robot_dir)
        .map_err(|e| format!("Failed to create robot directory: {e}"))?;

    let config_file = robot_dir.join("robot.json");
    let json = serde_json::to_string_pretty(config)
        .map_err(|e| format!("Failed to serialize robot config: {e}"))?;
    std::fs::write(&config_file, &json)
        .map_err(|e| format!("Failed to write robot config: {e}"))?;

    Ok(RobotDetail {
        id: robot_id.to_string(),
        path: config_file.to_string_lossy().to_string(),
        config: config.clone(),
    })
}

pub fn delete_robot(workspace_config_dir: &Path, robot_id: &str) -> Result<(), String> {
    if !is_safe_robot_id(robot_id) {
        return Err(format!("Invalid robot id: {robot_id}"));
    }
    let robot_dir = robots_dir(workspace_config_dir).join(robot_id);
    if !robot_dir.exists() {
        return Err(format!("Robot not found: {robot_id}"));
    }
    std::fs::remove_dir_all(&robot_dir)
        .map_err(|e| format!("Failed to delete robot: {e}"))
}

pub fn list_all_available_skills(workspace_config_dir: &Path) -> Vec<AvailableSkillEntry> {
    let mut entries = Vec::new();

    let skills_dir = workspace_config_dir.join("skills");
    if let Ok(dir_entries) = std::fs::read_dir(&skills_dir) {
        for entry in dir_entries.flatten() {
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
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            let content = std::fs::read_to_string(&skill_md).unwrap_or_default();
            let (name, description) = parse_skill_name_desc(&content);
            entries.push(AvailableSkillEntry {
                name: if name.is_empty() { id.clone() } else { name },
                description,
                id,
                source: "local".to_string(),
                plugin_id: None,
            });
        }
    }

    for plugin_skill in plugin_loader::list_plugin_skill_prompt_entries(workspace_config_dir) {
        entries.push(AvailableSkillEntry {
            id: plugin_skill.skill_name.clone(),
            name: plugin_skill.skill_name,
            description: plugin_skill.description,
            source: "plugin".to_string(),
            plugin_id: Some(plugin_skill.plugin_id),
        });
    }

    entries.sort_by(|a, b| a.source.cmp(&b.source).then(a.id.cmp(&b.id)));
    entries
}

/// Collect the full SKILL.md contents for all skills referenced by a robot.
pub fn collect_robot_skill_contents(
    workspace_config_dir: &Path,
    config: &RobotConfig,
) -> Vec<(String, String)> {
    let mut results = Vec::new();
    let mut seen = HashSet::new();

    for skill_id in &config.skills {
        let skill_md = workspace_config_dir
            .join("skills")
            .join(skill_id)
            .join("SKILL.md");
        if skill_md.is_file() {
            if let Ok(content) = std::fs::read_to_string(&skill_md) {
                if seen.insert(format!("local:{skill_id}")) {
                    results.push((skill_id.clone(), content));
                }
            }
        }
    }

    for ps in &config.plugin_skills {
        let plugins_dir = workspace_config_dir.join("plugins");
        let Ok(plugin_entries) = std::fs::read_dir(&plugins_dir) else {
            continue;
        };
        for entry in plugin_entries.flatten() {
            let plugin_path = entry.path();
            if !plugin_path.is_dir() {
                continue;
            }
            let plugin_id = plugin_path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            if plugin_id != ps.plugin_id {
                continue;
            }
            let skill_md = plugin_path
                .join("skills")
                .join(&ps.skill_id)
                .join("SKILL.md");
            if skill_md.is_file() {
                if let Ok(content) = std::fs::read_to_string(&skill_md) {
                    let key = format!("plugin:{}:{}", ps.plugin_id, ps.skill_id);
                    if seen.insert(key) {
                        let label = format!("{}/{}", ps.plugin_id, ps.skill_id);
                        results.push((label, content));
                    }
                }
            }
        }
    }

    results
}

fn parse_skill_name_desc(content: &str) -> (String, String) {
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
