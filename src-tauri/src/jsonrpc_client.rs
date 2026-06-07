use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{mpsc, oneshot, Mutex};
use tracing::{error, info, warn};

use crate::error::AppError;
use crate::protocol::{JSONRPCNotification, JSONRPCRequest, JSONRPCResponse, RequestId};

static REQUEST_COUNTER: AtomicI64 = AtomicI64::new(1);

pub fn next_request_id() -> RequestId {
    RequestId::Integer(REQUEST_COUNTER.fetch_add(1, Ordering::Relaxed))
}

#[derive(Debug)]
pub enum ServerEvent {
    Notification {
        method: String,
        params: Option<serde_json::Value>,
    },
    ServerRequest {
        id: RequestId,
        method: String,
        params: Option<serde_json::Value>,
    },
}

type PendingMap = Arc<Mutex<HashMap<String, oneshot::Sender<JSONRPCResponse>>>>;

pub struct JsonRpcClient {
    child: Option<Child>,
    stdin_tx: mpsc::Sender<String>,
    event_rx: Mutex<mpsc::Receiver<ServerEvent>>,
    pending: PendingMap,
}

fn id_key(id: &RequestId) -> String {
    match id {
        RequestId::Integer(n) => n.to_string(),
        RequestId::String(s) => s.clone(),
    }
}

impl JsonRpcClient {
    fn resolve_codex_executable(explicit: Option<String>) -> String {
        if let Some(path) = explicit.filter(|value| !value.trim().is_empty()) {
            return path;
        }

        if let Ok(path) = std::env::var("CODEX_CLI_PATH") {
            if !path.trim().is_empty() {
                return path;
            }
        }

        let lookup = if cfg!(target_os = "windows") {
            ("where.exe", vec!["codex.exe"])
        } else {
            ("which", vec!["codex"])
        };

        if let Ok(output) = std::process::Command::new(lookup.0)
            .args(lookup.1)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output()
        {
            if output.status.success() {
                if let Some(path) = String::from_utf8_lossy(&output.stdout)
                    .lines()
                    .map(str::trim)
                    .find(|line| !line.is_empty())
                {
                    return path.to_string();
                }
            }
        }

        if cfg!(target_os = "windows") {
            "codex.exe".to_string()
        } else {
            "codex".to_string()
        }
    }

    pub async fn start(
        codex_exe: Option<String>,
        project_root: PathBuf,
        workspace_config_dir: PathBuf,
    ) -> Result<Self, AppError> {
        let exe = Self::resolve_codex_executable(codex_exe);

        info!(
            "Starting codex app-server: {exe} app-server --stdio (cwd={}, config_dir={})",
            project_root.display(),
            workspace_config_dir.display()
        );

        let mut child = Command::new(&exe)
            .args(["app-server", "--stdio"])
            .current_dir(&project_root)
            .env("CODEX_HOME", &workspace_config_dir)
            .env("CN_CODEX_PROJECT_ROOT", &project_root)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| AppError::Custom(format!("Failed to start codex process '{exe}': {e}")))?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| AppError::Custom("Failed to capture codex stdin".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| AppError::Custom("Failed to capture codex stdout".into()))?;

        let (stdin_tx, mut stdin_rx) = mpsc::channel::<String>(256);
        let (event_tx, event_rx) = mpsc::channel::<ServerEvent>(256);
        let pending: PendingMap = Arc::new(Mutex::new(HashMap::new()));

        tokio::spawn(async move {
            let mut stdin = stdin;
            while let Some(line) = stdin_rx.recv().await {
                if let Err(e) = stdin.write_all(line.as_bytes()).await {
                    error!("Failed to write to codex stdin: {e}");
                    break;
                }
                if let Err(e) = stdin.write_all(b"\n").await {
                    error!("Failed to write newline to codex stdin: {e}");
                    break;
                }
                if let Err(e) = stdin.flush().await {
                    error!("Failed to flush codex stdin: {e}");
                    break;
                }
            }
        });

        let pending_clone = pending.clone();
        tokio::spawn(async move {
            let reader = BufReader::new(stdout);
            let mut lines = reader.lines();

            loop {
                match lines.next_line().await {
                    Ok(Some(line)) => {
                        let line = line.trim().to_string();
                        if line.is_empty() {
                            continue;
                        }

                        let parsed: serde_json::Value = match serde_json::from_str(&line) {
                            Ok(value) => value,
                            Err(e) => {
                                warn!("Non-JSON line from codex: {e}: {line}");
                                continue;
                            }
                        };

                        if parsed.get("id").is_some() && parsed.get("method").is_some() {
                            let id = serde_json::from_value::<RequestId>(parsed["id"].clone())
                                .unwrap_or(RequestId::Integer(0));
                            let method = parsed["method"].as_str().unwrap_or("").to_string();
                            let params = parsed.get("params").cloned();
                            let _ = event_tx
                                .send(ServerEvent::ServerRequest { id, method, params })
                                .await;
                        } else if parsed.get("id").is_some()
                            && (parsed.get("result").is_some() || parsed.get("error").is_some())
                        {
                            if let Ok(resp) = serde_json::from_value::<JSONRPCResponse>(parsed) {
                                if let Some(ref id) = resp.id {
                                    let key = id_key(id);
                                    let sender = {
                                        let mut pending = pending_clone.lock().await;
                                        pending.remove(&key)
                                    };
                                    if let Some(tx) = sender {
                                        let _ = tx.send(resp);
                                    }
                                }
                            }
                        } else if parsed.get("method").is_some() {
                            let method = parsed["method"].as_str().unwrap_or("").to_string();
                            let params = parsed.get("params").cloned();
                            let _ = event_tx
                                .send(ServerEvent::Notification { method, params })
                                .await;
                        }
                    }
                    Ok(None) => {
                        info!("Codex stdout stream ended");
                        break;
                    }
                    Err(e) => {
                        error!("Error reading codex stdout: {e}");
                        break;
                    }
                }
            }
        });

        let client = Self {
            child: Some(child),
            stdin_tx,
            event_rx: Mutex::new(event_rx),
            pending,
        };

        client.initialize_session().await?;

        info!("Codex app-server started successfully");

        Ok(client)
    }

    pub async fn request(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, AppError> {
        let id = next_request_id();
        let request = JSONRPCRequest::new(id.clone(), method, Some(params));

        let (tx, rx) = oneshot::channel();
        {
            let mut pending = self.pending.lock().await;
            pending.insert(id_key(&id), tx);
        }

        let json = serde_json::to_string(&request)
            .map_err(|e| AppError::Custom(format!("JSON serialize error: {e}")))?;

        self.stdin_tx
            .send(json)
            .await
            .map_err(|e| AppError::Custom(format!("Failed to send to codex: {e}")))?;

        let resp = rx
            .await
            .map_err(|_| AppError::Custom("Response channel dropped".into()))?;

        if let Some(err) = resp.error {
            Err(AppError::ServerRequest(format!(
                "[{}] {}",
                err.code, err.message
            )))
        } else {
            Ok(resp.result.unwrap_or(serde_json::Value::Null))
        }
    }

    pub async fn request_typed<R: serde::de::DeserializeOwned>(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<R, AppError> {
        let result = self.request(method, params).await?;
        serde_json::from_value(result)
            .map_err(|e| AppError::Custom(format!("Response deserialize error: {e}")))
    }

    pub async fn next_event(&self) -> Option<ServerEvent> {
        let mut event_rx = self.event_rx.lock().await;
        event_rx.recv().await
    }

    async fn notify(
        &self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> Result<(), AppError> {
        let message = JSONRPCNotification {
            jsonrpc: "2.0".to_string(),
            method: method.to_string(),
            params,
        };
        let json = serde_json::to_string(&message)
            .map_err(|e| AppError::Custom(format!("JSON serialize error: {e}")))?;
        self.stdin_tx
            .send(json)
            .await
            .map_err(|e| AppError::Custom(format!("Failed to send notification: {e}")))
    }

    async fn initialize_session(&self) -> Result<(), AppError> {
        let params = serde_json::json!({
            "clientInfo": {
                "name": "cn-codex",
                "title": "CN-Codex",
                "version": env!("CARGO_PKG_VERSION"),
            },
            "capabilities": {
                "experimentalApi": false
            }
        });

        let _: serde_json::Value = self.request("initialize", params).await?;
        self.notify("initialized", None).await
    }

    pub async fn resolve_server_request(
        &self,
        request_id: RequestId,
        result: serde_json::Value,
    ) -> Result<(), AppError> {
        let resp = serde_json::json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "result": result
        });
        let json = serde_json::to_string(&resp)
            .map_err(|e| AppError::Custom(format!("JSON serialize error: {e}")))?;
        self.stdin_tx
            .send(json)
            .await
            .map_err(|e| AppError::Custom(format!("Failed to send resolve: {e}")))
    }

    pub async fn reject_server_request(
        &self,
        request_id: RequestId,
        error: crate::protocol::JSONRPCErrorError,
    ) -> Result<(), AppError> {
        let resp = serde_json::json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "error": error
        });
        let json = serde_json::to_string(&resp)
            .map_err(|e| AppError::Custom(format!("JSON serialize error: {e}")))?;
        self.stdin_tx
            .send(json)
            .await
            .map_err(|e| AppError::Custom(format!("Failed to send reject: {e}")))
    }

    pub async fn shutdown(&mut self) -> Result<(), AppError> {
        info!("Shutting down codex app-server");
        if let Some(ref mut child) = self.child {
            let _ = child.kill().await;
        }
        Ok(())
    }
}

impl Clone for JsonRpcClient {
    fn clone(&self) -> Self {
        panic!("JsonRpcClient should not be cloned; use Arc<JsonRpcClient> instead");
    }
}
