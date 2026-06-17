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
    pub workflow_nodes: Vec<WorkflowNode>,
    #[serde(default)]
    pub system_prompt: String,
    #[serde(default)]
    pub created_at: i64,
    #[serde(default)]
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
pub struct PluginSkillRef {
    pub plugin_id: String,
    pub skill_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowNode {
    /// 节点目标：描述本节点需要完成的核心任务。
    #[serde(default)]
    pub objective: String,
    /// 本节点绑定的本地技能（codey/skills）。
    #[serde(default)]
    pub skills: Vec<String>,
    /// 本节点绑定的插件技能（codey/plugins/<plugin>/skills）。
    #[serde(default)]
    pub plugin_skills: Vec<PluginSkillRef>,
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

impl RobotConfig {
    /// 归一化 workflow 节点，兼容旧版 `workflow: string[]`。
    /// - 新版优先：存在 `workflow_nodes` 时以其为准。
    /// - 旧版兜底：将 `workflow` 字符串步骤映射为节点，并继承顶层技能集合。
    /// - 节点缺失技能时，自动继承顶层技能，避免旧配置在强约束模式下失效。
    pub fn normalized_workflow_nodes(&self) -> Vec<WorkflowNode> {
        let fallback_local_skills = dedupe_skill_ids(&self.skills);
        let fallback_plugin_skills = dedupe_plugin_skill_refs(&self.plugin_skills);

        if !self.workflow_nodes.is_empty() {
            return self
                .workflow_nodes
                .iter()
                .filter_map(|node| {
                    let objective = node.objective.trim().to_string();
                    if objective.is_empty() {
                        return None;
                    }
                    let local_skills = if node.skills.is_empty() {
                        fallback_local_skills.clone()
                    } else {
                        dedupe_skill_ids(&node.skills)
                    };
                    let plugin_skills = if node.plugin_skills.is_empty() {
                        fallback_plugin_skills.clone()
                    } else {
                        dedupe_plugin_skill_refs(&node.plugin_skills)
                    };
                    Some(WorkflowNode {
                        objective,
                        skills: local_skills,
                        plugin_skills,
                    })
                })
                .collect();
        }

        self.workflow
            .iter()
            .filter_map(|step| {
                let objective = step.trim();
                if objective.is_empty() {
                    return None;
                }
                Some(WorkflowNode {
                    objective: objective.to_string(),
                    skills: fallback_local_skills.clone(),
                    plugin_skills: fallback_plugin_skills.clone(),
                })
            })
            .collect()
    }

    /// 返回兼容后的 workflow 文本步骤（用于旧 UI 展示与统计）。
    pub fn normalized_workflow_steps(&self) -> Vec<String> {
        let nodes = self.normalized_workflow_nodes();
        if !nodes.is_empty() {
            return nodes.into_iter().map(|node| node.objective).collect();
        }
        self.workflow
            .iter()
            .map(|step| step.trim().to_string())
            .filter(|step| !step.is_empty())
            .collect()
    }

    /// 返回机器人关联的所有本地技能（顶层 + 节点级并集，去重）。
    pub fn all_local_skills(&self) -> Vec<String> {
        let mut merged = self.skills.clone();
        for node in self.normalized_workflow_nodes() {
            merged.extend(node.skills);
        }
        dedupe_skill_ids(&merged)
    }

    /// 返回机器人关联的所有插件技能（顶层 + 节点级并集，去重）。
    pub fn all_plugin_skills(&self) -> Vec<PluginSkillRef> {
        let mut merged = self.plugin_skills.clone();
        for node in self.normalized_workflow_nodes() {
            merged.extend(node.plugin_skills);
        }
        dedupe_plugin_skill_refs(&merged)
    }

    /// 将配置写回兼容态：
    /// - `workflow_nodes` 持久化为标准结构；
    /// - `workflow` 同步为纯文本步骤，兼容旧前端与旧数据读取路径；
    /// - 顶层 skills/pluginSkills 去重，避免重复项不断膨胀。
    pub fn ensure_workflow_compatibility(&mut self) {
        let normalized_nodes = self.normalized_workflow_nodes();
        self.workflow = normalized_nodes
            .iter()
            .map(|node| node.objective.clone())
            .collect();
        self.workflow_nodes = normalized_nodes;
        self.skills = dedupe_skill_ids(&self.skills);
        self.plugin_skills = dedupe_plugin_skill_refs(&self.plugin_skills);
    }
}

fn dedupe_skill_ids(values: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut deduped = Vec::new();
    for value in values {
        let normalized = value.trim();
        if normalized.is_empty() {
            continue;
        }
        let key = normalized.to_string();
        if seen.insert(key.clone()) {
            deduped.push(key);
        }
    }
    deduped
}

fn dedupe_plugin_skill_refs(values: &[PluginSkillRef]) -> Vec<PluginSkillRef> {
    let mut seen = HashSet::new();
    let mut deduped = Vec::new();
    for value in values {
        let plugin_id = value.plugin_id.trim();
        let skill_id = value.skill_id.trim();
        if plugin_id.is_empty() || skill_id.is_empty() {
            continue;
        }
        let key = format!("{plugin_id}/{skill_id}");
        if seen.insert(key) {
            deduped.push(PluginSkillRef {
                plugin_id: plugin_id.to_string(),
                skill_id: skill_id.to_string(),
            });
        }
    }
    deduped
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
            let mut config: RobotConfig = serde_json::from_str(&content).ok()?;
            // 列表阶段也执行一次兼容归一化，确保统计数据稳定。
            config.ensure_workflow_compatibility();
            let skills_count = config.all_local_skills().len();
            let plugin_skills_count = config.all_plugin_skills().len();
            let workflow_steps = config.normalized_workflow_nodes().len();
            Some(RobotSummary {
                id,
                name: config.name,
                description: config.description,
                icon: config.icon,
                skills_count,
                plugin_skills_count,
                workflow_steps,
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
    let mut config: RobotConfig = serde_json::from_str(&content).ok()?;
    // 读详情时补齐 workflowNodes，避免旧配置在前端展示为空。
    config.ensure_workflow_compatibility();
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

    let mut normalized_config = config.clone();
    // 保存前统一归一化，保证磁盘结构长期一致且兼容旧字段。
    normalized_config.ensure_workflow_compatibility();

    let config_file = robot_dir.join("robot.json");
    let json = serde_json::to_string_pretty(&normalized_config)
        .map_err(|e| format!("Failed to serialize robot config: {e}"))?;
    std::fs::write(&config_file, &json)
        .map_err(|e| format!("Failed to write robot config: {e}"))?;

    Ok(RobotDetail {
        id: robot_id.to_string(),
        path: config_file.to_string_lossy().to_string(),
        config: normalized_config,
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
    std::fs::remove_dir_all(&robot_dir).map_err(|e| format!("Failed to delete robot: {e}"))
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
    let local_skills = config.all_local_skills();
    let plugin_skills = config.all_plugin_skills();
    collect_skill_contents_by_refs(workspace_config_dir, &local_skills, &plugin_skills)
}

/// Collect the full SKILL.md contents for one normalized workflow node.
pub fn collect_robot_node_skill_contents(
    workspace_config_dir: &Path,
    config: &RobotConfig,
    node_index: usize,
) -> Vec<(String, String)> {
    let nodes = config.normalized_workflow_nodes();
    let Some(node) = nodes.get(node_index) else {
        return Vec::new();
    };
    collect_skill_contents_by_refs(workspace_config_dir, &node.skills, &node.plugin_skills)
}

fn collect_skill_contents_by_refs(
    workspace_config_dir: &Path,
    local_skills: &[String],
    plugin_skills: &[PluginSkillRef],
) -> Vec<(String, String)> {
    let mut results = Vec::new();
    let mut seen = HashSet::new();

    for skill_id in local_skills {
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

    for ps in plugin_skills {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn plugin_skill(plugin_id: &str, skill_id: &str) -> PluginSkillRef {
        PluginSkillRef {
            plugin_id: plugin_id.to_string(),
            skill_id: skill_id.to_string(),
        }
    }

    fn base_config() -> RobotConfig {
        RobotConfig {
            name: "test".to_string(),
            description: String::new(),
            icon: String::new(),
            skills: Vec::new(),
            plugin_skills: Vec::new(),
            workflow: Vec::new(),
            workflow_nodes: Vec::new(),
            system_prompt: "system".to_string(),
            created_at: 0,
            updated_at: 0,
        }
    }

    #[test]
    fn normalized_nodes_fallback_to_legacy_workflow_with_global_skills() {
        let mut config = base_config();
        config.skills = vec!["local-a".to_string()];
        config.plugin_skills = vec![plugin_skill("browser", "control")];
        config.workflow = vec!["step one".to_string(), "step two".to_string()];

        let nodes = config.normalized_workflow_nodes();
        assert_eq!(nodes.len(), 2);
        assert_eq!(nodes[0].objective, "step one");
        assert_eq!(nodes[1].objective, "step two");
        assert_eq!(nodes[0].skills, vec!["local-a".to_string()]);
        assert_eq!(nodes[1].skills, vec!["local-a".to_string()]);
        assert_eq!(
            nodes[0].plugin_skills,
            vec![plugin_skill("browser", "control")]
        );
    }

    #[test]
    fn normalized_nodes_keep_node_specific_skills() {
        let mut config = base_config();
        config.skills = vec!["global-skill".to_string()];
        config.workflow_nodes = vec![WorkflowNode {
            objective: "node objective".to_string(),
            skills: vec!["node-skill".to_string()],
            plugin_skills: vec![plugin_skill("documents", "documents")],
        }];

        let nodes = config.normalized_workflow_nodes();
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].skills, vec!["node-skill".to_string()]);
        assert_eq!(
            nodes[0].plugin_skills,
            vec![plugin_skill("documents", "documents")]
        );
    }

    #[test]
    fn ensure_workflow_compatibility_populates_legacy_workflow_and_nodes() {
        let mut config = base_config();
        config.workflow_nodes = vec![
            WorkflowNode {
                objective: "alpha".to_string(),
                skills: vec!["s1".to_string()],
                plugin_skills: Vec::new(),
            },
            WorkflowNode {
                objective: "beta".to_string(),
                skills: Vec::new(),
                plugin_skills: vec![plugin_skill("browser", "control")],
            },
        ];

        config.ensure_workflow_compatibility();
        assert_eq!(
            config.workflow,
            vec!["alpha".to_string(), "beta".to_string()]
        );
        assert_eq!(config.workflow_nodes.len(), 2);
        assert_eq!(config.workflow_nodes[0].objective, "alpha");
        assert_eq!(config.workflow_nodes[1].objective, "beta");
    }
}
