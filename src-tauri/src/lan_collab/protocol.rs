use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};

use super::types::{
    ChatMessage, CollabGroup, DiscoveredGroupSummary, NodeIdentity, RemoteKnowledgeDoc,
    RemoteKnowledgeHit, SharedKnowledgeOffer, SharedModelOffer, SharedSkillOffer,
    SharedWorkflowOffer,
};

pub const MAX_FRAME_BYTES: usize = 1_048_576;
pub const PROTOCOL_VERSION: u16 = 1;

/// 节点间线协议消息（长度前缀 JSON）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WireMessage {
    Hello {
        #[serde(default = "default_protocol_version")]
        protocol_version: u16,
        node_id: String,
        display_name: String,
        device_pubkey: String,
        created_at: i64,
        /// 本机对外监听端口（入站对端的 ephemeral 端口不可用，需单独交换）
        listen_port: u16,
    },
    HelloOk {
        #[serde(default = "default_protocol_version")]
        protocol_version: u16,
        node_id: String,
        display_name: String,
        device_pubkey: String,
        created_at: i64,
        listen_port: u16,
    },
    GroupJoinRequest {
        request_id: String,
        invite_code: String,
        node_id: String,
        display_name: String,
    },
    GroupJoinAccept {
        request_id: String,
        group: CollabGroup,
    },
    GroupJoinReject {
        request_id: String,
        reason: String,
    },
    GroupSnapshot {
        group: CollabGroup,
    },
    /// 广播本机作为 Owner 的公开协作组目录（不含邀请码）
    GroupDirectoryAdvert {
        groups: Vec<DiscoveredGroupSummary>,
    },
    /// 请求对端发送协作组目录
    GroupDirectoryQuery {},
    ChatText {
        message: ChatMessage,
    },
    /// 发布本机当前可共享模型目录（整表替换视角，按 host_node_id 收敛）
    ModelShareAdvert {
        offers: Vec<SharedModelOffer>,
    },
    /// 请求对端重新发送其模型共享目录
    ModelShareQuery {},
    /// 发布本机知识共享目录
    KnowledgeShareAdvert {
        offers: Vec<SharedKnowledgeOffer>,
    },
    /// 请求对端重新发送知识共享目录
    KnowledgeShareQuery {},
    /// 发布本机 Skill 共享目录
    SkillShareAdvert {
        offers: Vec<SharedSkillOffer>,
    },
    /// 请求对端重新发送 Skill 共享目录
    SkillShareQuery {},
    /// 发布本机 Workflow 共享目录
    WorkflowShareAdvert {
        offers: Vec<SharedWorkflowOffer>,
    },
    /// 请求对端重新发送 Workflow 共享目录
    WorkflowShareQuery {},
    /// 对端按需拉取 Skill 正文
    SkillFetchRequest {
        request_id: String,
        share_id: String,
    },
    SkillFetchResponse {
        request_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        skill_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        content: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        content_hash: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    /// 对端按需拉取 Workflow 正文
    WorkflowFetchRequest {
        request_id: String,
        share_id: String,
    },
    WorkflowFetchResponse {
        request_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        workflow_name: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        content_hash: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        workflow_json: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        skill_md: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        scripts: Option<Vec<super::workflow_share::SharedWorkflowScript>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        scripts_manifest_json: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    /// 对端检索共享知识
    KnowledgeSearchRequest {
        request_id: String,
        share_id: String,
        query: String,
        top_k: usize,
    },
    KnowledgeSearchResponse {
        request_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        hits: Option<Vec<RemoteKnowledgeHit>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    /// 对端按需拉取文档正文
    KnowledgeFetchRequest {
        request_id: String,
        share_id: String,
        doc_id: String,
    },
    KnowledgeFetchResponse {
        request_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        doc: Option<RemoteKnowledgeDoc>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    Ping {
        ts: i64,
    },
    Pong {
        ts: i64,
    },
    Error {
        message: String,
    },
}

fn default_protocol_version() -> u16 {
    PROTOCOL_VERSION
}

impl WireMessage {
    pub fn hello_from(identity: &NodeIdentity, listen_port: u16) -> Self {
        Self::Hello {
            protocol_version: PROTOCOL_VERSION,
            node_id: identity.node_id.clone(),
            display_name: identity.display_name.clone(),
            device_pubkey: identity.device_pubkey.clone(),
            created_at: identity.created_at,
            listen_port,
        }
    }

    pub fn hello_ok_from(identity: &NodeIdentity, listen_port: u16) -> Self {
        Self::HelloOk {
            protocol_version: PROTOCOL_VERSION,
            node_id: identity.node_id.clone(),
            display_name: identity.display_name.clone(),
            device_pubkey: identity.device_pubkey.clone(),
            created_at: identity.created_at,
            listen_port,
        }
    }
}

pub fn encode_frame(message: &WireMessage) -> Result<Vec<u8>, String> {
    let body = serde_json::to_vec(message).map_err(|e| format!("序列化协议帧失败: {e}"))?;
    if body.len() > MAX_FRAME_BYTES {
        return Err(format!("协议帧过大: {} bytes", body.len()));
    }
    let mut out = Vec::with_capacity(4 + body.len());
    out.extend_from_slice(&(body.len() as u32).to_be_bytes());
    out.extend_from_slice(&body);
    Ok(out)
}

pub async fn write_frame(writer: &mut OwnedWriteHalf, message: &WireMessage) -> Result<(), String> {
    let frame = encode_frame(message)?;
    writer
        .write_all(&frame)
        .await
        .map_err(|e| format!("写入协议帧失败: {e}"))?;
    writer
        .flush()
        .await
        .map_err(|e| format!("刷新协议帧失败: {e}"))
}

pub async fn read_frame(reader: &mut OwnedReadHalf) -> Result<WireMessage, String> {
    let mut len_buf = [0u8; 4];
    reader
        .read_exact(&mut len_buf)
        .await
        .map_err(|e| format!("读取帧长度失败: {e}"))?;
    let len = u32::from_be_bytes(len_buf) as usize;
    if len == 0 || len > MAX_FRAME_BYTES {
        return Err(format!("非法帧长度: {len}"));
    }
    let mut body = vec![0u8; len];
    reader
        .read_exact(&mut body)
        .await
        .map_err(|e| format!("读取帧正文失败: {e}"))?;
    serde_json::from_slice(&body).map_err(|e| format!("解析协议帧失败: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lan_collab::types::{
        RemoteKnowledgeHit, SharedKnowledgeDocMeta, SharedKnowledgeOffer,
    };

    #[test]
    fn knowledge_share_messages_roundtrip() {
        let offer = SharedKnowledgeOffer {
            share_id: "kshare_1".into(),
            host_node_id: "node_a".into(),
            host_display_name: "Alice".into(),
            title: "KB".into(),
            group_id: None,
            permission: "search_and_read".into(),
            source_group: None,
            domain: Some("test".into()),
            doc_count: 1,
            docs: vec![SharedKnowledgeDocMeta {
                doc_id: "doc_a".into(),
                title: "A".into(),
                domain: Some("test".into()),
                source_group: None,
                added_at: 1,
                chunk_count: 1,
            }],
            online: true,
        };
        let msg = WireMessage::KnowledgeShareAdvert {
            offers: vec![offer.clone()],
        };
        let frame = encode_frame(&msg).unwrap();
        // 跳过 4 字节长度前缀直接解析 body 验证可序列化
        let body = &frame[4..];
        let parsed: WireMessage = serde_json::from_slice(body).unwrap();
        match parsed {
            WireMessage::KnowledgeShareAdvert { offers } => {
                assert_eq!(offers.len(), 1);
                assert_eq!(offers[0].share_id, "kshare_1");
                assert_eq!(offers[0].docs[0].doc_id, "doc_a");
            }
            other => panic!("unexpected: {other:?}"),
        }

        let hit = RemoteKnowledgeHit {
            share_id: "kshare_1".into(),
            host_node_id: "node_a".into(),
            host_display_name: "Alice".into(),
            doc_id: "doc_a".into(),
            title: "A".into(),
            score: 1.5,
            domain: None,
            source_group: None,
            tags: vec![],
            is_chunk: false,
            chunk_index: None,
        };
        let resp = WireMessage::KnowledgeSearchResponse {
            request_id: "req1".into(),
            hits: Some(vec![hit]),
            error: None,
        };
        let encoded = serde_json::to_vec(&resp).unwrap();
        let decoded: WireMessage = serde_json::from_slice(&encoded).unwrap();
        match decoded {
            WireMessage::KnowledgeSearchResponse { hits, error, .. } => {
                assert!(error.is_none());
                assert_eq!(hits.unwrap()[0].doc_id, "doc_a");
            }
            other => panic!("unexpected: {other:?}"),
        }
    }
}
