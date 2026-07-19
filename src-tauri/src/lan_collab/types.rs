use serde::{Deserialize, Serialize};

/// 本机节点身份（不含私钥）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeIdentity {
    pub node_id: String,
    pub display_name: String,
    pub device_pubkey: String,
    pub created_at: i64,
}

/// 附近节点（发现结果 / 已连接会话）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NearbyPeer {
    pub node_id: String,
    pub display_name: String,
    pub address: String,
    pub port: u16,
    pub last_seen_at: i64,
    pub trusted: bool,
    /// 当前是否存在活跃 TCP 会话
    #[serde(default)]
    pub connected: bool,
}

/// 协作组成员。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupMember {
    pub node_id: String,
    pub display_name: String,
    /// owner | admin | member
    pub role: String,
    pub joined_at: i64,
}

/// 协作组（Owner 权威快照的本地缓存视图）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CollabGroup {
    pub group_id: String,
    pub name: String,
    pub owner_node_id: String,
    pub created_at: i64,
    pub snapshot_version: u64,
    pub invite_code: Option<String>,
    pub members: Vec<GroupMember>,
    /// 本机是否为该组 Owner（控制面可写）
    pub is_owner: bool,
    /// Owner 是否在线；离线时控制面冻结
    pub owner_online: bool,
}

/// 组内聊天消息。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessage {
    pub message_id: String,
    pub group_id: String,
    pub from_node_id: String,
    pub from_display_name: String,
    pub text: String,
    pub created_at: i64,
}

/// 发现到的公开协作组摘要（不含成员细节，供附近列表展示）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveredGroupSummary {
    pub group_id: String,
    pub name: String,
    pub owner_node_id: String,
    pub owner_display_name: String,
    pub member_count: usize,
    /// 是否公开显示邀请码提示（仅提示有邀请码，不广播真实码）
    #[serde(default)]
    pub has_invite: bool,
    pub owner_address: String,
    pub owner_port: u16,
    pub last_seen_at: i64,
}

/// 自动发现状态摘要。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveryStatus {
    pub scanning: bool,
    pub last_scan_at: Option<i64>,
    pub discovered_peer_count: usize,
}

/// 局域网协作总状态（前端面板使用）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LanCollabStatus {
    pub enabled: bool,
    pub identity: NodeIdentity,
    pub bind_port: u16,
    pub local_address: Option<String>,
    pub peers: Vec<NearbyPeer>,
    pub groups: Vec<CollabGroup>,
    /// 自动扫描到的附近协作组（含尚未加入的）
    #[serde(default)]
    pub discovered_groups: Vec<DiscoveredGroupSummary>,
    /// 自动发现运行状态
    #[serde(default)]
    pub discovery: DiscoveryStatus,
    pub connected_peer_count: usize,
    /// 本机发布的模型共享
    #[serde(default)]
    pub local_shared_models: Vec<SharedModelOffer>,
    /// 从对端发现的模型共享
    #[serde(default)]
    pub remote_shared_models: Vec<SharedModelOffer>,
    /// 本机发布的知识共享
    #[serde(default)]
    pub local_shared_knowledge: Vec<SharedKnowledgeOffer>,
    /// 从对端发现的知识共享
    #[serde(default)]
    pub remote_shared_knowledge: Vec<SharedKnowledgeOffer>,
    /// 本机发布的 Skill 共享
    #[serde(default)]
    pub local_shared_skills: Vec<SharedSkillOffer>,
    /// 从对端发现的 Skill 共享
    #[serde(default)]
    pub remote_shared_skills: Vec<SharedSkillOffer>,
    /// 本机发布的 Workflow 共享
    #[serde(default)]
    pub local_shared_workflows: Vec<SharedWorkflowOffer>,
    /// 从对端发现的 Workflow 共享
    #[serde(default)]
    pub remote_shared_workflows: Vec<SharedWorkflowOffer>,
    pub architecture: String,
    pub note: String,
}

/// 模型共享条目（本机发布或远端发现）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SharedModelOffer {
    pub share_id: String,
    pub host_node_id: String,
    pub host_display_name: String,
    pub host_address: String,
    pub proxy_port: u16,
    pub model_id: String,
    pub display_name: String,
    pub provider_id: String,
    pub upstream_model: String,
    pub group_id: Option<String>,
    /// 访问代理用 token（仅同组/已连接节点应持有）
    pub access_token: String,
    pub online: bool,
}

/// 知识共享清单中的文档元数据（不含正文）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SharedKnowledgeDocMeta {
    pub doc_id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_group: Option<String>,
    pub added_at: i64,
    pub chunk_count: usize,
}

/// 知识共享条目（本机发布或远端发现）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SharedKnowledgeOffer {
    pub share_id: String,
    pub host_node_id: String,
    pub host_display_name: String,
    pub title: String,
    pub group_id: Option<String>,
    /// search_and_read
    pub permission: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_group: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
    pub doc_count: usize,
    /// 预览文档列表（截断）
    #[serde(default)]
    pub docs: Vec<SharedKnowledgeDocMeta>,
    pub online: bool,
}

/// Skill 共享条目（本机发布或远端发现）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SharedSkillOffer {
    pub share_id: String,
    pub host_node_id: String,
    pub host_display_name: String,
    pub skill_id: String,
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub tags: Vec<String>,
    pub group_id: Option<String>,
    pub online: bool,
    /// 内容哈希（sha256:...）；旧节点可能缺省
    #[serde(default)]
    pub content_hash: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<i64>,
}

/// Workflow 共享条目（本机发布或远端发现）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SharedWorkflowOffer {
    pub share_id: String,
    pub host_node_id: String,
    pub host_display_name: String,
    pub workflow_name: String,
    pub title: String,
    pub description: String,
    pub node_count: usize,
    /// 内容哈希（sha256:...）
    #[serde(default)]
    pub content_hash: String,
    pub group_id: Option<String>,
    pub online: bool,
}

/// 远端知识检索命中。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteKnowledgeHit {
    pub share_id: String,
    pub host_node_id: String,
    pub host_display_name: String,
    pub doc_id: String,
    pub title: String,
    pub score: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_group: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub is_chunk: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chunk_index: Option<usize>,
}

/// 远端知识文档正文。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteKnowledgeDoc {
    pub share_id: String,
    pub host_node_id: String,
    pub host_display_name: String,
    pub doc_id: String,
    pub title: String,
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_group: Option<String>,
}
