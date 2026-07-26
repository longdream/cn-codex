use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, State};
use tracing::info;

use crate::adapter;
use crate::adapter::types::InternalMessage;
use crate::agent::UserAttachment;
use crate::error::{AppError, AppResult};
use crate::state::AppState;

/// Skill 实验室草稿的隔离存储目录: codey/skills-lab/
fn get_skill_lab_dir(state: &AppState) -> PathBuf {
    state.workspace_config_dir.join("skills-lab")
}

/// 实验室中单个 skill 草稿的目录
fn get_skill_lab_entry_dir(state: &AppState, skill_id: &str) -> PathBuf {
    get_skill_lab_dir(state).join(skill_id)
}

static ACTIVE_SKILL_LAB_RUNS: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();

fn active_skill_lab_runs() -> &'static Mutex<HashSet<String>> {
    ACTIVE_SKILL_LAB_RUNS.get_or_init(|| Mutex::new(HashSet::new()))
}

fn register_active_skill_lab_run(skill_id: &str) -> bool {
    let mut guard = active_skill_lab_runs()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    guard.insert(skill_id.to_string())
}

fn unregister_active_skill_lab_run(skill_id: &str) {
    let mut guard = active_skill_lab_runs()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    guard.remove(skill_id);
}

fn is_skill_lab_run_active(skill_id: &str) -> bool {
    let guard = active_skill_lab_runs()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    guard.contains(skill_id)
}

fn is_live_skill_lab_status(status: &str) -> bool {
    matches!(status, "testing" | "evaluating" | "rewriting")
}

fn recover_stale_skill_lab_status(meta_path: &Path, skill_id: &str, meta: &mut SkillLabMeta) {
    if !is_live_skill_lab_status(&meta.status) || is_skill_lab_run_active(skill_id) {
        return;
    }

    info!(
        "Recover stale skill lab status for {skill_id}: {} -> failed",
        meta.status
    );
    meta.status = "failed".to_string();
    write_skill_lab_meta(meta_path, meta);
}

struct ActiveSkillLabRunGuard {
    skill_id: String,
}

impl ActiveSkillLabRunGuard {
    fn acquire(skill_id: &str) -> AppResult<Self> {
        if !register_active_skill_lab_run(skill_id) {
            return Err(AppError::Custom(format!(
                "Skill lab test is already running: {skill_id}"
            )));
        }
        Ok(Self {
            skill_id: skill_id.to_string(),
        })
    }
}

impl Drop for ActiveSkillLabRunGuard {
    fn drop(&mut self) {
        unregister_active_skill_lab_run(&self.skill_id);
    }
}

fn parse_python_version_output(raw: &str) -> Option<String> {
    raw.lines().map(str::trim).find_map(|line| {
        if line.to_ascii_lowercase().starts_with("python ") {
            Some(line.to_string())
        } else {
            None
        }
    })
}

fn probe_python_command(command: &str, args: &[&str]) -> Option<(String, String)> {
    let output = std::process::Command::new(command)
        .args(args)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{stdout}\n{stderr}");
    let version = parse_python_version_output(&combined)?;
    Some((command.to_string(), version))
}

fn python_install_hint() -> String {
    #[cfg(target_os = "windows")]
    {
        return "未检测到 Python。请先安装 Python 3，并在安装界面勾选 Add Python to PATH。可参考：winget install Python.Python.3"
            .to_string();
    }

    #[cfg(not(target_os = "windows"))]
    {
        "Python 3 is not available in PATH. Install Python 3 before generating scripts.".to_string()
    }
}

fn check_python_env_internal() -> PythonEnvCheckResult {
    #[cfg(target_os = "windows")]
    let candidates: &[(&str, &[&str])] = &[
        ("python", &["--version"]),
        ("py", &["-3", "--version"]),
        ("py", &["--version"]),
    ];

    #[cfg(not(target_os = "windows"))]
    let candidates: &[(&str, &[&str])] = &[("python3", &["--version"]), ("python", &["--version"])];

    for (command, args) in candidates {
        if let Some((exe, version)) = probe_python_command(command, args) {
            return PythonEnvCheckResult {
                available: true,
                executable: Some(exe),
                version: Some(version),
                install_hint: String::new(),
            };
        }
    }

    PythonEnvCheckResult {
        available: false,
        executable: None,
        version: None,
        install_hint: python_install_hint(),
    }
}

/// Skill 实验室草稿摘要（列表用）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillLabSummary {
    pub id: String,
    pub name: String,
    /// 当前状态：idle / testing / passed / failed
    pub status: String,
    /// 累计迭代次数
    pub iteration_count: u32,
}

/// Skill 实验室草稿完整内容
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillLabDetail {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub goal: String,
    pub content: String,
    pub test_prompt: String,
    pub status: String,
    pub iteration_count: u32,
    /// 该草稿配置的最大自动迭代次数
    pub max_iterations: u32,
    /// 最近一次测试结果（可能为空）
    pub last_test_result: Option<String>,
    /// 最近一次 AI 评估意见（可能为空）
    pub last_evaluation: Option<String>,
    /// 当前最佳总分
    pub best_score: Option<f64>,
    /// 评分轨迹
    pub score_history: Vec<SkillLabScoreRecord>,
    /// 当前最佳版本对应的评估内容
    pub best_evaluation: Option<String>,
    /// 连续平台期轮次
    pub stable_rounds: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillLabScoreRecord {
    pub iteration: u32,
    pub total_score: f64,
    pub clarity: f64,
    pub robustness: f64,
    pub executability: f64,
    pub maintainability: f64,
}

/// 持久化到磁盘的草稿元数据
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SkillLabMeta {
    name: String,
    #[serde(default)]
    goal: String,
    test_prompt: String,
    status: String,
    iteration_count: u32,
    /// 最大自动改写迭代次数（按草稿配置，缺省走默认值）
    #[serde(default = "default_max_iterations")]
    max_iterations: u32,
    #[serde(default)]
    last_test_result: Option<String>,
    #[serde(default)]
    last_evaluation: Option<String>,
    #[serde(default)]
    best_score: Option<f64>,
    #[serde(default)]
    score_history: Vec<SkillLabScoreRecord>,
    #[serde(default)]
    best_evaluation: Option<String>,
    #[serde(default)]
    stable_rounds: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PythonEnvCheckResult {
    pub available: bool,
    pub executable: Option<String>,
    pub version: Option<String>,
    pub install_hint: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillLabGenerateFromGoalParams {
    pub skill_id: String,
    pub goal: String,
    pub name_hint: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillLabGeneratedScript {
    pub path: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillLabGenerateFromGoalResult {
    pub skill_id: String,
    pub name: String,
    pub content: String,
    pub test_prompt: String,
    pub scripts: Vec<SkillLabGeneratedScript>,
}

/// 列出所有实验室草稿
#[tauri::command]
pub async fn skill_lab_list(state: State<'_, AppState>) -> AppResult<Vec<SkillLabSummary>> {
    let lab_dir = get_skill_lab_dir(&state);
    if !lab_dir.exists() {
        return Ok(vec![]);
    }

    let entries = std::fs::read_dir(&lab_dir)
        .map_err(|e| AppError::Custom(format!("Failed to read skills-lab dir: {e}")))?;

    let mut skills = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let meta_path = path.join("meta.json");
        if !meta_path.exists() {
            continue;
        }
        let id = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();

        if let Ok(content) = std::fs::read_to_string(&meta_path) {
            if let Ok(mut meta) = serde_json::from_str::<SkillLabMeta>(&content) {
                recover_stale_skill_lab_status(&meta_path, &id, &mut meta);
                skills.push(SkillLabSummary {
                    id,
                    name: meta.name,
                    status: meta.status,
                    iteration_count: meta.iteration_count,
                });
            }
        }
    }

    skills.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(skills)
}

/// 读取实验室草稿完整内容
#[tauri::command]
pub async fn skill_lab_read(
    state: State<'_, AppState>,
    skill_id: String,
) -> AppResult<SkillLabDetail> {
    let dir = get_skill_lab_entry_dir(&state, &skill_id);
    let meta_path = dir.join("meta.json");
    let skill_md_path = dir.join("SKILL.md");

    if !meta_path.exists() {
        return Err(AppError::Custom(format!(
            "Skill lab entry not found: {skill_id}"
        )));
    }

    let meta_str = std::fs::read_to_string(&meta_path)
        .map_err(|e| AppError::Custom(format!("Failed to read meta: {e}")))?;
    let mut meta: SkillLabMeta = serde_json::from_str(&meta_str)
        .map_err(|e| AppError::Custom(format!("Failed to parse meta: {e}")))?;
    recover_stale_skill_lab_status(&meta_path, &skill_id, &mut meta);

    let content = if skill_md_path.exists() {
        std::fs::read_to_string(&skill_md_path).unwrap_or_default()
    } else {
        String::new()
    };

    Ok(SkillLabDetail {
        id: skill_id,
        name: meta.name,
        goal: meta.goal,
        content,
        test_prompt: meta.test_prompt,
        status: meta.status,
        iteration_count: meta.iteration_count,
        max_iterations: normalize_max_iterations(meta.max_iterations),
        last_test_result: meta.last_test_result,
        last_evaluation: meta.last_evaluation,
        best_score: meta.best_score,
        score_history: meta.score_history,
        best_evaluation: meta.best_evaluation,
        stable_rounds: meta.stable_rounds,
    })
}

/// 保存实验室草稿
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillLabSaveParams {
    pub skill_id: String,
    pub name: String,
    #[serde(default)]
    pub goal: String,
    pub content: String,
    pub test_prompt: String,
    /// 可选：该草稿最大自动迭代次数
    #[serde(default)]
    pub max_iterations: Option<u32>,
}

#[tauri::command]
pub async fn skill_lab_save(
    state: State<'_, AppState>,
    params: SkillLabSaveParams,
) -> AppResult<String> {
    let incoming_id = params.skill_id.trim();
    if incoming_id.is_empty() {
        return Err(AppError::Custom("skillId must not be empty".to_string()));
    }

    // 普通保存保持原 skill_id 不变；标准命名由自动生成/部署阶段负责。
    let skill_id = incoming_id.to_string();
    let dir = get_skill_lab_entry_dir(&state, &skill_id);
    std::fs::create_dir_all(&dir)
        .map_err(|e| AppError::Custom(format!("Failed to create skill-lab dir: {e}")))?;

    // 读取已有 meta 保留迭代计数和测试结果
    let existing_meta = dir
        .join("meta.json")
        .exists()
        .then(|| {
            std::fs::read_to_string(dir.join("meta.json"))
                .ok()
                .and_then(|s| serde_json::from_str::<SkillLabMeta>(&s).ok())
        })
        .flatten();

    let meta = SkillLabMeta {
        name: params.name.trim().to_string(),
        goal: params.goal.trim().to_string(),
        test_prompt: params.test_prompt.clone(),
        status: existing_meta
            .as_ref()
            .map(|m| m.status.clone())
            .unwrap_or_else(|| "idle".to_string()),
        iteration_count: existing_meta
            .as_ref()
            .map(|m| m.iteration_count)
            .unwrap_or(0),
        max_iterations: normalize_max_iterations(
            params
                .max_iterations
                .or_else(|| existing_meta.as_ref().map(|m| m.max_iterations))
                .unwrap_or_else(default_max_iterations),
        ),
        last_test_result: existing_meta
            .as_ref()
            .and_then(|m| m.last_test_result.clone()),
        last_evaluation: existing_meta
            .as_ref()
            .and_then(|m| m.last_evaluation.clone()),
        best_score: existing_meta.as_ref().and_then(|m| m.best_score),
        score_history: existing_meta
            .as_ref()
            .map(|m| m.score_history.clone())
            .unwrap_or_default(),
        best_evaluation: existing_meta
            .as_ref()
            .and_then(|m| m.best_evaluation.clone()),
        stable_rounds: existing_meta.as_ref().map(|m| m.stable_rounds).unwrap_or(0),
    };

    let meta_json = serde_json::to_string_pretty(&meta)
        .map_err(|e| AppError::Custom(format!("Failed to serialize meta: {e}")))?;

    std::fs::write(dir.join("meta.json"), meta_json)
        .map_err(|e| AppError::Custom(format!("Failed to write meta: {e}")))?;
    std::fs::write(dir.join("SKILL.md"), &params.content)
        .map_err(|e| AppError::Custom(format!("Failed to write SKILL.md: {e}")))?;

    Ok(skill_id)
}

#[tauri::command]
pub async fn skill_lab_check_python_env() -> AppResult<PythonEnvCheckResult> {
    Ok(check_python_env_internal())
}

fn validate_script_relative_path(path: &str) -> bool {
    let normalized = path.replace('\\', "/");
    if !normalized.starts_with("scripts/") || !normalized.ends_with(".py") {
        return false;
    }
    let p = Path::new(&normalized);
    if p.is_absolute() {
        return false;
    }
    !p.components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
}

fn normalize_relative_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn collect_generated_scripts_recursive(
    root: &Path,
    dir: &Path,
    out: &mut Vec<SkillLabGeneratedScript>,
) -> AppResult<()> {
    if !dir.exists() {
        return Ok(());
    }
    let entries = std::fs::read_dir(dir)
        .map_err(|e| AppError::Custom(format!("Failed to scan scripts dir: {e}")))?;
    for entry in entries {
        let entry =
            entry.map_err(|e| AppError::Custom(format!("Failed to read script entry: {e}")))?;
        let path = entry.path();
        if path.is_dir() {
            collect_generated_scripts_recursive(root, &path, out)?;
            continue;
        }
        if !path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.eq_ignore_ascii_case("py"))
            .unwrap_or(false)
        {
            continue;
        }
        let rel = path
            .strip_prefix(root)
            .map(normalize_relative_path)
            .map_err(|e| AppError::Custom(format!("Invalid generated script path: {e}")))?;
        if !validate_script_relative_path(&rel) {
            return Err(AppError::Custom(format!(
                "Generated script has invalid path: {rel}"
            )));
        }
        let content = std::fs::read_to_string(&path)
            .map_err(|e| AppError::Custom(format!("Failed to read generated script {rel}: {e}")))?;
        out.push(SkillLabGeneratedScript { path: rel, content });
    }
    Ok(())
}

fn collect_generated_scripts(skill_dir: &Path) -> AppResult<Vec<SkillLabGeneratedScript>> {
    let mut scripts = Vec::new();
    collect_generated_scripts_recursive(skill_dir, &skill_dir.join("scripts"), &mut scripts)?;
    scripts.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(scripts)
}

fn validate_generation_artifacts(
    skill_md_path: &Path,
    scripts: &[SkillLabGeneratedScript],
) -> AppResult<()> {
    if !skill_md_path.exists() {
        return Err(AppError::Custom(
            "Agent generation failed: SKILL.md was not created".to_string(),
        ));
    }
    if scripts.is_empty() {
        return Err(AppError::Custom(
            "Agent generation failed: no Python scripts were generated under scripts/".to_string(),
        ));
    }
    for script in scripts {
        if !validate_script_relative_path(&script.path) {
            return Err(AppError::Custom(format!(
                "Generated script has invalid path: {}",
                script.path
            )));
        }
        if script.content.trim().is_empty() {
            return Err(AppError::Custom(format!(
                "Generated script is empty: {}",
                script.path
            )));
        }
    }
    Ok(())
}

fn parse_skill_name_from_markdown(content: &str) -> Option<String> {
    if !content.starts_with("---") {
        return None;
    }
    let tail = content.get(3..)?;
    let end = tail.find("---")?;
    let frontmatter = &tail[..end];
    for line in frontmatter.lines() {
        let trimmed = line.trim();
        if let Some(value) = trimmed.strip_prefix("name:") {
            let name = value.trim().trim_matches('"').trim_matches('\'').trim();
            if !name.is_empty() {
                return Some(name.to_string());
            }
        }
    }
    None
}

fn is_standard_skill_id(value: &str) -> bool {
    let value = value.trim();
    if value.is_empty() || value.len() > 64 {
        return false;
    }
    if value.starts_with('-') || value.ends_with('-') || value.contains("--") {
        return false;
    }
    value
        .chars()
        .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-')
}

fn looks_like_temporary_lab_id(value: &str) -> bool {
    let value = value.trim();
    value
        .strip_prefix("lab-")
        .is_some_and(|rest| !rest.is_empty() && rest.chars().all(|ch| ch.is_ascii_digit()))
}

fn slugify_skill_id(value: &str) -> String {
    let mut output = String::new();
    let mut last_dash = false;
    for ch in value.trim().chars() {
        if ch.is_ascii_alphanumeric() {
            output.push(ch.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash && !output.is_empty() {
            output.push('-');
            last_dash = true;
        }
    }
    let trimmed = output.trim_matches('-').to_string();
    if trimmed.is_empty() {
        return "untitled-skill".to_string();
    }
    if trimmed.len() <= 64 {
        return trimmed;
    }
    let mut truncated = trimmed.chars().take(64).collect::<String>();
    while truncated.ends_with('-') {
        truncated.pop();
    }
    if truncated.is_empty() {
        "untitled-skill".to_string()
    } else {
        truncated
    }
}

fn derive_preferred_skill_id(
    name: &str,
    name_hint: Option<&str>,
    goal: &str,
    current_id: &str,
) -> String {
    for candidate in [name, name_hint.unwrap_or(""), goal, current_id] {
        let trimmed = candidate.trim();
        if trimmed.is_empty() {
            continue;
        }
        if is_standard_skill_id(trimmed) && !looks_like_temporary_lab_id(trimmed) {
            return trimmed.to_string();
        }
        let slug = slugify_skill_id(trimmed);
        if slug != "untitled-skill" && !looks_like_temporary_lab_id(&slug) {
            return slug;
        }
    }
    "generated-skill".to_string()
}

fn skill_id_conflicts(
    lab_root: &Path,
    prod_skills_root: &Path,
    skill_id: &str,
    current_id: &str,
) -> bool {
    if skill_id == current_id {
        return false;
    }
    lab_root.join(skill_id).exists() || prod_skills_root.join(skill_id).exists()
}

fn allocate_unique_skill_id(
    lab_root: &Path,
    prod_skills_root: &Path,
    preferred: &str,
    current_id: &str,
) -> String {
    let base = {
        let slug = if is_standard_skill_id(preferred) {
            preferred.trim().to_string()
        } else {
            slugify_skill_id(preferred)
        };
        if slug.is_empty() || looks_like_temporary_lab_id(&slug) {
            "generated-skill".to_string()
        } else {
            slug
        }
    };

    if !skill_id_conflicts(lab_root, prod_skills_root, &base, current_id) {
        return base;
    }

    for index in 2..1000 {
        let candidate = format!("{base}-{index}");
        if !skill_id_conflicts(lab_root, prod_skills_root, &candidate, current_id) {
            return candidate;
        }
    }

    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    format!("{base}-{millis}")
}

fn rename_skill_lab_entry(lab_root: &Path, from_id: &str, to_id: &str) -> AppResult<()> {
    if from_id == to_id {
        return Ok(());
    }
    let from_dir = lab_root.join(from_id);
    let to_dir = lab_root.join(to_id);
    if !from_dir.exists() {
        return Err(AppError::Custom(format!(
            "Skill lab entry not found: {from_id}"
        )));
    }
    if to_dir.exists() {
        return Err(AppError::Custom(format!(
            "Skill lab entry already exists: {to_id}"
        )));
    }
    std::fs::rename(&from_dir, &to_dir).map_err(|e| {
        AppError::Custom(format!(
            "Failed to rename skill lab entry from {from_id} to {to_id}: {e}"
        ))
    })?;
    Ok(())
}

fn yaml_quote(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

fn extract_skill_description_fallback(content: &str) -> Option<String> {
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed == "---" {
            continue;
        }
        let cleaned = trimmed
            .trim_start_matches(['*', '-', '>'])
            .trim()
            .to_string();
        if !cleaned.is_empty() {
            return Some(cleaned);
        }
    }
    None
}

fn ensure_skill_frontmatter(content: &str, name: &str, description: &str) -> String {
    let name = name.trim();
    let description = description.trim();
    let fallback_name = if name.is_empty() {
        "untitled-skill"
    } else {
        name
    };
    let fallback_description = if description.is_empty() {
        format!("Skill lab deployed skill: {fallback_name}")
    } else {
        description.to_string()
    };

    if content.starts_with("---") {
        if let Some(end) = content[3..].find("---") {
            let front = &content[3..3 + end];
            let body = content[3 + end + 3..].trim_start_matches('\r');
            let body = body.trim_start_matches('\n');

            let mut has_name = false;
            let mut has_description = false;
            let mut lines = Vec::new();
            for line in front.lines() {
                let trimmed = line.trim();
                if let Some(value) = trimmed.strip_prefix("name:") {
                    has_name = true;
                    // Skill 正式名称统一为标准 id（kebab-case），始终覆盖写入
                    let _ = value;
                    lines.push(format!("name: {}", yaml_quote(fallback_name)));
                    continue;
                }
                if let Some(value) = trimmed.strip_prefix("description:") {
                    has_description = true;
                    let existing = value.trim().trim_matches('"').trim_matches('\'').trim();
                    if existing.is_empty() {
                        lines.push(format!(
                            "description: {}",
                            yaml_quote(&fallback_description)
                        ));
                    } else {
                        lines.push(line.to_string());
                    }
                    continue;
                }
                lines.push(line.to_string());
            }

            if !has_name {
                lines.insert(0, format!("name: {}", yaml_quote(fallback_name)));
            }
            if !has_description {
                lines.insert(
                    1.min(lines.len()),
                    format!("description: {}", yaml_quote(&fallback_description)),
                );
            }

            let mut out = String::from("---\n");
            out.push_str(&lines.join("\n"));
            if !out.ends_with('\n') {
                out.push('\n');
            }
            out.push_str("---\n");
            if !body.is_empty() {
                out.push_str(body);
                if !body.ends_with('\n') {
                    out.push('\n');
                }
            }
            return out;
        }
    }

    let mut out = String::from("---\n");
    out.push_str(&format!("name: {}\n", yaml_quote(fallback_name)));
    out.push_str(&format!(
        "description: {}\n",
        yaml_quote(&fallback_description)
    ));
    out.push_str("---\n");
    let body = content.trim_start_matches('\u{feff}').trim_start();
    if !body.is_empty() {
        out.push_str(body);
        if !body.ends_with('\n') {
            out.push('\n');
        }
    }
    out
}

fn extract_json_object(raw: &str) -> Option<&str> {
    let start = raw.find('{')?;
    let end = raw.rfind('}')?;
    (end > start).then_some(&raw[start..=end])
}

fn parse_generation_response(
    thread_messages: &[crate::thread_store::ThreadMessage],
    fallback_name: &str,
    fallback_test_prompt: &str,
) -> (String, String) {
    let mut name = fallback_name.to_string();
    let mut test_prompt = fallback_test_prompt.to_string();

    let assistant_text = thread_messages
        .iter()
        .rev()
        .find(|m| m.role == "assistant" && !m.content.trim().is_empty())
        .map(|m| m.content.as_str());
    let Some(assistant_text) = assistant_text else {
        return (name, test_prompt);
    };

    let Some(json_text) = extract_json_object(assistant_text) else {
        return (name, test_prompt);
    };
    let Ok(parsed) = serde_json::from_str::<serde_json::Value>(json_text) else {
        return (name, test_prompt);
    };

    if let Some(value) = parsed
        .get("name")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        name = value.to_string();
    }
    if let Some(value) = parsed
        .get("testPrompt")
        .or_else(|| parsed.get("test_prompt"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        test_prompt = value.to_string();
    }

    (name, test_prompt)
}

fn clear_generation_artifacts(dir: &Path) {
    let _ = std::fs::remove_file(dir.join("SKILL.md"));
    let _ = std::fs::remove_dir_all(dir.join("scripts"));
}

#[tauri::command]
pub async fn skill_lab_generate_from_goal(
    state: State<'_, AppState>,
    app_handle: AppHandle,
    params: SkillLabGenerateFromGoalParams,
) -> AppResult<SkillLabGenerateFromGoalResult> {
    let skill_id = params.skill_id.trim();
    if skill_id.is_empty() {
        return Err(AppError::Custom("skillId must not be empty".to_string()));
    }
    let goal = params.goal.trim();
    if goal.is_empty() {
        return Err(AppError::Custom("goal must not be empty".to_string()));
    }

    let python = check_python_env_internal();
    if !python.available {
        return Err(AppError::Custom(python.install_hint));
    }

    let dir = get_skill_lab_entry_dir(&state, skill_id);
    std::fs::create_dir_all(&dir)
        .map_err(|e| AppError::Custom(format!("Failed to create skill lab dir: {e}")))?;
    clear_generation_artifacts(&dir);

    let config = state.config_manager.read()?;
    let thread = state
        .thread_store
        .create_thread(config.model.clone())
        .await?;

    let path_hint = format!("codey/skills-lab/{skill_id}");
    let name_hint = params.name_hint.unwrap_or_else(|| skill_id.to_string());
    let generation_prompt = format!(
        "你是 Skill 生成代理。请根据目标需求生成一个可测试的 Skill 草稿，并直接写入工作区文件。\n\n\
         目标需求：{goal}\n\
         名称提示：{name_hint}\n\n\
         强制要求：\n\
         1) 只允许在 `{path_hint}/` 目录下写文件。\n\
         2) 必须写出 `{path_hint}/SKILL.md`。\n\
         3) 必须写出至少一个 Python 脚本，路径必须是 `{path_hint}/scripts/*.py`。\n\
         4) 不要写入其他目录，不要删除无关文件。\n\
         5) 最后一条助手回复仅输出一个 JSON 对象，格式为：\n\
            {{\"name\":\"...\",\"testPrompt\":\"...\",\"scripts\":[\"scripts/xxx.py\"]}}\n\
         6) JSON 中的 name 必须是标准英文 skill 名称（kebab-case，仅小写字母、数字、连字符），例如 stock-evaluation、code-review-helper。不要使用 lab- 前缀或中文。\n\
         7) SKILL.md frontmatter 的 name 字段也必须使用同一个标准英文 skill 名称。\n\
         8) JSON 必须合法，不要加解释文字。"
    );

    state
        .agent_engine
        .run_turn(
            &app_handle,
            &config,
            &thread.id,
            &generation_prompt,
            Vec::<UserAttachment>::new(),
            Some(state.project_root.as_path()),
            None,
            None,
            None,
            None,
        )
        .await?;

    let skill_md_path = dir.join("SKILL.md");
    let scripts = collect_generated_scripts(&dir)?;
    validate_generation_artifacts(&skill_md_path, &scripts)?;
    let content = std::fs::read_to_string(&skill_md_path)
        .map_err(|e| AppError::Custom(format!("Failed to read generated SKILL.md: {e}")))?;

    let fallback_name =
        parse_skill_name_from_markdown(&content).unwrap_or_else(|| name_hint.clone());
    let fallback_test_prompt = format!("请验证该 Skill 是否能完成目标需求：{goal}");
    let thread_messages = state.thread_store.get_thread_messages(&thread.id).await;
    let (name, test_prompt) =
        parse_generation_response(&thread_messages, &fallback_name, &fallback_test_prompt);

    let preferred_id = derive_preferred_skill_id(&name, Some(name_hint.as_str()), goal, skill_id);
    let lab_root = get_skill_lab_dir(&state);
    let prod_skills_root = state.workspace_config_dir.join("skills");
    let final_skill_id =
        allocate_unique_skill_id(&lab_root, &prod_skills_root, &preferred_id, skill_id);
    if final_skill_id != skill_id {
        rename_skill_lab_entry(&lab_root, skill_id, &final_skill_id)?;
    }

    // 确保 frontmatter 与正式 skill 名一致（标准英文 kebab-case）
    let content = ensure_skill_frontmatter(&content, &final_skill_id, goal);
    let final_dir = get_skill_lab_entry_dir(&state, &final_skill_id);
    std::fs::write(final_dir.join("SKILL.md"), &content)
        .map_err(|e| AppError::Custom(format!("Failed to write normalized SKILL.md: {e}")))?;

    // 同步 meta，确保生成后立刻以标准 skill 名称出现在列表中
    let existing_meta = final_dir
        .join("meta.json")
        .exists()
        .then(|| {
            std::fs::read_to_string(final_dir.join("meta.json"))
                .ok()
                .and_then(|s| serde_json::from_str::<SkillLabMeta>(&s).ok())
        })
        .flatten();
    let meta = SkillLabMeta {
        name: final_skill_id.clone(),
        goal: goal.to_string(),
        test_prompt: test_prompt.clone(),
        status: existing_meta
            .as_ref()
            .map(|m| m.status.clone())
            .unwrap_or_else(|| "idle".to_string()),
        iteration_count: existing_meta
            .as_ref()
            .map(|m| m.iteration_count)
            .unwrap_or(0),
        max_iterations: existing_meta
            .as_ref()
            .map(|m| m.max_iterations)
            .unwrap_or_else(default_max_iterations),
        last_test_result: existing_meta
            .as_ref()
            .and_then(|m| m.last_test_result.clone()),
        last_evaluation: existing_meta
            .as_ref()
            .and_then(|m| m.last_evaluation.clone()),
        best_score: existing_meta.as_ref().and_then(|m| m.best_score),
        score_history: existing_meta
            .as_ref()
            .map(|m| m.score_history.clone())
            .unwrap_or_default(),
        best_evaluation: existing_meta
            .as_ref()
            .and_then(|m| m.best_evaluation.clone()),
        stable_rounds: existing_meta.as_ref().map(|m| m.stable_rounds).unwrap_or(0),
    };
    write_skill_lab_meta(&final_dir.join("meta.json"), &meta);

    Ok(SkillLabGenerateFromGoalResult {
        skill_id: final_skill_id.clone(),
        name: final_skill_id,
        content,
        test_prompt,
        scripts,
    })
}

/// 更新实验室草稿的测试结果和状态
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillLabUpdateResultParams {
    pub skill_id: String,
    pub status: String,
    pub test_result: Option<String>,
    pub evaluation: Option<String>,
    pub updated_content: Option<String>,
    pub increment_iteration: bool,
}

#[tauri::command]
pub async fn skill_lab_update_result(
    state: State<'_, AppState>,
    params: SkillLabUpdateResultParams,
) -> AppResult<()> {
    let dir = get_skill_lab_entry_dir(&state, &params.skill_id);
    let meta_path = dir.join("meta.json");

    if !meta_path.exists() {
        return Err(AppError::Custom(format!(
            "Skill lab entry not found: {}",
            params.skill_id
        )));
    }

    let meta_str = std::fs::read_to_string(&meta_path)
        .map_err(|e| AppError::Custom(format!("Failed to read meta: {e}")))?;
    let mut meta: SkillLabMeta = serde_json::from_str(&meta_str)
        .map_err(|e| AppError::Custom(format!("Failed to parse meta: {e}")))?;

    meta.status = params.status;
    if let Some(result) = params.test_result {
        meta.last_test_result = Some(result);
    }
    if let Some(eval) = params.evaluation {
        meta.last_evaluation = Some(eval);
    }
    if params.increment_iteration {
        meta.iteration_count += 1;
    }

    let meta_json = serde_json::to_string_pretty(&meta)
        .map_err(|e| AppError::Custom(format!("Failed to serialize meta: {e}")))?;
    std::fs::write(&meta_path, meta_json)
        .map_err(|e| AppError::Custom(format!("Failed to write meta: {e}")))?;

    // 如果提供了更新后的内容，写入 SKILL.md
    if let Some(content) = params.updated_content {
        std::fs::write(dir.join("SKILL.md"), &content)
            .map_err(|e| AppError::Custom(format!("Failed to write SKILL.md: {e}")))?;
    }

    Ok(())
}

/// 将实验室草稿推广为正式 Skill
fn copy_dir_recursive(src: &Path, dst: &Path) -> AppResult<()> {
    std::fs::create_dir_all(dst).map_err(|e| {
        AppError::Custom(format!("Failed to create directory {}: {e}", dst.display()))
    })?;
    let entries = std::fs::read_dir(src).map_err(|e| {
        AppError::Custom(format!("Failed to read directory {}: {e}", src.display()))
    })?;
    for entry in entries {
        let entry =
            entry.map_err(|e| AppError::Custom(format!("Failed to read directory entry: {e}")))?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path).map_err(|e| {
                AppError::Custom(format!(
                    "Failed to copy {} to {}: {e}",
                    src_path.display(),
                    dst_path.display()
                ))
            })?;
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn skill_lab_deploy(
    app_handle: AppHandle,
    state: State<'_, AppState>,
    skill_id: String,
) -> AppResult<String> {
    let lab_dir = get_skill_lab_entry_dir(&state, &skill_id);
    let skill_md_src = lab_dir.join("SKILL.md");

    if !skill_md_src.exists() {
        return Err(AppError::Custom(format!(
            "Skill lab entry has no SKILL.md: {skill_id}"
        )));
    }

    let content = std::fs::read_to_string(&skill_md_src)
        .map_err(|e| AppError::Custom(format!("Failed to read lab SKILL.md: {e}")))?;
    let meta_path = lab_dir.join("meta.json");
    let meta = std::fs::read_to_string(&meta_path)
        .ok()
        .and_then(|meta_str| serde_json::from_str::<SkillLabMeta>(&meta_str).ok());
    let display_name = meta
        .as_ref()
        .map(|item| item.name.trim())
        .filter(|name| !name.is_empty())
        .unwrap_or(skill_id.as_str());
    let frontmatter_name = parse_skill_name_from_markdown(&content);
    let preferred_id = derive_preferred_skill_id(
        frontmatter_name.as_deref().unwrap_or(""),
        Some(display_name),
        meta.as_ref().map(|item| item.goal.as_str()).unwrap_or(""),
        &skill_id,
    );
    let lab_root = get_skill_lab_dir(&state);
    let prod_skills_root = state.workspace_config_dir.join("skills");
    let deploy_id =
        allocate_unique_skill_id(&lab_root, &prod_skills_root, &preferred_id, &skill_id);

    // 正式 Skill 存储在 codey/skills/<id>/SKILL.md，id 使用标准 skill 名称
    let prod_dir = prod_skills_root.join(&deploy_id);
    std::fs::create_dir_all(&prod_dir)
        .map_err(|e| AppError::Custom(format!("Failed to create skills dir: {e}")))?;

    let description = meta
        .as_ref()
        .map(|item| item.goal.trim())
        .filter(|goal| !goal.is_empty())
        .map(str::to_string)
        .or_else(|| extract_skill_description_fallback(&content))
        .unwrap_or_else(|| format!("Skill lab deployed skill: {deploy_id}"));
    let normalized_content = ensure_skill_frontmatter(&content, &deploy_id, &description);
    std::fs::write(prod_dir.join("SKILL.md"), &normalized_content)
        .map_err(|e| AppError::Custom(format!("Failed to write prod SKILL.md: {e}")))?;

    let scripts_src = lab_dir.join("scripts");
    let scripts_dst = prod_dir.join("scripts");
    if scripts_src.exists() {
        if scripts_dst.exists() {
            let _ = std::fs::remove_dir_all(&scripts_dst);
        }
        copy_dir_recursive(&scripts_src, &scripts_dst)?;
    }

    // 更新状态
    if let Some(mut meta) = meta {
        meta.status = "deployed".to_string();
        // 同步展示名到标准 skill id，避免继续显示临时 lab-* 名称
        if meta.name.trim().is_empty() || looks_like_temporary_lab_id(meta.name.trim()) {
            meta.name = deploy_id.clone();
        }
        write_skill_lab_meta(&meta_path, &meta);
    }

    // 若实验室草稿仍是临时 lab-* id，则同步重命名到正式 skill 名
    if looks_like_temporary_lab_id(&skill_id) && skill_id != deploy_id {
        if let Err(err) = rename_skill_lab_entry(&lab_root, &skill_id, &deploy_id) {
            // 部署本身已成功，重命名失败时仅记录日志，避免阻断
            info!(
                "Deployed skill {} but failed to rename lab draft {}: {}",
                deploy_id, skill_id, err
            );
        }
    }

    let _ = app_handle.emit(
        "skills-changed",
        serde_json::json!({
            "skillId": deploy_id,
            "source": "skill-lab-deploy",
        }),
    );

    Ok(deploy_id)
}

/// 删除实验室草稿
#[tauri::command]
pub async fn skill_lab_delete(state: State<'_, AppState>, skill_id: String) -> AppResult<()> {
    let dir = get_skill_lab_entry_dir(&state, &skill_id);
    if dir.exists() {
        std::fs::remove_dir_all(&dir)
            .map_err(|e| AppError::Custom(format!("Failed to delete skill lab entry: {e}")))?;
    }
    Ok(())
}

// ── Skill Lab 自动测试闭环 ──────────────────────────────────

/// 默认最大自动改写迭代次数（可按草稿配置覆盖）
const DEFAULT_MAX_EVOLUTION_ITERATIONS: u32 = 3;
/// 单草稿允许配置的最大迭代次数上限
const MAX_ALLOWED_EVOLUTION_ITERATIONS: u32 = 20;
const HIGH_SCORE_THRESHOLD: f64 = 90.0;
const SCORE_PLATEAU_DELTA: f64 = 1.0;
const SCORE_PLATEAU_ROUNDS: u32 = 2;
const TEST_CALL_RETRY_MAX_ATTEMPTS: u32 = 3;
const TEST_CALL_RETRY_BASE_DELAY_MS: u64 = 1000;
const TEST_CALL_RETRY_JITTER_MAX_MS: u64 = 250;
const PROGRESS_LOG_SNIPPET_MAX_CHARS: usize = 240;
const PROGRESS_LOG_MAX_LINES: usize = 5;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SkillEvaluation {
    #[serde(default)]
    clarity: f64,
    #[serde(default)]
    robustness: f64,
    #[serde(default)]
    executability: f64,
    #[serde(default)]
    maintainability: f64,
    #[serde(default, alias = "totalScore")]
    total_score: f64,
    #[serde(default)]
    critical_issues: Vec<String>,
    #[serde(default)]
    improve_hints: Vec<String>,
}

fn clamp_score(score: f64) -> f64 {
    score.clamp(0.0, 100.0)
}

fn default_max_iterations() -> u32 {
    DEFAULT_MAX_EVOLUTION_ITERATIONS
}

fn normalize_max_iterations(value: u32) -> u32 {
    if value == 0 {
        DEFAULT_MAX_EVOLUTION_ITERATIONS
    } else {
        value.clamp(1, MAX_ALLOWED_EVOLUTION_ITERATIONS)
    }
}

fn parse_skill_evaluation(raw: &str) -> SkillEvaluation {
    if let Some(json_text) = extract_json_object(raw) {
        if let Ok(mut parsed) = serde_json::from_str::<SkillEvaluation>(json_text) {
            parsed.clarity = clamp_score(parsed.clarity);
            parsed.robustness = clamp_score(parsed.robustness);
            parsed.executability = clamp_score(parsed.executability);
            parsed.maintainability = clamp_score(parsed.maintainability);
            parsed.total_score = if parsed.total_score > 0.0 {
                clamp_score(parsed.total_score)
            } else {
                clamp_score(
                    (parsed.clarity
                        + parsed.robustness
                        + parsed.executability
                        + parsed.maintainability)
                        / 4.0,
                )
            };
            return parsed;
        }
    }

    let first_line = raw.lines().next().unwrap_or_default().to_ascii_uppercase();
    let total_score = if first_line.contains("PASS") {
        90.0
    } else {
        60.0
    };
    SkillEvaluation {
        clarity: total_score,
        robustness: total_score,
        executability: total_score,
        maintainability: total_score,
        total_score,
        critical_issues: Vec::new(),
        improve_hints: Vec::new(),
    }
}

fn weakest_dimension(eval: &SkillEvaluation) -> &'static str {
    let mut weakest = ("clarity", eval.clarity);
    for candidate in [
        ("robustness", eval.robustness),
        ("executability", eval.executability),
        ("maintainability", eval.maintainability),
    ] {
        if candidate.1 < weakest.1 {
            weakest = candidate;
        }
    }
    weakest.0
}

fn should_stop_evolution(
    _total_score: f64,
    _stable_rounds: u32,
    iteration: u32,
    max_iterations: u32,
) -> bool {
    iteration >= normalize_max_iterations(max_iterations)
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SkillLabClarifyAction {
    ContinueWithGuidance,
    Stop,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SkillLabClarifyDecision {
    action: SkillLabClarifyAction,
    guidance: String,
}

fn preferred_option_labels() -> [&'static str; 3] {
    [
        "按建议自动改写并继续",
        "我补充说明后再改写",
        "停止本轮，保留当前最佳",
    ]
}

fn parse_skill_lab_clarify_result(result: &serde_json::Value) -> SkillLabClarifyDecision {
    let answer = result
        .pointer("/answers/skill_lab_critical/answers/0")
        .and_then(|v| v.as_str())
        .or_else(|| {
            result
                .get("answers")
                .and_then(|answers| answers.get("skill_lab_critical"))
                .and_then(|question| question.get("answers"))
                .and_then(|answers| answers.as_array())
                .and_then(|answers| answers.first())
                .and_then(|value| value.as_str())
        })
        .unwrap_or("")
        .trim()
        .to_string();

    let labels = preferred_option_labels();
    if answer == labels[2] || answer.contains("停止") {
        return SkillLabClarifyDecision {
            action: SkillLabClarifyAction::Stop,
            guidance: String::new(),
        };
    }

    if answer == labels[0] {
        return SkillLabClarifyDecision {
            action: SkillLabClarifyAction::ContinueWithGuidance,
            guidance: String::new(),
        };
    }

    // 选项2，或用户在「其他」中填写的自由文本，都视为补充说明后继续
    let guidance = if answer == labels[1] {
        String::new()
    } else {
        answer
    };
    SkillLabClarifyDecision {
        action: SkillLabClarifyAction::ContinueWithGuidance,
        guidance,
    }
}

async fn ask_skill_lab_critical_clarification(
    app_handle: &AppHandle,
    skill_id: &str,
    iteration: u32,
    max_iterations: u32,
    total_score: f64,
    critical_issues: &[String],
    improve_hints: &[String],
) -> Result<SkillLabClarifyDecision, String> {
    let issues_text = critical_issues
        .iter()
        .map(|item| format!("- {item}"))
        .collect::<Vec<_>>()
        .join("\n");
    let hints_text = if improve_hints.is_empty() {
        "无".to_string()
    } else {
        improve_hints
            .iter()
            .map(|item| format!("- {item}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let labels = preferred_option_labels();
    let question_text = format!(
        "第 {iteration}/{max_iterations} 轮评估发现关键问题（当前总分 {total_score:.1}）：\n\
         {issues_text}\n\n\
         改进建议：\n{hints_text}\n\n\
         请选择如何继续："
    );

    let call_id = format!(
        "skill-lab-clarify-{}-{}-{}",
        skill_id,
        iteration,
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
    );
    let request_id = crate::protocol::RequestId::String(call_id.clone());

    emit_skill_lab_progress(
        app_handle,
        skill_id,
        "rewriting",
        iteration,
        max_iterations,
        "clarify",
        Some("发现关键问题，等待你的澄清与选择…".to_string()),
        Some(total_score),
        None,
        None,
        None,
    );

    app_handle
        .emit(
            "server-request",
            serde_json::json!({
                "requestId": &call_id,
                "id": &call_id,
                "method": "request_user_input",
                "params": {
                    "threadId": format!("skill-lab:{skill_id}"),
                    "callId": &call_id,
                    "questions": [{
                        "id": "skill_lab_critical",
                        "header": "Skill 实验室澄清",
                        "question": question_text,
                        "options": [
                            {
                                "label": labels[0],
                                "description": "根据评估中的关键问题与改进建议自动改写，然后进入下一轮。",
                                "recommended": true
                            },
                            {
                                "label": labels[1],
                                "description": "在下方「其他」输入你的补充要求，改写时会优先遵循。"
                            },
                            {
                                "label": labels[2],
                                "description": "结束本轮优化，保留当前最高分版本。"
                            }
                        ]
                    }]
                }
            }),
        )
        .map_err(|e| format!("failed to emit clarification request: {e}"))?;

    let result =
        crate::tool_executor::wait_for_approval_result_public(app_handle, &request_id, 600_000)
            .await?;
    Ok(parse_skill_lab_clarify_result(&result))
}

fn update_best_candidate(
    best_score: &mut f64,
    best_content: &mut String,
    best_evaluation: &mut String,
    candidate_score: f64,
    candidate_content: &str,
    candidate_evaluation: &str,
) {
    if candidate_score >= *best_score {
        *best_score = candidate_score;
        *best_content = candidate_content.to_string();
        *best_evaluation = candidate_evaluation.to_string();
    }
}

fn is_too_many_requests_error(message: &str) -> bool {
    message.contains("API error (429") || message.contains("429 Too Many Requests")
}

fn retry_backoff_base_ms(retry_attempt: u32) -> u64 {
    let exp = retry_attempt.saturating_sub(1).min(10);
    TEST_CALL_RETRY_BASE_DELAY_MS.saturating_mul(1u64 << exp)
}

fn retry_backoff_delay_ms(retry_attempt: u32) -> u64 {
    let base = retry_backoff_base_ms(retry_attempt);
    let jitter = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
        % (TEST_CALL_RETRY_JITTER_MAX_MS + 1);
    base.saturating_add(jitter)
}

fn compact_progress_snippet(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let mut snippet = trimmed
        .replace('\r', "")
        .lines()
        .take(PROGRESS_LOG_MAX_LINES)
        .collect::<Vec<_>>()
        .join("\n");
    if snippet.chars().count() > PROGRESS_LOG_SNIPPET_MAX_CHARS {
        snippet = format!(
            "{}…",
            snippet
                .chars()
                .take(PROGRESS_LOG_SNIPPET_MAX_CHARS)
                .collect::<String>()
        );
    }
    Some(snippet)
}

fn emit_skill_lab_progress(
    app_handle: &AppHandle,
    skill_id: &str,
    phase: &str,
    iteration: u32,
    max_iterations: u32,
    log_type: &str,
    log_snippet: Option<String>,
    score: Option<f64>,
    retry_attempt: Option<u32>,
    retry_delay_ms: Option<u64>,
    retry_reason: Option<&str>,
) {
    let mut payload = serde_json::json!({
        "skillId": skill_id,
        "phase": phase,
        "iteration": iteration,
        "maxIterations": max_iterations,
        "logType": log_type,
    });
    if let Some(obj) = payload.as_object_mut() {
        if let Some(snippet) = log_snippet {
            obj.insert("logSnippet".to_string(), serde_json::Value::String(snippet));
        }
        if let Some(value) = score {
            obj.insert("score".to_string(), serde_json::json!(value));
        }
        if let Some(value) = retry_attempt {
            obj.insert("retryAttempt".to_string(), serde_json::json!(value));
            obj.insert(
                "retryMax".to_string(),
                serde_json::json!(TEST_CALL_RETRY_MAX_ATTEMPTS),
            );
        }
        if let Some(value) = retry_delay_ms {
            obj.insert("retryDelayMs".to_string(), serde_json::json!(value));
        }
        if let Some(value) = retry_reason {
            obj.insert("retryReason".to_string(), serde_json::json!(value));
        }
    }
    let _ = app_handle.emit("skill-lab-progress", payload);
}

fn write_skill_lab_meta(meta_path: &Path, meta: &SkillLabMeta) {
    if let Ok(json) = serde_json::to_string_pretty(meta) {
        let _ = std::fs::write(meta_path, json);
    }
}

async fn call_test_non_streaming_with_retry(
    http: &reqwest::Client,
    adapter: &dyn adapter::ProviderAdapter,
    base_url: &str,
    api_key: &str,
    model: &str,
    max_tokens: Option<i64>,
    messages: &[InternalMessage],
    query_params: Option<&HashMap<String, String>>,
    extra_headers: Option<&HashMap<String, String>>,
    app_handle: &AppHandle,
    skill_id: &str,
    iteration: u32,
    max_iterations: u32,
) -> Result<String, String> {
    let mut retry_attempt = 0u32;
    loop {
        match call_ai_non_streaming(
            http,
            adapter,
            base_url,
            api_key,
            model,
            max_tokens,
            messages,
            query_params,
            extra_headers,
        )
        .await
        {
            Ok(output) => return Ok(output),
            Err(err) => {
                if !is_too_many_requests_error(&err) {
                    return Err(err);
                }
                if retry_attempt >= TEST_CALL_RETRY_MAX_ATTEMPTS {
                    return Err(format!(
                        "{err} (429 retry exhausted after {TEST_CALL_RETRY_MAX_ATTEMPTS} attempts)"
                    ));
                }
                retry_attempt += 1;
                let delay_ms = retry_backoff_delay_ms(retry_attempt);
                emit_skill_lab_progress(
                    app_handle,
                    skill_id,
                    "testing",
                    iteration,
                    max_iterations,
                    "retry",
                    Some(format!(
                        "429 限流，准备第 {retry_attempt}/{TEST_CALL_RETRY_MAX_ATTEMPTS} 次重试，{delay_ms}ms 后继续。"
                    )),
                    None,
                    Some(retry_attempt),
                    Some(delay_ms),
                    Some("429"),
                );
                tokio::time::sleep(Duration::from_millis(delay_ms)).await;
            }
        }
    }
}

/// 测试闭环结果
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillLabTestResult {
    /// 最终状态：passed / failed
    pub status: String,
    /// 总迭代次数
    pub iterations: u32,
    /// AI 在最后一轮输出的文本
    pub last_output: String,
    /// AI 评估意见
    pub evaluation: String,
    /// 最终 Skill 内容（可能经过改写）
    pub final_content: String,
    /// 最优总分
    pub best_score: f64,
    /// 本次评分轨迹
    pub score_history: Vec<SkillLabScoreRecord>,
    /// 连续平台期轮次
    pub stable_rounds: u32,
}

/// 运行自动测试闭环：测试 → 评估 → 改写 → 重复
#[tauri::command]
pub async fn skill_lab_run_test(
    state: State<'_, AppState>,
    app_handle: AppHandle,
    skill_id: String,
) -> AppResult<SkillLabTestResult> {
    let _active_run_guard = ActiveSkillLabRunGuard::acquire(&skill_id)?;
    let dir = get_skill_lab_entry_dir(&state, &skill_id);
    let meta_path = dir.join("meta.json");
    let skill_md_path = dir.join("SKILL.md");

    if !meta_path.exists() {
        return Err(AppError::Custom(format!(
            "Skill lab entry not found: {skill_id}"
        )));
    }

    let meta_str = std::fs::read_to_string(&meta_path)
        .map_err(|e| AppError::Custom(format!("Failed to read meta: {e}")))?;
    let mut meta: SkillLabMeta = serde_json::from_str(&meta_str)
        .map_err(|e| AppError::Custom(format!("Failed to parse meta: {e}")))?;

    let mut skill_content = std::fs::read_to_string(&skill_md_path).unwrap_or_default();
    let test_prompt = meta.test_prompt.clone();
    let max_iterations = normalize_max_iterations(meta.max_iterations);
    meta.max_iterations = max_iterations;

    if skill_content.trim().is_empty() || test_prompt.trim().is_empty() {
        return Err(AppError::Custom(
            "Skill content and test prompt must not be empty".to_string(),
        ));
    }

    // 读取当前 provider 配置
    let config = state.config_manager.read()?;
    let (_provider_id, provider_info) = config.resolve_provider();
    let model = config.resolve_model();
    if model.is_empty() {
        return Err(AppError::Custom("No model configured".to_string()));
    }
    let base_url = provider_info
        .resolve_base_url()
        .ok_or_else(|| AppError::Custom("No base URL configured for provider".to_string()))?;
    let api_key = provider_info.resolve_api_key().unwrap_or_default();
    let wire_api = provider_info
        .wire_api
        .as_deref()
        .unwrap_or("chat")
        .to_string();

    // 与主 agent 保持一致：使用配置中的 max_output_tokens，并夹到 API 允许的范围内
    // （部分 API 要求 max_tokens ∈ [1, 65536]，适配器默认 131072 会触发 400）。
    let max_output_tokens = config
        .max_output_tokens
        .map(|v| v.clamp(1, 65536))
        .unwrap_or(65536);

    let http = reqwest::Client::new();
    let adapter = adapter::get_adapter(&wire_api);

    let mut last_output = String::new();
    let mut evaluation_raw = String::new();
    let mut iterations = 0u32;
    let mut score_history: Vec<SkillLabScoreRecord> = Vec::new();
    let mut best_score = meta.best_score.unwrap_or(0.0);
    let mut best_content = skill_content.clone();
    let mut best_evaluation = meta.best_evaluation.clone().unwrap_or_default();
    let mut stable_rounds = 0u32;
    let mut previous_score: Option<f64> = None;
    let mut evolution_converged = false;

    for iteration in 0..max_iterations {
        iterations = iteration + 1;
        info!("Skill lab evolution iteration {iterations}/{max_iterations} for {skill_id}");

        meta.status = "testing".to_string();
        write_skill_lab_meta(&meta_path, &meta);

        // 通知前端当前阶段
        emit_skill_lab_progress(
            &app_handle,
            &skill_id,
            "testing",
            iterations,
            max_iterations,
            "phase",
            Some(format!("开始第 {iterations}/{max_iterations} 轮测试。")),
            None,
            None,
            None,
            None,
        );

        // ── 步骤1：用 Skill 内容作为 system prompt，测试提示词作为 user prompt ──
        let test_messages = vec![
            InternalMessage {
                role: "system".to_string(),
                content: adapter::types::text_content(&skill_content),
                tool_calls: None,
                tool_call_id: None,
                name: None,
            },
            InternalMessage {
                role: "user".to_string(),
                content: adapter::types::text_content(&test_prompt),
                tool_calls: None,
                tool_call_id: None,
                name: None,
            },
        ];

        last_output = call_test_non_streaming_with_retry(
            &http,
            &*adapter,
            &base_url,
            &api_key,
            &model,
            Some(max_output_tokens),
            &test_messages,
            provider_info.query_params.as_ref(),
            provider_info.http_headers.as_ref(),
            &app_handle,
            &skill_id,
            iterations,
            max_iterations,
        )
        .await
        .map_err(|e| {
            meta.status = "failed".to_string();
            write_skill_lab_meta(&meta_path, &meta);
            let _ = app_handle.emit(
                "skill-lab-progress",
                serde_json::json!({
                    "skillId": &skill_id,
                    "phase": "done",
                    "status": "failed",
                    "iteration": iterations,
                    "maxIterations": max_iterations,
                    "logType": "error",
                    "logSnippet": format!("测试失败: {e}"),
                }),
            );
            AppError::Custom(format!("Test call failed: {e}"))
        })?;

        // ── 步骤2：AI 评估 ──
        meta.status = "evaluating".to_string();
        write_skill_lab_meta(&meta_path, &meta);
        emit_skill_lab_progress(
            &app_handle,
            &skill_id,
            "evaluating",
            iterations,
            max_iterations,
            "testOutput",
            compact_progress_snippet(&last_output),
            None,
            None,
            None,
            None,
        );

        let eval_system = "你是一个 Skill 质量评审员。\
            你会收到一个 Skill 指令（system prompt）和它在测试提示词下产生的 AI 输出。\
            你必须返回一个 JSON 对象，不允许任何额外文本。\
            输出格式：\
            {\
              \"clarity\": 0-100,\
              \"robustness\": 0-100,\
              \"executability\": 0-100,\
              \"maintainability\": 0-100,\
              \"totalScore\": 0-100,\
              \"criticalIssues\": [\"关键问题\"],\
              \"improveHints\": [\"改进建议\"]\
            }";
        let eval_user = format!(
            "## Skill 指令\n{skill_content}\n\n## 测试提示词\n{test_prompt}\n\n## AI 输出\n{last_output}"
        );
        let eval_messages = vec![
            InternalMessage {
                role: "system".to_string(),
                content: adapter::types::text_content(eval_system),
                tool_calls: None,
                tool_call_id: None,
                name: None,
            },
            InternalMessage {
                role: "user".to_string(),
                content: adapter::types::text_content(&eval_user),
                tool_calls: None,
                tool_call_id: None,
                name: None,
            },
        ];

        evaluation_raw = call_ai_non_streaming(
            &http,
            &*adapter,
            &base_url,
            &api_key,
            &model,
            Some(max_output_tokens),
            &eval_messages,
            provider_info.query_params.as_ref(),
            provider_info.http_headers.as_ref(),
        )
        .await
        .map_err(|e| {
            meta.status = "failed".to_string();
            write_skill_lab_meta(&meta_path, &meta);
            let _ = app_handle.emit(
                "skill-lab-progress",
                serde_json::json!({
                    "skillId": &skill_id,
                    "phase": "done",
                    "status": "failed",
                    "iteration": iterations,
                    "maxIterations": max_iterations,
                    "logType": "error",
                    "logSnippet": format!("评估失败: {e}"),
                }),
            );
            AppError::Custom(format!("Evaluation call failed: {e}"))
        })?;
        let evaluation = parse_skill_evaluation(&evaluation_raw);
        let total_score = clamp_score(evaluation.total_score);

        if let Some(prev) = previous_score {
            if (total_score - prev).abs() < SCORE_PLATEAU_DELTA {
                stable_rounds += 1;
            } else {
                stable_rounds = 0;
            }
        } else {
            stable_rounds = 0;
        }
        previous_score = Some(total_score);
        emit_skill_lab_progress(
            &app_handle,
            &skill_id,
            "evaluating",
            iterations,
            max_iterations,
            "score",
            Some(format!(
                "第 {iterations} 轮评分 {:.1}，平台期轮次 {stable_rounds}。",
                total_score
            )),
            Some(total_score),
            None,
            None,
            None,
        );

        score_history.push(SkillLabScoreRecord {
            iteration: iterations,
            total_score,
            clarity: evaluation.clarity,
            robustness: evaluation.robustness,
            executability: evaluation.executability,
            maintainability: evaluation.maintainability,
        });

        update_best_candidate(
            &mut best_score,
            &mut best_content,
            &mut best_evaluation,
            total_score,
            &skill_content,
            &evaluation_raw,
        );

        if should_stop_evolution(total_score, stable_rounds, iteration + 1, max_iterations) {
            evolution_converged =
                total_score >= HIGH_SCORE_THRESHOLD && stable_rounds >= SCORE_PLATEAU_ROUNDS;
            break;
        }

        // ── 步骤3：自动改写 ──
        meta.status = "rewriting".to_string();
        write_skill_lab_meta(&meta_path, &meta);
        emit_skill_lab_progress(
            &app_handle,
            &skill_id,
            "rewriting",
            iterations,
            max_iterations,
            "rewriteInput",
            compact_progress_snippet(&evaluation_raw),
            Some(total_score),
            None,
            None,
            None,
        );

        // 仅在存在关键问题时，弹出与 ask 相同的澄清窗口，由用户决定如何继续
        let mut user_clarification = String::new();
        if !evaluation.critical_issues.is_empty() {
            match ask_skill_lab_critical_clarification(
                &app_handle,
                &skill_id,
                iterations,
                max_iterations,
                total_score,
                &evaluation.critical_issues,
                &evaluation.improve_hints,
            )
            .await
            {
                Ok(decision) => match decision.action {
                    SkillLabClarifyAction::Stop => {
                        emit_skill_lab_progress(
                            &app_handle,
                            &skill_id,
                            "rewriting",
                            iterations,
                            max_iterations,
                            "clarify",
                            Some("用户选择停止本轮优化，保留当前最佳结果。".to_string()),
                            Some(total_score),
                            None,
                            None,
                            None,
                        );
                        break;
                    }
                    SkillLabClarifyAction::ContinueWithGuidance => {
                        user_clarification = decision.guidance;
                        emit_skill_lab_progress(
                            &app_handle,
                            &skill_id,
                            "rewriting",
                            iterations,
                            max_iterations,
                            "clarify",
                            Some(if user_clarification.trim().is_empty() {
                                "用户确认按建议继续自动改写。".to_string()
                            } else {
                                format!("用户补充说明后继续改写：{}", user_clarification.trim())
                            }),
                            Some(total_score),
                            None,
                            None,
                            None,
                        );
                    }
                },
                Err(err) => {
                    // 用户拒绝/超时：视为停止本轮，保留当前最佳
                    emit_skill_lab_progress(
                        &app_handle,
                        &skill_id,
                        "rewriting",
                        iterations,
                        max_iterations,
                        "clarify",
                        Some(format!("澄清未完成（{err}），停止本轮优化。")),
                        Some(total_score),
                        None,
                        None,
                        None,
                    );
                    break;
                }
            }
        }

        let rewrite_system = "你是一个 Skill 指令优化专家。\
            根据评分与关键问题改进 Skill 指令内容，使其总分持续提高并更稳定。\
            只返回改进后的完整 SKILL.md 内容，不要包含任何解释。";
        let weakest = weakest_dimension(&evaluation);
        let critical_issues = if evaluation.critical_issues.is_empty() {
            "- 无".to_string()
        } else {
            evaluation
                .critical_issues
                .iter()
                .map(|item| format!("- {item}"))
                .collect::<Vec<_>>()
                .join("\n")
        };
        let improve_hints = if evaluation.improve_hints.is_empty() {
            "- 无".to_string()
        } else {
            evaluation
                .improve_hints
                .iter()
                .map(|item| format!("- {item}"))
                .collect::<Vec<_>>()
                .join("\n")
        };
        let user_guidance_block = if user_clarification.trim().is_empty() {
            String::new()
        } else {
            format!(
                "\n\n## 用户补充说明（优先遵循）\n{}",
                user_clarification.trim()
            )
        };
        let rewrite_user = format!(
            "## 原始 Skill 指令\n{skill_content}\n\n\
             ## 测试提示词\n{test_prompt}\n\n\
             ## AI 输出\n{last_output}\n\n\
             ## 评分结果\n\
             - totalScore: {total_score}\n\
             - clarity: {}\n\
             - robustness: {}\n\
             - executability: {}\n\
             - maintainability: {}\n\
             - weakestDimension: {weakest}\n\n\
             ## 关键问题\n{critical_issues}\n\n\
             ## 改进建议\n{improve_hints}{user_guidance_block}\n\n\
             请输出改进后的完整 Skill 指令内容：",
            evaluation.clarity,
            evaluation.robustness,
            evaluation.executability,
            evaluation.maintainability
        );
        let rewrite_messages = vec![
            InternalMessage {
                role: "system".to_string(),
                content: adapter::types::text_content(rewrite_system),
                tool_calls: None,
                tool_call_id: None,
                name: None,
            },
            InternalMessage {
                role: "user".to_string(),
                content: adapter::types::text_content(&rewrite_user),
                tool_calls: None,
                tool_call_id: None,
                name: None,
            },
        ];

        let rewritten = call_ai_non_streaming(
            &http,
            &*adapter,
            &base_url,
            &api_key,
            &model,
            Some(max_output_tokens),
            &rewrite_messages,
            provider_info.query_params.as_ref(),
            provider_info.http_headers.as_ref(),
        )
        .await
        .map_err(|e| {
            meta.status = "failed".to_string();
            write_skill_lab_meta(&meta_path, &meta);
            let _ = app_handle.emit(
                "skill-lab-progress",
                serde_json::json!({
                    "skillId": &skill_id,
                    "phase": "done",
                    "status": "failed",
                    "iteration": iterations,
                    "maxIterations": max_iterations,
                    "logType": "error",
                    "logSnippet": format!("改写失败: {e}"),
                }),
            );
            AppError::Custom(format!("Rewrite call failed: {e}"))
        })?;

        if !rewritten.trim().is_empty() {
            skill_content = rewritten;
            // 写入改写后的内容
            let _ = std::fs::write(&skill_md_path, &skill_content);
        }
    }

    // ── 保存最终结果 ──
    let final_status = if best_score >= HIGH_SCORE_THRESHOLD {
        "passed".to_string()
    } else {
        "failed".to_string()
    };

    meta.status = final_status.clone();
    meta.iteration_count += iterations;
    meta.last_test_result = Some(last_output.clone());
    meta.last_evaluation = Some(evaluation_raw.clone());
    meta.best_score = Some(best_score);
    meta.score_history = score_history.clone();
    meta.best_evaluation = if best_evaluation.trim().is_empty() {
        None
    } else {
        Some(best_evaluation.clone())
    };
    meta.stable_rounds = stable_rounds;

    if !best_content.trim().is_empty() {
        let _ = std::fs::write(&skill_md_path, &best_content);
    }

    write_skill_lab_meta(&meta_path, &meta);

    // 通知前端完成
    let _ = app_handle.emit(
        "skill-lab-progress",
        serde_json::json!({
            "skillId": &skill_id,
            "phase": "done",
            "status": &final_status,
            "iteration": iterations,
            "maxIterations": max_iterations,
            "bestScore": best_score,
            "stableRounds": stable_rounds,
            "converged": evolution_converged,
            "logType": "done",
            "logSnippet": format!("测试结束：{}，最佳分数 {:.1}。", &final_status, best_score),
        }),
    );

    Ok(SkillLabTestResult {
        status: final_status,
        iterations,
        last_output,
        evaluation: evaluation_raw,
        final_content: best_content,
        best_score,
        score_history,
        stable_rounds,
    })
}

/// 非流式调用 AI，返回完整文本回复
async fn call_ai_non_streaming(
    http: &reqwest::Client,
    adapter: &dyn adapter::ProviderAdapter,
    base_url: &str,
    api_key: &str,
    model: &str,
    max_tokens: Option<i64>,
    messages: &[InternalMessage],
    query_params: Option<&HashMap<String, String>>,
    extra_headers: Option<&HashMap<String, String>>,
) -> Result<String, String> {
    let url = adapter.build_url(base_url, model);
    let headers = adapter.build_headers(api_key);
    let (url, headers) =
        crate::adapter::apply_request_overrides(url, headers, query_params, extra_headers)
            .map_err(|e| format!("Request override error: {e}"))?;
    // ponytail: 非流式请求使用统一辅助函数，确保 stream=false 时移除 stream_options
    let body = crate::adapter::build_non_stream_body(adapter, model, messages, None, max_tokens);

    let response = http
        .post(&url)
        .headers(headers)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("HTTP request failed: {e}"))?;

    if !response.status().is_success() {
        let status = response.status();
        let body_text = response.text().await.unwrap_or_default();
        return Err(format!("API error ({status}): {body_text}"));
    }

    let body_text = response
        .text()
        .await
        .map_err(|e| format!("Failed to read response: {e}"))?;

    // 尝试从 JSON 响应中提取文本内容
    extract_completion_text(&body_text).ok_or_else(|| {
        format!(
            "Could not extract text from API response: {}",
            &body_text[..body_text.len().min(500)]
        )
    })
}

/// 从非流式 API 响应中提取文本内容（兼容多种供应商格式）
fn extract_completion_text(body: &str) -> Option<String> {
    let json: serde_json::Value = serde_json::from_str(body).ok()?;
    let pointers = [
        "/choices/0/message/content",
        "/output_text",
        "/output/0/content/0/text",
        "/content/0/text",
        "/candidates/0/content/parts/0/text",
    ];
    for ptr in pointers {
        if let Some(val) = json.pointer(ptr) {
            if let Some(text) = val.as_str().filter(|s| !s.trim().is_empty()) {
                return Some(text.trim().to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_openai_chat_response() {
        let body = r#"{"choices":[{"message":{"content":"Hello world"}}]}"#;
        assert_eq!(
            extract_completion_text(body),
            Some("Hello world".to_string())
        );
    }

    #[test]
    fn extract_openai_responses_api() {
        let body = r#"{"output_text":"Test output"}"#;
        assert_eq!(
            extract_completion_text(body),
            Some("Test output".to_string())
        );
    }

    #[test]
    fn extract_gemini_response() {
        let body = r#"{"candidates":[{"content":{"parts":[{"text":"Gemini says hello"}]}}]}"#;
        assert_eq!(
            extract_completion_text(body),
            Some("Gemini says hello".to_string())
        );
    }

    #[test]
    fn extract_anthropic_response() {
        let body = r#"{"content":[{"text":"Claude says hi"}]}"#;
        assert_eq!(
            extract_completion_text(body),
            Some("Claude says hi".to_string())
        );
    }

    #[test]
    fn extract_empty_body_returns_none() {
        assert_eq!(extract_completion_text("{}"), None);
        assert_eq!(extract_completion_text("not json"), None);
    }

    #[test]
    fn meta_round_trip() {
        let meta = SkillLabMeta {
            name: "Test Skill".to_string(),
            goal: String::new(),
            test_prompt: "Say hello".to_string(),
            status: "idle".to_string(),
            iteration_count: 0,
            max_iterations: DEFAULT_MAX_EVOLUTION_ITERATIONS,
            last_test_result: None,
            last_evaluation: None,
            best_score: None,
            score_history: Vec::new(),
            best_evaluation: None,
            stable_rounds: 0,
        };
        let json = serde_json::to_string(&meta).unwrap();
        let parsed: SkillLabMeta = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.name, "Test Skill");
        assert_eq!(parsed.status, "idle");
        assert_eq!(parsed.iteration_count, 0);
        assert_eq!(parsed.max_iterations, DEFAULT_MAX_EVOLUTION_ITERATIONS);
    }

    #[test]
    fn meta_deserialization_with_optional_fields() {
        let json = r#"{"name":"X","testPrompt":"p","status":"passed","iterationCount":3,"lastTestResult":"ok","lastEvaluation":"good"}"#;
        let meta: SkillLabMeta = serde_json::from_str(json).unwrap();
        assert_eq!(meta.name, "X");
        assert_eq!(meta.iteration_count, 3);
        assert_eq!(meta.last_test_result, Some("ok".to_string()));
        assert_eq!(meta.last_evaluation, Some("good".to_string()));
        assert!(meta.best_score.is_none());
        assert!(meta.score_history.is_empty());
        assert_eq!(meta.max_iterations, DEFAULT_MAX_EVOLUTION_ITERATIONS);
    }

    #[test]
    fn parse_python_version_extracts_valid_line() {
        let raw = "Python 3.11.9\n";
        assert_eq!(
            parse_python_version_output(raw),
            Some("Python 3.11.9".to_string())
        );
    }

    #[test]
    fn parse_python_version_empty_output_returns_none() {
        assert_eq!(parse_python_version_output(""), None);
        assert_eq!(parse_python_version_output("unknown"), None);
    }

    #[test]
    fn validate_script_relative_path_rejects_parent_segments() {
        assert!(validate_script_relative_path("scripts/run.py"));
        assert!(!validate_script_relative_path("../scripts/run.py"));
        assert!(!validate_script_relative_path("scripts/../run.py"));
        assert!(!validate_script_relative_path("scripts/run.sh"));
    }

    #[test]
    fn parse_skill_evaluation_fallback_supports_pass_fail() {
        let pass = parse_skill_evaluation("PASS\nLooks good");
        assert_eq!(pass.total_score, 90.0);
        let fail = parse_skill_evaluation("FAIL\nNeed rewrite");
        assert_eq!(fail.total_score, 60.0);
    }

    #[test]
    fn parse_skill_evaluation_from_json() {
        let raw = r#"{"clarity":88,"robustness":90,"executability":92,"maintainability":86,"totalScore":89,"criticalIssues":["a"],"improveHints":["b"]}"#;
        let parsed = parse_skill_evaluation(raw);
        assert_eq!(parsed.total_score, 89.0);
        assert_eq!(parsed.critical_issues.len(), 1);
    }

    #[test]
    fn parse_skill_evaluation_missing_dimensions_fallback() {
        let raw = r#"{"totalScore":85}"#;
        let parsed = parse_skill_evaluation(raw);
        assert_eq!(parsed.total_score, 85.0);
        assert_eq!(parsed.clarity, 0.0);
    }

    #[test]
    fn should_stop_when_high_score_and_plateau() {
        // 当前策略仅按 max_iterations 停止；高分/平台期保留字段供后续策略扩展
        assert!(should_stop_evolution(92.0, 2, 6, 6));
        assert!(!should_stop_evolution(92.0, 1, 5, 6));
    }

    #[test]
    fn should_stop_when_reaching_max_iterations() {
        assert!(should_stop_evolution(
            70.0,
            0,
            DEFAULT_MAX_EVOLUTION_ITERATIONS,
            DEFAULT_MAX_EVOLUTION_ITERATIONS
        ));
        assert!(!should_stop_evolution(
            70.0,
            0,
            DEFAULT_MAX_EVOLUTION_ITERATIONS - 1,
            DEFAULT_MAX_EVOLUTION_ITERATIONS
        ));
        assert!(should_stop_evolution(70.0, 0, 10, 10));
        assert!(!should_stop_evolution(70.0, 0, 9, 10));
    }

    #[test]
    fn normalize_max_iterations_clamps_range() {
        assert_eq!(
            normalize_max_iterations(0),
            DEFAULT_MAX_EVOLUTION_ITERATIONS
        );
        assert_eq!(normalize_max_iterations(1), 1);
        assert_eq!(normalize_max_iterations(20), 20);
        assert_eq!(
            normalize_max_iterations(99),
            MAX_ALLOWED_EVOLUTION_ITERATIONS
        );
    }

    #[test]
    fn parse_skill_lab_clarify_result_handles_options() {
        let labels = preferred_option_labels();
        let stop = serde_json::json!({
            "answers": {
                "skill_lab_critical": {
                    "answers": [labels[2]]
                }
            }
        });
        assert_eq!(
            parse_skill_lab_clarify_result(&stop).action,
            SkillLabClarifyAction::Stop
        );

        let auto = serde_json::json!({
            "answers": {
                "skill_lab_critical": {
                    "answers": [labels[0]]
                }
            }
        });
        let auto_decision = parse_skill_lab_clarify_result(&auto);
        assert_eq!(
            auto_decision.action,
            SkillLabClarifyAction::ContinueWithGuidance
        );
        assert!(auto_decision.guidance.is_empty());

        let custom = serde_json::json!({
            "answers": {
                "skill_lab_critical": {
                    "answers": ["请强化错误处理示例"]
                }
            }
        });
        let custom_decision = parse_skill_lab_clarify_result(&custom);
        assert_eq!(
            custom_decision.action,
            SkillLabClarifyAction::ContinueWithGuidance
        );
        assert_eq!(custom_decision.guidance, "请强化错误处理示例");
    }

    #[test]
    fn update_best_candidate_keeps_best_content() {
        let mut best_score = 80.0;
        let mut best_content = "best-v1".to_string();
        let mut best_evaluation = "eval-v1".to_string();
        update_best_candidate(
            &mut best_score,
            &mut best_content,
            &mut best_evaluation,
            88.0,
            "best-v2",
            "eval-v2",
        );
        update_best_candidate(
            &mut best_score,
            &mut best_content,
            &mut best_evaluation,
            84.0,
            "worse-v3",
            "eval-v3",
        );
        assert_eq!(best_score, 88.0);
        assert_eq!(best_content, "best-v2");
        assert_eq!(best_evaluation, "eval-v2");
    }

    #[test]
    fn validate_generation_artifacts_requires_skill_md() {
        let dir = tempfile::tempdir().unwrap();
        let scripts = vec![SkillLabGeneratedScript {
            path: "scripts/run.py".to_string(),
            content: "print('ok')".to_string(),
        }];
        let err = validate_generation_artifacts(&dir.path().join("SKILL.md"), &scripts)
            .expect_err("missing SKILL.md should fail");
        assert!(format!("{err}").contains("SKILL.md"));
    }

    #[test]
    fn validate_generation_artifacts_rejects_empty_script() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("SKILL.md"), "# skill").unwrap();
        let scripts = vec![SkillLabGeneratedScript {
            path: "scripts/run.py".to_string(),
            content: "   ".to_string(),
        }];
        let err = validate_generation_artifacts(&dir.path().join("SKILL.md"), &scripts)
            .expect_err("empty script should fail");
        assert!(format!("{err}").contains("empty"));
    }

    #[test]
    fn ensure_skill_frontmatter_adds_missing_header() {
        let content = "# 股票评价\n\n对输入股票做分析。\n";
        let normalized = ensure_skill_frontmatter(content, "stock-evaluation", "多维度股票分析");
        assert!(normalized.starts_with("---\n"));
        assert!(normalized.contains("name: \"stock-evaluation\""));
        assert!(normalized.contains("description: \"多维度股票分析\""));
        assert!(normalized.contains("# 股票评价"));
    }

    #[test]
    fn ensure_skill_frontmatter_fills_empty_fields() {
        let content = "---\nname: \ndescription:\ntags: [lab]\n---\n# Body\n";
        let normalized = ensure_skill_frontmatter(content, "stock-evaluation", "多维度股票分析");
        assert!(normalized.contains("name: \"stock-evaluation\""));
        assert!(normalized.contains("description: \"多维度股票分析\""));
        assert!(normalized.contains("tags: [lab]"));
        assert!(normalized.contains("# Body"));
    }

    #[test]
    fn extract_skill_description_fallback_skips_headings() {
        let content = "# 股票评价\n\n对输入股票做分析。\n";
        assert_eq!(
            extract_skill_description_fallback(content),
            Some("对输入股票做分析。".to_string())
        );
    }

    #[test]
    fn ensure_skill_frontmatter_overwrites_existing_name() {
        let content = "---\nname: \"股票评价\"\ndescription: \"旧描述\"\n---\n# Body\n";
        let normalized = ensure_skill_frontmatter(content, "stock-evaluation", "多维度股票分析");
        assert!(normalized.contains("name: \"stock-evaluation\""));
        assert!(normalized.contains("description: \"旧描述\""));
    }

    #[test]
    fn slugify_skill_id_from_english_and_mixed_text() {
        assert_eq!(slugify_skill_id("Stock Evaluation"), "stock-evaluation");
        assert_eq!(slugify_skill_id("code_review_helper"), "code-review-helper");
        assert_eq!(slugify_skill_id("  Hello--World  "), "hello-world");
        assert_eq!(slugify_skill_id("股票评价"), "untitled-skill");
    }

    #[test]
    fn derive_preferred_skill_id_prefers_standard_english_name() {
        assert_eq!(
            derive_preferred_skill_id(
                "stock-evaluation",
                Some("股票评价"),
                "对股票做分析",
                "lab-123"
            ),
            "stock-evaluation"
        );
        assert_eq!(
            derive_preferred_skill_id("股票评价", Some("股票评价"), "对股票做分析", "lab-123"),
            "generated-skill"
        );
        assert!(!looks_like_temporary_lab_id("stock-evaluation"));
        assert!(looks_like_temporary_lab_id("lab-1783428061706"));
    }

    #[test]
    fn allocate_unique_skill_id_avoids_existing_names() {
        let lab = tempfile::tempdir().unwrap();
        let prod = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(lab.path().join("stock-evaluation")).unwrap();
        let allocated =
            allocate_unique_skill_id(lab.path(), prod.path(), "stock-evaluation", "lab-1");
        assert_eq!(allocated, "stock-evaluation-2");
    }
}
