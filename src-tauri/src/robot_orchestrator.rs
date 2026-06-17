use std::path::{Path, PathBuf};

use tracing::warn;

use crate::error::{AppError, AppResult};
use crate::robot_loader;
use crate::thread_store::{ThreadGoalStatus, ThreadRobotState, ThreadStore};

/// 机器人节点完成信号：模型在“当前节点完成”时必须输出该标记。
/// 该标记只用于运行时流程控制，不会直接展示给用户。
pub const ROBOT_NODE_DONE_SENTINEL: &str = "<workflow_node_done/>";

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
             - When CURRENT node is fully complete, include `{done_marker}` exactly once.\n\
             - If node is not complete, do NOT output `{done_marker}`.",
            root_objective = state.root_objective,
            workflow_block = workflow_block,
            current_objective = current_runtime_objective,
            local_skills_block = local_skills_block,
            plugin_skills_block = plugin_skills_block,
            skills_block = skills_block,
            done_marker = ROBOT_NODE_DONE_SENTINEL
        ))
    }

    /// 应用当前轮“节点完成信号”的状态迁移：
    /// - 未完成：保持当前节点；
    /// - 完成且仍有后续节点：推进并重绑 goal objective；
    /// - 完成且已到最后节点：清理机器人状态并将 goal 标记为 complete。
    pub async fn apply_node_progress(
        &self,
        thread_store: &ThreadStore,
        thread_id: &str,
        mut state: ThreadRobotState,
        node_done_signal: bool,
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

        let next_index = state.current_node_index.saturating_add(1);
        if next_index >= state.runtime_nodes.len() {
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

/// 当前节点完成后，提示模型切换到下一节点继续执行。
pub fn build_robot_node_advance_prompt(next_node_index: usize, total_nodes: usize) -> String {
    format!(
        "Workflow node completed. Continue with node {}/{}. \
         Focus ONLY on this new current node. \
         After it is fully complete, include `{}` exactly once.",
        next_node_index.saturating_add(1),
        total_nodes.max(1),
        ROBOT_NODE_DONE_SENTINEL
    )
}

/// 当前节点尚未完成时的强制提示。
pub fn build_robot_node_completion_nudge(current_node_index: usize, total_nodes: usize) -> String {
    format!(
        "Current workflow node {}/{} is not complete yet. \
         Continue working on the CURRENT node only, use tools to produce concrete progress, \
         and include `{}` only when this node is fully done.",
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
        };
        let overlay = orchestrator.build_overlay_prompt(&state).unwrap();
        assert!(overlay.contains("Robot Workflow Overlay"));
        assert!(overlay.contains(ROBOT_NODE_DONE_SENTINEL));
        assert!(!overlay.contains("THIS_TEXT_MUST_NOT_BE_IN_OVERLAY"));

        let _ = std::fs::remove_dir_all(workspace_root);
    }
}
