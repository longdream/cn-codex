use serde::{Deserialize, Serialize};

/// PC → Relay 的消息
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PcToRelay {
    /// PC 返回 HTTP 响应
    HttpResponse {
        id: String,
        status: u16,
        body: serde_json::Value,
    },
    /// PC 广播事件给所有手机 WebSocket 客户端
    Broadcast {
        event: String,
        payload: serde_json::Value,
    },
}

/// Relay → PC 的消息
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RelayToPc {
    /// 转发手机的 HTTP 请求给 PC
    HttpRequest {
        id: String,
        method: String,
        path: String,
        body: Option<serde_json::Value>,
    },
    /// 转发手机的 WebSocket 消息给 PC（预留）
    WsMessage { data: String },
}

/// Relay → 手机 WebSocket 的消息（直接透传 PC 的 broadcast）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhoneBroadcast {
    pub event: String,
    pub payload: serde_json::Value,
}
