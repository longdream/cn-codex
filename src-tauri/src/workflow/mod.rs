pub mod commands;
pub mod extractor;
pub mod prompts;
pub mod skill_gen;

use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};

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

/// Workflow 脚本快照支持的脚本后缀白名单。
///
/// 说明：
/// - 仅复制这些后缀，避免将无关二进制或大文件误当作“脚本资产”快照；
/// - 该白名单覆盖 Windows/跨平台常见脚本形态；
/// - 判断时统一使用小写扩展名。
const SCRIPT_EXTENSION_WHITELIST: [&str; 7] = ["ps1", "bat", "cmd", "sh", "py", "js", "ts"];

/// workflow 保存时生成的脚本快照映射清单条目。
///
/// sourcePath:
/// - 相对项目根目录的原始脚本路径（canonicalize 后再转相对路径，避免重复与歧义）
///
/// snapshotPath:
/// - 落到 workflow 目录内的快照路径（统一以 `scripts/` 开头）
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct ScriptSnapshotEntry {
    source_path: String,
    snapshot_path: String,
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
pub fn save_workflow(
    workspace_config_dir: &Path,
    project_root: &Path,
    def: &WorkflowDef,
) -> Result<PathBuf, String> {
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

    // 保存 workflow 时同步做“脚本资产快照”：
    // - 从 args_hints 提取可能的脚本路径；
    // - 仅复制项目根目录内、且真实存在的脚本文件；
    // - 将快照映射写入 scripts-manifest.json，便于后续追踪来源。
    let script_manifest = snapshot_referenced_scripts(&normalized, project_root, &dir)?;
    write_scripts_manifest(&dir, &script_manifest)?;

    let snapshot_paths: Vec<String> = script_manifest
        .iter()
        .map(|entry| entry.snapshot_path.clone())
        .collect();
    let skill_content = skill_gen::generate_skill_md(&normalized, &snapshot_paths);
    let skill_path = dir.join("SKILL.md");
    std::fs::write(&skill_path, skill_content)
        .map_err(|e| format!("Failed to write SKILL.md: {e}"))?;

    Ok(dir)
}

/// 将 workflow 中引用到的脚本复制到当前 workflow 目录下的 `scripts/`。
///
/// 安全策略：
/// 1. 仅从 args_hints 的字符串字段提取候选脚本路径；
/// 2. 仅允许白名单后缀；
/// 3. canonicalize 后必须位于 project_root 内（阻断 `..` 越界与外部绝对路径）；
/// 4. 缺失文件/非法路径直接跳过，不阻塞 workflow 主保存流程。
fn snapshot_referenced_scripts(
    def: &WorkflowDef,
    project_root: &Path,
    workflow_dir: &Path,
) -> Result<Vec<ScriptSnapshotEntry>, String> {
    let canonical_project_root = project_root
        .canonicalize()
        .unwrap_or_else(|_| project_root.to_path_buf());
    let scripts_dir = workflow_dir.join("scripts");

    let mut snapshots = Vec::new();
    let mut seen_source_paths = BTreeSet::new();

    for candidate in collect_script_path_candidates(def) {
        let Some((source_path, source_display_path)) =
            resolve_script_candidate_path(&canonical_project_root, &candidate)
        else {
            continue;
        };
        if !seen_source_paths.insert(source_display_path.clone()) {
            continue;
        }

        let snapshot_display_path = format!("scripts/{source_display_path}");
        let snapshot_abs_path =
            scripts_dir.join(relative_display_path_to_pathbuf(&source_display_path));
        if let Some(parent_dir) = snapshot_abs_path.parent() {
            std::fs::create_dir_all(parent_dir)
                .map_err(|e| format!("Failed to create script snapshot directory: {e}"))?;
        }
        std::fs::copy(&source_path, &snapshot_abs_path).map_err(|e| {
            format!("Failed to copy script '{source_display_path}' into workflow snapshot: {e}")
        })?;

        snapshots.push(ScriptSnapshotEntry {
            source_path: source_display_path,
            snapshot_path: snapshot_display_path,
        });
    }

    snapshots.sort_by(|left, right| left.source_path.cmp(&right.source_path));
    Ok(snapshots)
}

/// 从 args_hints 中提取全部“可能是脚本路径”的候选值。
///
/// 提取策略：
/// - 递归遍历 JSON 任意层级，收集字符串值；
/// - 对每个字符串做 token 拆分（处理 `--file=...`、被引号包裹等场景）；
/// - 只保留后缀命中白名单的候选。
fn collect_script_path_candidates(def: &WorkflowDef) -> Vec<String> {
    let mut candidates = Vec::new();
    let mut seen = BTreeSet::new();

    for node in &def.nodes {
        let Some(args_hints) = &node.args_hints else {
            continue;
        };

        let mut string_values = Vec::new();
        collect_json_string_values(args_hints, &mut string_values);
        for value in string_values {
            for candidate in extract_script_candidates_from_text(&value) {
                if seen.insert(candidate.clone()) {
                    candidates.push(candidate);
                }
            }
        }
    }

    candidates
}

/// 递归收集 JSON 中的字符串值，供脚本路径提取使用。
fn collect_json_string_values(value: &serde_json::Value, out: &mut Vec<String>) {
    match value {
        serde_json::Value::String(text) => out.push(text.clone()),
        serde_json::Value::Array(items) => {
            for item in items {
                collect_json_string_values(item, out);
            }
        }
        serde_json::Value::Object(map) => {
            for item in map.values() {
                collect_json_string_values(item, out);
            }
        }
        _ => {}
    }
}

/// 从一段文本中抽取脚本路径候选。
///
/// 兼容场景：
/// - 文本本身就是路径：`scripts/deploy.ps1`
/// - 命令参数：`pwsh -File scripts/deploy.ps1`
/// - 键值写法：`--script=tools/build.py`
fn extract_script_candidates_from_text(text: &str) -> Vec<String> {
    let mut candidates = Vec::new();
    let mut seen = BTreeSet::new();

    let mut try_push = |raw: &str| {
        let Some(candidate) = normalize_script_candidate(raw) else {
            return;
        };
        if !has_allowed_script_extension(&candidate) {
            return;
        }
        if seen.insert(candidate.clone()) {
            candidates.push(candidate);
        }
    };

    try_push(text);
    for token in text.split_whitespace() {
        try_push(token);
        if let Some((_, rhs)) = token.rsplit_once('=') {
            try_push(rhs);
        }
    }

    candidates
}

/// 规范化脚本候选文本，剥离常见包裹符号并统一路径分隔符。
fn normalize_script_candidate(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    // 对模板变量占位直接跳过：这类值通常是待运行时替换，不是可快照的确定文件路径。
    if trimmed.contains("{{") || trimmed.contains("}}") {
        return None;
    }

    let stripped = trimmed.trim_matches(|ch: char| {
        matches!(
            ch,
            '"' | '\'' | '`' | ',' | ';' | '(' | ')' | '[' | ']' | '{' | '}'
        )
    });
    if stripped.is_empty() {
        return None;
    }

    Some(stripped.replace('\\', "/"))
}

/// 判断候选是否命中脚本后缀白名单。
fn has_allowed_script_extension(candidate: &str) -> bool {
    let lower = candidate.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with('-') {
        return false;
    }
    SCRIPT_EXTENSION_WHITELIST
        .iter()
        .any(|ext| lower.ends_with(&format!(".{ext}")))
}

/// 解析脚本候选到真实文件路径，并转换为“项目内相对显示路径”。
///
/// 返回 None 的情况：
/// - 文件不存在；
/// - 不在 project_root 内；
/// - 后缀不在白名单；
/// - 路径无法安全规范化。
fn resolve_script_candidate_path(
    project_root: &Path,
    candidate: &str,
) -> Option<(PathBuf, String)> {
    let normalized_candidate = normalize_script_candidate(candidate)?;
    if !has_allowed_script_extension(&normalized_candidate) {
        return None;
    }

    let candidate_path = PathBuf::from(&normalized_candidate);
    let source_path = if candidate_path.is_absolute() {
        candidate_path
    } else {
        project_root.join(candidate_path)
    };

    let canonical_source_path = source_path.canonicalize().ok()?;
    if !canonical_source_path.is_file() || !canonical_source_path.starts_with(project_root) {
        return None;
    }

    let relative_path = canonical_source_path.strip_prefix(project_root).ok()?;
    let display_path = normalize_relative_display_path(relative_path)?;
    Some((canonical_source_path, display_path))
}

/// 将相对路径标准化为 `/` 分隔字符串，并拒绝 `..` / 绝对路径成分。
fn normalize_relative_display_path(path: &Path) -> Option<String> {
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => parts.push(part.to_string_lossy().to_string()),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("/"))
    }
}

/// 将以 `/` 分隔的展示路径还原为 PathBuf，用于在磁盘上创建目录结构。
fn relative_display_path_to_pathbuf(relative_path: &str) -> PathBuf {
    let mut path = PathBuf::new();
    for part in relative_path.split('/') {
        if !part.trim().is_empty() {
            path.push(part);
        }
    }
    path
}

/// 写入脚本快照清单，便于后续追踪“原路径 -> 快照路径”映射。
fn write_scripts_manifest(
    workflow_dir: &Path,
    entries: &[ScriptSnapshotEntry],
) -> Result<(), String> {
    let manifest_path = workflow_dir.join("scripts-manifest.json");
    let manifest_content = serde_json::to_string_pretty(entries)
        .map_err(|e| format!("Failed to serialize scripts manifest: {e}"))?;
    std::fs::write(&manifest_path, manifest_content)
        .map_err(|e| format!("Failed to write scripts-manifest.json: {e}"))
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
    use std::path::{Path, PathBuf};

    use serde_json::json;

    use super::{
        ScriptSnapshotEntry, WorkflowDef, WorkflowNode, load_workflow, save_workflow, workflows_dir,
    };

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

    /// 构造一个临时“项目根 + codey 配置目录”。
    ///
    /// 说明：
    /// - `project_root` 用于脚本候选路径解析与越界校验；
    /// - `workspace_config_dir` 对应真实运行时的 `project_root/codey`。
    fn setup_workspace() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let temp_dir = tempfile::tempdir().expect("should create temp dir");
        let project_root = temp_dir.path().join("project");
        let workspace_config_dir = project_root.join("codey");
        std::fs::create_dir_all(&workspace_config_dir).expect("should create workspace config dir");
        (temp_dir, project_root, workspace_config_dir)
    }

    /// 读取 scripts-manifest.json 为结构化条目，便于断言原路径与快照路径映射。
    fn read_scripts_manifest(workflow_dir: &Path) -> Vec<ScriptSnapshotEntry> {
        let manifest_path = workflow_dir.join("scripts-manifest.json");
        let content = std::fs::read_to_string(&manifest_path)
            .expect("should read scripts-manifest.json content");
        serde_json::from_str(&content).expect("should parse scripts-manifest.json")
    }

    #[test]
    fn save_workflow_backfills_created_at_when_empty() {
        let workspace_dir = tempfile::tempdir().expect("should create temp dir");
        let def = sample_workflow("");

        let saved_dir = save_workflow(workspace_dir.path(), workspace_dir.path(), &def)
            .expect("should save workflow");
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

        let saved_dir = save_workflow(workspace_dir.path(), workspace_dir.path(), &loaded)
            .expect("should save workflow");
        let saved = load_workflow(&saved_dir).expect("should reload saved workflow");
        assert!(!saved.created_at.trim().is_empty());
    }

    #[test]
    fn save_workflow_snapshots_referenced_scripts_and_writes_manifest() {
        let (_temp_dir, project_root, workspace_config_dir) = setup_workspace();

        let deploy_script = project_root.join("scripts").join("deploy.ps1");
        std::fs::create_dir_all(
            deploy_script
                .parent()
                .expect("script path should have parent"),
        )
        .expect("should create script parent directory");
        std::fs::write(&deploy_script, "Write-Host 'deploy'").expect("should write deploy script");

        let build_script = project_root.join("tools").join("build.py");
        std::fs::create_dir_all(
            build_script
                .parent()
                .expect("script path should have parent"),
        )
        .expect("should create script parent directory");
        std::fs::write(&build_script, "print('build')").expect("should write build script");

        let mut def = sample_workflow("");
        def.nodes[0].args_hints = Some(json!({
            "shell": {
                "command": "pwsh -File scripts/deploy.ps1 --script=tools/build.py"
            }
        }));

        let saved_dir = save_workflow(&workspace_config_dir, &project_root, &def)
            .expect("should save workflow");

        assert!(saved_dir.join("scripts/scripts/deploy.ps1").is_file());
        assert!(saved_dir.join("scripts/tools/build.py").is_file());

        let manifest = read_scripts_manifest(&saved_dir);
        assert_eq!(
            manifest,
            vec![
                ScriptSnapshotEntry {
                    source_path: "scripts/deploy.ps1".to_string(),
                    snapshot_path: "scripts/scripts/deploy.ps1".to_string(),
                },
                ScriptSnapshotEntry {
                    source_path: "tools/build.py".to_string(),
                    snapshot_path: "scripts/tools/build.py".to_string(),
                }
            ]
        );

        let skill_content =
            std::fs::read_to_string(saved_dir.join("SKILL.md")).expect("should read SKILL.md");
        assert!(skill_content.contains("## 脚本快照"));
        assert!(skill_content.contains("`scripts/scripts/deploy.ps1`"));
        assert!(skill_content.contains("`scripts/tools/build.py`"));
    }

    #[test]
    fn save_workflow_skips_missing_or_outside_scripts_without_failing() {
        let (temp_dir, project_root, workspace_config_dir) = setup_workspace();

        let inside_script = project_root.join("scripts").join("inside.ps1");
        std::fs::create_dir_all(
            inside_script
                .parent()
                .expect("script path should have parent"),
        )
        .expect("should create inside script parent directory");
        std::fs::write(&inside_script, "Write-Host 'inside'").expect("should write inside script");

        let outside_script = temp_dir.path().join("outside.ps1");
        std::fs::write(&outside_script, "Write-Host 'outside'")
            .expect("should write outside script");

        let mut def = sample_workflow("");
        def.nodes[0].args_hints = Some(json!({
            "shell": {
                "command": format!(
                    "pwsh -File scripts/inside.ps1 --fallback=../outside.ps1 --abs={} --missing=tools/missing.py",
                    outside_script.to_string_lossy()
                )
            }
        }));

        let saved_dir = save_workflow(&workspace_config_dir, &project_root, &def)
            .expect("should save workflow");

        let manifest = read_scripts_manifest(&saved_dir);
        assert_eq!(
            manifest,
            vec![ScriptSnapshotEntry {
                source_path: "scripts/inside.ps1".to_string(),
                snapshot_path: "scripts/scripts/inside.ps1".to_string(),
            }]
        );
        assert!(saved_dir.join("scripts/scripts/inside.ps1").is_file());
        assert!(!saved_dir.join("scripts/tools/missing.py").is_file());
    }

    #[test]
    fn save_workflow_deduplicates_referenced_scripts() {
        let (_temp_dir, project_root, workspace_config_dir) = setup_workspace();

        let reused_script = project_root.join("scripts").join("reused.ps1");
        std::fs::create_dir_all(
            reused_script
                .parent()
                .expect("script path should have parent"),
        )
        .expect("should create reused script parent directory");
        std::fs::write(&reused_script, "Write-Host 'reused'").expect("should write reused script");

        let mut def = sample_workflow("");
        def.nodes[0].args_hints = Some(json!({
            "shell": {
                "command": "pwsh -File scripts/reused.ps1 --script=scripts/reused.ps1"
            },
            "fallback": ["scripts/reused.ps1"]
        }));

        let saved_dir = save_workflow(&workspace_config_dir, &project_root, &def)
            .expect("should save workflow");
        let manifest = read_scripts_manifest(&saved_dir);

        assert_eq!(
            manifest,
            vec![ScriptSnapshotEntry {
                source_path: "scripts/reused.ps1".to_string(),
                snapshot_path: "scripts/scripts/reused.ps1".to_string(),
            }]
        );
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
