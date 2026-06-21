use crate::thread_store::ThreadMessage;

const EXTRACTION_SYSTEM_PROMPT: &str = r#"你是一个 Workflow 提取专家。你的任务是分析一段对话记录（包含用户消息、AI 回复、工具调用），将其中的操作流程提取为一个结构化的、可复用的 Workflow。

输出要求：
1. 输出一个 JSON 对象，格式如下（不要添加多余文字，只输出 JSON）
2. name: URL 安全的 slug 标识符（小写英文+连字符）
3. title: 简洁的中文标题
4. description: 一句话描述这个 workflow 做什么
5. triggerPhrases: 3-5 个触发短语，用户说这些话时应该触发此 workflow
6. variables: 提取可参数化的变量（如路径、名称等），使其可复用
7. nodes: 按执行顺序排列的节点数组，每个节点包含：
   - nodeId: step_1, step_2, ...
   - objective: 该步骤的目标（一句话）
   - tools: 该步骤使用的工具名称数组
   - argsHints: 工具参数模板（用 {{变量名}} 标记可替换部分）
   - dependsOn: 依赖的前置节点 ID 数组
   - expectedOutput: 期望输出描述
   - tokenBudget: 预估该节点消耗的 token 数
8. totalEstimatedTokens: 整个 workflow 预估总 token 数

提取原则：
- 只提取有意义的操作步骤，忽略纯聊天/确认/闲聊
- 将具体值（文件路径、项目名等）抽象为变量
- 合并琐碎的连续同类操作为一个节点
- 每个节点应是一个有明确目标的独立步骤
- tokenBudget 应反映该节点所需的上下文大小

JSON 格式示例：
```json
{
  "name": "setup-react-project",
  "title": "初始化 React 项目",
  "description": "创建新的 React 项目并配置基本开发环境",
  "triggerPhrases": ["创建 React 项目", "初始化前端项目", "新建 React 应用"],
  "variables": {
    "projectName": { "type": "string", "description": "项目名称", "default": "my-app" }
  },
  "nodes": [
    {
      "nodeId": "step_1",
      "objective": "使用 create-react-app 创建项目",
      "tools": ["shell"],
      "argsHints": { "shell": { "command": "npx create-react-app {{projectName}}" } },
      "dependsOn": [],
      "expectedOutput": "项目目录创建成功",
      "tokenBudget": 1500
    }
  ],
  "totalEstimatedTokens": 5000
}
```"#;

/// Build messages for the LLM extraction call.
pub fn build_extraction_messages(
    history: &[ThreadMessage],
) -> Vec<(String, String)> {
    let mut messages = Vec::new();
    messages.push(("system".to_string(), EXTRACTION_SYSTEM_PROMPT.to_string()));

    let conversation_summary = summarize_thread_for_extraction(history);
    messages.push(("user".to_string(), format!(
        "请分析以下对话记录，提取为一个结构化 Workflow。只输出 JSON，不要其他文字。\n\n{conversation_summary}"
    )));

    messages
}

/// Summarize a thread into a compact format suitable for workflow extraction.
/// Only includes tool calls and their surrounding context to save tokens.
fn summarize_thread_for_extraction(history: &[ThreadMessage]) -> String {
    let mut summary = String::new();
    summary.push_str("## 对话记录\n\n");

    for msg in history {
        let role = &msg.role;
        let content = &msg.content;

        if let Some(tool_calls) = &msg.tool_calls {
            if !tool_calls.is_empty() {
                summary.push_str(&format!("[{role}] (含工具调用)\n"));
                if !content.is_empty() {
                    let truncated = truncate_str(content, 200);
                    summary.push_str(&format!("  文本: {truncated}\n"));
                }
                for tc in tool_calls {
                    let args_preview = truncate_str(&tc.arguments, 300);
                    summary.push_str(&format!(
                        "  工具: {} | 参数: {}\n",
                        tc.name, args_preview,
                    ));
                }
                summary.push('\n');
                continue;
            }
        }

        // Tool result messages (role == "tool")
        if role == "tool" && !content.is_empty() {
            let truncated = truncate_str(content, 200);
            summary.push_str(&format!("[tool-result] {truncated}\n\n"));
            continue;
        }

        if role == "user" || (role == "assistant" && !content.is_empty()) {
            let truncated = truncate_str(content, 300);
            summary.push_str(&format!("[{role}] {truncated}\n\n"));
        }
    }

    if summary.len() > 12000 {
        summary.truncate(12000);
        summary.push_str("\n...(truncated)\n");
    }

    summary
}

fn truncate_str(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        let end: String = s.chars().take(max_chars).collect();
        format!("{end}...")
    }
}

/// Parse the LLM extraction output (JSON) into a WorkflowDef.
pub fn parse_extraction_output(raw: &str) -> Result<super::WorkflowDef, String> {
    let json_str = extract_json_block(raw);
    serde_json::from_str(json_str).map_err(|e| format!("Failed to parse workflow JSON: {e}"))
}

/// Extract a JSON block from LLM output (may be wrapped in ```json ... ```).
fn extract_json_block(raw: &str) -> &str {
    let trimmed = raw.trim();
    if let Some(start) = trimmed.find("```json") {
        let after_fence = &trimmed[start + 7..];
        if let Some(end) = after_fence.find("```") {
            return after_fence[..end].trim();
        }
    }
    if let Some(start) = trimmed.find("```") {
        let after_fence = &trimmed[start + 3..];
        if let Some(newline) = after_fence.find('\n') {
            let content = &after_fence[newline + 1..];
            if let Some(end) = content.find("```") {
                return content[..end].trim();
            }
        }
    }
    if trimmed.starts_with('{') {
        return trimmed;
    }
    trimmed
}
