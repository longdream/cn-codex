use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use axum::Json;
use axum::Router;
use axum::extract::{Path as AxumPath, State as AxumState};
use axum::extract::WebSocketUpgrade;
use axum::extract::ws::{Message, WebSocket};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tauri::AppHandle;
use tokio::sync::broadcast;
use axum::http::HeaderValue;
use axum::middleware;
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;
use tracing::{error, info};

use tokio::sync::RwLock;

use crate::agent::AgentEngine;
use crate::config_system::ConfigManager;
use crate::thread_store::ThreadStore;

#[derive(Clone, Debug)]
pub struct BroadcastEvent {
    pub event: String,
    pub payload: serde_json::Value,
}

pub struct MobileServerState {
    pub broadcast_tx: broadcast::Sender<BroadcastEvent>,
    pub thread_store: Arc<ThreadStore>,
    pub current_thread_id: Arc<RwLock<Option<String>>>,
    pub app_handle: AppHandle,
    pub agent_engine: Arc<AgentEngine>,
    pub config_manager: ConfigManager,
}

pub type SharedMobileState = Arc<MobileServerState>;

pub async fn start(
    state: SharedMobileState,
    static_dir: PathBuf,
    port: u16,
) -> Result<u16, String> {
    let serve_dir = ServeDir::new(&static_dir).append_index_html_on_directories(true);

    let api = Router::new()
        .route("/health", get(health_handler))
        .route("/active-thread", get(active_thread_handler))
        .route("/threads", get(list_threads_handler))
        .route("/threads/{id}", get(get_thread_handler))
        .route("/threads/{id}/messages", get(get_thread_messages_handler))
        .route("/threads/{id}/chat", post(send_message_handler));

    let app = Router::new()
        .route("/ws", get(ws_handler))
        .nest("/api", api)
        .fallback_service(serve_dir)
        .layer(middleware::from_fn(no_cache_middleware))
        .layer(CorsLayer::permissive())
        .with_state(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let bind_result = tokio::time::timeout(
        std::time::Duration::from_secs(3),
        tokio::net::TcpListener::bind(addr),
    )
    .await;

    let listener = match bind_result {
        Ok(Ok(l)) => l,
        _ => {
            let fallback = SocketAddr::from(([0, 0, 0, 0], 0u16));
            tokio::net::TcpListener::bind(fallback)
                .await
                .map_err(|e| format!("Failed to bind any port: {e}"))?
        }
    };

    let actual_port = listener.local_addr().map(|a| a.port()).unwrap_or(port);
    info!("Mobile server listening on 0.0.0.0:{actual_port}");

    tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            error!("Mobile server error: {e}");
        }
    });

    Ok(actual_port)
}

// --- Middleware ---

async fn no_cache_middleware(
    req: axum::http::Request<axum::body::Body>,
    next: middleware::Next,
) -> axum::response::Response {
    let mut response = next.run(req).await;
    response.headers_mut().insert(
        axum::http::header::CACHE_CONTROL,
        HeaderValue::from_static("no-cache, no-store, must-revalidate"),
    );
    response
}

// --- Health ---

async fn health_handler() -> &'static str {
    "ok"
}

// --- Active Thread ---

async fn active_thread_handler(
    AxumState(state): AxumState<SharedMobileState>,
) -> Json<serde_json::Value> {
    let id = state.current_thread_id.read().await.clone();
    Json(serde_json::json!({ "threadId": id }))
}

// --- Threads API ---

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ThreadSummaryResponse {
    id: String,
    name: Option<String>,
    preview: String,
    updated_at: i64,
    message_count: usize,
}

async fn list_threads_handler(
    AxumState(state): AxumState<SharedMobileState>,
) -> Json<Vec<ThreadSummaryResponse>> {
    let threads = state.thread_store.list_threads().await;
    let result: Vec<ThreadSummaryResponse> = threads
        .into_iter()
        .filter(|t| !t.turns.is_empty())
        .take(20)
        .map(|t| {
            let message_count = t.all_messages().len();
            let preview = t.preview();
            ThreadSummaryResponse {
                id: t.id,
                name: t.name,
                preview,
                updated_at: t.updated_at,
                message_count,
            }
        })
        .collect();
    Json(result)
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct MessageResponse {
    role: String,
    content: String,
    timestamp: i64,
}

async fn get_thread_handler(
    AxumState(state): AxumState<SharedMobileState>,
    AxumPath(id): AxumPath<String>,
) -> Json<serde_json::Value> {
    match state.thread_store.get_thread(&id).await {
        Some(thread) => Json(serde_json::json!({
            "id": thread.id,
            "name": thread.name,
            "createdAt": thread.created_at,
            "goal": thread.goal,
        })),
        None => Json(serde_json::json!({ "error": "not found" })),
    }
}

async fn get_thread_messages_handler(
    AxumState(state): AxumState<SharedMobileState>,
    AxumPath(id): AxumPath<String>,
) -> Json<Vec<MessageResponse>> {
    let messages = state.thread_store.get_thread_messages(&id).await;
    let result: Vec<MessageResponse> = messages
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
            MessageResponse {
                role: m.role,
                content,
                timestamp: m.timestamp,
            }
        })
        .collect();
    let start = result.len().saturating_sub(100);
    Json(result[start..].to_vec())
}

// --- WebSocket ---

async fn ws_handler(
    ws: WebSocketUpgrade,
    AxumState(state): AxumState<SharedMobileState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_ws_connection(socket, state))
}

async fn handle_ws_connection(socket: WebSocket, state: SharedMobileState) {
    let (mut sender, mut receiver) = socket.split();
    let mut rx = state.broadcast_tx.subscribe();

    let send_task = tokio::spawn(async move {
        while let Ok(event) = rx.recv().await {
            let msg = serde_json::json!({
                "event": event.event,
                "payload": event.payload,
            });
            if let Ok(text) = serde_json::to_string(&msg) {
                if sender.send(Message::Text(text.into())).await.is_err() {
                    break;
                }
            }
        }
    });

    let recv_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = receiver.next().await {
            match msg {
                Message::Close(_) => break,
                _ => {}
            }
        }
    });

    tokio::select! {
        _ = send_task => {},
        _ = recv_task => {},
    }
}

// --- Send Message ---

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SendMessageRequest {
    message: String,
}

async fn send_message_handler(
    AxumState(state): AxumState<SharedMobileState>,
    AxumPath(thread_id): AxumPath<String>,
    Json(body): Json<SendMessageRequest>,
) -> Json<serde_json::Value> {
    *state.current_thread_id.write().await = Some(thread_id.clone());

    let engine = state.agent_engine.clone();
    let app_handle = state.app_handle.clone();
    let config_manager = state.config_manager.clone();

    tokio::spawn(async move {
        let config = match config_manager.read() {
            Ok(c) => c,
            Err(e) => {
                error!("Failed to read config for mobile chat: {e}");
                return;
            }
        };
        if let Err(e) = engine
            .run_turn(
                &app_handle,
                &config,
                &thread_id,
                &body.message,
                vec![],
                None,
                None,
                None,
            )
            .await
        {
            error!("Mobile run_turn error: {e}");
        }
    });

    Json(serde_json::json!({ "status": "ok" }))
}

// --- Utilities ---

pub fn get_local_ip() -> String {
    local_ip_address::local_ip()
        .map(|ip| ip.to_string())
        .unwrap_or_else(|_| "127.0.0.1".to_string())
}

pub fn broadcast(event: &str, payload: serde_json::Value) {
    if let Some(info) = crate::MOBILE_SERVER.get() {
        let _ = info.broadcast_tx.send(BroadcastEvent {
            event: event.to_string(),
            payload,
        });
    }
}

pub fn generate_qrcode_svg(url: &str) -> Result<String, String> {
    use qrcode::QrCode;
    use qrcode::render::svg;

    let code = QrCode::new(url.as_bytes()).map_err(|e| format!("QR encode error: {e}"))?;
    let svg_string = code
        .render::<svg::Color>()
        .min_dimensions(200, 200)
        .quiet_zone(true)
        .build();
    Ok(svg_string)
}
