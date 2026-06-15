use std::sync::Arc;

use dashmap::DashMap;
use futures_util::stream::SplitSink;
use axum::extract::ws::{Message, WebSocket};
use tokio::sync::{broadcast, Mutex};

use crate::protocol::PhoneBroadcast;

/// 一个 Room 代表一台已注册的 PC 实例
pub struct Room {
    /// 向 PC WebSocket 发送消息的 sender
    pub pc_sender: Mutex<SplitSink<WebSocket, Message>>,
    /// PC 广播通道，手机 WS 订阅此通道接收实时事件
    pub broadcast_tx: broadcast::Sender<PhoneBroadcast>,
    /// 等待 HTTP 响应的 oneshot 通道集合（request_id → sender）
    pub pending_requests: DashMap<String, tokio::sync::oneshot::Sender<(u16, serde_json::Value)>>,
}

/// 全局 Room 管理器
pub struct RoomManager {
    pub rooms: DashMap<String, Arc<Room>>,
}

impl RoomManager {
    pub fn new() -> Self {
        Self {
            rooms: DashMap::new(),
        }
    }

    pub fn register(
        &self,
        room_id: String,
        pc_sender: SplitSink<WebSocket, Message>,
    ) -> Arc<Room> {
        let (broadcast_tx, _) = broadcast::channel::<PhoneBroadcast>(256);
        let room = Arc::new(Room {
            pc_sender: Mutex::new(pc_sender),
            broadcast_tx,
            pending_requests: DashMap::new(),
        });
        self.rooms.insert(room_id, Arc::clone(&room));
        room
    }

    pub fn unregister(&self, room_id: &str) {
        self.rooms.remove(room_id);
    }

    pub fn get(&self, room_id: &str) -> Option<Arc<Room>> {
        self.rooms.get(room_id).map(|r| Arc::clone(r.value()))
    }
}
