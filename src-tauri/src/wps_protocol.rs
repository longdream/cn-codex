use serde::{Deserialize, Serialize};

/// A request sent from cn-codex to the WPS add-in.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WpsRequest {
    pub id: String,
    pub method: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
}

/// A successful response from the WPS add-in.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WpsResponse {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<WpsErrorPayload>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WpsErrorPayload {
    pub code: Option<i32>,
    pub message: String,
}

/// An unsolicited notification from the WPS add-in (no `id`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WpsNotification {
    pub method: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
}

/// A raw incoming message from WebSocket, either a response or a notification.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum WpsIncoming {
    Response(WpsResponse),
    Notification(WpsNotification),
}

/// Handshake message sent by the add-in right after connecting.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WpsHandshake {
    #[serde(rename = "type")]
    pub msg_type: String,
    pub addin_name: String,
    #[serde(default)]
    pub addin_version: Option<String>,
    #[serde(default)]
    pub wps_version: Option<String>,
    #[serde(default)]
    pub active_document: Option<WpsDocumentInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WpsDocumentInfo {
    pub name: String,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub saved: Option<bool>,
}

/// Connection-level status exposed to the frontend / tool executor.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WpsConnectionStatus {
    pub connected: bool,
    pub addin_name: Option<String>,
    pub addin_version: Option<String>,
    pub wps_version: Option<String>,
    pub active_document: Option<WpsDocumentInfo>,
}

/// Overall server status exposed to the frontend.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WpsServerStatus {
    pub running: bool,
    pub port: u16,
    pub connections: Vec<WpsConnectionStatus>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_serializes() {
        let req = WpsRequest {
            id: "req-001".into(),
            method: "document.open".into(),
            params: Some(serde_json::json!({"path": "C:\\doc.docx"})),
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("req-001"));
        assert!(json.contains("document.open"));
    }

    #[test]
    fn response_deserializes_success() {
        let json = r#"{"id":"req-001","result":{"success":true}}"#;
        let resp: WpsResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.id, "req-001");
        assert!(resp.error.is_none());
        assert!(resp.result.is_some());
    }

    #[test]
    fn response_deserializes_error() {
        let json = r#"{"id":"req-002","error":{"code":-1,"message":"not found"}}"#;
        let resp: WpsResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.id, "req-002");
        assert!(resp.result.is_none());
        let err = resp.error.unwrap();
        assert_eq!(err.message, "not found");
    }

    #[test]
    fn notification_deserializes() {
        let json = r#"{"method":"event.documentChanged","params":{"name":"test.docx"}}"#;
        let notif: WpsNotification = serde_json::from_str(json).unwrap();
        assert_eq!(notif.method, "event.documentChanged");
    }

    #[test]
    fn incoming_dispatches_response() {
        let json = r#"{"id":"r1","result":{"ok":true}}"#;
        let incoming: WpsIncoming = serde_json::from_str(json).unwrap();
        assert!(matches!(incoming, WpsIncoming::Response(_)));
    }

    #[test]
    fn incoming_dispatches_notification() {
        let json = r#"{"method":"event.saved","params":{}}"#;
        let incoming: WpsIncoming = serde_json::from_str(json).unwrap();
        assert!(matches!(incoming, WpsIncoming::Notification(_)));
    }

    #[test]
    fn handshake_deserializes() {
        let json = r#"{
            "type": "handshake",
            "addinName": "cn-codex-wps",
            "addinVersion": "0.1.0",
            "wpsVersion": "12.1.0",
            "activeDocument": {"name": "report.docx", "path": "C:\\report.docx", "saved": true}
        }"#;
        let hs: WpsHandshake = serde_json::from_str(json).unwrap();
        assert_eq!(hs.msg_type, "handshake");
        assert_eq!(hs.addin_name, "cn-codex-wps");
        assert!(hs.active_document.is_some());
    }
}
