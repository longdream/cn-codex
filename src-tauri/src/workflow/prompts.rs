use crate::thread_store::ThreadMessage;

const EXTRACTION_SYSTEM_PROMPT: &str = r#"你是一个 Workflow 提取专家。你的任务是分析一段对话记录（包含用户消息、AI 回复、工具调用及其结果），将其中的操作流程提取为一个结构化的、可复用的 Workflow。**特别关注失败的工具调用，将其归纳为反例经验。**

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
   - knownFailures: 该节点已知的失败案例数组（可选），每条包含：
     * error: 失败现象
     * cause: 失败根因
     * fix: 正确做法/避坑建议
8. totalEstimatedTokens: 整个 workflow 预估总 token 数
9. createdAt: 可选，ISO 时间字符串；如未提供由系统自动回填
10. 类型约束（必须严格遵守）：
   - nodeId 和 dependsOn 数组元素必须是 JSON 字符串（例如 "step_1"），不能写成数字或布尔值
   - variables 里的 default 必须是字符串（例如 "60"），不能写成 60 或 true
   - tokenBudget 和 totalEstimatedTokens 必须是数字，不能加引号

提取原则：
- 只提取有意义的操作步骤，忽略纯聊天/确认/闲聊
- 将具体值（文件路径、项目名等）抽象为变量
- 合并琐碎的连续同类操作为一个节点
- 每个节点应是一个有明确目标的独立步骤
- tokenBudget 应反映该节点所需的上下文大小
- **重要：仔细观察对话中每个工具调用的执行结果（success/failed），将失败的工具调用归纳为 knownFailures 反例。失败案例是宝贵的经验，必须提取到对应节点中，帮助后续用户避开相同错误。**

JSON 格式示例：
```json
{
  "name": "setup-react-project",
  "title": "初始化 React 项目",
  "description": "创建新的 React 项目并配置基本开发环境",
  "createdAt": "2026-06-22T00:00:00Z",
  "triggerPhrases": ["创建 React 项目", "初始化前端项目", "新建 React 应用"],
  "variables": {
    "projectName": { "type": "string", "description": "项目名称", "default": "my-app" },
    "timeoutSec": { "type": "string", "description": "命令超时时间（秒）", "default": "60" }
  },
  "nodes": [
    {
      "nodeId": "step_1",
      "objective": "使用 create-react-app 创建项目",
      "tools": ["shell"],
      "argsHints": { "shell": { "command": "npx create-react-app {{projectName}}" } },
      "dependsOn": [],
      "expectedOutput": "项目目录创建成功",
      "tokenBudget": 1500,
      "knownFailures": [
        {
          "error": "npx 报错 'create-react-app' 不是内部命令",
          "cause": "Node.js 版本过低或 npx 未正确安装",
          "fix": "先执行 'node -v' 确认 Node.js >= 16，必要时使用 'npm install -g create-react-app' 或改用 'npm create vite@latest {{projectName}}'"
        }
      ]
    }
  ],
  "totalEstimatedTokens": 5000
}
```"#;

/// Build messages for the LLM extraction call.
pub fn build_extraction_messages(history: &[ThreadMessage]) -> Vec<(String, String)> {
    let mut messages = Vec::new();
    messages.push(("system".to_string(), EXTRACTION_SYSTEM_PROMPT.to_string()));

    let conversation_summary = summarize_thread_for_extraction(history);
    messages.push(("user".to_string(), format!(
        "请分析以下对话记录，提取为一个结构化 Workflow。只输出 JSON，不要其他文字。\n\n{conversation_summary}"
    )));

    messages
}

/// Summarize a thread into a compact format suitable for workflow extraction.
/// Includes tool call results and success/failure markers to help the LLM extract
/// known failures as counter-examples.
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
                    summary.push_str(&format!("  工具: {} | 参数: {}\n", tc.name, args_preview));
                }
                summary.push('\n');
                continue;
            }
        }

        // Tool result / error messages (role == "tool")
        if role == "tool" && !content.is_empty() {
            // Highlight failure markers for LLM to notice
            let content_lower = content.to_lowercase();
            let status_marker = if content_lower.contains("error")
                || content_lower.contains("fail")
                || content_lower.contains("exit code: 1")
                || content_lower.contains("exit code: 2")
                || content_lower.contains("❌")
            {
                " [状态: 失败]"
            } else if content_lower.contains("exit code: 0")
                || content_lower.contains("success")
                || content_lower.contains("✅")
            {
                " [状态: 成功]"
            } else {
                ""
            };
            let truncated = truncate_str(content, 400);
            summary.push_str(&format!(
                "[tool-result]{status_marker} {truncated}\n\n"
            ));
            continue;
        }

        if role == "user" || (role == "assistant" && !content.is_empty()) {
            let truncated = truncate_str(content, 300);
            summary.push_str(&format!("[{role}] {truncated}\n\n"));
        }
    }

    if summary.len() > 16000 {
        summary.truncate(16000);
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

#[cfg(test)]
mod tests {
    use super::parse_extraction_output;

    #[test]
    fn parse_extraction_output_accepts_missing_created_at() {
        let raw = r#"{
  "name": "workflow-a",
  "title": "测试流程",
  "description": "用于测试 createdAt 缺失",
  "nodes": [
    {
      "nodeId": "step_1",
      "objective": "执行第一步",
      "tools": ["shell"]
    }
  ]
}"#;

        let parsed = parse_extraction_output(raw).expect("should parse without createdAt");
        assert_eq!(parsed.name, "workflow-a");
        assert_eq!(parsed.created_at, "");
        assert_eq!(parsed.nodes.len(), 1);
    }

    #[test]
    fn parse_extraction_output_accepts_fenced_json_without_created_at() {
        let raw = r#"
提取结果如下：
```json
{
  "name": "workflow-b",
  "title": "围栏测试",
  "description": "fenced json 解析",
  "nodes": []
}
```
"#;

        let parsed = parse_extraction_output(raw).expect("should parse fenced json");
        assert_eq!(parsed.name, "workflow-b");
        assert_eq!(parsed.created_at, "");
    }

    #[test]
    fn parse_extraction_output_coerces_string_like_fields() {
        let raw = r#"{
  "name": "workflow-coercion",
  "title": "容错提取",
  "description": "测试 integer/bool 到字符串字段的容错",
  "variables": {
    "timeoutSec": { "type": "string", "description": "超时秒数", "default": 60 },
    "dryRun": { "type": "string", "description": "是否演练", "default": true }
  },
  "nodes": [
    {
      "nodeId": 1,
      "objective": "执行脚本",
      "tools": ["shell"],
      "dependsOn": [1]
    }
  ],
  "totalEstimatedTokens": 1200
}"#;

        let parsed = parse_extraction_output(raw).expect("should parse coercible workflow json");
        assert_eq!(parsed.nodes[0].node_id, "1");
        assert_eq!(parsed.nodes[0].depends_on, vec!["1".to_string()]);
        assert_eq!(
            parsed
                .variables
                .get("timeoutSec")
                .and_then(|variable| variable.default.as_deref()),
            Some("60")
        );
        assert_eq!(
            parsed
                .variables
                .get("dryRun")
                .and_then(|variable| variable.default.as_deref()),
            Some("true")
        );
    }
}
