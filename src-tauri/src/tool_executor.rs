use std::path::PathBuf;
use std::process::Stdio;

use serde::Deserialize;
use tauri::{AppHandle, Emitter};
use tokio::process::Command;
use tracing::info;

use crate::error::AppResult;

pub struct ToolExecutor {
    cwd: PathBuf,
}

impl ToolExecutor {
    pub fn new(cwd: PathBuf) -> Self {
        Self { cwd }
    }

    pub fn set_cwd(&mut self, cwd: PathBuf) {
        self.cwd = cwd;
    }

    pub fn tool_specs(&self) -> Vec<serde_json::Value> {
        vec![
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "shell",
                    "description": "Execute a shell command and return the output (30 second timeout). Use this to run commands, install packages, run tests, etc. Do NOT use this to start long-running processes like servers — they will be killed after 30 seconds.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "command": {
                                "type": "array",
                                "items": { "type": "string" },
                                "description": "The command and its arguments as an array of strings."
                            }
                        },
                        "required": ["command"]
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "read_file",
                    "description": "Read the contents of a file at the given path.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "path": {
                                "type": "string",
                                "description": "The file path to read."
                            }
                        },
                        "required": ["path"]
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "write_file",
                    "description": "Write content to a file at the given path. Creates the file if it does not exist.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "path": {
                                "type": "string",
                                "description": "The file path to write."
                            },
                            "content": {
                                "type": "string",
                                "description": "The content to write to the file."
                            }
                        },
                        "required": ["path", "content"]
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "list_directory",
                    "description": "List files and directories at the given path.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "path": {
                                "type": "string",
                                "description": "The directory path to list. Defaults to current working directory."
                            }
                        },
                        "required": []
                    }
                }
            }),
        ]
    }

    pub async fn execute(
        &self,
        tool_name: &str,
        arguments: &str,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
    ) -> AppResult<String> {
        match tool_name {
            "shell" => self.exec_shell(arguments, call_id, app_handle, thread_id).await,
            "read_file" => self.exec_read_file(arguments, call_id, app_handle, thread_id).await,
            "write_file" => self.exec_write_file(arguments, call_id, app_handle, thread_id).await,
            "list_directory" => self.exec_list_dir(arguments, call_id, app_handle, thread_id).await,
            other => Ok(format!("Unknown tool: {other}")),
        }
    }

    fn emit_tool_start(&self, app_handle: &AppHandle, thread_id: &str, call_id: &str, tool: &str, command: &str) {
        app_handle
            .emit(
                "tool-exec-start",
                serde_json::json!({
                    "threadId": thread_id,
                    "callId": call_id,
                    "tool": tool,
                    "command": command,
                }),
            )
            .ok();
    }

    fn emit_tool_end(&self, app_handle: &AppHandle, thread_id: &str, call_id: &str, tool: &str, exit_code: i32, output: &str) {
        let truncated_output = if output.len() > 4000 {
            let prefix: String = output.chars().take(2000).collect();
            let suffix: String = output.chars().rev().take(1500).collect::<Vec<_>>().into_iter().rev().collect();
            format!("{prefix}\n\n... [{} chars truncated] ...\n\n{suffix}", output.len() - 3500)
        } else {
            output.to_string()
        };
        app_handle
            .emit(
                "tool-exec-end",
                serde_json::json!({
                    "threadId": thread_id,
                    "callId": call_id,
                    "tool": tool,
                    "exitCode": exit_code,
                    "output": truncated_output,
                }),
            )
            .ok();
    }

    async fn exec_shell(
        &self,
        arguments: &str,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
    ) -> AppResult<String> {
        #[derive(Deserialize)]
        struct ShellArgs {
            command: Vec<String>,
        }

        let args: ShellArgs = serde_json::from_str(arguments)
            .map_err(|e| crate::error::AppError::Custom(format!("Invalid shell args: {e}")))?;

        if args.command.is_empty() {
            return Ok("Error: empty command".to_string());
        }

        let cmd_display = args.command.join(" ");
        info!("Executing shell: {cmd_display}");

        self.emit_tool_start(app_handle, thread_id, call_id, "shell", &cmd_display);

        let (program, cmd_args) = if cfg!(target_os = "windows") {
            ("cmd".to_string(), vec!["/C".to_string(), cmd_display.clone()])
        } else {
            ("sh".to_string(), vec!["-c".to_string(), cmd_display.clone()])
        };

        let mut child = Command::new(&program)
            .args(&cmd_args)
            .current_dir(&self.cwd)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| {
                crate::error::AppError::Custom(format!("Failed to spawn command: {e}"))
            })?;

        let child_stdout = child.stdout.take();
        let child_stderr = child.stderr.take();

        let stdout_handle = tokio::spawn(async move {
            let mut buf = Vec::new();
            if let Some(mut out) = child_stdout {
                tokio::io::AsyncReadExt::read_to_end(&mut out, &mut buf).await.ok();
            }
            buf
        });
        let stderr_handle = tokio::spawn(async move {
            let mut buf = Vec::new();
            if let Some(mut err) = child_stderr {
                tokio::io::AsyncReadExt::read_to_end(&mut err, &mut buf).await.ok();
            }
            buf
        });

        let timeout = std::time::Duration::from_secs(30);
        match tokio::time::timeout(timeout, child.wait()).await {
            Ok(Ok(status)) => {
                let stdout_bytes = stdout_handle.await.unwrap_or_default();
                let stderr_bytes = stderr_handle.await.unwrap_or_default();
                let stdout = String::from_utf8_lossy(&stdout_bytes);
                let stderr = String::from_utf8_lossy(&stderr_bytes);
                let exit_code = status.code().unwrap_or(-1);

                let result = if exit_code == 0 {
                    if stderr.is_empty() {
                        stdout.to_string()
                    } else {
                        format!("{stdout}\n[stderr]\n{stderr}")
                    }
                } else {
                    format!("[exit code: {exit_code}]\n{stdout}\n[stderr]\n{stderr}")
                };

                let truncated = truncate_output(&result, 8000);
                self.emit_tool_end(app_handle, thread_id, call_id, "shell", exit_code, &truncated);
                Ok(truncated)
            }
            Ok(Err(e)) => {
                let msg = format!("Failed to wait for command: {e}");
                self.emit_tool_end(app_handle, thread_id, call_id, "shell", -1, &msg);
                Ok(msg)
            }
            Err(_) => {
                child.kill().await.ok();
                stdout_handle.abort();
                stderr_handle.abort();
                info!("Shell command timed out after 30s: {cmd_display}");
                let msg = format!("Command timed out after 30 seconds.\nThe command '{cmd_display}' did not complete within the time limit.\nIf this is a long-running process (like a server), it has been terminated.\nConsider using a different approach for long-running processes.");
                self.emit_tool_end(app_handle, thread_id, call_id, "shell", 124, &msg);
                Ok(msg)
            }
        }
    }

    async fn exec_read_file(
        &self,
        arguments: &str,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
    ) -> AppResult<String> {
        #[derive(Deserialize)]
        struct ReadArgs {
            path: String,
        }

        let args: ReadArgs = serde_json::from_str(arguments)
            .map_err(|e| crate::error::AppError::Custom(format!("Invalid read_file args: {e}")))?;

        let full_path = self.cwd.join(&args.path);
        info!("Reading file: {}", full_path.display());

        self.emit_tool_start(app_handle, thread_id, call_id, "read_file", &args.path);

        let result = match tokio::fs::read_to_string(&full_path).await {
            Ok(content) => {
                let truncated = truncate_output(&content, 16000);
                self.emit_tool_end(app_handle, thread_id, call_id, "read_file", 0, &truncated);
                truncated
            }
            Err(e) => {
                let msg = format!("Error reading {}: {e}", args.path);
                self.emit_tool_end(app_handle, thread_id, call_id, "read_file", -1, &msg);
                msg
            }
        };
        Ok(result)
    }

    async fn exec_write_file(
        &self,
        arguments: &str,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
    ) -> AppResult<String> {
        #[derive(Deserialize)]
        struct WriteArgs {
            path: String,
            content: String,
        }

        let args: WriteArgs = serde_json::from_str(arguments)
            .map_err(|e| crate::error::AppError::Custom(format!("Invalid write_file args: {e}")))?;

        let full_path = self.cwd.join(&args.path);
        info!("Writing file: {}", full_path.display());

        self.emit_tool_start(app_handle, thread_id, call_id, "write_file", &args.path);

        if let Some(parent) = full_path.parent() {
            tokio::fs::create_dir_all(parent).await.ok();
        }

        let result = match tokio::fs::write(&full_path, &args.content).await {
            Ok(()) => {
                let msg = format!("Successfully wrote {} bytes to {}", args.content.len(), args.path);
                self.emit_tool_end(app_handle, thread_id, call_id, "write_file", 0, &msg);
                msg
            }
            Err(e) => {
                let msg = format!("Error writing {}: {e}", args.path);
                self.emit_tool_end(app_handle, thread_id, call_id, "write_file", -1, &msg);
                msg
            }
        };
        Ok(result)
    }

    async fn exec_list_dir(
        &self,
        arguments: &str,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
    ) -> AppResult<String> {
        #[derive(Deserialize, Default)]
        struct ListArgs {
            #[serde(default)]
            path: Option<String>,
        }

        let args: ListArgs = serde_json::from_str(arguments).unwrap_or_default();
        let dir = match args.path {
            Some(ref p) if !p.is_empty() => self.cwd.join(p),
            _ => self.cwd.clone(),
        };

        let display_path = args.path.as_deref().unwrap_or(".");
        info!("Listing directory: {}", dir.display());

        self.emit_tool_start(app_handle, thread_id, call_id, "list_directory", display_path);

        let mut entries = Vec::new();
        match tokio::fs::read_dir(&dir).await {
            Ok(mut reader) => {
                while let Ok(Some(entry)) = reader.next_entry().await {
                    let name = entry.file_name().to_string_lossy().to_string();
                    let is_dir = entry.file_type().await.map(|t| t.is_dir()).unwrap_or(false);
                    entries.push(if is_dir {
                        format!("{name}/")
                    } else {
                        name
                    });
                }
                entries.sort();
                let output = entries.join("\n");
                self.emit_tool_end(app_handle, thread_id, call_id, "list_directory", 0, &output);
                Ok(output)
            }
            Err(e) => {
                let msg = format!("Error listing {}: {e}", dir.display());
                self.emit_tool_end(app_handle, thread_id, call_id, "list_directory", -1, &msg);
                Ok(msg)
            }
        }
    }
}

fn truncate_output(s: &str, max_chars: usize) -> String {
    if s.len() <= max_chars {
        s.to_string()
    } else {
        let half = max_chars / 2;
        let start: String = s.chars().take(half).collect();
        let end: String = s.chars().rev().take(half).collect::<Vec<_>>().into_iter().rev().collect();
        format!("{start}\n\n... [truncated {remaining} chars] ...\n\n{end}", remaining = s.len() - max_chars)
    }
}
