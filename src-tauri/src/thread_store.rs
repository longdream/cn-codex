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
    pub messages: Vec<ThreadMessage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredThread {
    pub id: String,
    pub name: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    pub model: Option<String>,
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
                if m.content.len() > 60 { format!("{s}...") } else { s }
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
    },
    Message(ThreadMessage),
    TurnEnd {
        turn_id: String,
        completed_at: i64,
    },
    ThreadUpdate {
        name: Option<String>,
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
                        turns: Vec::new(),
                    });
                }
                RolloutLine::TurnStart {
                    turn_id,
                    started_at,
                } => {
                    current_turn = Some(StoredTurn {
                        turn_id,
                        started_at,
                        completed_at: None,
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
                } => {
                    if let Some(mut turn) = current_turn.take() {
                        turn.completed_at = Some(completed_at);
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

    pub async fn create_thread(&self, model: Option<String>) -> AppResult<StoredThread> {
        let thread_id = uuid::Uuid::new_v4().to_string();
        let now = now_secs();

        let thread = StoredThread {
            id: thread_id.clone(),
            name: None,
            created_at: now,
            updated_at: now,
            model,
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

        self.threads
            .write()
            .await
            .insert(thread_id, thread.clone());
        Ok(thread)
    }

    pub async fn start_turn(&self, thread_id: &str) -> AppResult<String> {
        let turn_id = uuid::Uuid::new_v4().to_string();
        let now = now_secs();

        self.append_line(
            thread_id,
            &RolloutLine::TurnStart {
                turn_id: turn_id.clone(),
                started_at: now,
            },
        )?;

        let mut threads = self.threads.write().await;
        if let Some(thread) = threads.get_mut(thread_id) {
            thread.turns.push(StoredTurn {
                turn_id: turn_id.clone(),
                started_at: now,
                completed_at: None,
                messages: Vec::new(),
            });
            thread.updated_at = now;
        }

        Ok(turn_id)
    }

    pub async fn add_message(
        &self,
        thread_id: &str,
        msg: ThreadMessage,
    ) -> AppResult<()> {
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

    pub async fn end_turn(&self, thread_id: &str, turn_id: &str) -> AppResult<()> {
        let now = now_secs();
        self.append_line(
            thread_id,
            &RolloutLine::TurnEnd {
                turn_id: turn_id.to_string(),
                completed_at: now,
            },
        )?;

        let mut threads = self.threads.write().await;
        if let Some(thread) = threads.get_mut(thread_id) {
            if let Some(turn) = thread.turns.iter_mut().find(|t| t.turn_id == turn_id) {
                turn.completed_at = Some(now);
            }
            thread.updated_at = now;
        }
        Ok(())
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
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
