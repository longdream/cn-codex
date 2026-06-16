use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket};
use axum::extract::{Path, State, WebSocketUpgrade};
use axum::response::IntoResponse;
use futures_util::StreamExt;
use tracing::{info, warn};

use crate::protocol::PcToRelay;
use crate::AppState;

pub async fn pc_ws_handler(
    ws: WebSocketUpgrade,
    Path(room_id): Path<String>,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    info!("PC connecting: room_id={room_id}");
    ws.on_upgrade(move |socket| handle_pc_socket(socket, room_id, state))
}

async fn handle_pc_socket(socket: WebSocket, room_id: String, state: Arc<AppState>) {
    let (sender, mut receiver) = socket.split();
    let room = state.room_manager.register(room_id.clone(), sender);
    info!("PC registered: room_id={room_id}");

    while let Some(msg) = receiver.next().await {
        let msg = match msg {
            Ok(m) => m,
            Err(e) => {
                warn!("PC WS error room_id={room_id}: {e}");
                break;
            }
        };

        match msg {
            Message::Text(text) => {
                if let Ok(pc_msg) = serde_json::from_str::<PcToRelay>(&text) {
                    match pc_msg {
                        PcToRelay::HttpResponse { id, status, body } => {
                            if let Some((_, sender)) = room.pending_requests.remove(&id) {
                                let _ = sender.send((status, body));
                            }
                        }
                        PcToRelay::Broadcast { event, payload } => {
                            let broadcast = crate::protocol::PhoneBroadcast { event, payload };
                            let _ = room.broadcast_tx.send(broadcast);
                        }
                    }
                }
            }
            Message::Close(_) => break,
            _ => {}
        }
    }

    state.room_manager.unregister(&room_id);
    info!("PC disconnected: room_id={room_id}");
}
