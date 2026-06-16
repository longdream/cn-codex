use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use axum::Router;
use axum::extract::WebSocketUpgrade;
use axum::extract::ws::{Message, WebSocket};
use axum::extract::State as AxumState;
use axum::response::IntoResponse;
use axum::routing::get;
use futures_util::{SinkExt, StreamExt};
use tokio::sync::{Mutex, RwLock, mpsc, oneshot};
use tower_http::cors::CorsLayer;
use tracing::{error, info, warn};

use crate::wps_protocol::{
    WpsConnectionStatus, WpsDocumentInfo, WpsHandshake, WpsRequest, WpsResponse, WpsServerStatus,
};

const DEFAULT_PORT: u16 = 23300;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

struct PendingRequest {
    tx: oneshot::Sender<WpsResponse>,
}

struct WpsConnection {
    sender: mpsc::Sender<String>,
    pending: Arc<Mutex<HashMap<String, PendingRequest>>>,
    addin_name: String,
    addin_version: Option<String>,
    wps_version: Option<String>,
    active_document: Arc<RwLock<Option<WpsDocumentInfo>>>,
}

struct ServerInner {
    connections: RwLock<HashMap<String, Arc<WpsConnection>>>,
    request_counter: AtomicU64,
    event_tx: mpsc::Sender<WpsEvent>,
}

/// Events emitted by the WPS server for the host application.
#[derive(Debug, Clone)]
pub enum WpsEvent {
    Connected {
        conn_id: String,
        addin_name: String,
    },
    Disconnected {
        conn_id: String,
    },
    Notification {
        conn_id: String,
        method: String,
        params: Option<serde_json::Value>,
    },
    DocumentChanged {
        conn_id: String,
        document: Option<WpsDocumentInfo>,
    },
}

/// The WPS WebSocket server that cn-codex hosts for WPS add-in connections.
pub struct WpsServer {
    inner: Arc<ServerInner>,
    running: Arc<AtomicBool>,
    port: std::sync::Mutex<u16>,
    event_rx: Mutex<Option<mpsc::Receiver<WpsEvent>>>,
}

impl WpsServer {
    pub fn new() -> Self {
        let (event_tx, event_rx) = mpsc::channel(256);
        Self {
            inner: Arc::new(ServerInner {
                connections: RwLock::new(HashMap::new()),
                request_counter: AtomicU64::new(1),
                event_tx,
            }),
            running: Arc::new(AtomicBool::new(false)),
            port: std::sync::Mutex::new(DEFAULT_PORT),
            event_rx: Mutex::new(Some(event_rx)),
        }
    }

    /// Take the event receiver (can only be called once).
    pub async fn take_event_receiver(&self) -> Option<mpsc::Receiver<WpsEvent>> {
        self.event_rx.lock().await.take()
    }

    /// Start the WebSocket server on the configured port.
    pub async fn start(&self, port: Option<u16>) -> Result<u16, String> {
        if self.running.load(Ordering::SeqCst) {
            return Err("WPS server is already running".into());
        }

        let port = port.unwrap_or_else(|| *self.port.lock().unwrap());
        let inner = self.inner.clone();

        let app = Router::new()
            .route("/wps", get(ws_handler))
            .route("/health", get(health_handler))
            .layer(CorsLayer::permissive())
            .with_state(inner.clone());

        let addr = SocketAddr::from(([127, 0, 0, 1], port));
        let bind_result = tokio::time::timeout(
            Duration::from_secs(3),
            tokio::net::TcpListener::bind(addr),
        )
        .await;

        let listener = match bind_result {
            Ok(Ok(l)) => l,
            Ok(Err(e)) => return Err(format!("Failed to bind port {port}: {e}")),
            Err(_) => return Err(format!("Timeout binding port {port}")),
        };

        let actual_port = listener
            .local_addr()
            .map(|a| a.port())
            .unwrap_or(port);

        info!("WPS WebSocket server listening on 127.0.0.1:{actual_port}");

        self.running.store(true, Ordering::SeqCst);
        *self.port.lock().unwrap() = actual_port;

        let running_flag = self.running.clone();
        tokio::spawn(async move {
            if let Err(e) = axum::serve(listener, app).await {
                error!("WPS server error: {e}");
            }
            running_flag.store(false, Ordering::SeqCst);
        });

        Ok(actual_port)
    }

    /// Send a command to the first available WPS connection and await the response.
    pub async fn send_command(
        &self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> Result<WpsResponse, String> {
        let connections = self.inner.connections.read().await;
        let conn = connections
            .values()
            .next()
            .cloned()
            .ok_or_else(|| "No WPS add-in connected".to_string())?;
        drop(connections);

        self.send_to_connection(&conn, method, params).await
    }

    /// Send a command to a specific connection by ID.
    pub async fn send_to(
        &self,
        conn_id: &str,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> Result<WpsResponse, String> {
        let connections = self.inner.connections.read().await;
        let conn = connections
            .get(conn_id)
            .cloned()
            .ok_or_else(|| format!("WPS connection '{conn_id}' not found"))?;
        drop(connections);

        self.send_to_connection(&conn, method, params).await
    }

    async fn send_to_connection(
        &self,
        conn: &WpsConnection,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> Result<WpsResponse, String> {
        let id = format!(
            "req-{}",
            self.inner.request_counter.fetch_add(1, Ordering::SeqCst)
        );

        let request = WpsRequest {
            id: id.clone(),
            method: method.to_string(),
            params,
        };

        let json =
            serde_json::to_string(&request).map_err(|e| format!("Serialize error: {e}"))?;

        let (tx, rx) = oneshot::channel();
        conn.pending
            .lock()
            .await
            .insert(id.clone(), PendingRequest { tx });

        conn.sender
            .send(json)
            .await
            .map_err(|_| "Failed to send to WPS connection".to_string())?;

        match tokio::time::timeout(REQUEST_TIMEOUT, rx).await {
            Ok(Ok(response)) => Ok(response),
            Ok(Err(_)) => {
                conn.pending.lock().await.remove(&id);
                Err("WPS response channel closed".into())
            }
            Err(_) => {
                conn.pending.lock().await.remove(&id);
                Err(format!(
                    "WPS command '{method}' timed out after {}s",
                    REQUEST_TIMEOUT.as_secs()
                ))
            }
        }
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    pub fn port(&self) -> u16 {
        *self.port.lock().unwrap()
    }

    pub async fn status(&self) -> WpsServerStatus {
        let running = self.is_running();
        let port = self.port();
        let connections = self.inner.connections.read().await;
        let conn_statuses: Vec<WpsConnectionStatus> = {
            let mut out = Vec::with_capacity(connections.len());
            for conn in connections.values() {
                let doc = conn.active_document.read().await.clone();
                out.push(WpsConnectionStatus {
                    connected: true,
                    addin_name: Some(conn.addin_name.clone()),
                    addin_version: conn.addin_version.clone(),
                    wps_version: conn.wps_version.clone(),
                    active_document: doc,
                });
            }
            out
        };

        WpsServerStatus {
            running,
            port,
            connections: conn_statuses,
        }
    }

    pub async fn has_connections(&self) -> bool {
        !self.inner.connections.read().await.is_empty()
    }
}

// --- Axum handlers ---

async fn health_handler() -> &'static str {
    "ok"
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    AxumState(inner): AxumState<Arc<ServerInner>>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_connection(socket, inner))
}

async fn handle_connection(socket: WebSocket, inner: Arc<ServerInner>) {
    let (mut ws_sender, mut ws_receiver) = socket.split();

    // Wait for handshake as the first message.
    let handshake = match wait_for_handshake(&mut ws_receiver).await {
        Some(hs) => hs,
        None => {
            warn!("WPS client disconnected before handshake");
            return;
        }
    };

    let conn_id = uuid::Uuid::new_v4().to_string();
    let (tx, mut rx) = mpsc::channel::<String>(128);

    let connection = Arc::new(WpsConnection {
        sender: tx,
        pending: Arc::new(Mutex::new(HashMap::new())),
        addin_name: handshake.addin_name.clone(),
        addin_version: handshake.addin_version.clone(),
        wps_version: handshake.wps_version.clone(),
        active_document: Arc::new(RwLock::new(handshake.active_document.clone())),
    });

    inner
        .connections
        .write()
        .await
        .insert(conn_id.clone(), connection.clone());

    info!(
        "WPS add-in connected: {} (conn_id={conn_id})",
        handshake.addin_name
    );

    let _ = inner.event_tx.send(WpsEvent::Connected {
        conn_id: conn_id.clone(),
        addin_name: handshake.addin_name.clone(),
    }).await;

    // Send acknowledgement to add-in.
    let ack = serde_json::json!({
        "type": "handshake_ack",
        "connId": conn_id,
        "serverVersion": env!("CARGO_PKG_VERSION"),
    });
    if let Ok(ack_text) = serde_json::to_string(&ack) {
        let _ = ws_sender.send(Message::Text(ack_text.into())).await;
    }

    let pending = connection.pending.clone();
    let active_doc = connection.active_document.clone();
    let event_tx = inner.event_tx.clone();
    let cid_recv = conn_id.clone();

    // Spawn send task: forward outgoing messages from the channel to the WebSocket.
    let send_task = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if ws_sender.send(Message::Text(msg.into())).await.is_err() {
                break;
            }
        }
    });

    // Receive task: read incoming WebSocket messages.
    let recv_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = ws_receiver.next().await {
            let text = match &msg {
                Message::Text(t) => t.to_string(),
                Message::Close(_) => break,
                _ => continue,
            };

            // Try to parse as a response first (has `id`).
            if let Ok(resp) = serde_json::from_str::<WpsResponse>(&text) {
                if !resp.id.is_empty() {
                    let mut map = pending.lock().await;
                    if let Some(req) = map.remove(&resp.id) {
                        let _ = req.tx.send(resp);
                    }
                    continue;
                }
            }

            // Otherwise treat as notification.
            if let Ok(notif) = serde_json::from_str::<crate::wps_protocol::WpsNotification>(&text)
            {
                // Handle document change events specially.
                if notif.method == "event.documentChanged"
                    || notif.method == "event.documentOpened"
                {
                    let doc_info = notif
                        .params
                        .as_ref()
                        .and_then(|p| serde_json::from_value::<WpsDocumentInfo>(p.clone()).ok());
                    *active_doc.write().await = doc_info.clone();
                    let _ = event_tx
                        .send(WpsEvent::DocumentChanged {
                            conn_id: cid_recv.clone(),
                            document: doc_info,
                        })
                        .await;
                }

                let _ = event_tx
                    .send(WpsEvent::Notification {
                        conn_id: cid_recv.clone(),
                        method: notif.method,
                        params: notif.params,
                    })
                    .await;
            }
        }
    });

    tokio::select! {
        _ = send_task => {},
        _ = recv_task => {},
    }

    // Clean up the connection.
    inner.connections.write().await.remove(&conn_id);
    info!("WPS add-in disconnected: conn_id={conn_id}");

    let _ = inner
        .event_tx
        .send(WpsEvent::Disconnected {
            conn_id: conn_id.clone(),
        })
        .await;
}

async fn wait_for_handshake(
    receiver: &mut futures_util::stream::SplitStream<WebSocket>,
) -> Option<WpsHandshake> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            warn!("WPS handshake timeout");
            return None;
        }

        match tokio::time::timeout(remaining, receiver.next()).await {
            Ok(Some(Ok(Message::Text(text)))) => {
                match serde_json::from_str::<WpsHandshake>(&text.to_string()) {
                    Ok(hs) if hs.msg_type == "handshake" => return Some(hs),
                    Ok(_) => {
                        warn!("WPS first message is not a handshake");
                        continue;
                    }
                    Err(e) => {
                        warn!("WPS handshake parse error: {e}");
                        continue;
                    }
                }
            }
            Ok(Some(Ok(Message::Close(_)))) | Ok(None) => return None,
            Ok(Some(Err(e))) => {
                warn!("WPS WebSocket error during handshake: {e}");
                return None;
            }
            Ok(Some(Ok(_))) => continue,
            Err(_) => {
                warn!("WPS handshake timeout");
                return None;
            }
        }
    }
}
