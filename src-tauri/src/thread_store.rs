use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
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
}

pub struct ThreadStore {
    sessions_dir: PathBuf,
    threads: Arc<RwLock<HashMap<String, StoredThread>>>,
}

impl ThreadStore {
    pub fn new(workspace_dir: &Path) -> Self {
        let sessions_dir = workspace_dir.join("sessions");
        let _ = std::fs::create_dir_all(&sessions_dir);

        let store = Self {
            sessions_dir,
            threads: Arc::new(RwLock::new(HashMap::new())),
        };

        store.load_all_sync();
        store
    }

    fn thread_file(&self, thread_id: &str) -> PathBuf {
        self.sessions_dir.join(format!("{thread_id}.jsonl"))
    }

    fn load_all_sync(&self) {
        let Ok(entries) = std::fs::read_dir(&self.sessions_dir) else {
            return;
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

        info!("Loaded {} threads from disk", threads.len());
        *self.threads.blocking_write() = threads;
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
        writeln!(file, "{json}")
            .map_err(|e| AppError::Custom(format!("Write error: {e}")))?;

        for turn in &thread.turns {
            let ts = RolloutLine::TurnStart {
                turn_id: turn.turn_id.clone(),
                started_at: turn.started_at,
                mode: turn.mode.clone(),
                goal_budget_tokens: turn.goal_budget_tokens,
            };
            let json = serde_json::to_string(&ts)
                .map_err(|e| AppError::Custom(format!("Serialize error: {e}")))?;
            writeln!(file, "{json}")
                .map_err(|e| AppError::Custom(format!("Write error: {e}")))?;

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
        let thread_id = uuid::Uuid::new_v4().to_string();
        let now = now_secs();

        let thread = StoredThread {
            id: thread_id.clone(),
            name: None,
            created_at: now,
            updated_at: now,
            model,
            goal: None,
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
        if !self.threads.read().await.contains_key(thread_id) {
            return Err(AppError::Custom(format!("Thread not found: {thread_id}")));
        }

        let now = now_secs();
        self.append_line(thread_id, &RolloutLine::ThreadGoalClear { updated_at: now })?;

        let mut threads = self.threads.write().await;
        if let Some(thread) = threads.get_mut(thread_id) {
            thread.goal = None;
            thread.updated_at = now;
        }
        Ok(())
    }

    pub async fn record_goal_usage(
        &self,
        thread_id: &str,
        total_tokens: u64,
    ) -> AppResult<Option<ThreadGoal>> {
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
        let threads = self.threads.read().await;
        let mut list: Vec<StoredThread> = threads.values().cloned().collect();
        list.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        list
    }

    pub async fn get_thread(&self, thread_id: &str) -> Option<StoredThread> {
        self.threads.read().await.get(thread_id).cloned()
    }

    pub async fn get_thread_messages(&self, thread_id: &str) -> Vec<ThreadMessage> {
        let threads = self.threads.read().await;
        threads
            .get(thread_id)
            .map(|t| t.all_messages().into_iter().cloned().collect())
            .unwrap_or_default()
    }

    pub async fn get_thread_total_tokens(&self, thread_id: &str) -> u64 {
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

    pub async fn replace_messages(
        &self,
        thread_id: &str,
        new_messages: Vec<ThreadMessage>,
    ) -> AppResult<()> {
        let mut threads = self.threads.write().await;
        let Some(thread) = threads.get_mut(thread_id) else {
            return Err(AppError::Custom(format!(
                "Thread not found: {thread_id}"
            )));
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
}
