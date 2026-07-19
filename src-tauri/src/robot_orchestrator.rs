use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
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

const PROJECT_ROBOT_STATE_DIR: &str = ".cn-codex";
const PROJECT_ROBOT_STATE_FILE: &str = "robot-workflows.json";

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProjectRobotWorkflowStates {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    robots: BTreeMap<String, ThreadRobotState>,
}

/// 节点推进结果：
/// - ContinueCurrent: 当前节点未完成，继续留在本节点；
/// - Advanced: 已推进到下一节点，并完成 goal 目标重绑定；
/// - Completed: 全部节点完成，并保留可供 UI/项目恢复读取的完成快照。
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
    Completed {
        state: ThreadRobotState,
    },
}

/// 机器人编排器：
/// - 仅负责“机器人外层流程编排”；
/// - 不负责通用 goal/chat 主流程，降低与 agent 主链路耦合。
pub struct RobotOrchestrator {
    workspace_config_dir: PathBuf,
    project_root: PathBuf,
}

impl RobotOrchestrator {
    /// 以工作区根目录创建编排器，内部固定读取 `codey/` 配置目录。
    pub fn new(workspace_root: &Path) -> Self {
        Self::with_project_root(workspace_root, workspace_root)
    }

    /// 机器人定义从应用工作区读取，运行进度按当前项目根目录持久化。
    pub fn with_project_root(workspace_root: &Path, project_root: &Path) -> Self {
        Self {
            workspace_config_dir: workspace_root.join("codey"),
            project_root: project_root.to_path_buf(),
        }
    }

    fn project_state_path(&self) -> PathBuf {
        self.project_root
            .join(PROJECT_ROBOT_STATE_DIR)
            .join(PROJECT_ROBOT_STATE_FILE)
    }

    fn load_project_state(&self, robot_id: &str) -> Option<ThreadRobotState> {
        let path = self.project_state_path();
        let content = std::fs::read_to_string(&path).ok()?;
        match serde_json::from_str::<ProjectRobotWorkflowStates>(&content) {
            Ok(states) => states.robots.get(robot_id).cloned(),
            Err(error) => {
                warn!(
                    "Failed to parse project robot workflow state {}: {error}",
                    path.display()
                );
                None
            }
        }
    }

    fn persist_project_state(&self, state: &ThreadRobotState) -> AppResult<()> {
        let path = self.project_state_path();
        let mut states = std::fs::read_to_string(&path)
            .ok()
            .and_then(|content| serde_json::from_str::<ProjectRobotWorkflowStates>(&content).ok())
            .unwrap_or_default();
        states.version = 1;

        let mut persisted_state = state.clone();
        persisted_state.current_node_start_message_id = None;
        states
            .robots
            .insert(persisted_state.robot_id.clone(), persisted_state);

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                AppError::Custom(format!(
                    "Failed to create project robot state directory {}: {error}",
                    parent.display()
                ))
            })?;
        }
        let json = serde_json::to_string_pretty(&states).map_err(|error| {
            AppError::Custom(format!("Failed to serialize project robot state: {error}"))
        })?;
        std::fs::write(&path, format!("{json}\n")).map_err(|error| {
            AppError::Custom(format!(
                "Failed to write project robot state {}: {error}",
                path.display()
            ))
        })
    }

    /// 准备机器人运行态：
    /// - 若当前对话已有同机器人且有效状态，则复用并迁移到项目文件；
    /// - 否则仅在机器人模式入口读取项目状态并恢复到新对话；
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
            if existing_state.robot_id == robot_id
                && !existing_state.completed
                && !existing_state.runtime_nodes.is_empty()
            {
                // 防御式修正：兼容旧状态或异常状态导致的越界索引。
                if existing_state.current_node_index >= existing_state.runtime_nodes.len() {
                    existing_state.current_node_index =
                        existing_state.runtime_nodes.len().saturating_sub(1);
                    thread_store
                        .set_thread_robot_state(thread_id, existing_state.clone())
                        .await?;
                }
                // 兼容升级前只存在于 session JSONL 的进度：首次继续时迁移到项目文件。
                self.persist_project_state(&existing_state)?;
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

        if let Some(mut project_state) = self.load_project_state(robot_id)
            && !project_state.completed
            && project_state.runtime_nodes.len() == fixed_nodes.len()
            && !project_state.runtime_nodes.is_empty()
        {
            project_state.current_node_index = project_state
                .current_node_index
                .min(project_state.runtime_nodes.len().saturating_sub(1));
            project_state.current_node_start_message_id = None;
            thread_store
                .set_thread_robot_state(thread_id, project_state.clone())
                .await?;
            self.bind_goal_to_current_node(thread_store, thread_id, &project_state)
                .await?;
            return Ok(project_state);
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
            completed: false,
            current_node_start_message_id: None,
        };

        thread_store
            .set_thread_robot_state(thread_id, state.clone())
            .await?;
        self.persist_project_state(&state)?;
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
             wrap a concise delivery summary inside `{summary_marker}` ... `{summary_end_marker}`.\n\
             - Delivery summary MUST include these sections (keep labels exactly):\n\
               Artifacts:\n\
               Decisions:\n\
               Validation:\n\
               Open items:\n\
             - Prefer concrete paths/commands/evidence. If verification is incomplete, say so under \
             Validation/Open items instead of inventing success.\n\
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
    /// - 完成且已到最后节点：保留完成快照并将 goal 标记为 complete。
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
            self.persist_project_state(&state)?;
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
        let node_objective = state
            .runtime_nodes
            .get(completed_index)
            .map(String::as_str)
            .unwrap_or("");
        let delivery = truncate_utf8_by_bytes(
            &normalize_node_delivery_summary(completed_index, node_objective, delivery_summary),
            4000,
        )
        .to_string();

        let next_index = state.current_node_index.saturating_add(1);
        if next_index >= state.runtime_nodes.len() {
            // 末节点完成：保留完成快照供 UI 展示，并将 goal 标记为 complete。
            state.node_deliveries.push(delivery);
            state.completed = true;
            thread_store
                .set_thread_robot_state(thread_id, state.clone())
                .await?;
            self.persist_project_state(&state)?;
            // 结束全部节点后尝试把 goal 标记为 complete；失败仅记录日志，不阻断主流程。
            if let Err(err) = thread_store
                .set_thread_goal_status(thread_id, ThreadGoalStatus::Complete)
                .await
            {
                warn!("Failed to mark goal complete after robot workflow finished: {err}");
            }
            return Ok(NodeProgressResult::Completed { state });
        }

        state.current_node_index = next_index;
        state.node_deliveries.push(delivery);
        thread_store
            .set_thread_robot_state(thread_id, state.clone())
            .await?;
        self.persist_project_state(&state)?;
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
/// - 仅保留 `current_node_start_message_id` 边界之后的消息（即“当前节点自身”的对话，
///   包含其多轮工具调用/结果），保证当前节点工作记忆不丢失；
/// - 丢弃已完成上游节点的原始杂乱消息——那些信息已由 `node_deliveries` 以总结形式注入 overlay。
/// - 根目标由 `ThreadRobotState.root_objective` 注入 overlay，不从旧对话重复携带。
///
/// 注意：该函数只影响“喂给模型”的上下文，不修改 thread store 中的原始历史，
/// 因此 UI 可观测性与断点续跑不受影响。
pub fn build_robot_model_history(
    history: &[ThreadMessage],
    state: &ThreadRobotState,
) -> Vec<ThreadMessage> {
    let Some(boundary_index) = state
        .current_node_start_message_id
        .as_ref()
        .and_then(|bid| history.iter().position(|m| &m.id == bid))
    else {
        return history.to_vec();
    };

    history[boundary_index..].to_vec()
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
         delivery summary inside `{}` ... `{}`. \
         Delivery summary sections: Artifacts / Decisions / Validation / Open items.",
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

/// 规范化节点交付总结：
/// - 有显式 summary 时尽量补齐结构化小节；
/// - 无 summary 时使用带风险提示的结构化占位，避免下游误判“已验证完成”。
fn normalize_node_delivery_summary(
    node_index: usize,
    node_objective: &str,
    delivery_summary: Option<String>,
) -> String {
    let node_no = node_index.saturating_add(1);
    let objective = {
        let trimmed = node_objective.trim();
        if trimmed.is_empty() {
            "(unspecified node objective)".to_string()
        } else {
            trimmed.to_string()
        }
    };

    match delivery_summary
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
    {
        Some(summary) => ensure_structured_delivery_summary(&summary),
        None => format!(
            "Node {node_no} completed without an explicit delivery summary.\n\
             Node objective: {objective}\n\
             Artifacts: (none verified)\n\
             Decisions: (none recorded)\n\
             Validation: not provided by model; downstream MUST re-confirm before relying on this node.\n\
             Open items: re-check node outputs and evidence before continuing."
        ),
    }
}

fn ensure_structured_delivery_summary(summary: &str) -> String {
    let lower = summary.to_ascii_lowercase();
    let has_artifacts = lower.contains("artifacts:");
    let has_decisions = lower.contains("decisions:");
    let has_validation = lower.contains("validation:");
    let has_open_items = lower.contains("open items:");

    if has_artifacts && has_decisions && has_validation && has_open_items {
        return summary.trim().to_string();
    }

    let mut sections = Vec::new();
    if !has_artifacts {
        sections.push("Artifacts: (see free-form summary above; no structured list provided)");
    }
    if !has_decisions {
        sections.push("Decisions: (see free-form summary above; no structured list provided)");
    }
    if !has_validation {
        sections.push("Validation: not explicitly stated; treat as unverified");
    }
    if !has_open_items {
        sections.push("Open items: none listed");
    }

    format!("{}\n{}", summary.trim(), sections.join("\n"))
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

    #[tokio::test]
    async fn project_robot_state_resumes_in_a_new_thread() {
        let temp_dir = tempfile::tempdir().unwrap();
        let workspace_root = temp_dir.path().join("app");
        let project_root = temp_dir.path().join("project");
        let robot_dir = workspace_root
            .join("codey")
            .join("robots")
            .join("resume-bot");
        std::fs::create_dir_all(&robot_dir).unwrap();
        std::fs::create_dir_all(&project_root).unwrap();
        std::fs::write(
            robot_dir.join("robot.json"),
            serde_json::json!({
                "name": "Resume Bot",
                "description": "test",
                "icon": "robot",
                "skills": [],
                "pluginSkills": [],
                "workflowNodes": [
                    { "objective": "Analyze {{goal}}", "skills": [], "pluginSkills": [] },
                    { "objective": "Implement {{goal}}", "skills": [], "pluginSkills": [] }
                ],
                "workflow": ["Analyze {{goal}}", "Implement {{goal}}"],
                "createdAt": 0,
                "updatedAt": 0
            })
            .to_string(),
        )
        .unwrap();

        let store = ThreadStore::new(&workspace_root.join("codey"));
        let first_thread = store.create_thread(None).await.unwrap();
        let orchestrator = RobotOrchestrator::with_project_root(&workspace_root, &project_root);
        let initial = orchestrator
            .prepare_state(&store, &first_thread.id, "resume-bot", "ship feature")
            .await
            .unwrap();
        let advanced = orchestrator
            .apply_node_progress(
                &store,
                &first_thread.id,
                initial,
                true,
                Some(
                    "Artifacts: analysis.md\nDecisions: reuse API\nValidation: reviewed\nOpen items: none"
                        .to_string(),
                ),
            )
            .await
            .unwrap();
        assert!(matches!(advanced, NodeProgressResult::Advanced { .. }));
        assert!(
            project_root
                .join(PROJECT_ROBOT_STATE_DIR)
                .join(PROJECT_ROBOT_STATE_FILE)
                .is_file()
        );

        let second_thread = store.create_thread(None).await.unwrap();
        let resumed = orchestrator
            .prepare_state(&store, &second_thread.id, "resume-bot", "continue")
            .await
            .unwrap();

        assert_eq!(resumed.current_node_index, 1);
        assert_eq!(resumed.root_objective, "ship feature");
        assert_eq!(resumed.node_deliveries.len(), 1);
        assert!(resumed.node_deliveries[0].contains("Artifacts: analysis.md"));
        assert!(resumed.current_node_start_message_id.is_none());
        assert_eq!(
            store
                .get_thread_robot_state(&second_thread.id)
                .await
                .unwrap()
                .current_node_index,
            1
        );
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
            completed: false,
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
    fn build_robot_model_history_keeps_current_node_only() {
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
            completed: false,
            current_node_start_message_id: Some("m4".to_string()),
        };
        let trimmed = build_robot_model_history(&history, &state);
        let ids: Vec<&str> = trimmed.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, vec!["m4", "m5"]);
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
            completed: false,
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
            completed: false,
            current_node_start_message_id: None,
        };
        let overlay = orchestrator.build_overlay_prompt(&state).unwrap();
        assert!(overlay.contains("Completed Node Deliveries"));
        assert!(overlay.contains("Do NOT call `update_goal` with status `complete`"));
        assert!(overlay.contains("已完成需求分析与接口设计"));
        assert!(overlay.contains("Artifacts:"));
        assert!(overlay.contains("Decisions:"));
        assert!(overlay.contains("Validation:"));
        assert!(overlay.contains("Open items:"));
        let _ = std::fs::remove_dir_all(workspace_root);
    }

    #[test]
    fn normalize_node_delivery_summary_builds_safe_fallback() {
        let fallback = normalize_node_delivery_summary(0, "阶段 1：需求分析", None);
        assert!(fallback.contains("Node 1 completed without an explicit delivery summary."));
        assert!(fallback.contains("Node objective: 阶段 1：需求分析"));
        assert!(fallback.contains("Artifacts: (none verified)"));
        assert!(fallback.contains("Validation: not provided by model"));
        assert!(fallback.contains("downstream MUST re-confirm"));
        assert!(!fallback.contains("no explicit delivery summary provided"));
    }

    #[test]
    fn normalize_node_delivery_summary_fills_missing_sections() {
        let normalized = normalize_node_delivery_summary(
            1,
            "阶段 2：实现",
            Some("已完成接口草案，待联调。".to_string()),
        );
        assert!(normalized.contains("已完成接口草案，待联调。"));
        assert!(normalized.contains("Artifacts:"));
        assert!(normalized.contains("Decisions:"));
        assert!(normalized.contains("Validation: not explicitly stated"));
        assert!(normalized.contains("Open items: none listed"));
    }

    #[test]
    fn normalize_node_delivery_summary_keeps_complete_structure() {
        let input =
            "Artifacts: a.rs\nDecisions: use REST\nValidation: cargo test ok\nOpen items: none";
        let normalized = normalize_node_delivery_summary(0, "obj", Some(input.to_string()));
        assert_eq!(normalized, input);
    }
}
