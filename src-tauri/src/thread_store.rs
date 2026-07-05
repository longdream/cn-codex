use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Instant;

use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, RwLock};
use tracing::{error, info};

use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallInfo {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FileChange {
    pub path: String,
    pub action: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TurnUsage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
    /// 本轮内成功发起并完成的 LLM 请求次数。
    #[serde(default)]
    pub call_count: u32,
    /// 最后一次单次 API 调用返回的 prompt_tokens（代表当前 context 实际大小）
    #[serde(default)]
    pub last_single_prompt_tokens: u64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ThreadGoalStatus {
    Active,
    Paused,
    Blocked,
    UsageLimited,
    BudgetLimited,
    Complete,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ThreadGoal {
    pub objective: String,
    pub status: ThreadGoalStatus,
    #[serde(default)]
    pub token_budget: Option<u64>,
    #[serde(default)]
    pub tokens_used: u64,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ThreadRobotState {
    /// 当前线程绑定的机器人 ID，用于在多机器人切换时识别状态归属。
    pub robot_id: String,
    /// 当前正在执行的 workflow 节点下标（从 0 开始）。
    pub current_node_index: usize,
    /// 本轮机器人编排对应的用户目标快照，用于跨 turn 稳定复用同一运行计划。
    #[serde(default)]
    pub root_objective: String,
    /// 固定顺序实例化后的节点目标队列（每个元素都作为真实 goal objective 使用）。
    #[serde(default)]
    pub runtime_nodes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ThreadActivePlan {
    /// 当前线程关联的活动计划文件路径。
    pub path: String,
    /// 计划修订版本号（从 1 开始，后续每次更新递增）。
    pub revision: u64,
    /// 最近一次更新活动计划的时间戳（秒）。
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadMessageAttachment {
    pub name: String,
    #[serde(rename = "type")]
    pub mime_type: String,
    pub data_url: String,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreadMessage {
    pub id: String,
    pub role: String,
    pub content: String,
    pub timestamp: i64,
    #[serde(default)]
    pub tool_call_id: Option<String>,
    #[serde(default)]
    pub tool_name: Option<String>,
    #[serde(default)]
    pub tool_calls: Option<Vec<ToolCallInfo>>,
    /// 兼容旧 rollout：历史消息里可能没有 attachments 字段。
    /// 缺失时按空数组反序列化，避免启动时整条记录解析失败。
    #[serde(default)]
    pub attachments: Vec<ThreadMessageAttachment>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredTurn {
    pub turn_id: String,
    pub started_at: i64,
    pub completed_at: Option<i64>,
    #[serde(default)]
    pub mode: Option<String>,
    #[serde(default)]
    pub duration_ms: Option<u64>,
    #[serde(default)]
    pub changed_files: Vec<FileChange>,
    #[serde(default)]
    pub usage: Option<TurnUsage>,
    #[serde(default)]
    pub goal_budget_tokens: Option<u64>,
    #[serde(default)]
    pub budget_limited: bool,
    pub messages: Vec<ThreadMessage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredThread {
    pub id: String,
    pub name: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    pub model: Option<String>,
    #[serde(default)]
    pub goal: Option<ThreadGoal>,
    #[serde(default)]
    pub active_plan: Option<ThreadActivePlan>,
    #[serde(default)]
    pub robot_state: Option<ThreadRobotState>,
    pub turns: Vec<StoredTurn>,
}

impl StoredThread {
    pub fn all_messages(&self) -> Vec<&ThreadMessage> {
        self.turns.iter().flat_map(|t| &t.messages).collect()
    }

    pub fn preview(&self) -> String {
        self.turns
            .iter()
            .flat_map(|t| &t.messages)
            .find(|m| m.role == "user")
            .map(|m| {
                let s: String = m.content.chars().take(60).collect();
                if m.content.len() > 60 {
                    format!("{s}...")
                } else {
                    s
                }
            })
            .unwrap_or_default()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum RolloutLine {
    ThreadMeta {
        thread_id: String,
        name: Option<String>,
        created_at: i64,
        model: Option<String>,
    },
    TurnStart {
        turn_id: String,
        started_at: i64,
        #[serde(default)]
        mode: Option<String>,
        #[serde(default)]
        goal_budget_tokens: Option<u64>,
    },
    Message(ThreadMessage),
    TurnEnd {
        turn_id: String,
        completed_at: i64,
        #[serde(default)]
        duration_ms: Option<u64>,
        #[serde(default)]
        changed_files: Vec<FileChange>,
        #[serde(default)]
        usage: Option<TurnUsage>,
        #[serde(default)]
        budget_limited: bool,
    },
    ThreadUpdate {
        name: Option<String>,
        updated_at: i64,
    },
    ThreadGoalSet {
        goal: ThreadGoal,
    },
    ThreadGoalClear {
        updated_at: i64,
    },
    ThreadPlanSet {
        plan: ThreadActivePlan,
    },
    ThreadPlanClear {
        updated_at: i64,
    },
    ThreadRobotStateSet {
        robot_state: ThreadRobotState,
        updated_at: i64,
    },
    ThreadRobotStateClear {
        updated_at: i64,
    },
}

pub struct ThreadStore {
    sessions_dir: PathBuf,
    threads: Arc<RwLock<HashMap<String, StoredThread>>>,
    /// 标记会话是否已完成首次磁盘加载。
    /// 启动时保持 false，可显著缩短 AppState::new 的同步路径。
    loaded: AtomicBool,
    /// 确保首次加载只有一个任务执行，避免并发命令重复扫盘。
    load_lock: Mutex<()>,
}

impl ThreadStore {
    pub fn new(workspace_dir: &Path) -> Self {
        let sessions_dir = workspace_dir.join("sessions");
        let _ = std::fs::create_dir_all(&sessions_dir);

        Self {
            sessions_dir,
            threads: Arc::new(RwLock::new(HashMap::new())),
            loaded: AtomicBool::new(false),
            load_lock: Mutex::new(()),
        }
    }

    fn thread_file(&self, thread_id: &str) -> PathBuf {
        self.sessions_dir.join(format!("{thread_id}.jsonl"))
    }

    /// 后台预热线程数据（可在 setup 阶段 fire-and-forget 调用）。
    /// 即使未预热，首次读写线程时也会自动触发按需加载。
    pub async fn preload_threads(&self) {
        self.ensure_loaded().await;
    }

    /// 确保线程索引至少加载一次。
    /// 采用“双重检查 + 互斥锁”避免并发请求重复扫盘。
    async fn ensure_loaded(&self) {
        if self.loaded.load(Ordering::Acquire) {
            return;
        }

        let _guard = self.load_lock.lock().await;
        if self.loaded.load(Ordering::Acquire) {
            return;
        }

        let sessions_dir = self.sessions_dir.clone();
        let started_at = Instant::now();
        let load_task = tokio::task::spawn_blocking(move || Self::load_all_sync(&sessions_dir));
        let loaded_threads = match load_task.await {
            Ok(threads) => threads,
            Err(join_err) => {
                error!(
                    "ThreadStore lazy load task failed: {join_err}. fallback to empty thread map"
                );
                HashMap::new()
            }
        };

        let loaded_count = loaded_threads.len();
        *self.threads.write().await = loaded_threads;
        self.loaded.store(true, Ordering::Release);
        info!(
            "ThreadStore lazy load completed: {} threads, {} ms",
            loaded_count,
            started_at.elapsed().as_millis()
        );
    }

    /// 同步扫描 sessions 目录并重建内存索引。
    /// 仅在 spawn_blocking 线程里调用，避免阻塞 async runtime。
    fn load_all_sync(sessions_dir: &Path) -> HashMap<String, StoredThread> {
        let Ok(entries) = std::fs::read_dir(sessions_dir) else {
            return HashMap::new();
        };

        let mut threads = HashMap::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "jsonl") {
                if let Some(thread) = Self::load_thread_file(&path) {
                    threads.insert(thread.id.clone(), thread);
                }
            }
        }

        threads
    }

    fn load_thread_file(path: &Path) -> Option<StoredThread> {
        let file = std::fs::File::open(path).ok()?;
        let reader = std::io::BufReader::new(file);

        let mut thread: Option<StoredThread> = None;
        let mut current_turn: Option<StoredTurn> = None;

        for line in reader.lines() {
            let line = match line {
                Ok(l) if !l.trim().is_empty() => l,
                _ => continue,
            };

            let item: RolloutLine = match serde_json::from_str(&line) {
                Ok(item) => item,
                Err(e) => {
                    error!("Failed to parse rollout line: {e}");
                    continue;
                }
            };

            match item {
                RolloutLine::ThreadMeta {
                    thread_id,
                    name,
                    created_at,
                    model,
                } => {
                    thread = Some(StoredThread {
                        id: thread_id,
                        name,
                        created_at,
                        updated_at: created_at,
                        model,
                        goal: None,
                        active_plan: None,
                        robot_state: None,
                        turns: Vec::new(),
                    });
                }
                RolloutLine::TurnStart {
                    turn_id,
                    started_at,
                    mode,
                    goal_budget_tokens,
                } => {
                    current_turn = Some(StoredTurn {
                        turn_id,
                        started_at,
                        completed_at: None,
                        mode,
                        duration_ms: None,
                        changed_files: Vec::new(),
                        usage: None,
                        goal_budget_tokens,
                        budget_limited: false,
                        messages: Vec::new(),
                    });
                }
                RolloutLine::Message(msg) => {
                    if let Some(ref mut turn) = current_turn {
                        turn.messages.push(msg);
                    }
                }
                RolloutLine::TurnEnd {
                    turn_id: _,
                    completed_at,
                    duration_ms,
                    changed_files,
                    usage,
                    budget_limited,
                } => {
                    if let Some(mut turn) = current_turn.take() {
                        turn.completed_at = Some(completed_at);
                        turn.duration_ms = duration_ms;
                        turn.changed_files = changed_files;
                        turn.usage = usage;
                        turn.budget_limited = budget_limited;
                        if let Some(ref mut t) = thread {
                            t.updated_at = completed_at;
                            t.turns.push(turn);
                        }
                    }
                }
                RolloutLine::ThreadUpdate { name, updated_at } => {
                    if let Some(ref mut t) = thread {
                        if name.is_some() {
                            t.name = name;
                        }
                        t.updated_at = updated_at;
                    }
                }
                RolloutLine::ThreadGoalSet { goal } => {
                    if let Some(ref mut t) = thread {
                        t.updated_at = goal.updated_at;
                        t.goal = Some(goal);
                    }
                }
                RolloutLine::ThreadGoalClear { updated_at } => {
                    if let Some(ref mut t) = thread {
                        t.updated_at = updated_at;
                        t.goal = None;
                    }
                }
                RolloutLine::ThreadPlanSet { plan } => {
                    if let Some(ref mut t) = thread {
                        t.updated_at = plan.updated_at;
                        t.active_plan = Some(plan);
                    }
                }
                RolloutLine::ThreadPlanClear { updated_at } => {
                    if let Some(ref mut t) = thread {
                        t.updated_at = updated_at;
                        t.active_plan = None;
                    }
                }
                RolloutLine::ThreadRobotStateSet {
                    robot_state,
                    updated_at,
                } => {
                    if let Some(ref mut t) = thread {
                        t.updated_at = updated_at;
                        t.robot_state = Some(robot_state);
                    }
                }
                RolloutLine::ThreadRobotStateClear { updated_at } => {
                    if let Some(ref mut t) = thread {
                        t.updated_at = updated_at;
                        t.robot_state = None;
                    }
                }
            }
        }

        // 应用崩溃或被强制关闭时 TurnEnd 可能未写入文件，
        // 此时 current_turn 中仍有未闭合的消息。将其作为未完成 turn 保留，
        // 避免历史消息丢失。
        if let (Some(t), Some(orphan_turn)) = (&mut thread, current_turn) {
            if !orphan_turn.messages.is_empty() {
                t.turns.push(orphan_turn);
            }
        }

        thread
    }

    fn append_line(&self, thread_id: &str, line: &RolloutLine) -> AppResult<()> {
        let path = self.thread_file(thread_id);
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|e| AppError::Custom(format!("Failed to open rollout file: {e}")))?;
        let json = serde_json::to_string(line)
            .map_err(|e| AppError::Custom(format!("Failed to serialize rollout: {e}")))?;
        writeln!(file, "{json}")
            .map_err(|e| AppError::Custom(format!("Failed to write rollout: {e}")))?;
        Ok(())
    }

    fn rewrite_thread_file(&self, thread_id: &str, thread: &StoredThread) -> AppResult<()> {
        let path = self.thread_file(thread_id);
        let mut file = std::fs::File::create(&path)
            .map_err(|e| AppError::Custom(format!("Failed to create rollout file: {e}")))?;

        let meta = RolloutLine::ThreadMeta {
            thread_id: thread.id.clone(),
            name: thread.name.clone(),
            created_at: thread.created_at,
            model: thread.model.clone(),
        };
        let json = serde_json::to_string(&meta)
            .map_err(|e| AppError::Custom(format!("Serialize error: {e}")))?;
        writeln!(file, "{json}").map_err(|e| AppError::Custom(format!("Write error: {e}")))?;

        // 在重写文件时同步持久化机器人流程状态，保证崩溃恢复后仍能继续当前节点。
        if let Some(robot_state) = &thread.robot_state {
            let line = RolloutLine::ThreadRobotStateSet {
                robot_state: robot_state.clone(),
                updated_at: thread.updated_at,
            };
            let json = serde_json::to_string(&line)
                .map_err(|e| AppError::Custom(format!("Serialize error: {e}")))?;
            writeln!(file, "{json}").map_err(|e| AppError::Custom(format!("Write error: {e}")))?;
        }

        // 在重写文件时同步持久化活动计划信息，保证线程重载后仍能恢复计划卡片。
        if let Some(active_plan) = &thread.active_plan {
            let line = RolloutLine::ThreadPlanSet {
                plan: active_plan.clone(),
            };
            let json = serde_json::to_string(&line)
                .map_err(|e| AppError::Custom(format!("Serialize error: {e}")))?;
            writeln!(file, "{json}").map_err(|e| AppError::Custom(format!("Write error: {e}")))?;
        }

        for turn in &thread.turns {
            let ts = RolloutLine::TurnStart {
                turn_id: turn.turn_id.clone(),
                started_at: turn.started_at,
                mode: turn.mode.clone(),
                goal_budget_tokens: turn.goal_budget_tokens,
            };
            let json = serde_json::to_string(&ts)
                .map_err(|e| AppError::Custom(format!("Serialize error: {e}")))?;
            writeln!(file, "{json}").map_err(|e| AppError::Custom(format!("Write error: {e}")))?;

            for msg in &turn.messages {
                let line = RolloutLine::Message(msg.clone());
                let json = serde_json::to_string(&line)
                    .map_err(|e| AppError::Custom(format!("Serialize error: {e}")))?;
                writeln!(file, "{json}")
                    .map_err(|e| AppError::Custom(format!("Write error: {e}")))?;
            }

            if let Some(completed_at) = turn.completed_at {
                let te = RolloutLine::TurnEnd {
                    turn_id: turn.turn_id.clone(),
                    completed_at,
                    duration_ms: turn.duration_ms,
                    changed_files: turn.changed_files.clone(),
                    usage: turn.usage.clone(),
                    budget_limited: turn.budget_limited,
                };
                let json = serde_json::to_string(&te)
                    .map_err(|e| AppError::Custom(format!("Serialize error: {e}")))?;
                writeln!(file, "{json}")
                    .map_err(|e| AppError::Custom(format!("Write error: {e}")))?;
            }
        }

        Ok(())
    }

    pub async fn create_thread(&self, model: Option<String>) -> AppResult<StoredThread> {
        // 先确保内存索引已与磁盘对齐，避免后续插入覆盖尚未加载的数据。
        self.ensure_loaded().await;
        let thread_id = uuid::Uuid::new_v4().to_string();
        let now = now_secs();

        let thread = StoredThread {
            id: thread_id.clone(),
            name: None,
            created_at: now,
            updated_at: now,
            model,
            goal: None,
            active_plan: None,
            robot_state: None,
            turns: Vec::new(),
        };

        self.append_line(
            &thread_id,
            &RolloutLine::ThreadMeta {
                thread_id: thread_id.clone(),
                name: None,
                created_at: now,
                model: thread.model.clone(),
            },
        )?;

        self.threads.write().await.insert(thread_id, thread.clone());
        Ok(thread)
    }

    pub async fn start_turn(
        &self,
        thread_id: &str,
        mode: Option<String>,
        goal_budget_tokens: Option<u64>,
    ) -> AppResult<String> {
        self.ensure_loaded().await;
        let turn_id = uuid::Uuid::new_v4().to_string();
        let now = now_secs();

        self.append_line(
            thread_id,
            &RolloutLine::TurnStart {
                turn_id: turn_id.clone(),
                started_at: now,
                mode: mode.clone(),
                goal_budget_tokens,
            },
        )?;

        let mut threads = self.threads.write().await;
        if let Some(thread) = threads.get_mut(thread_id) {
            thread.turns.push(StoredTurn {
                turn_id: turn_id.clone(),
                started_at: now,
                completed_at: None,
                mode,
                duration_ms: None,
                changed_files: Vec::new(),
                usage: None,
                goal_budget_tokens,
                budget_limited: false,
                messages: Vec::new(),
            });
            thread.updated_at = now;
        }

        Ok(turn_id)
    }

    pub async fn add_message(&self, thread_id: &str, msg: ThreadMessage) -> AppResult<()> {
        self.ensure_loaded().await;
        self.append_line(thread_id, &RolloutLine::Message(msg.clone()))?;

        let mut threads = self.threads.write().await;
        if let Some(thread) = threads.get_mut(thread_id) {
            if let Some(turn) = thread.turns.last_mut() {
                turn.messages.push(msg);
            }
            thread.updated_at = now_secs();
        }
        Ok(())
    }

    pub async fn end_turn(
        &self,
        thread_id: &str,
        turn_id: &str,
        duration_ms: Option<u64>,
        changed_files: Vec<FileChange>,
        usage: Option<TurnUsage>,
        budget_limited: bool,
    ) -> AppResult<i64> {
        self.ensure_loaded().await;
        let now = now_secs();
        self.append_line(
            thread_id,
            &RolloutLine::TurnEnd {
                turn_id: turn_id.to_string(),
                completed_at: now,
                duration_ms,
                changed_files: changed_files.clone(),
                usage: usage.clone(),
                budget_limited,
            },
        )?;

        let mut threads = self.threads.write().await;
        if let Some(thread) = threads.get_mut(thread_id) {
            if let Some(turn) = thread.turns.iter_mut().find(|t| t.turn_id == turn_id) {
                turn.completed_at = Some(now);
                turn.duration_ms = duration_ms;
                turn.changed_files = changed_files;
                turn.usage = usage;
                turn.budget_limited = budget_limited;
            }
            thread.updated_at = now;
        }
        Ok(now)
    }

    pub async fn set_thread_name(&self, thread_id: &str, name: String) -> AppResult<()> {
        self.ensure_loaded().await;
        let now = now_secs();
        self.append_line(
            thread_id,
            &RolloutLine::ThreadUpdate {
                name: Some(name.clone()),
                updated_at: now,
            },
        )?;

        let mut threads = self.threads.write().await;
        if let Some(thread) = threads.get_mut(thread_id) {
            thread.name = Some(name);
            thread.updated_at = now;
        }
        Ok(())
    }

    pub async fn set_thread_goal(
        &self,
        thread_id: &str,
        objective: String,
        status: ThreadGoalStatus,
        token_budget: Option<u64>,
    ) -> AppResult<ThreadGoal> {
        self.ensure_loaded().await;
        if objective.trim().is_empty() {
            return Err(AppError::Custom(
                "Goal objective cannot be empty".to_string(),
            ));
        }
        if !self.threads.read().await.contains_key(thread_id) {
            return Err(AppError::Custom(format!("Thread not found: {thread_id}")));
        }

        let now = now_secs();
        let goal = ThreadGoal {
            objective,
            status,
            token_budget: token_budget.filter(|value| *value > 0),
            tokens_used: 0,
            created_at: now,
            updated_at: now,
        };
        self.append_line(
            thread_id,
            &RolloutLine::ThreadGoalSet { goal: goal.clone() },
        )?;

        let mut threads = self.threads.write().await;
        if let Some(thread) = threads.get_mut(thread_id) {
            thread.goal = Some(goal.clone());
            thread.updated_at = now;
        }
        Ok(goal)
    }

    pub async fn set_thread_goal_status(
        &self,
        thread_id: &str,
        status: ThreadGoalStatus,
    ) -> AppResult<ThreadGoal> {
        self.ensure_loaded().await;
        let current_goal = self
            .get_thread(thread_id)
            .await
            .ok_or_else(|| AppError::Custom(format!("Thread not found: {thread_id}")))?
            .goal
            .ok_or_else(|| AppError::Custom("No goal is set for this thread".to_string()))?;

        let now = now_secs();
        let mut goal = current_goal;
        goal.status = status;
        goal.updated_at = now;
        self.append_line(
            thread_id,
            &RolloutLine::ThreadGoalSet { goal: goal.clone() },
        )?;

        let mut threads = self.threads.write().await;
        if let Some(thread) = threads.get_mut(thread_id) {
            thread.goal = Some(goal.clone());
            thread.updated_at = now;
        }
        Ok(goal)
    }

    pub async fn edit_thread_goal(
        &self,
        thread_id: &str,
        objective: String,
        token_budget: Option<u64>,
    ) -> AppResult<ThreadGoal> {
        self.ensure_loaded().await;
        if objective.trim().is_empty() {
            return Err(AppError::Custom(
                "Goal objective cannot be empty".to_string(),
            ));
        }
        let current_goal = self
            .get_thread(thread_id)
            .await
            .ok_or_else(|| AppError::Custom(format!("Thread not found: {thread_id}")))?
            .goal
            .ok_or_else(|| AppError::Custom("No goal is set for this thread".to_string()))?;

        let now = now_secs();
        let mut goal = current_goal;
        goal.objective = objective;
        goal.status = edited_goal_status(goal.status);
        if let Some(token_budget) = token_budget {
            goal.token_budget = (token_budget > 0).then_some(token_budget);
        }
        goal.updated_at = now;
        self.append_line(
            thread_id,
            &RolloutLine::ThreadGoalSet { goal: goal.clone() },
        )?;

        let mut threads = self.threads.write().await;
        if let Some(thread) = threads.get_mut(thread_id) {
            thread.goal = Some(goal.clone());
            thread.updated_at = now;
        }
        Ok(goal)
    }

    pub async fn clear_thread_goal(&self, thread_id: &str) -> AppResult<()> {
        self.ensure_loaded().await;
        if !self.threads.read().await.contains_key(thread_id) {
            return Err(AppError::Custom(format!("Thread not found: {thread_id}")));
        }

        let now = now_secs();
        self.append_line(thread_id, &RolloutLine::ThreadGoalClear { updated_at: now })?;

        let mut threads = self.threads.write().await;
        if let Some(thread) = threads.get_mut(thread_id) {
            thread.goal = None;
            // 目标被清除时同步重置机器人流程状态，避免下一次 goal 恢复时跳到旧节点。
            if thread.robot_state.is_some() {
                self.append_line(
                    thread_id,
                    &RolloutLine::ThreadRobotStateClear { updated_at: now },
                )?;
                thread.robot_state = None;
            }
            thread.updated_at = now;
        }
        Ok(())
    }

    pub async fn set_thread_active_plan(
        &self,
        thread_id: &str,
        path: String,
        revision: u64,
    ) -> AppResult<ThreadActivePlan> {
        self.ensure_loaded().await;
        if path.trim().is_empty() {
            return Err(AppError::Custom(
                "Active plan path cannot be empty".to_string(),
            ));
        }
        if !self.threads.read().await.contains_key(thread_id) {
            return Err(AppError::Custom(format!("Thread not found: {thread_id}")));
        }

        let now = now_secs();
        let plan = ThreadActivePlan {
            path,
            revision: revision.max(1),
            updated_at: now,
        };
        self.append_line(
            thread_id,
            &RolloutLine::ThreadPlanSet { plan: plan.clone() },
        )?;

        let mut threads = self.threads.write().await;
        if let Some(thread) = threads.get_mut(thread_id) {
            thread.active_plan = Some(plan.clone());
            thread.updated_at = now;
        }
        Ok(plan)
    }

    pub async fn clear_thread_active_plan(&self, thread_id: &str) -> AppResult<()> {
        self.ensure_loaded().await;
        if !self.threads.read().await.contains_key(thread_id) {
            return Err(AppError::Custom(format!("Thread not found: {thread_id}")));
        }

        let now = now_secs();
        self.append_line(thread_id, &RolloutLine::ThreadPlanClear { updated_at: now })?;

        let mut threads = self.threads.write().await;
        if let Some(thread) = threads.get_mut(thread_id) {
            thread.active_plan = None;
            thread.updated_at = now;
        }
        Ok(())
    }

    pub async fn get_thread_active_plan(&self, thread_id: &str) -> Option<ThreadActivePlan> {
        self.ensure_loaded().await;
        self.threads
            .read()
            .await
            .get(thread_id)
            .and_then(|thread| thread.active_plan.clone())
    }

    pub async fn get_thread_robot_state(&self, thread_id: &str) -> Option<ThreadRobotState> {
        self.ensure_loaded().await;
        self.threads
            .read()
            .await
            .get(thread_id)
            .and_then(|thread| thread.robot_state.clone())
    }

    pub async fn set_thread_robot_state(
        &self,
        thread_id: &str,
        robot_state: ThreadRobotState,
    ) -> AppResult<ThreadRobotState> {
        self.ensure_loaded().await;
        if !self.threads.read().await.contains_key(thread_id) {
            return Err(AppError::Custom(format!("Thread not found: {thread_id}")));
        }

        let now = now_secs();
        self.append_line(
            thread_id,
            &RolloutLine::ThreadRobotStateSet {
                robot_state: robot_state.clone(),
                updated_at: now,
            },
        )?;

        let mut threads = self.threads.write().await;
        if let Some(thread) = threads.get_mut(thread_id) {
            thread.robot_state = Some(robot_state.clone());
            thread.updated_at = now;
        }
        Ok(robot_state)
    }

    pub async fn clear_thread_robot_state(&self, thread_id: &str) -> AppResult<()> {
        self.ensure_loaded().await;
        if !self.threads.read().await.contains_key(thread_id) {
            return Err(AppError::Custom(format!("Thread not found: {thread_id}")));
        }

        let now = now_secs();
        self.append_line(
            thread_id,
            &RolloutLine::ThreadRobotStateClear { updated_at: now },
        )?;

        let mut threads = self.threads.write().await;
        if let Some(thread) = threads.get_mut(thread_id) {
            thread.robot_state = None;
            thread.updated_at = now;
        }
        Ok(())
    }

    pub async fn record_goal_usage(
        &self,
        thread_id: &str,
        total_tokens: u64,
    ) -> AppResult<Option<ThreadGoal>> {
        self.ensure_loaded().await;
        let Some(current_goal) = self
            .get_thread(thread_id)
            .await
            .ok_or_else(|| AppError::Custom(format!("Thread not found: {thread_id}")))?
            .goal
        else {
            return Ok(None);
        };

        let now = now_secs();
        let mut goal = current_goal;
        goal.tokens_used = goal.tokens_used.saturating_add(total_tokens);
        if goal
            .token_budget
            .is_some_and(|budget| budget > 0 && goal.tokens_used >= budget)
        {
            goal.status = ThreadGoalStatus::BudgetLimited;
        }
        goal.updated_at = now;
        self.append_line(
            thread_id,
            &RolloutLine::ThreadGoalSet { goal: goal.clone() },
        )?;

        let mut threads = self.threads.write().await;
        if let Some(thread) = threads.get_mut(thread_id) {
            thread.goal = Some(goal.clone());
            thread.updated_at = now;
        }
        Ok(Some(goal))
    }

    pub async fn list_threads(&self) -> Vec<StoredThread> {
        self.ensure_loaded().await;
        let threads = self.threads.read().await;
        let mut list: Vec<StoredThread> = threads.values().cloned().collect();
        list.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        list
    }

    pub async fn get_thread(&self, thread_id: &str) -> Option<StoredThread> {
        self.ensure_loaded().await;
        self.threads.read().await.get(thread_id).cloned()
    }

    pub async fn get_thread_messages(&self, thread_id: &str) -> Vec<ThreadMessage> {
        self.ensure_loaded().await;
        let threads = self.threads.read().await;
        threads
            .get(thread_id)
            .map(|t| t.all_messages().into_iter().cloned().collect())
            .unwrap_or_default()
    }

    pub async fn get_thread_total_tokens(&self, thread_id: &str) -> u64 {
        self.ensure_loaded().await;
        let threads = self.threads.read().await;
        let Some(thread) = threads.get(thread_id) else {
            return 0;
        };
        thread
            .turns
            .iter()
            .rev()
            .find_map(|t| t.usage.as_ref())
            .map(|u| {
                // 优先使用最后一次单次 API 的 prompt_tokens（真实 context 大小），
                // 回退到 prompt_tokens（兼容旧数据和 compaction turn 的估算值）
                if u.last_single_prompt_tokens > 0 {
                    u.last_single_prompt_tokens
                } else {
                    u.prompt_tokens
                }
            })
            .unwrap_or(0)
    }

    pub async fn delete_thread(&self, thread_id: &str) -> AppResult<()> {
        self.ensure_loaded().await;
        let path = self.thread_file(thread_id);
        if path.exists() {
            std::fs::remove_file(&path).map_err(|e| {
                crate::error::AppError::Custom(format!("Failed to delete thread file: {e}"))
            })?;
        }
        self.threads.write().await.remove(thread_id);
        Ok(())
    }

    pub async fn replace_messages(
        &self,
        thread_id: &str,
        new_messages: Vec<ThreadMessage>,
    ) -> AppResult<()> {
        self.ensure_loaded().await;
        let mut threads = self.threads.write().await;
        let Some(thread) = threads.get_mut(thread_id) else {
            return Err(AppError::Custom(format!("Thread not found: {thread_id}")));
        };

        // Estimate token count of compacted history to prevent re-triggering compaction
        let estimated_tokens: u64 = new_messages
            .iter()
            .map(|m| (m.content.len() / 3) as u64)
            .sum();

        thread.turns.clear();
        let turn_id = uuid::Uuid::new_v4().to_string();
        let now = now_secs();
        thread.turns.push(StoredTurn {
            turn_id: turn_id.clone(),
            started_at: now,
            completed_at: Some(now),
            mode: Some("compaction".to_string()),
            duration_ms: None,
            changed_files: Vec::new(),
            usage: Some(TurnUsage {
                prompt_tokens: estimated_tokens,
                completion_tokens: 0,
                total_tokens: estimated_tokens,
                call_count: 0,
                last_single_prompt_tokens: estimated_tokens,
            }),
            goal_budget_tokens: None,
            budget_limited: false,
            messages: new_messages.clone(),
        });
        thread.updated_at = now;

        self.rewrite_thread_file(thread_id, thread)?;
        Ok(())
    }
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn edited_goal_status(status: ThreadGoalStatus) -> ThreadGoalStatus {
    match status {
        ThreadGoalStatus::Active => ThreadGoalStatus::Active,
        ThreadGoalStatus::Paused | ThreadGoalStatus::Blocked | ThreadGoalStatus::UsageLimited => {
            status
        }
        ThreadGoalStatus::BudgetLimited | ThreadGoalStatus::Complete => ThreadGoalStatus::Active,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn end_turn_persists_usage_metadata() {
        let workspace_dir =
            std::env::temp_dir().join(format!("cn-codex-thread-store-{}", uuid::Uuid::new_v4()));
        let store = ThreadStore::new(&workspace_dir);
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let usage = TurnUsage {
            prompt_tokens: 100,
            completion_tokens: 40,
            total_tokens: 140,
            call_count: 3,
            last_single_prompt_tokens: 100,
        };
        let thread_id = runtime.block_on(async {
            let thread = store
                .create_thread(Some("test-model".to_string()))
                .await
                .unwrap();
            let turn_id = store
                .start_turn(&thread.id, Some("goal".to_string()), Some(120))
                .await
                .unwrap();

            store
                .end_turn(
                    &thread.id,
                    &turn_id,
                    Some(25),
                    Vec::new(),
                    Some(usage.clone()),
                    true,
                )
                .await
                .unwrap();

            let loaded = store.get_thread(&thread.id).await.unwrap();
            assert_eq!(loaded.turns[0].usage, Some(usage.clone()));
            assert_eq!(loaded.turns[0].goal_budget_tokens, Some(120));
            assert!(loaded.turns[0].budget_limited);
            thread.id
        });
        drop(runtime);

        let reloaded_store = ThreadStore::new(&workspace_dir);
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let reloaded = runtime
            .block_on(async { reloaded_store.get_thread(&thread_id).await })
            .unwrap();
        assert_eq!(reloaded.turns[0].usage, Some(usage));
        assert_eq!(reloaded.turns[0].goal_budget_tokens, Some(120));
        assert!(reloaded.turns[0].budget_limited);

        let _ = std::fs::remove_dir_all(workspace_dir);
    }

    #[test]
    fn thread_goal_lifecycle_persists_status_and_usage() {
        let workspace_dir =
            std::env::temp_dir().join(format!("cn-codex-thread-goal-{}", uuid::Uuid::new_v4()));
        let store = ThreadStore::new(&workspace_dir);
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let thread_id = runtime.block_on(async {
            let thread = store.create_thread(None).await.unwrap();
            let goal = store
                .set_thread_goal(
                    &thread.id,
                    "ship goal controls".to_string(),
                    ThreadGoalStatus::Active,
                    Some(100),
                )
                .await
                .unwrap();
            assert_eq!(goal.status, ThreadGoalStatus::Active);
            assert_eq!(goal.token_budget, Some(100));

            let paused = store
                .set_thread_goal_status(&thread.id, ThreadGoalStatus::Paused)
                .await
                .unwrap();
            assert_eq!(paused.status, ThreadGoalStatus::Paused);

            let resumed = store
                .set_thread_goal_status(&thread.id, ThreadGoalStatus::Active)
                .await
                .unwrap();
            assert_eq!(resumed.status, ThreadGoalStatus::Active);

            let limited = store
                .record_goal_usage(&thread.id, 120)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(limited.tokens_used, 120);
            assert_eq!(limited.status, ThreadGoalStatus::BudgetLimited);
            thread.id
        });
        drop(runtime);

        let reloaded_store = ThreadStore::new(&workspace_dir);
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let reloaded = runtime
            .block_on(async { reloaded_store.get_thread(&thread_id).await })
            .unwrap();
        let goal = reloaded.goal.unwrap();
        assert_eq!(goal.objective, "ship goal controls");
        assert_eq!(goal.status, ThreadGoalStatus::BudgetLimited);
        assert_eq!(goal.tokens_used, 120);

        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime
            .block_on(async { reloaded_store.clear_thread_goal(&thread_id).await })
            .unwrap();
        let cleared = runtime
            .block_on(async { reloaded_store.get_thread(&thread_id).await })
            .unwrap();
        assert!(cleared.goal.is_none());
        drop(runtime);

        let final_store = ThreadStore::new(&workspace_dir);
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let final_thread = runtime
            .block_on(async { final_store.get_thread(&thread_id).await })
            .unwrap();
        assert!(final_thread.goal.is_none());

        let _ = std::fs::remove_dir_all(workspace_dir);
    }

    #[test]
    fn thread_goal_edit_preserves_usage_and_updates_status_like_codex() {
        let workspace_dir =
            std::env::temp_dir().join(format!("cn-codex-goal-edit-{}", uuid::Uuid::new_v4()));
        let store = ThreadStore::new(&workspace_dir);
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let thread_id = runtime.block_on(async {
            let thread = store.create_thread(None).await.unwrap();
            store
                .set_thread_goal(
                    &thread.id,
                    "old goal".to_string(),
                    ThreadGoalStatus::Active,
                    Some(100),
                )
                .await
                .unwrap();
            store.record_goal_usage(&thread.id, 120).await.unwrap();

            let edited = store
                .edit_thread_goal(&thread.id, "new goal".to_string(), Some(500))
                .await
                .unwrap();
            assert_eq!(edited.objective, "new goal");
            assert_eq!(edited.status, ThreadGoalStatus::Active);
            assert_eq!(edited.tokens_used, 120);
            assert_eq!(edited.token_budget, Some(500));

            store
                .set_thread_goal_status(&thread.id, ThreadGoalStatus::Paused)
                .await
                .unwrap();
            let paused_edit = store
                .edit_thread_goal(&thread.id, "paused goal".to_string(), None)
                .await
                .unwrap();
            assert_eq!(paused_edit.status, ThreadGoalStatus::Paused);
            assert_eq!(paused_edit.token_budget, Some(500));
            thread.id
        });
        drop(runtime);

        let reloaded_store = ThreadStore::new(&workspace_dir);
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let reloaded = runtime
            .block_on(async { reloaded_store.get_thread(&thread_id).await })
            .unwrap()
            .goal
            .unwrap();
        assert_eq!(reloaded.objective, "paused goal");
        assert_eq!(reloaded.status, ThreadGoalStatus::Paused);
        assert_eq!(reloaded.tokens_used, 120);
        assert_eq!(reloaded.token_budget, Some(500));

        let _ = std::fs::remove_dir_all(workspace_dir);
    }

    #[test]
    fn thread_robot_state_persists_and_clears_with_goal_clear() {
        let workspace_dir =
            std::env::temp_dir().join(format!("cn-codex-robot-state-{}", uuid::Uuid::new_v4()));
        let store = ThreadStore::new(&workspace_dir);
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let thread_id = runtime.block_on(async {
            let thread = store.create_thread(None).await.unwrap();
            store
                .set_thread_goal(
                    &thread.id,
                    "finish robot workflow".to_string(),
                    ThreadGoalStatus::Active,
                    None,
                )
                .await
                .unwrap();
            store
                .set_thread_robot_state(
                    &thread.id,
                    ThreadRobotState {
                        robot_id: "fullstack-bot".to_string(),
                        current_node_index: 2,
                        root_objective: "finish robot workflow".to_string(),
                        runtime_nodes: vec![
                            "阶段 1：采集信息".to_string(),
                            "阶段 2：执行变更".to_string(),
                            "阶段 3：验证并总结".to_string(),
                        ],
                    },
                )
                .await
                .unwrap();
            let loaded = store.get_thread(&thread.id).await.unwrap();
            assert_eq!(
                loaded.robot_state,
                Some(ThreadRobotState {
                    robot_id: "fullstack-bot".to_string(),
                    current_node_index: 2,
                    root_objective: "finish robot workflow".to_string(),
                    runtime_nodes: vec![
                        "阶段 1：采集信息".to_string(),
                        "阶段 2：执行变更".to_string(),
                        "阶段 3：验证并总结".to_string(),
                    ],
                })
            );
            thread.id
        });
        drop(runtime);

        let reloaded_store = ThreadStore::new(&workspace_dir);
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let reloaded = runtime
            .block_on(async { reloaded_store.get_thread(&thread_id).await })
            .unwrap();
        assert_eq!(
            reloaded.robot_state,
            Some(ThreadRobotState {
                robot_id: "fullstack-bot".to_string(),
                current_node_index: 2,
                root_objective: "finish robot workflow".to_string(),
                runtime_nodes: vec![
                    "阶段 1：采集信息".to_string(),
                    "阶段 2：执行变更".to_string(),
                    "阶段 3：验证并总结".to_string(),
                ],
            })
        );

        runtime
            .block_on(async { reloaded_store.clear_thread_goal(&thread_id).await })
            .unwrap();
        let cleared = runtime
            .block_on(async { reloaded_store.get_thread(&thread_id).await })
            .unwrap();
        assert!(cleared.goal.is_none());
        assert!(cleared.robot_state.is_none());
        drop(runtime);

        let _ = std::fs::remove_dir_all(workspace_dir);
    }

    #[test]
    fn thread_active_plan_persists_latest_revision() {
        let workspace_dir =
            std::env::temp_dir().join(format!("cn-codex-active-plan-{}", uuid::Uuid::new_v4()));
        let store = ThreadStore::new(&workspace_dir);
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let thread_id = runtime.block_on(async {
            let thread = store.create_thread(None).await.unwrap();
            let first = store
                .set_thread_active_plan(&thread.id, "codey/plans/first.pmd".to_string(), 1)
                .await
                .unwrap();
            assert_eq!(first.revision, 1);
            assert_eq!(first.path, "codey/plans/first.pmd");

            let second = store
                .set_thread_active_plan(&thread.id, "codey/plans/first.pmd".to_string(), 2)
                .await
                .unwrap();
            assert_eq!(second.revision, 2);

            let loaded = store.get_thread(&thread.id).await.unwrap();
            let active = loaded.active_plan.expect("active plan should exist");
            assert_eq!(active.path, "codey/plans/first.pmd");
            assert_eq!(active.revision, 2);
            thread.id
        });
        drop(runtime);

        let reloaded_store = ThreadStore::new(&workspace_dir);
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let reloaded = runtime
            .block_on(async { reloaded_store.get_thread(&thread_id).await })
            .unwrap();
        let active = reloaded.active_plan.expect("active plan should persist");
        assert_eq!(active.path, "codey/plans/first.pmd");
        assert_eq!(active.revision, 2);

        let _ = std::fs::remove_dir_all(workspace_dir);
    }
}
