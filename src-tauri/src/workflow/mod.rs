pub mod commands;
pub mod extractor;
pub mod prompts;
pub mod skill_gen;

use std::path::{Path, PathBuf};

use serde::{Deserialize, Deserializer, Serialize};

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum StringLikeValue {
    String(String),
    Number(serde_json::Number),
    Bool(bool),
}

impl StringLikeValue {
    fn into_string(self) -> String {
        match self {
            Self::String(value) => value,
            Self::Number(value) => value.to_string(),
            Self::Bool(value) => value.to_string(),
        }
    }
}

fn deserialize_string_like<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let value = StringLikeValue::deserialize(deserializer)?;
    Ok(value.into_string())
}

fn deserialize_option_string_like<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<StringLikeValue>::deserialize(deserializer)?;
    Ok(value.map(StringLikeValue::into_string))
}

fn deserialize_vec_string_like<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let values = Vec::<StringLikeValue>::deserialize(deserializer)?;
    Ok(values
        .into_iter()
        .map(StringLikeValue::into_string)
        .collect())
}

/// Root directory for workflow data: `codey/workflows/`.
pub fn workflows_dir(workspace_config_dir: &Path) -> PathBuf {
    workspace_config_dir.join("workflows")
}

/// A single variable declaration in a workflow.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowVariable {
    #[serde(rename = "type")]
    pub var_type: String,
    pub description: String,
    #[serde(
        default,
        deserialize_with = "deserialize_option_string_like",
        skip_serializing_if = "Option::is_none"
    )]
    pub default: Option<String>,
}

/// A single node/step in a workflow.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowNode {
    #[serde(deserialize_with = "deserialize_string_like")]
    pub node_id: String,
    pub objective: String,
    pub tools: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args_hints: Option<serde_json::Value>,
    #[serde(default, deserialize_with = "deserialize_vec_string_like")]
    pub depends_on: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_output: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_budget: Option<u32>,
}

/// The full workflow definition stored as `workflow.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowDef {
    pub name: String,
    pub title: String,
    pub description: String,
    #[serde(default)]
    pub trigger_phrases: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_thread_id: Option<String>,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub variables: std::collections::HashMap<String, WorkflowVariable>,
    pub nodes: Vec<WorkflowNode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_estimated_tokens: Option<u32>,
}

/// Summary returned by list operations.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowSummary {
    pub name: String,
    pub title: String,
    pub description: String,
    pub node_count: usize,
    pub created_at: String,
    pub path: String,
}

/// Load a workflow definition from its directory.
pub fn load_workflow(workflow_dir: &Path) -> Option<WorkflowDef> {
    let json_path = workflow_dir.join("workflow.json");
    let content = std::fs::read_to_string(&json_path).ok()?;
    serde_json::from_str(&content).ok()
}

/// Save a workflow definition to its directory, also generating SKILL.md.
pub fn save_workflow(workspace_config_dir: &Path, def: &WorkflowDef) -> Result<PathBuf, String> {
    let dir = workflows_dir(workspace_config_dir).join(&def.name);
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("Failed to create workflow directory: {e}"))?;

    let mut normalized = def.clone();
    if normalized.created_at.trim().is_empty() {
        normalized.created_at = chrono::Utc::now().to_rfc3339();
    }

    let json_path = dir.join("workflow.json");
    let json_content = serde_json::to_string_pretty(&normalized)
        .map_err(|e| format!("Failed to serialize workflow: {e}"))?;
    std::fs::write(&json_path, json_content)
        .map_err(|e| format!("Failed to write workflow.json: {e}"))?;

    let skill_content = skill_gen::generate_skill_md(&normalized);
    let skill_path = dir.join("SKILL.md");
    std::fs::write(&skill_path, skill_content)
        .map_err(|e| format!("Failed to write SKILL.md: {e}"))?;

    Ok(dir)
}

/// List all workflows in the workspace.
pub fn list_workflows(workspace_config_dir: &Path) -> Vec<WorkflowSummary> {
    let dir = workflows_dir(workspace_config_dir);
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };

    let mut workflows = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        if let Some(def) = load_workflow(&path) {
            workflows.push(WorkflowSummary {
                name: def.name,
                title: def.title,
                description: def.description,
                node_count: def.nodes.len(),
                created_at: def.created_at,
                path: path.to_string_lossy().to_string(),
            });
        }
    }
    workflows.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    workflows
}

/// Delete a workflow by name.
pub fn delete_workflow(workspace_config_dir: &Path, name: &str) -> Result<(), String> {
    let dir = workflows_dir(workspace_config_dir).join(name);
    if !dir.exists() {
        return Err(format!("Workflow '{name}' not found"));
    }
    std::fs::remove_dir_all(&dir).map_err(|e| format!("Failed to delete workflow: {e}"))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::{WorkflowDef, WorkflowNode, load_workflow, save_workflow, workflows_dir};

    fn sample_workflow(created_at: &str) -> WorkflowDef {
        WorkflowDef {
            name: "workflow-test".to_string(),
            title: "Workflow Test".to_string(),
            description: "test workflow".to_string(),
            trigger_phrases: vec!["run workflow test".to_string()],
            source_thread_id: None,
            created_at: created_at.to_string(),
            variables: HashMap::new(),
            nodes: vec![WorkflowNode {
                node_id: "step_1".to_string(),
                objective: "run command".to_string(),
                tools: vec!["shell".to_string()],
                args_hints: None,
                depends_on: Vec::new(),
                expected_output: None,
                token_budget: None,
            }],
            total_estimated_tokens: None,
        }
    }

    #[test]
    fn save_workflow_backfills_created_at_when_empty() {
        let workspace_dir = tempfile::tempdir().expect("should create temp dir");
        let def = sample_workflow("");

        let saved_dir = save_workflow(workspace_dir.path(), &def).expect("should save workflow");
        let saved = load_workflow(&saved_dir).expect("should load saved workflow");

        assert!(!saved.created_at.trim().is_empty());
        assert!(chrono::DateTime::parse_from_rfc3339(&saved.created_at).is_ok());
    }

    #[test]
    fn load_workflow_accepts_missing_created_at_field() {
        let workspace_dir = tempfile::tempdir().expect("should create temp dir");
        let workflow_dir = workflows_dir(workspace_dir.path()).join("missing-created-at");
        std::fs::create_dir_all(&workflow_dir).expect("should create workflow dir");

        std::fs::write(
            workflow_dir.join("workflow.json"),
            r#"{
  "name": "missing-created-at",
  "title": "Missing createdAt",
  "description": "createdAt omitted from extraction output",
  "triggerPhrases": ["test"],
  "variables": {},
  "nodes": []
}"#,
        )
        .expect("should write workflow file");

        let loaded = load_workflow(&workflow_dir).expect("should parse workflow without createdAt");
        assert_eq!(loaded.created_at, "");

        let saved_dir = save_workflow(workspace_dir.path(), &loaded).expect("should save workflow");
        let saved = load_workflow(&saved_dir).expect("should reload saved workflow");
        assert!(!saved.created_at.trim().is_empty());
    }

    #[test]
    fn workflow_deserialization_coerces_string_like_fields() {
        let raw = r#"{
  "name": "coercion-workflow",
  "title": "类型容错测试",
  "description": "验证 string-like 字段容错",
  "variables": {
    "timeoutSec": { "type": "string", "description": "超时时间（秒）", "default": 60 },
    "dryRun": { "type": "string", "description": "是否为演练模式", "default": true }
  },
  "nodes": [
    {
      "nodeId": 1,
      "objective": "执行命令",
      "tools": ["shell"],
      "dependsOn": [1, "step_0", true],
      "tokenBudget": 1200
    }
  ],
  "totalEstimatedTokens": 3000
}"#;

        let parsed: WorkflowDef =
            serde_json::from_str(raw).expect("should parse with string-like coercion");
        let timeout = parsed
            .variables
            .get("timeoutSec")
            .and_then(|variable| variable.default.as_deref());
        let dry_run = parsed
            .variables
            .get("dryRun")
            .and_then(|variable| variable.default.as_deref());
        assert_eq!(timeout, Some("60"));
        assert_eq!(dry_run, Some("true"));

        assert_eq!(parsed.nodes.len(), 1);
        let node = &parsed.nodes[0];
        assert_eq!(node.node_id, "1");
        assert_eq!(
            node.depends_on,
            vec!["1".to_string(), "step_0".to_string(), "true".to_string()]
        );
        assert_eq!(node.token_budget, Some(1200));
    }

    #[test]
    fn workflow_deserialization_keeps_numeric_budget_strict() {
        let raw = r#"{
  "name": "strict-budget",
  "title": "数值字段严格校验",
  "description": "tokenBudget 必须是数字",
  "variables": {},
  "nodes": [
    {
      "nodeId": "step_1",
      "objective": "执行命令",
      "tools": ["shell"],
      "tokenBudget": "1500"
    }
  ]
}"#;

        let error =
            serde_json::from_str::<WorkflowDef>(raw).expect_err("tokenBudget string should fail");
        let message = error.to_string();
        assert!(
            message.contains("invalid type") || message.contains("u32"),
            "unexpected error message: {message}"
        );
    }
}
