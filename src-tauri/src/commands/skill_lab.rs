use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::{AppHandle, Emitter, State};
use tracing::info;

use crate::adapter;
use crate::adapter::types::InternalMessage;
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
        iteration_count: existing_meta.as_ref().map(|m| m.iteration_count).unwrap_or(0),
        last_test_result: existing_meta.as_ref().and_then(|m| m.last_test_result.clone()),
        last_evaluation: existing_meta.as_ref().and_then(|m| m.last_evaluation.clone()),
    };

    let meta_json = serde_json::to_string_pretty(&meta)
        .map_err(|e| AppError::Custom(format!("Failed to serialize meta: {e}")))?;

    std::fs::write(dir.join("meta.json"), meta_json)
        .map_err(|e| AppError::Custom(format!("Failed to write meta: {e}")))?;
    std::fs::write(dir.join("SKILL.md"), &params.content)
        .map_err(|e| AppError::Custom(format!("Failed to write SKILL.md: {e}")))?;

    Ok(())
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
#[tauri::command]
pub async fn skill_lab_promote(
    state: State<'_, AppState>,
    skill_id: String,
) -> AppResult<()> {
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
pub async fn skill_lab_delete(
    state: State<'_, AppState>,
    skill_id: String,
) -> AppResult<()> {
    let dir = get_skill_lab_entry_dir(&state, &skill_id);
    if dir.exists() {
        std::fs::remove_dir_all(&dir)
            .map_err(|e| AppError::Custom(format!("Failed to delete skill lab entry: {e}")))?;
    }
    Ok(())
}

// ── Skill Lab 自动测试闭环 ──────────────────────────────────

/// 最大自动改写迭代次数
const MAX_REWRITE_ITERATIONS: u32 = 3;

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
    let base_url = provider_info.resolve_base_url().ok_or_else(|| {
        AppError::Custom("No base URL configured for provider".to_string())
    })?;
    let api_key = provider_info.resolve_api_key().unwrap_or_default();
    let wire_api = provider_info
        .wire_api
        .as_deref()
        .unwrap_or("chat")
        .to_string();

    let http = reqwest::Client::new();
    let adapter = adapter::get_adapter(&wire_api);

    let mut last_output = String::new();
    let mut evaluation = String::new();
    let mut iterations = 0u32;
    let mut passed = false;

    for iteration in 0..MAX_REWRITE_ITERATIONS {
        iterations = iteration + 1;
        info!(
            "Skill lab test iteration {iterations}/{MAX_REWRITE_ITERATIONS} for {skill_id}"
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
            请判断该输出是否符合 Skill 指令的要求。\
            回复格式：第一行写 PASS 或 FAIL，后面给出简要理由（3-5句）。";
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

        evaluation = call_ai_non_streaming(
            &http,
            &*adapter,
            &base_url,
            &api_key,
            &model,
            &eval_messages,
        )
        .await
        .map_err(|e| AppError::Custom(format!("Evaluation call failed: {e}")))?;

        let eval_first_line = evaluation.lines().next().unwrap_or("").trim().to_uppercase();
        if eval_first_line.contains("PASS") {
            passed = true;
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
            根据评审员的反馈改进 Skill 指令内容，使其能产生更好的输出。\
            只返回改进后的完整 SKILL.md 内容，不要包含任何解释。";
        let rewrite_user = format!(
            "## 原始 Skill 指令\n{skill_content}\n\n\
             ## 测试提示词\n{test_prompt}\n\n\
             ## AI 输出\n{last_output}\n\n\
             ## 评审反馈\n{evaluation}\n\n\
             请输出改进后的完整 Skill 指令内容："
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
    let final_status = if passed {
        "passed".to_string()
    } else {
        "failed".to_string()
    };

    meta.status = final_status.clone();
    meta.iteration_count += iterations;
    meta.last_test_result = Some(last_output.clone());
    meta.last_evaluation = Some(evaluation.clone());

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
        }),
    );

    Ok(SkillLabTestResult {
        status: final_status,
        iterations,
        last_output,
        evaluation,
        final_content: skill_content,
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
) -> Result<String, String> {
    let url = adapter.build_url(base_url, model);
    let headers = adapter.build_headers(api_key);
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
        format!("Could not extract text from API response: {}", &body_text[..body_text.len().min(500)])
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
    }
}
