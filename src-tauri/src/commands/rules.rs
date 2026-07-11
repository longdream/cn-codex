use std::io::ErrorKind;
use std::path::Path;

use tauri::State;

use crate::error::AppResult;
use crate::state::AppState;

const USER_RULES_FILE: &str = "user-rules.md";
const DEFAULT_USER_RULES: &str = r#"# 默认规则

- 默认使用简体中文回复，除非用户明确要求使用其他语言。
- 处理中文内容时，确保输出为正常可读的中文，不要出现乱码、错码、异常转义或不可读字符。
- 生成代码、注释、文档、JSON、TOML、YAML、Markdown 等内容时，如包含中文，优先按 UTF-8 语义处理；若原文件已有明确编码或格式约束，则保持一致。
- 输出内容保持清晰格式：标题、列表、表格、引用、代码块使用标准 Markdown；多行代码块必须带语言标识。
- 如果发现输入内容本身已经乱码，先指出并给出修正后的可读版本，再继续处理后续任务。
"#;

pub(crate) fn read_user_rules_with_default(workspace_config_dir: &Path) -> String {
    let path = workspace_config_dir.join(USER_RULES_FILE);
    match std::fs::read_to_string(&path) {
        Ok(content) => content,
        Err(err) if err.kind() == ErrorKind::NotFound => {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let default = DEFAULT_USER_RULES.to_string();
            let _ = std::fs::write(&path, &default);
            default
        }
        Err(_) => DEFAULT_USER_RULES.to_string(),
    }
}

#[tauri::command]
pub async fn rules_read(state: State<'_, AppState>) -> AppResult<String> {
    Ok(read_user_rules_with_default(&state.workspace_config_dir))
}

#[tauri::command]
pub async fn rules_write(state: State<'_, AppState>, content: String) -> AppResult<()> {
    let path = state.workspace_config_dir.join(USER_RULES_FILE);
    std::fs::write(&path, content)?;
    Ok(())
}
