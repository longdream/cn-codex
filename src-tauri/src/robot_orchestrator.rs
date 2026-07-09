use std::path::{Path, PathBuf};

use tracing::warn;

use crate::agent::truncate_utf8_by_bytes;
use crate::error::{AppError, AppResult};
use crate::robot_loader;
use crate::thread_store::{ThreadGoalStatus, ThreadMessage, ThreadRobotState, ThreadStore};

/// 机器人节点完成信号：模型在“当前节点完成”时必须输出该标记。
/// 该标记只用于运行时流程控制，不会直接展示给用户。
pub const ROBOT_NODE_DONE_SENTINEL: &str = "<workflow_node_done/>";

/// 节点交付总结标记（起始/结束）：模型在完成当前节点时，需将本节点交付内容
/// 包裹于该标记内，作为交接给下一节点的“纯净总结”，替代上游原始杂乱历史。
/// 该标记同样只用于运行时流程控制，不会直接展示给用户。
pub const ROBOT_NODE_SUMMARY_SENTINEL: &str = "<workflow_node_summary>";
pub const ROBOT_NODE_SUMMARY_END_SENTINEL: &str = "</workflow_node_summary>";

/// 节点推进结果：
/// - ContinueCurrent: 当前节点未完成，继续留在本节点；
/// - Advanced: 已推进到下一节点，并完成 goal 目标重绑定；
/// - Completed: 全部节点完成，机器人运行态已清理。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeProgressResult {
    ContinueCurrent {
        state: ThreadRobotState,
        nudge: String,
    },
    Advanced {
        state: ThreadRobotState,
        nudge: String,
    },
    Completed,
}

/// 机器人编排器：
/// - 仅负责“机器人外层流程编排”；
/// - 不负责通用 goal/chat 主流程，降低与 agent 主链路耦合。
pub struct RobotOrchestrator {
    workspace_config_dir: PathBuf,
}

impl RobotOrchestrator {
    /// 以工作区根目录创建编排器，内部固定读取 `codey/` 配置目录。
    pub fn new(workspace_root: &Path) -> Self {
        Self {
            workspace_config_dir: workspace_root.join("codey"),
        }
    }

    /// 准备机器人运行态：
    /// - 若已有同机器人且有效状态，则复用；
    /// - 否则按固定 workflowNodes 顺序编译 runtime_nodes；
    /// - 完成后将当前节点目标绑定到真实 thread goal objective。
    pub async fn prepare_state(
        &self,
        thread_store: &ThreadStore,
        thread_id: &str,
        robot_id: &str,
        user_objective: &str,
    ) -> AppResult<ThreadRobotState> {
        if let Some(mut existing_state) = thread_store.get_thread_robot_state(thread_id).await {
            if existing_state.robot_id == robot_id && !existing_state.runtime_nodes.is_empty() {
                // 防御式修正：兼容旧状态或异常状态导致的越界索引。
                if existing_state.current_node_index >= existing_state.runtime_nodes.len() {
                    existing_state.current_node_index =
                        existing_state.runtime_nodes.len().saturating_sub(1);
                    thread_store
                        .set_thread_robot_state(thread_id, existing_state.clone())
                        .await?;
                }
                self.bind_goal_to_current_node(thread_store, thread_id, &existing_state)
                    .await?;
                return Ok(existing_state);
            }
        }

        let detail = robot_loader::read_robot(&self.workspace_config_dir, robot_id)
            .ok_or_else(|| AppError::Custom(format!("Robot not found: {robot_id}")))?;
        let fixed_nodes = detail.config.normalized_workflow_nodes();
        if fixed_nodes.is_empty() {
            return Err(AppError::Custom(format!(
                "Robot workflow is empty: {robot_id}. Please configure workflowNodes first."
            )));
        }

        let root_objective = normalize_root_objective(user_objective);
        let runtime_nodes = compile_runtime_nodes(&root_objective, &fixed_nodes);
        if runtime_nodes.is_empty() {
            return Err(AppError::Custom(format!(
                "Robot runtime plan is empty: {robot_id}. Please check workflowNodes objectives."
            )));
        }

        let state = ThreadRobotState {
            robot_id: robot_id.to_string(),
            current_node_index: 0,
            root_objective,
            runtime_nodes,
            node_deliveries: Vec::new(),
            current_node_start_message_id: None,
        };

        thread_store
            .set_thread_robot_state(thread_id, state.clone())
            .await?;
        self.bind_goal_to_current_node(thread_store, thread_id, &state)
            .await?;
        Ok(state)
    }

    /// 构造“机器人节点 overlay system prompt”：
    /// - 只注入当前节点要求与节点技能；
    /// - 明确声明“补充 goal 模式，不覆盖 goal 主 system prompt”。
    pub fn build_overlay_prompt(&self, state: &ThreadRobotState) -> AppResult<String> {
        if state.runtime_nodes.is_empty() {
            return Err(AppError::Custom(
                "Robot runtime nodes are empty. Cannot build overlay prompt.".to_string(),
            ));
        }

        let detail = robot_loader::read_robot(&self.workspace_config_dir, &state.robot_id)
            .ok_or_else(|| AppError::Custom(format!("Robot not found: {}", state.robot_id)))?;
        let fixed_nodes = detail.config.normalized_workflow_nodes();
        if fixed_nodes.is_empty() {
            return Err(AppError::Custom(format!(
                "Robot workflow is empty: {}",
                state.robot_id
            )));
        }

        let safe_runtime_index = state
            .current_node_index
            .min(state.runtime_nodes.len().saturating_sub(1));
        let safe_skill_index = safe_runtime_index.min(fixed_nodes.len().saturating_sub(1));
        let current_runtime_objective = state
            .runtime_nodes
            .get(safe_runtime_index)
            .cloned()
            .unwrap_or_default();
        let current_fixed_node = fixed_nodes.get(safe_skill_index).ok_or_else(|| {
            AppError::Custom("Current robot node is missing in fixed workflow".to_string())
        })?;

        let mut workflow_block = String::new();
        for (index, node_objective) in state.runtime_nodes.iter().enumerate() {
            let marker = if index < safe_runtime_index {
                "DONE"
            } else if index == safe_runtime_index {
                "CURRENT"
            } else {
                "PENDING"
            };
            workflow_block.push_str(&format!(
                "- [{marker}] Node {}: {}\n",
                index + 1,
                node_objective
            ));
        }

        let local_skills_block = if current_fixed_node.skills.is_empty() {
            "(none; rely on plugin skills for this node)".to_string()
        } else {
            current_fixed_node
                .skills
                .iter()
                .map(|skill| format!("- {skill}"))
                .collect::<Vec<_>>()
                .join("\n")
        };
        let plugin_skills_block = if current_fixed_node.plugin_skills.is_empty() {
            "(none; rely on local skills for this node)".to_string()
        } else {
            current_fixed_node
                .plugin_skills
                .iter()
                .map(|skill| format!("- {}/{}", skill.plugin_id, skill.skill_id))
                .collect::<Vec<_>>()
                .join("\n")
        };

        let skill_contents = robot_loader::collect_robot_node_skill_contents(
            &self.workspace_config_dir,
            &detail.config,
            safe_skill_index,
        );
        let mut skills_block = String::new();
        for (label, content) in &skill_contents {
            skills_block.push_str(&format!("\n--- Skill: {label} ---\n"));
            let truncated = if content.len() > 8000 {
                &content[..8000]
            } else {
                content.as_str()
            };
            skills_block.push_str(truncated);
            skills_block.push('\n');
        }
        if skills_block.trim().is_empty() {
            skills_block.push_str("No skill markdown was found for the current workflow node.\n");
        }

        // 已完成节点交付给当前节点的“纯净总结”：只携带交付结论，不携带上游原始过程历史。
        let deliveries_block = if state.node_deliveries.is_empty() {
            "(none; this is the first node or prior nodes produced no deliveries)".to_string()
        } else {
            state
                .node_deliveries
                .iter()
                .enumerate()
                .map(|(index, delivery)| format!("- Node {} delivery: {}", index + 1, delivery))
                .collect::<Vec<_>>()
                .join("\n")
        };

        Ok(format!(
            "## Robot Workflow Overlay\n\
             This overlay augments Goal mode and MUST NOT replace the base Goal-mode system prompt.\n\
             \n\
             ## Root Objective\n\
             {root_objective}\n\
             \n\
             ## Workflow Progress\n\
             {workflow_block}\n\
             \n\
             ## Completed Node Deliveries\n\
             The following are concise handoffs from already-completed upstream nodes. \
             They REPLACE the raw upstream conversation history, so rely on them instead of \
             guessing what prior nodes did.\n\
             {deliveries_block}\n\
             \n\
             ## Current Node Objective\n\
             {current_objective}\n\
             \n\
             ## Current Node Assigned Local Skills\n\
             {local_skills_block}\n\
             \n\
             ## Current Node Assigned Plugin Skills\n\
             {plugin_skills_block}\n\
             \n\
             ## Current Node Loaded Skills\n\
             {skills_block}\n\
             \n\
             STRICT WORKFLOW ENFORCEMENT:\n\
             - Work ONLY on CURRENT node.\n\
             - Do NOT skip nodes.\n\
             - Do NOT claim done without tool evidence.\n\
             - Do NOT call `update_goal` with status `complete` to finish a node. Node completion \
             must be signaled with `{done_marker}`, and the orchestrator will advance or finish the \
             overall workflow for you.\n\
             - When CURRENT node is fully complete, include `{done_marker}` exactly once, and \
             wrap a concise delivery summary (what you produced, decided, or changed for the next \
             node) inside `{summary_marker}` ... `{summary_end_marker}`.\n\
             - If node is not complete, do NOT output `{done_marker}` or `{summary_marker}`.\n\
             - Keep the delivery summary self-contained: it is the ONLY context the next node gets \
             about this node, so include concrete artifacts, decisions, file paths, and results.",
            root_objective = state.root_objective,
            workflow_block = workflow_block,
            deliveries_block = deliveries_block,
            current_objective = current_runtime_objective,
            local_skills_block = local_skills_block,
            plugin_skills_block = plugin_skills_block,
            skills_block = skills_block,
            done_marker = ROBOT_NODE_DONE_SENTINEL,
            summary_marker = ROBOT_NODE_SUMMARY_SENTINEL,
            summary_end_marker = ROBOT_NODE_SUMMARY_END_SENTINEL
        ))
    }

    /// 应用当前轮“节点完成信号”的状态迁移：
    /// - 未完成：保持当前节点；
    /// - 完成且仍有后续节点：推进并重绑 goal objective，并累计当前节点交付总结；
    /// - 完成且已到最后节点：清理机器人状态并将 goal 标记为 complete。
    ///
    /// `delivery_summary` 为模型在输出完成信号时附带的“交付总结”，会被累计进
    /// `node_deliveries`，作为下一节点的纯净交接上下文。为空时回退占位文本，避免链路断裂。
    pub async fn apply_node_progress(
        &self,
        thread_store: &ThreadStore,
        thread_id: &str,
        mut state: ThreadRobotState,
        node_done_signal: bool,
        delivery_summary: Option<String>,
    ) -> AppResult<NodeProgressResult> {
        if state.runtime_nodes.is_empty() {
            return Err(AppError::Custom(
                "Robot runtime nodes are empty. Cannot advance workflow.".to_string(),
            ));
        }

        if state.current_node_index >= state.runtime_nodes.len() {
            state.current_node_index = state.runtime_nodes.len().saturating_sub(1);
            thread_store
                .set_thread_robot_state(thread_id, state.clone())
                .await?;
        }

        if !node_done_signal {
            let nudge = build_robot_node_completion_nudge(
                state.current_node_index,
                state.runtime_nodes.len(),
            );
            return Ok(NodeProgressResult::ContinueCurrent { state, nudge });
        }

        // 累计当前已完成节点的交付总结（截断到 4000 字符，避免总结本身膨胀污染上下文）。
        let completed_index = state.current_node_index;
        let delivery = delivery_summary
            .filter(|s| !s.trim().is_empty())
            .map(|s| truncate_utf8_by_bytes(&s, 4000).to_string())
            .unwrap_or_else(|| {
                format!(
                    "Node {} completed (no explicit delivery summary provided).",
                    completed_index.saturating_add(1)
                )
            });

        let next_index = state.current_node_index.saturating_add(1);
        if next_index >= state.runtime_nodes.len() {
            // 末节点完成：先累计交付总结，再清理运行态并将 goal 标记为 complete。
            state.node_deliveries.push(delivery);
            thread_store
                .set_thread_robot_state(thread_id, state.clone())
                .await?;
            thread_store.clear_thread_robot_state(thread_id).await?;
            // 结束全部节点后尝试把 goal 标记为 complete；失败仅记录日志，不阻断主流程。
            if let Err(err) = thread_store
                .set_thread_goal_status(thread_id, ThreadGoalStatus::Complete)
                .await
            {
                warn!("Failed to mark goal complete after robot workflow finished: {err}");
            }
            return Ok(NodeProgressResult::Completed);
        }

        state.current_node_index = next_index;
        state.node_deliveries.push(delivery);
        thread_store
            .set_thread_robot_state(thread_id, state.clone())
            .await?;
        self.bind_goal_to_current_node(thread_store, thread_id, &state)
            .await?;

        let nudge =
            build_robot_node_advance_prompt(state.current_node_index, state.runtime_nodes.len());
        Ok(NodeProgressResult::Advanced { state, nudge })
    }

    /// 将当前节点目标绑定到真实 thread goal objective。
    /// 说明：如果线程当前还没有 goal，则自动创建 active goal，避免运行态异常。
    async fn bind_goal_to_current_node(
        &self,
        thread_store: &ThreadStore,
        thread_id: &str,
        state: &ThreadRobotState,
    ) -> AppResult<()> {
        let objective = state
            .runtime_nodes
            .get(state.current_node_index)
            .cloned()
            .ok_or_else(|| {
                AppError::Custom("Current robot node objective is missing".to_string())
            })?;

        let has_goal = thread_store
            .get_thread(thread_id)
            .await
            .and_then(|thread| thread.goal)
            .is_some();
        if has_goal {
            thread_store
                .edit_thread_goal(thread_id, objective, None)
                .await
                .map(|_| ())
        } else {
            thread_store
                .set_thread_goal(thread_id, objective, ThreadGoalStatus::Active, None)
                .await
                .map(|_| ())
        }
    }
}

/// 构造喂给模型的“纯净上下文历史”：
/// - 始终保留首个用户消息（原始目标，作为本轮需求种子）；
/// - 仅保留 `current_node_start_message_id` 边界之后的消息（即“当前节点自身”的对话，
///   包含其多轮工具调用/结果），保证当前节点工作记忆不丢失；
/// - 丢弃已完成上游节点的原始杂乱消息——那些信息已由 `node_deliveries` 以总结形式注入 overlay。
///
/// 注意：该函数只影响“喂给模型”的上下文，不修改 thread store 中的原始历史，
/// 因此 UI 可观测性与断点续跑不受影响。
pub fn build_robot_model_history(
    history: &[ThreadMessage],
    state: &ThreadRobotState,
) -> Vec<ThreadMessage> {
    let boundary_index = state
        .current_node_start_message_id
        .as_ref()
        .and_then(|bid| history.iter().position(|m| &m.id == bid));

    let mut result = Vec::with_capacity(history.len());
    for (index, message) in history.iter().enumerate() {
        // 首个用户消息（原始目标种子）始终保留。
        let is_seed_user =
            message.role == "user" && history.iter().take(index).all(|prev| prev.role != "user");
        // 边界之后（含边界）的当前节点自身消息保留。
        let after_boundary = match boundary_index {
            Some(boundary) => index >= boundary,
            // 尚未设置边界（首个节点/未初始化）：保留全部，等同于不裁剪。
            None => true,
        };
        if is_seed_user || after_boundary {
            result.push(message.clone());
        }
    }
    result
}

/// 是否启用机器人外层编排：
/// 仅当 mode=goal 且携带 robot_id 时启用，避免影响现有 chat/goal 主链路。
pub fn should_enable_robot_orchestration(mode: &str, robot_id: Option<&str>) -> bool {
    mode == "goal" && robot_id.is_some()
}

/// 从模型文本中移除节点完成标记，并返回“是否检测到标记”。
pub fn strip_robot_node_done_marker(text: &str) -> (String, bool) {
    let has_marker = text.contains(ROBOT_NODE_DONE_SENTINEL);
    let cleaned = text
        .replace(ROBOT_NODE_DONE_SENTINEL, "")
        .trim()
        .to_string();
    (cleaned, has_marker)
}

/// 单次解析模型输出的“节点完成产物”：
/// - 剥离 `<workflow_node_done/>` 完成信号；
/// - 提取 `<workflow_node_summary>…</workflow_node_summary>` 交付总结；
/// - 返回（剥离后的文本, 是否完成, 交付总结可选）。
///
/// 兼容多种形态：只输出 done 无 summary、summary 在 done 之前/之后、多行 summary、
/// 以及 summary 标记缺失/为空等异常情形（此时 summary 为 None，由上层回退占位文本）。
pub fn parse_robot_node_completion(text: &str) -> (String, bool, Option<String>) {
    let (after_done, done_signal) = strip_robot_node_done_marker(text);
    let (cleaned, summary) = extract_and_strip_summary(&after_done);
    (cleaned.trim().to_string(), done_signal, summary)
}

/// 从文本中提取首个交付总结块，并返回（移除标记后的文本, 总结可选）。
fn extract_and_strip_summary(text: &str) -> (String, Option<String>) {
    let open = ROBOT_NODE_SUMMARY_SENTINEL;
    let close = ROBOT_NODE_SUMMARY_END_SENTINEL;
    match (text.find(open), text.find(close)) {
        (Some(start), Some(end)) if end > start + open.len() => {
            let summary = text[start + open.len()..end].trim().to_string();
            let mut cleaned = String::with_capacity(text.len());
            cleaned.push_str(&text[..start]);
            cleaned.push_str(&text[end + close.len()..]);
            let summary = if summary.is_empty() {
                None
            } else {
                Some(summary)
            };
            (cleaned, summary)
        }
        _ => (text.to_string(), None),
    }
}

/// 当前节点完成后，提示模型切换到下一节点继续执行。
pub fn build_robot_node_advance_prompt(next_node_index: usize, total_nodes: usize) -> String {
    format!(
        "Workflow node completed. Continue with node {}/{}. \
         Focus ONLY on this new current node. \
         When this new node is fully complete, include `{}` exactly once and wrap a concise \
         delivery summary inside `{}` ... `{}`.",
        next_node_index.saturating_add(1),
        total_nodes.max(1),
        ROBOT_NODE_DONE_SENTINEL,
        ROBOT_NODE_SUMMARY_SENTINEL,
        ROBOT_NODE_SUMMARY_END_SENTINEL
    )
}

/// 当前节点尚未完成时的强制提示。
pub fn build_robot_node_completion_nudge(current_node_index: usize, total_nodes: usize) -> String {
    format!(
        "Current workflow node {}/{} is not complete yet. \
         Continue working on the CURRENT node only and produce concrete artifacts \
         (analysis notes, plans, file edits, test results). Do NOT keep repeating generic \
         requirement questions. If critical information is missing, call `request_user_input` \
         once with specific options, then wait for user response. Include `{}` only when this \
         node is fully done.",
        current_node_index.saturating_add(1),
        total_nodes.max(1),
        ROBOT_NODE_DONE_SENTINEL
    )
}

/// 标准化用户需求快照，作为本轮机器人运行态 root objective。
fn normalize_root_objective(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        "用户未提供明确目标，请按当前节点要求执行。".to_string()
    } else {
        trimmed.to_string()
    }
}

/// 根据固定 workflowNodes 编译本轮 runtime 节点目标（固定顺序，不增删节点）。
/// 支持模板变量：
/// - `{{goal}}`
/// - `{{objective}}`
fn compile_runtime_nodes(
    root_objective: &str,
    nodes: &[robot_loader::WorkflowNode],
) -> Vec<String> {
    let total = nodes.len().max(1);
    nodes
        .iter()
        .enumerate()
        .map(|(index, node)| {
            let template = node.objective.trim();
            let replaced = template
                .replace("{{goal}}", root_objective)
                .replace("{{objective}}", root_objective);
            if replaced == template {
                format!(
                    "阶段 {}/{}：{}\n关联用户需求：{}",
                    index + 1,
                    total,
                    template,
                    root_objective
                )
            } else {
                replaced
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_enable_robot_orchestration_only_for_goal_with_robot() {
        assert!(!should_enable_robot_orchestration("chat", Some("bot")));
        assert!(!should_enable_robot_orchestration("goal", None));
        assert!(should_enable_robot_orchestration("goal", Some("bot")));
    }

    #[test]
    fn strip_robot_node_done_marker_removes_control_token() {
        let (cleaned, done) = strip_robot_node_done_marker(
            "Node completed. <workflow_node_done/> Moving to next stage.",
        );
        assert!(done);
        assert_eq!(cleaned, "Node completed.  Moving to next stage.");
    }

    #[test]
    fn compile_runtime_nodes_preserves_fixed_order() {
        let nodes = vec![
            robot_loader::WorkflowNode {
                objective: "先做信息采集".to_string(),
                skills: vec!["a".to_string()],
                plugin_skills: Vec::new(),
            },
            robot_loader::WorkflowNode {
                objective: "再做结果整理".to_string(),
                skills: vec!["b".to_string()],
                plugin_skills: Vec::new(),
            },
        ];
        let compiled = compile_runtime_nodes("修复目标", &nodes);
        assert_eq!(compiled.len(), 2);
        assert!(compiled[0].contains("先做信息采集"));
        assert!(compiled[1].contains("再做结果整理"));
    }

    #[test]
    fn overlay_prompt_does_not_include_robot_system_prompt() {
        let workspace_root = std::env::temp_dir().join(format!(
            "cn-codex-robot-overlay-test-{}",
            uuid::Uuid::new_v4()
        ));
        let codey_dir = workspace_root.join("codey");
        let robot_dir = codey_dir.join("robots").join("robot-overlay");
        let skill_dir = codey_dir.join("skills").join("local-test-skill");
        std::fs::create_dir_all(&robot_dir).unwrap();
        std::fs::create_dir_all(&skill_dir).unwrap();

        std::fs::write(
            skill_dir.join("SKILL.md"),
            "name: local-test-skill\ndescription: for test\n",
        )
        .unwrap();
        std::fs::write(
            robot_dir.join("robot.json"),
            serde_json::json!({
                "name": "Robot Overlay",
                "description": "test",
                "icon": "robot",
                "skills": ["local-test-skill"],
                "pluginSkills": [],
                "workflowNodes": [
                    {
                        "objective": "执行当前节点",
                        "skills": ["local-test-skill"],
                        "pluginSkills": []
                    }
                ],
                "workflow": ["执行当前节点"],
                "systemPrompt": "THIS_TEXT_MUST_NOT_BE_IN_OVERLAY",
                "createdAt": 0,
                "updatedAt": 0
            })
            .to_string(),
        )
        .unwrap();

        let orchestrator = RobotOrchestrator::new(&workspace_root);
        let state = ThreadRobotState {
            robot_id: "robot-overlay".to_string(),
            current_node_index: 0,
            root_objective: "修复目标".to_string(),
            runtime_nodes: vec!["阶段 1：执行当前节点".to_string()],
            node_deliveries: vec![],
            current_node_start_message_id: None,
        };
        let overlay = orchestrator.build_overlay_prompt(&state).unwrap();
        assert!(overlay.contains("Robot Workflow Overlay"));
        assert!(overlay.contains(ROBOT_NODE_DONE_SENTINEL));
        assert!(!overlay.contains("THIS_TEXT_MUST_NOT_BE_IN_OVERLAY"));

        let _ = std::fs::remove_dir_all(workspace_root);
    }

    fn make_message(id: &str, role: &str, content: &str) -> ThreadMessage {
        ThreadMessage {
            id: id.to_string(),
            role: role.to_string(),
            content: content.to_string(),
            timestamp: 0,
            tool_call_id: None,
            tool_name: None,
            tool_calls: None,
            attachments: Vec::new(),
        }
    }

    #[test]
    fn parse_completion_extracts_summary_after_done() {
        let text = "分析完成。<workflow_node_done/><workflow_node_summary>已梳理需求清单与接口契约</workflow_node_summary>";
        let (cleaned, done, summary) = parse_robot_node_completion(text);
        assert!(done);
        assert_eq!(cleaned, "分析完成。");
        assert_eq!(summary.as_deref(), Some("已梳理需求清单与接口契约"));
    }

    #[test]
    fn parse_completion_handles_summary_before_done() {
        let text =
            "<workflow_node_summary>阶段一产出设计稿</workflow_node_summary><workflow_node_done/>";
        let (cleaned, done, summary) = parse_robot_node_completion(text);
        assert!(done);
        assert!(cleaned.is_empty());
        assert_eq!(summary.as_deref(), Some("阶段一产出设计稿"));
    }

    #[test]
    fn parse_completion_without_summary_falls_back_to_none() {
        let text = "节点完成。<workflow_node_done/>";
        let (cleaned, done, summary) = parse_robot_node_completion(text);
        assert!(done);
        assert_eq!(cleaned, "节点完成。");
        assert!(summary.is_none());
    }

    #[test]
    fn build_robot_model_history_keeps_seed_and_current_node_only() {
        let history = vec![
            make_message("m0", "user", "原始目标：修复登录"),
            make_message("m1", "assistant", "上游节点1的杂乱过程"),
            make_message("m2", "tool", "上游工具结果"),
            make_message("m3", "user", "某条非种子用户消息"),
            make_message("m4", "assistant", "当前节点自身消息"),
            make_message("m5", "tool", "当前节点工具结果"),
        ];
        let state = ThreadRobotState {
            robot_id: "bot".to_string(),
            current_node_index: 1,
            root_objective: "修复登录".to_string(),
            runtime_nodes: vec!["节点1".to_string(), "节点2".to_string()],
            node_deliveries: vec![],
            current_node_start_message_id: Some("m4".to_string()),
        };
        let trimmed = build_robot_model_history(&history, &state);
        let ids: Vec<&str> = trimmed.iter().map(|m| m.id.as_str()).collect();
        // 种子用户消息（原始目标）必须保留，即使它排在边界之前。
        assert!(ids.contains(&"m0"));
        // 当前节点边界之后的消息保留。
        assert!(ids.contains(&"m4"));
        assert!(ids.contains(&"m5"));
        // 已完成上游节点的原始消息被丢弃（被 node_deliveries 总结替代）。
        assert!(!ids.contains(&"m1"));
        assert!(!ids.contains(&"m2"));
        assert!(!ids.contains(&"m3"));
    }

    #[test]
    fn build_robot_model_history_without_boundary_keeps_all() {
        let history = vec![
            make_message("m0", "user", "原始目标"),
            make_message("m1", "assistant", "第一个节点消息"),
        ];
        let state = ThreadRobotState {
            robot_id: "bot".to_string(),
            current_node_index: 0,
            root_objective: "原始目标".to_string(),
            runtime_nodes: vec!["节点1".to_string()],
            node_deliveries: vec![],
            current_node_start_message_id: None,
        };
        let trimmed = build_robot_model_history(&history, &state);
        assert_eq!(trimmed.len(), 2);
    }

    #[test]
    fn overlay_prompt_injects_completed_node_deliveries() {
        let workspace_root = std::env::temp_dir().join(format!(
            "cn-codex-robot-delivery-test-{}",
            uuid::Uuid::new_v4()
        ));
        let codey_dir = workspace_root.join("codey");
        let robot_dir = codey_dir.join("robots").join("robot-delivery");
        std::fs::create_dir_all(&robot_dir).unwrap();
        std::fs::write(
            robot_dir.join("robot.json"),
            serde_json::json!({
                "name": "Robot Delivery",
                "description": "test",
                "icon": "robot",
                "skills": [],
                "pluginSkills": [],
                "workflowNodes": [
                    { "objective": "节点一", "skills": [], "pluginSkills": [] },
                    { "objective": "节点二", "skills": [], "pluginSkills": [] }
                ],
                "workflow": ["节点一", "节点二"],
                "createdAt": 0,
                "updatedAt": 0
            })
            .to_string(),
        )
        .unwrap();

        let orchestrator = RobotOrchestrator::new(&workspace_root);
        let state = ThreadRobotState {
            robot_id: "robot-delivery".to_string(),
            current_node_index: 1,
            root_objective: "用户目标".to_string(),
            runtime_nodes: vec!["节点一".to_string(), "节点二".to_string()],
            node_deliveries: vec!["已完成需求分析与接口设计".to_string()],
            current_node_start_message_id: None,
        };
        let overlay = orchestrator.build_overlay_prompt(&state).unwrap();
        assert!(overlay.contains("Completed Node Deliveries"));
        assert!(overlay.contains("Do NOT call `update_goal` with status `complete`"));
        assert!(overlay.contains("已完成需求分析与接口设计"));
        let _ = std::fs::remove_dir_all(workspace_root);
    }
}
