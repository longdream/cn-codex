use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::extract::ws::{Message, WebSocket};
use axum::extract::{Path, State, WebSocketUpgrade};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use tokio::sync::oneshot;
use tracing::info;

use crate::protocol::RelayToPc;
use crate::AppState;

pub async fn phone_ws_handler(
    ws: WebSocketUpgrade,
    Path(room_id): Path<String>,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    let room = state.room_manager.get(&room_id);
    if room.is_none() {
        return (StatusCode::NOT_FOUND, "Room not found").into_response();
    }
    ws.on_upgrade(move |socket| handle_phone_socket(socket, room_id, state))
        .into_response()
}

async fn handle_phone_socket(socket: WebSocket, room_id: String, state: Arc<AppState>) {
    let room = match state.room_manager.get(&room_id) {
        Some(r) => r,
        None => return,
    };

    let (mut sender, mut receiver) = socket.split();
    let mut broadcast_rx = room.broadcast_tx.subscribe();

    info!("Phone WS connected: room_id={room_id}");

    let send_task = tokio::spawn(async move {
        while let Ok(broadcast) = broadcast_rx.recv().await {
            let json = serde_json::to_string(&broadcast).unwrap_or_default();
            if sender.send(Message::Text(json.into())).await.is_err() {
                break;
            }
        }
    });

    let recv_task = tokio::spawn(async move {
        while let Some(msg) = receiver.next().await {
            match msg {
                Ok(Message::Close(_)) | Err(_) => break,
                _ => {}
            }
        }
    });

    tokio::select! {
        _ = send_task => {}
        _ = recv_task => {}
    }

    info!("Phone WS disconnected: room_id={room_id}");
}

pub async fn phone_api_proxy(
    Path((room_id, path)): Path<(String, String)>,
    State(state): State<Arc<AppState>>,
    method: axum::http::Method,
    body: Option<Json<Value>>,
) -> Response {
    let room = match state.room_manager.get(&room_id) {
        Some(r) => r,
        None => {
            return (StatusCode::NOT_FOUND, "Room not found or PC offline").into_response();
        }
    };

    let request_id = uuid::Uuid::new_v4().to_string();
    let (tx, rx) = oneshot::channel::<(u16, Value)>();

    room.pending_requests.insert(request_id.clone(), tx);

    let relay_msg = RelayToPc::HttpRequest {
        id: request_id.clone(),
        method: method.to_string(),
        path: format!("/api/{path}"),
        body: body.map(|b| b.0),
    };

    let msg_text = serde_json::to_string(&relay_msg).unwrap_or_default();
    {
        let mut pc_sender = room.pc_sender.lock().await;
        if pc_sender.send(Message::Text(msg_text.into())).await.is_err() {
            room.pending_requests.remove(&request_id);
            return (StatusCode::BAD_GATEWAY, "Failed to send to PC").into_response();
        }
    }

    match tokio::time::timeout(Duration::from_secs(30), rx).await {
        Ok(Ok((status, body))) => {
            let status_code = StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
            let json_body = serde_json::to_string(&body).unwrap_or_default();
            Response::builder()
                .status(status_code)
                .header("content-type", "application/json")
                .body(Body::from(json_body))
                .unwrap_or_else(|_| (StatusCode::INTERNAL_SERVER_ERROR, "").into_response())
        }
        Ok(Err(_)) => {
            (StatusCode::BAD_GATEWAY, "PC dropped the request").into_response()
        }
        Err(_) => {
            room.pending_requests.remove(&request_id);
            (StatusCode::GATEWAY_TIMEOUT, "PC response timeout").into_response()
        }
    }
}
