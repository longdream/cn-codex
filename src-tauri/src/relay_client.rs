use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio_tungstenite::tungstenite::Message;
use tracing::{error, info, warn};

use crate::mobile_server::SharedMobileState;

/// PC → Relay 的消息
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum PcToRelay {
    HttpResponse {
        id: String,
        status: u16,
        body: serde_json::Value,
    },
    Broadcast {
        event: String,
        payload: serde_json::Value,
    },
}

/// Relay → PC 的消息
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum RelayToPc {
    HttpRequest {
        id: String,
        method: String,
        path: String,
        body: Option<serde_json::Value>,
    },
    WsMessage {
        data: String,
    },
}

/// 启动 relay client，连接远程中转服务器
pub fn start_relay_client(
    relay_url: String,
    room_id: String,
    mobile_state: SharedMobileState,
) {
    tokio::spawn(async move {
        relay_loop(relay_url, room_id, mobile_state).await;
    });
}

/// relay 地址规范化：
/// - 去掉首尾空白
/// - 去掉末尾 `/`
/// 这样可以避免拼接 ws 地址时出现 `//pc/{room_id}` 导致路由不匹配。
fn normalize_relay_base_url(raw: &str) -> String {
    raw.trim().trim_end_matches('/').to_string()
}

async fn relay_loop(relay_url: String, room_id: String, mobile_state: SharedMobileState) {
    loop {
        let relay_base_url = normalize_relay_base_url(&relay_url);
        let ws_base_url = relay_base_url
            .replace("http://", "ws://")
            .replace("https://", "wss://");
        let ws_url = format!("{ws_base_url}/pc/{room_id}");
        info!("[relay_client] connecting to {ws_url}");

        match tokio_tungstenite::connect_async(&ws_url).await {
            Ok((ws_stream, _)) => {
                info!("[relay_client] connected to relay server");
                handle_relay_connection(ws_stream, mobile_state.clone()).await;
                warn!("[relay_client] disconnected from relay, reconnecting in 3s...");
            }
            Err(e) => {
                error!("[relay_client] connection failed: {e}, retrying in 5s...");
                tokio::time::sleep(Duration::from_secs(5)).await;
                continue;
            }
        }

        tokio::time::sleep(Duration::from_secs(3)).await;
    }
}

async fn handle_relay_connection(
    ws_stream: tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
    mobile_state: SharedMobileState,
) {
    let (ws_sender, mut ws_receiver) = ws_stream.split();
    let mut broadcast_rx = mobile_state.broadcast_tx.subscribe();

    let sender_arc = Arc::new(tokio::sync::Mutex::new(ws_sender));

    // 转发本地 broadcast 事件到 relay
    let broadcast_sender = Arc::clone(&sender_arc);
    let broadcast_task = tokio::spawn(async move {
        while let Ok(event) = broadcast_rx.recv().await {
            let msg = PcToRelay::Broadcast {
                event: event.event,
                payload: event.payload,
            };
            if let Ok(text) = serde_json::to_string(&msg) {
                let mut sender = broadcast_sender.lock().await;
                if sender.send(Message::Text(text.into())).await.is_err() {
                    break;
                }
            }
        }
    });

    // 接收 relay 消息并处理
    let recv_mobile_state = mobile_state.clone();
    let recv_sender = Arc::clone(&sender_arc);
    let recv_task = tokio::spawn(async move {
        while let Some(msg) = ws_receiver.next().await {
            let msg = match msg {
                Ok(m) => m,
                Err(e) => {
                    warn!("[relay_client] ws receive error: {e}");
                    break;
                }
            };

            if let Message::Text(text) = msg {
                if let Ok(relay_msg) = serde_json::from_str::<RelayToPc>(&text) {
                    match relay_msg {
                        RelayToPc::HttpRequest { id, method, path, body } => {
                            let response = handle_local_request(
                                &recv_mobile_state,
                                &method,
                                &path,
                                body,
                            ).await;
                            let reply = PcToRelay::HttpResponse {
                                id,
                                status: response.0,
                                body: response.1,
                            };
                            if let Ok(text) = serde_json::to_string(&reply) {
                                let mut sender = recv_sender.lock().await;
                                if sender.send(Message::Text(text.into())).await.is_err() {
                                    break;
                                }
                            }
                        }
                        RelayToPc::WsMessage { .. } => {}
                    }
                }
            }
        }
    });

    tokio::select! {
        _ = broadcast_task => {}
        _ = recv_task => {}
    }
}

/// 在本地处理来自手机的 HTTP 请求（透传到 mobile_server 的逻辑）
async fn handle_local_request(
    state: &SharedMobileState,
    method: &str,
    path: &str,
    body: Option<serde_json::Value>,
) -> (u16, serde_json::Value) {
    let path = path.trim_start_matches('/');

    match (method, path) {
        ("GET", "api/health") => (200, serde_json::json!("ok")),
        ("GET", "api/active-thread") => {
            let id = state.current_thread_id.read().await.clone();
            (200, serde_json::json!({ "threadId": id }))
        }
        ("GET", "api/threads") => {
            let threads = state.thread_store.list_threads().await;
            let result: Vec<serde_json::Value> = threads
                .into_iter()
                .filter(|t| !t.turns.is_empty())
                .take(20)
                .map(|t| {
                    let message_count = t.all_messages().len();
                    let preview = t.preview();
                    serde_json::json!({
                        "id": t.id,
                        "name": t.name,
                        "preview": preview,
                        "updatedAt": t.updated_at,
                        "messageCount": message_count,
                    })
                })
                .collect();
            (200, serde_json::json!(result))
        }
        ("GET", p) if p.starts_with("api/threads/") && p.ends_with("/messages") => {
            let thread_id = p.strip_prefix("api/threads/")
                .and_then(|s| s.strip_suffix("/messages"))
                .unwrap_or("");
            let messages = state.thread_store.get_thread_messages(thread_id).await;
            let result: Vec<serde_json::Value> = messages
                .into_iter()
                .filter(|m| m.role == "user" || m.role == "assistant")
                .filter(|m| !m.content.is_empty())
                .map(|m| {
                    let content = if m.content.len() > 4000 {
                        let truncated: String = m.content.chars().take(4000).collect();
                        format!("{truncated}\n\n[内容过长，已截断]")
                    } else {
                        m.content
                    };
                    serde_json::json!({
                        "role": m.role,
                        "content": content,
                        "timestamp": m.timestamp,
                    })
                })
                .collect();
            let start = result.len().saturating_sub(100);
            (200, serde_json::json!(&result[start..]))
        }
        ("GET", p) if p.starts_with("api/threads/") => {
            let thread_id = p.strip_prefix("api/threads/").unwrap_or("");
            match state.thread_store.get_thread(thread_id).await {
                Some(thread) => (200, serde_json::json!({
                    "id": thread.id,
                    "name": thread.name,
                    "createdAt": thread.created_at,
                    "goal": thread.goal,
                })),
                None => (404, serde_json::json!({ "error": "not found" })),
            }
        }
        ("POST", p) if p.starts_with("api/threads/") && p.ends_with("/chat") => {
            let thread_id = p.strip_prefix("api/threads/")
                .and_then(|s| s.strip_suffix("/chat"))
                .unwrap_or("")
                .to_string();

            let message = body
                .and_then(|b| b.get("message").and_then(|m| m.as_str().map(String::from)))
                .unwrap_or_default();

            if message.is_empty() {
                return (400, serde_json::json!({ "error": "empty message" }));
            }

            *state.current_thread_id.write().await = Some(thread_id.clone());

            let engine = state.agent_engine.clone();
            let app_handle = state.app_handle.clone();
            let config_manager = state.config_manager.clone();

            tokio::spawn(async move {
                let config = match config_manager.read() {
                    Ok(c) => c,
                    Err(e) => {
                        error!("Failed to read config for relay chat: {e}");
                        return;
                    }
                };
                if let Err(e) = engine
                    .run_turn(
                        &app_handle,
                        &config,
                        &thread_id,
                        &message,
                        vec![],
                        None,
                        None,
                        None,
                        None,
                    )
                    .await
                {
                    error!("Relay run_turn error: {e}");
                }
            });

            (200, serde_json::json!({ "status": "ok" }))
        }
        _ => (404, serde_json::json!({ "error": "not found" })),
    }
}
