use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Emitter, State};
use tracing::info;

use crate::adapter;
use crate::adapter::types::InternalMessage;
use crate::error::{AppError, AppResult};
use crate::protocol::UserAttachment;
use crate::state::AppState;

/// Skill 实验室草稿的隔离存储目录: codey/skills-lab/
fn get_skill_lab_dir(state: &AppState) -> PathBuf {
    state.workspace_config_dir.join("skills-lab")
}

/// 实验室中单个 skill 草稿的目录
fn get_skill_lab_entry_dir(state: &AppState, skill_id: &str) -> PathBuf {
    get_skill_lab_dir(state).join(skill_id)
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
    pub content: String,
    pub test_prompt: String,
    pub status: String,
    pub iteration_count: u32,
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
    test_prompt: String,
    status: String,
    iteration_count: u32,
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
            if let Ok(meta) = serde_json::from_str::<SkillLabMeta>(&content) {
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
    let meta: SkillLabMeta = serde_json::from_str(&meta_str)
        .map_err(|e| AppError::Custom(format!("Failed to parse meta: {e}")))?;

    let content = if skill_md_path.exists() {
        std::fs::read_to_string(&skill_md_path).unwrap_or_default()
    } else {
        String::new()
    };

    Ok(SkillLabDetail {
        id: skill_id,
        name: meta.name,
        content,
        test_prompt: meta.test_prompt,
        status: meta.status,
        iteration_count: meta.iteration_count,
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
    pub content: String,
    pub test_prompt: String,
}

#[tauri::command]
pub async fn skill_lab_save(
    state: State<'_, AppState>,
    params: SkillLabSaveParams,
) -> AppResult<()> {
    let dir = get_skill_lab_entry_dir(&state, &params.skill_id);
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
        name: params.name,
        test_prompt: params.test_prompt,
        status: existing_meta
            .as_ref()
            .map(|m| m.status.clone())
            .unwrap_or_else(|| "idle".to_string()),
        iteration_count: existing_meta
            .as_ref()
            .map(|m| m.iteration_count)
            .unwrap_or(0),
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

    Ok(())
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
         6) JSON 必须合法，不要加解释文字。"
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

    Ok(SkillLabGenerateFromGoalResult {
        name,
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
pub async fn skill_lab_promote(state: State<'_, AppState>, skill_id: String) -> AppResult<()> {
    let lab_dir = get_skill_lab_entry_dir(&state, &skill_id);
    let skill_md_src = lab_dir.join("SKILL.md");

    if !skill_md_src.exists() {
        return Err(AppError::Custom(format!(
            "Skill lab entry has no SKILL.md: {skill_id}"
        )));
    }

    // 正式 Skill 存储在 codey/skills/<id>/SKILL.md
    let prod_dir = state.workspace_config_dir.join("skills").join(&skill_id);
    std::fs::create_dir_all(&prod_dir)
        .map_err(|e| AppError::Custom(format!("Failed to create skills dir: {e}")))?;

    let content = std::fs::read_to_string(&skill_md_src)
        .map_err(|e| AppError::Custom(format!("Failed to read lab SKILL.md: {e}")))?;
    std::fs::write(prod_dir.join("SKILL.md"), &content)
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
    let meta_path = lab_dir.join("meta.json");
    if let Ok(meta_str) = std::fs::read_to_string(&meta_path) {
        if let Ok(mut meta) = serde_json::from_str::<SkillLabMeta>(&meta_str) {
            meta.status = "promoted".to_string();
            if let Ok(json) = serde_json::to_string_pretty(&meta) {
                let _ = std::fs::write(&meta_path, json);
            }
        }
    }

    Ok(())
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

/// 最大自动改写迭代次数
const MAX_EVOLUTION_ITERATIONS: u32 = 12;
const HIGH_SCORE_THRESHOLD: f64 = 90.0;
const SCORE_PLATEAU_DELTA: f64 = 1.0;
const SCORE_PLATEAU_ROUNDS: u32 = 2;

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

fn should_stop_evolution(total_score: f64, stable_rounds: u32, iteration: u32) -> bool {
    (total_score >= HIGH_SCORE_THRESHOLD && stable_rounds >= SCORE_PLATEAU_ROUNDS)
        || iteration >= MAX_EVOLUTION_ITERATIONS
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

    for iteration in 0..MAX_EVOLUTION_ITERATIONS {
        iterations = iteration + 1;
        info!(
            "Skill lab evolution iteration {iterations}/{MAX_EVOLUTION_ITERATIONS} for {skill_id}"
        );

        // 通知前端当前阶段
        let _ = app_handle.emit(
            "skill-lab-progress",
            serde_json::json!({
                "skillId": &skill_id,
                "phase": "testing",
                "iteration": iterations,
            }),
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

        last_output = call_ai_non_streaming(
            &http,
            &*adapter,
            &base_url,
            &api_key,
            &model,
            &test_messages,
            provider_info.query_params.as_ref(),
            provider_info.http_headers.as_ref(),
        )
        .await
        .map_err(|e| AppError::Custom(format!("Test call failed: {e}")))?;

        // ── 步骤2：AI 评估 ──
        let _ = app_handle.emit(
            "skill-lab-progress",
            serde_json::json!({
                "skillId": &skill_id,
                "phase": "evaluating",
                "iteration": iterations,
            }),
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
            &eval_messages,
            provider_info.query_params.as_ref(),
            provider_info.http_headers.as_ref(),
        )
        .await
        .map_err(|e| AppError::Custom(format!("Evaluation call failed: {e}")))?;
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

        if should_stop_evolution(total_score, stable_rounds, iteration + 1) {
            evolution_converged =
                total_score >= HIGH_SCORE_THRESHOLD && stable_rounds >= SCORE_PLATEAU_ROUNDS;
            break;
        }

        // ── 步骤3：自动改写 ──
        let _ = app_handle.emit(
            "skill-lab-progress",
            serde_json::json!({
                "skillId": &skill_id,
                "phase": "rewriting",
                "iteration": iterations,
            }),
        );

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
             ## 改进建议\n{improve_hints}\n\n\
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
            &rewrite_messages,
            provider_info.query_params.as_ref(),
            provider_info.http_headers.as_ref(),
        )
        .await
        .map_err(|e| AppError::Custom(format!("Rewrite call failed: {e}")))?;

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

    if let Ok(json) = serde_json::to_string_pretty(&meta) {
        let _ = std::fs::write(&meta_path, json);
    }

    // 通知前端完成
    let _ = app_handle.emit(
        "skill-lab-progress",
        serde_json::json!({
            "skillId": &skill_id,
            "phase": "done",
            "status": &final_status,
            "iteration": iterations,
            "bestScore": best_score,
            "stableRounds": stable_rounds,
            "converged": evolution_converged,
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
    messages: &[InternalMessage],
    query_params: Option<&HashMap<String, String>>,
    extra_headers: Option<&HashMap<String, String>>,
) -> Result<String, String> {
    let url = adapter.build_url(base_url, model);
    let headers = adapter.build_headers(api_key);
    let (url, headers) =
        crate::adapter::apply_request_overrides(url, headers, query_params, extra_headers)
            .map_err(|e| format!("Request override error: {e}"))?;
    // ponytail: 非流式请求设 stream=false 但部分 adapter 的 build_body
    // 默认会设 stream=true，这里构建后手动覆盖
    let mut body = adapter.build_body(model, messages, None, None);
    if let Some(obj) = body.as_object_mut() {
        obj.insert("stream".to_string(), serde_json::Value::Bool(false));
    }

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
            test_prompt: "Say hello".to_string(),
            status: "idle".to_string(),
            iteration_count: 0,
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
        assert!(should_stop_evolution(92.0, 2, 6));
        assert!(!should_stop_evolution(92.0, 1, 6));
    }

    #[test]
    fn should_stop_when_reaching_max_iterations() {
        assert!(should_stop_evolution(70.0, 0, MAX_EVOLUTION_ITERATIONS));
        assert!(!should_stop_evolution(
            70.0,
            0,
            MAX_EVOLUTION_ITERATIONS - 1
        ));
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
}
