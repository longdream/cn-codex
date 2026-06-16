use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use chrono::Utc;
use serde::Serialize;
use tracing::{info, warn};

use crate::adapter::types::InternalMessage;

#[derive(Debug, Serialize)]
struct LogEntry {
    timestamp: String,
    direction: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    log_type: Option<&'static str>,
    thread_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    iteration: Option<u32>,
    model: String,
    wire_api: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    messages: Option<Vec<LogMessage>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    response_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<LogToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    usage: Option<LogUsage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    finish_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    duration_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Debug, Serialize)]
struct LogMessage {
    role: String,
    content: String,
}

#[derive(Debug, Serialize)]
struct LogToolCall {
    id: String,
    name: String,
    arguments: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct LogUsage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
}

pub struct ConversationLogger {
    log_dir: PathBuf,
}

impl ConversationLogger {
    pub fn new(workspace_config_dir: &Path) -> Self {
        let log_dir = workspace_config_dir.join("logs").join("conversations");
        if let Err(e) = fs::create_dir_all(&log_dir) {
            warn!("Failed to create conversation log dir: {e}");
        }
        Self { log_dir }
    }

    fn log_file_path(&self, thread_id: &str) -> PathBuf {
        let date = Utc::now().format("%Y-%m-%d");
        let safe_id: String = thread_id
            .chars()
            .map(|c| if c.is_alphanumeric() || c == '-' { c } else { '_' })
            .collect();
        self.log_dir.join(format!("{safe_id}_{date}.jsonl"))
    }

    fn write_entry(&self, entry: &LogEntry) {
        let path = self.log_file_path(&entry.thread_id);
        let line = match serde_json::to_string(entry) {
            Ok(json) => json,
            Err(e) => {
                warn!("Failed to serialize conversation log entry: {e}");
                return;
            }
        };
        match OpenOptions::new().create(true).append(true).open(&path) {
            Ok(mut file) => {
                if let Err(e) = writeln!(file, "{line}") {
                    warn!("Failed to write conversation log: {e}");
                }
            }
            Err(e) => {
                warn!("Failed to open conversation log file {}: {e}", path.display());
            }
        }
    }

    pub fn log_request(
        &self,
        thread_id: &str,
        iteration: u32,
        model: &str,
        wire_api: &str,
        messages: &[InternalMessage],
        tools_count: usize,
    ) {
        let log_messages: Vec<LogMessage> = messages
            .iter()
            .map(|m| {
                let content = m
                    .content
                    .as_ref()
                    .map(|c| match c {
                        serde_json::Value::String(s) => truncate_string(s, 2000),
                        other => truncate_string(&other.to_string(), 2000),
                    })
                    .unwrap_or_default();
                LogMessage {
                    role: m.role.clone(),
                    content,
                }
            })
            .collect();

        let entry = LogEntry {
            timestamp: Utc::now().to_rfc3339(),
            direction: "request",
            log_type: None,
            thread_id: thread_id.to_string(),
            iteration: Some(iteration),
            model: model.to_string(),
            wire_api: wire_api.to_string(),
            messages: Some(log_messages),
            tools_count: Some(tools_count),
            response_text: None,
            tool_calls: None,
            usage: None,
            finish_reason: None,
            duration_ms: None,
            error: None,
        };
        self.write_entry(&entry);
        info!(
            "Conversation log: request thread={thread_id} iter={iteration} msgs={} tools={tools_count}",
            messages.len()
        );
    }

    pub fn log_response(
        &self,
        thread_id: &str,
        iteration: u32,
        model: &str,
        wire_api: &str,
        response_text: &str,
        tool_calls: &[(String, String, String)],
        usage: Option<LogUsage>,
        finish_reason: Option<&str>,
        duration_ms: u64,
    ) {
        let log_tool_calls: Vec<LogToolCall> = tool_calls
            .iter()
            .map(|(id, name, args)| LogToolCall {
                id: id.clone(),
                name: name.clone(),
                arguments: truncate_string(args, 2000),
            })
            .collect();

        let entry = LogEntry {
            timestamp: Utc::now().to_rfc3339(),
            direction: "response",
            log_type: None,
            thread_id: thread_id.to_string(),
            iteration: Some(iteration),
            model: model.to_string(),
            wire_api: wire_api.to_string(),
            messages: None,
            tools_count: None,
            response_text: Some(truncate_string(response_text, 5000)),
            tool_calls: if log_tool_calls.is_empty() {
                None
            } else {
                Some(log_tool_calls)
            },
            usage,
            finish_reason: finish_reason.map(str::to_string),
            duration_ms: Some(duration_ms),
            error: None,
        };
        self.write_entry(&entry);
    }

    pub fn log_error(
        &self,
        thread_id: &str,
        iteration: u32,
        model: &str,
        wire_api: &str,
        error: &str,
        duration_ms: u64,
    ) {
        let entry = LogEntry {
            timestamp: Utc::now().to_rfc3339(),
            direction: "response",
            log_type: None,
            thread_id: thread_id.to_string(),
            iteration: Some(iteration),
            model: model.to_string(),
            wire_api: wire_api.to_string(),
            messages: None,
            tools_count: None,
            response_text: None,
            tool_calls: None,
            usage: None,
            finish_reason: Some("error".to_string()),
            duration_ms: Some(duration_ms),
            error: Some(error.to_string()),
        };
        self.write_entry(&entry);
    }

    pub fn log_compaction_request(
        &self,
        thread_id: &str,
        model: &str,
        wire_api: &str,
        history_count: usize,
        estimated_input_tokens: usize,
    ) {
        let entry = LogEntry {
            timestamp: Utc::now().to_rfc3339(),
            direction: "request",
            log_type: Some("compaction"),
            thread_id: thread_id.to_string(),
            iteration: None,
            model: model.to_string(),
            wire_api: wire_api.to_string(),
            messages: None,
            tools_count: None,
            response_text: Some(format!(
                "history_msgs={history_count}, estimated_input_tokens={estimated_input_tokens}"
            )),
            tool_calls: None,
            usage: None,
            finish_reason: None,
            duration_ms: None,
            error: None,
        };
        self.write_entry(&entry);
    }

    pub fn log_compaction_response(
        &self,
        thread_id: &str,
        model: &str,
        wire_api: &str,
        summary_text: &str,
        duration_ms: u64,
    ) {
        let entry = LogEntry {
            timestamp: Utc::now().to_rfc3339(),
            direction: "response",
            log_type: Some("compaction"),
            thread_id: thread_id.to_string(),
            iteration: None,
            model: model.to_string(),
            wire_api: wire_api.to_string(),
            messages: None,
            tools_count: None,
            response_text: Some(truncate_string(summary_text, 5000)),
            tool_calls: None,
            usage: None,
            finish_reason: Some("stop".to_string()),
            duration_ms: Some(duration_ms),
            error: None,
        };
        self.write_entry(&entry);
    }

    pub fn log_compaction_error(
        &self,
        thread_id: &str,
        model: &str,
        wire_api: &str,
        error: &str,
        duration_ms: u64,
    ) {
        let entry = LogEntry {
            timestamp: Utc::now().to_rfc3339(),
            direction: "response",
            log_type: Some("compaction"),
            thread_id: thread_id.to_string(),
            iteration: None,
            model: model.to_string(),
            wire_api: wire_api.to_string(),
            messages: None,
            tools_count: None,
            response_text: None,
            tool_calls: None,
            usage: None,
            finish_reason: Some("error".to_string()),
            duration_ms: Some(duration_ms),
            error: Some(error.to_string()),
        };
        self.write_entry(&entry);
    }
}

fn truncate_string(s: &str, max_chars: usize) -> String {
    if s.len() <= max_chars {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max_chars).collect();
        format!("{truncated}... [truncated, total {} chars]", s.len())
    }
}
