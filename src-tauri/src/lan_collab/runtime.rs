use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use tauri::{AppHandle, Emitter};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Mutex, RwLock, mpsc, oneshot};
use tokio::task::JoinHandle;
use uuid::Uuid;

use crate::config_system::ConfigManager;

use super::discovery::{self, DiscoveredEndpoint, DiscoveryHandle, PresenceBeacon};
use super::identity::{load_or_create_identity, new_identity_with_display_name, save_identity};
use super::knowledge_share::KnowledgeShareService;
use super::model_share::{ModelShareService, SharedModelConfig};
use super::protocol::{WireMessage, read_frame, write_frame};
use super::skill_share::{SharedSkillPayload, SkillShareService};
use super::store::LanCollabStore;
use super::types::{
    ChatMessage, CollabGroup, DiscoveredGroupSummary, DiscoveryStatus, GroupMember,
    LanCollabStatus, NearbyPeer, NodeIdentity, RemoteKnowledgeDoc, RemoteKnowledgeHit,
    SharedKnowledgeOffer, SharedModelOffer, SharedSkillOffer, SharedWorkflowOffer,
};
use super::workflow_share::{SharedWorkflowPayload, WorkflowShareService};

const DEFAULT_PORT_START: u16 = 47800;
const DEFAULT_PORT_END: u16 = 47820;
const JOIN_TIMEOUT_SECS: u64 = 8;
const CONNECT_TIMEOUT_SECS: u64 = 6;
const HANDSHAKE_TIMEOUT_SECS: u64 = 6;

const EVENT_MESSAGE: &str = "lan-collab-message";
const EVENT_PEER: &str = "lan-collab-peer";
const EVENT_GROUP: &str = "lan-collab-group";
const EVENT_DISCOVERY: &str = "lan-collab-discovery";
const EVENT_MODEL_SHARE: &str = "lan-collab-model-share";
const EVENT_KNOWLEDGE_SHARE: &str = "lan-collab-knowledge-share";
const EVENT_SKILL_SHARE: &str = "lan-collab-skill-share";
const EVENT_WORKFLOW_SHARE: &str = "lan-collab-workflow-share";

#[derive(Debug)]
struct PeerSession {
    #[allow(dead_code)]
    node_id: String,
    #[allow(dead_code)]
    display_name: String,
    #[allow(dead_code)]
    address: String,
    #[allow(dead_code)]
    port: u16,
    tx: mpsc::UnboundedSender<WireMessage>,
}

#[derive(Debug)]
struct RuntimeInner {
    enabled: bool,
    identity: NodeIdentity,
    bind_port: u16,
    peers: Vec<NearbyPeer>,
    groups: Vec<CollabGroup>,
    /// 自动扫描到的附近协作组（尚未加入也可显示）
    discovered_groups: Vec<DiscoveredGroupSummary>,
    discovery_scanning: bool,
    discovery_last_scan_at: Option<i64>,
    /// 正在自动连接的 host:port，避免重复拨号
    auto_connect_inflight: HashSet<String>,
    /// node_id -> outbound channel
    sessions: HashMap<String, PeerSession>,
    /// pending join request_id -> reply channel
    pending_joins: HashMap<String, oneshot::Sender<Result<CollabGroup, String>>>,
    /// pending knowledge search request_id
    pending_kb_search: HashMap<String, oneshot::Sender<Result<Vec<RemoteKnowledgeHit>, String>>>,
    /// pending knowledge fetch request_id
    pending_kb_fetch: HashMap<String, oneshot::Sender<Result<RemoteKnowledgeDoc, String>>>,
    /// pending skill fetch request_id
    pending_skill_fetch: HashMap<String, oneshot::Sender<Result<SharedSkillPayload, String>>>,
    /// pending workflow fetch request_id
    pending_workflow_fetch: HashMap<String, oneshot::Sender<Result<SharedWorkflowPayload, String>>>,
    listener_task: Option<JoinHandle<()>>,
    stop_tx: Option<mpsc::Sender<()>>,
    app_handle: Option<AppHandle>,
    /// 对端广播的模型共享
    remote_shared_models: Vec<SharedModelOffer>,
    /// 对端广播的知识共享
    remote_shared_knowledge: Vec<SharedKnowledgeOffer>,
    /// 对端广播的 Skill 共享
    remote_shared_skills: Vec<SharedSkillOffer>,
    /// 对端广播的 Workflow 共享
    remote_shared_workflows: Vec<SharedWorkflowOffer>,
}

/// 局域网协作运行时（进程内单例状态）。
#[derive(Clone)]
pub struct LanCollabRuntime {
    store: LanCollabStore,
    inner: Arc<RwLock<RuntimeInner>>,
    /// 防止并发 connect 建立重复连接
    connect_lock: Arc<Mutex<()>>,
    /// 发现任务句柄
    discovery_handle: Arc<Mutex<Option<DiscoveryHandle>>>,
    /// 信标快照（供同步回调读取）
    beacon_cache: Arc<std::sync::RwLock<PresenceBeacon>>,
    model_share: ModelShareService,
    knowledge_share: KnowledgeShareService,
    skill_share: SkillShareService,
    workflow_share: WorkflowShareService,
}

impl LanCollabRuntime {
    pub fn open(
        data_dir: PathBuf,
        config_manager: ConfigManager,
        workspace_config_dir: PathBuf,
    ) -> Result<Self, String> {
        let store = LanCollabStore::new(data_dir);
        store.ensure_dirs()?;
        let identity = load_or_create_identity(store.data_dir())?;
        let beacon_identity = identity.clone();
        let mut peers = store.load_peers().unwrap_or_default();
        for peer in &mut peers {
            peer.connected = false;
        }
        // 历史版本可能同时持久化了 127.0.0.1 与局域网 IP 两条记录。
        dedupe_peers(&mut peers);
        let _ = store.save_peers(&peers);
        let mut groups = store.load_groups().unwrap_or_default();
        for group in &mut groups {
            group.is_owner = group.owner_node_id == identity.node_id;
            // 重启后默认未知在线状态；Owner 自身视为在线（控制面本机可写）
            group.owner_online = group.is_owner;
        }
        Ok(Self {
            store,
            inner: Arc::new(RwLock::new(RuntimeInner {
                enabled: false,
                identity,
                bind_port: DEFAULT_PORT_START,
                peers,
                groups,
                discovered_groups: Vec::new(),
                discovery_scanning: false,
                discovery_last_scan_at: None,
                auto_connect_inflight: HashSet::new(),
                sessions: HashMap::new(),
                pending_joins: HashMap::new(),
                pending_kb_search: HashMap::new(),
                pending_kb_fetch: HashMap::new(),
                pending_skill_fetch: HashMap::new(),
                pending_workflow_fetch: HashMap::new(),
                listener_task: None,
                stop_tx: None,
                app_handle: None,
                remote_shared_models: Vec::new(),
                remote_shared_knowledge: Vec::new(),
                remote_shared_skills: Vec::new(),
                remote_shared_workflows: Vec::new(),
            })),
            connect_lock: Arc::new(Mutex::new(())),
            discovery_handle: Arc::new(Mutex::new(None)),
            beacon_cache: Arc::new(std::sync::RwLock::new(PresenceBeacon::new(
                beacon_identity.node_id,
                beacon_identity.display_name,
                DEFAULT_PORT_START,
                Vec::new(),
            ))),
            model_share: ModelShareService::new(config_manager),
            knowledge_share: KnowledgeShareService::new(workspace_config_dir.clone()),
            skill_share: SkillShareService::new(workspace_config_dir.clone()),
            workflow_share: WorkflowShareService::new(workspace_config_dir),
        })
    }

    pub async fn attach_app_handle(&self, app: AppHandle) {
        let mut guard = self.inner.write().await;
        guard.app_handle = Some(app);
    }

    pub async fn status(&self) -> LanCollabStatus {
        // 先在写锁里固化 peers 去重，避免历史 loopback/LAN 双条目持续刷 UI。
        {
            let mut guard = self.inner.write().await;
            let before = guard.peers.len();
            dedupe_peers(&mut guard.peers);
            if guard.peers.len() != before {
                let _ = self.store.save_peers(&guard.peers);
            }
        }

        let guard = self.inner.read().await;
        let local_address = if guard.enabled {
            Some(format!(
                "{}:{}",
                local_ip_hint().unwrap_or_else(|| "127.0.0.1".to_string()),
                guard.bind_port
            ))
        } else {
            None
        };
        let connected_peer_count = guard.sessions.len();
        // 返回前再去重一次，清理历史 peers.json 里残留的 loopback/LAN 双条目。
        let mut peers: Vec<NearbyPeer> = guard
            .peers
            .iter()
            .map(|peer| {
                let mut p = peer.clone();
                p.connected = guard.sessions.contains_key(&peer.node_id);
                // 已连接时优先展示会话地址（通常更稳定）。
                if let Some(session) = guard.sessions.get(&peer.node_id) {
                    p.address = prefer_display_address(&session.address, &p.address);
                    p.port = session.port;
                }
                p
            })
            .collect();
        dedupe_peers(&mut peers);
        let remote_shared_models = guard.remote_shared_models.clone();
        let remote_shared_knowledge = guard.remote_shared_knowledge.clone();
        let remote_shared_skills = guard.remote_shared_skills.clone();
        let remote_shared_workflows = guard.remote_shared_workflows.clone();
        let identity = guard.identity.clone();
        let enabled = guard.enabled;
        let bind_port = guard.bind_port;
        let groups = guard.groups.clone();
        let discovered_groups = guard.discovered_groups.clone();
        let discovery = DiscoveryStatus {
            scanning: guard.discovery_scanning,
            last_scan_at: guard.discovery_last_scan_at,
            discovered_peer_count: guard
                .peers
                .iter()
                .filter(|p| p.trusted || p.connected)
                .count()
                .max(guard.discovered_groups.len()),
        };
        drop(guard);

        let host_addr = local_address
            .clone()
            .unwrap_or_else(|| "127.0.0.1".to_string());
        let local_shared_models = if enabled {
            self.model_share
                .local_offers(&identity.node_id, &identity.display_name, &host_addr, None)
                .await
        } else {
            Vec::new()
        };
        let local_shared_knowledge = if enabled {
            self.knowledge_share
                .local_offers(&identity.node_id, &identity.display_name, None)
                .await
        } else {
            Vec::new()
        };
        let local_shared_skills = if enabled {
            self.skill_share
                .local_offers(&identity.node_id, &identity.display_name, None)
                .await
        } else {
            Vec::new()
        };
        let local_shared_workflows = if enabled {
            self.workflow_share
                .local_offers(&identity.node_id, &identity.display_name, None)
                .await
        } else {
            Vec::new()
        };

        LanCollabStatus {
            enabled,
            identity,
            bind_port,
            local_address,
            peers,
            groups,
            discovered_groups,
            discovery,
            connected_peer_count,
            local_shared_models,
            remote_shared_models,
            local_shared_knowledge,
            remote_shared_knowledge,
            local_shared_skills,
            remote_shared_skills,
            local_shared_workflows,
            remote_shared_workflows,
            architecture: "weak-center-owner-plus-p2p".to_string(),
            note: if enabled {
                format!(
                    "协作已开启（TCP 监听 :{bind_port}）。已自动扫描附近节点/协作组；输入邀请码即可加入。控制面由组 Owner 权威管理；聊天/模型代理/知识拉取走 P2P。已连接 {connected_peer_count} 个对端。",
                )
            } else {
                "协作已关闭。开启后将自动扫描局域网协作组，输入邀请码即可加入；也可创建自己的组。"
                    .to_string()
            },
        }
    }

    pub async fn set_enabled(&self, enabled: bool) -> Result<LanCollabStatus, String> {
        if enabled {
            self.start_listener().await?;
            let _ = self.model_share.set_enabled(true).await;
            if let Err(err) = self.start_discovery().await {
                tracing::warn!("[lan_collab] start discovery failed: {err}");
            }
        } else {
            let _ = self.model_share.set_enabled(false).await;
            self.stop_discovery().await;
            self.stop_listener().await;
            {
                let mut guard = self.inner.write().await;
                guard.remote_shared_models.clear();
                guard.remote_shared_knowledge.clear();
                guard.remote_shared_skills.clear();
                guard.remote_shared_workflows.clear();
                guard.discovered_groups.clear();
                guard.discovery_scanning = false;
                guard.discovery_last_scan_at = None;
                guard.auto_connect_inflight.clear();
            }
        }
        Ok(self.status().await)
    }

    pub async fn set_display_name(&self, display_name: String) -> Result<NodeIdentity, String> {
        let name = display_name.trim().to_string();
        if name.is_empty() {
            return Err("显示名不能为空".to_string());
        }
        if name.chars().count() > 64 {
            return Err("显示名过长（最多 64 字符）".to_string());
        }
        let mut guard = self.inner.write().await;
        guard.identity.display_name = name;
        save_identity(self.store.data_dir(), &guard.identity)?;
        let identity = guard.identity.clone();
        drop(guard);
        self.refresh_beacon_cache().await;
        Ok(identity)
    }

    /// 检测到同 node_id 的其他实例时，尝试让本机换新身份。
    /// 规则：本机没有 Owner 组时优先自愈；两边都有/都没有时由更高端口一侧自愈。
    async fn try_heal_duplicate_identity(&self, remote: &NodeIdentity, remote_port: u16) -> bool {
        let (local_id, local_port, owns_groups) = {
            let guard = self.inner.read().await;
            (
                guard.identity.node_id.clone(),
                guard.bind_port,
                guard.groups.iter().any(|g| g.is_owner),
            )
        };
        if remote.node_id != local_id {
            return false;
        }
        // 有 Owner 组的一侧尽量保留，避免组权威丢失。
        if owns_groups && remote_port != 0 && remote_port != local_port {
            return false;
        }
        // 两边都无 Owner 组时，让端口较大的一侧换 ID，避免双边同时自愈抖动。
        if !owns_groups && remote_port != 0 && remote_port < local_port {
            return false;
        }
        self.regenerate_local_identity("handshake-duplicate").await
    }

    async fn try_heal_duplicate_identity_local(&self, remote_port: u16) -> bool {
        let (local_port, owns_groups) = {
            let guard = self.inner.read().await;
            (guard.bind_port, guard.groups.iter().any(|g| g.is_owner))
        };
        if owns_groups {
            return false;
        }
        if remote_port != 0 && remote_port < local_port {
            return false;
        }
        self.regenerate_local_identity("discovery-duplicate").await
    }

    async fn regenerate_local_identity(&self, reason: &str) -> bool {
        let mut guard = self.inner.write().await;
        let old = guard.identity.node_id.clone();
        let display_name = guard.identity.display_name.clone();
        let next = new_identity_with_display_name(display_name);
        if let Err(err) = save_identity(self.store.data_dir(), &next) {
            tracing::warn!("[lan_collab] regenerate identity failed ({reason}): {err}");
            return false;
        }
        // 本机若曾以旧 ID 作为 Owner 保存组，迁移 owner 字段，避免控制面错乱。
        for group in &mut guard.groups {
            if group.owner_node_id == old {
                group.owner_node_id = next.node_id.clone();
                group.is_owner = true;
                for member in &mut group.members {
                    if member.node_id == old {
                        member.node_id = next.node_id.clone();
                    }
                }
            } else {
                group.is_owner = group.owner_node_id == next.node_id;
            }
        }
        let _ = self.store.save_groups(&guard.groups);
        guard.identity = next.clone();
        drop(guard);
        self.refresh_beacon_cache().await;
        tracing::warn!(
            "[lan_collab] regenerated local node_id {old} -> {} ({reason})",
            next.node_id
        );
        true
    }

    pub async fn list_peers(&self) -> Vec<NearbyPeer> {
        let status = self.status().await;
        status.peers
    }

    /// 手动连接局域网节点：`host` + `port`。
    pub async fn connect_peer(&self, host: String, port: u16) -> Result<NearbyPeer, String> {
        let host = host.trim().to_string();
        if host.is_empty() {
            return Err("主机地址不能为空".to_string());
        }
        if port == 0 {
            return Err("端口无效".to_string());
        }
        {
            let guard = self.inner.read().await;
            if !guard.enabled {
                return Err("请先开启局域网协作".to_string());
            }
        }
        let addr: SocketAddr = format!("{host}:{port}")
            .parse()
            .map_err(|e| format!("地址无效 {host}:{port}: {e}"))?;
        self.dial(addr).await
    }

    pub async fn create_group(&self, name: String) -> Result<CollabGroup, String> {
        let name = name.trim().to_string();
        if name.is_empty() {
            return Err("组名称不能为空".to_string());
        }
        let mut guard = self.inner.write().await;
        if !guard.enabled {
            return Err("请先开启局域网协作".to_string());
        }
        let now = chrono::Utc::now().timestamp();
        let group_id = format!("grp_{}", Uuid::new_v4().simple());
        let invite_code = make_invite_code(&name);
        let group = CollabGroup {
            group_id: group_id.clone(),
            name,
            owner_node_id: guard.identity.node_id.clone(),
            created_at: now,
            snapshot_version: 1,
            invite_code: Some(invite_code),
            members: vec![GroupMember {
                node_id: guard.identity.node_id.clone(),
                display_name: guard.identity.display_name.clone(),
                role: "owner".to_string(),
                joined_at: now,
            }],
            is_owner: true,
            owner_online: true,
        };
        guard.groups.push(group.clone());
        self.store.save_groups(&guard.groups)?;
        self.emit_event(&guard, EVENT_GROUP, &group);
        drop(guard);
        self.refresh_beacon_cache().await;
        // 广播目录给已连接对端
        let peer_ids: Vec<String> = {
            let guard = self.inner.read().await;
            guard.sessions.keys().cloned().collect()
        };
        for peer_id in peer_ids {
            self.send_group_directory_to(&peer_id).await;
        }
        Ok(group)
    }

    pub async fn join_group(&self, invite_code: String) -> Result<CollabGroup, String> {
        let code = invite_code.trim().to_uppercase();
        if code.is_empty() {
            return Err("邀请码不能为空".to_string());
        }

        // 1) 本机已有该组且自己是成员
        {
            let guard = self.inner.read().await;
            if !guard.enabled {
                return Err("请先开启局域网协作".to_string());
            }
            if let Some(group) = guard.groups.iter().find(|g| {
                g.invite_code
                    .as_deref()
                    .map(|c| c.eq_ignore_ascii_case(&code))
                    .unwrap_or(false)
                    && g.members
                        .iter()
                        .any(|m| m.node_id == guard.identity.node_id)
            }) {
                return Ok(group.clone());
            }
        }

        // 2) 本机是 Owner：直接返回本地组（邀请码本机回环）
        {
            let guard = self.inner.read().await;
            if let Some(group) = guard.groups.iter().find(|g| {
                g.is_owner
                    && g.invite_code
                        .as_deref()
                        .map(|c| c.eq_ignore_ascii_case(&code))
                        .unwrap_or(false)
            }) {
                return Ok(group.clone());
            }
        }

        // 3) 向已连接对端请求加入（Owner 权威）
        let (request_id, identity, peer_count) = {
            let guard = self.inner.read().await;
            (
                format!("join_{}", Uuid::new_v4().simple()),
                guard.identity.clone(),
                guard.sessions.len(),
            )
        };

        // 没有会话时：根据发现结果自动连 Owner，再广播邀请码
        if peer_count == 0 {
            let auto_targets = {
                let guard = self.inner.read().await;
                guard
                    .discovered_groups
                    .iter()
                    .filter(|g| g.has_invite)
                    .map(|g| (g.owner_address.clone(), g.owner_port))
                    .collect::<Vec<_>>()
            };
            if auto_targets.is_empty() {
                // 再扫一轮
                let _ = self.refresh_scan().await;
            }
            let auto_targets = {
                let guard = self.inner.read().await;
                let mut targets = guard
                    .discovered_groups
                    .iter()
                    .map(|g| (g.owner_address.clone(), g.owner_port))
                    .collect::<Vec<_>>();
                // 已记录 peers 也尝试
                for peer in &guard.peers {
                    if peer.port > 0 {
                        targets.push((peer.address.clone(), peer.port));
                    }
                }
                targets.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
                targets.dedup();
                targets
            };
            if auto_targets.is_empty() {
                return Err(
                    "未发现附近协作节点。请保持双方都开启局域网协作，稍后再试或用高级手动连接。"
                        .to_string(),
                );
            }
            for (host, port) in auto_targets {
                match self.connect_peer(host, port).await {
                    Ok(_) => break,
                    Err(err) => {
                        tracing::debug!("[lan_collab] auto-connect for join failed: {err}");
                    }
                }
            }
            let still_empty = self.inner.read().await.sessions.is_empty();
            if still_empty {
                return Err(
                    "自动连接附近节点失败。请确认双方在同一局域网，或使用高级手动连接。"
                        .to_string(),
                );
            }
        }

        let (tx, rx) = oneshot::channel();
        {
            let mut guard = self.inner.write().await;
            guard.pending_joins.insert(request_id.clone(), tx);
        }

        let req = WireMessage::GroupJoinRequest {
            request_id: request_id.clone(),
            invite_code: code.clone(),
            node_id: identity.node_id,
            display_name: identity.display_name,
        };
        self.broadcast_wire(req).await;
        tracing::info!(
            "[lan_collab] join request sent code={code} peers={peer_count} req={request_id}"
        );

        match tokio::time::timeout(Duration::from_secs(JOIN_TIMEOUT_SECS), rx).await {
            Ok(Ok(Ok(group))) => Ok(group),
            Ok(Ok(Err(reason))) => Err(reason),
            Ok(Err(_)) => Err("加入请求已取消".to_string()),
            Err(_) => {
                let mut guard = self.inner.write().await;
                guard.pending_joins.remove(&request_id);
                Err(format!(
                    "加入超时（{JOIN_TIMEOUT_SECS}s）。请确认已连接到 Owner，且邀请码正确。"
                ))
            }
        }
    }

    pub async fn list_groups(&self) -> Vec<CollabGroup> {
        self.inner.read().await.groups.clone()
    }

    pub async fn send_message(
        &self,
        group_id: String,
        text: String,
    ) -> Result<ChatMessage, String> {
        let text = text.trim().to_string();
        if text.is_empty() {
            return Err("消息不能为空".to_string());
        }
        let (message, member_ids) = {
            let guard = self.inner.read().await;
            if !guard.enabled {
                return Err("请先开启局域网协作".to_string());
            }
            let group = guard
                .groups
                .iter()
                .find(|g| g.group_id == group_id)
                .ok_or_else(|| "组不存在".to_string())?;
            if !group
                .members
                .iter()
                .any(|m| m.node_id == guard.identity.node_id)
            {
                return Err("你不是该组成员".to_string());
            }
            let member_ids: Vec<String> = group.members.iter().map(|m| m.node_id.clone()).collect();
            let message = ChatMessage {
                message_id: format!("msg_{}", Uuid::new_v4().simple()),
                group_id,
                from_node_id: guard.identity.node_id.clone(),
                from_display_name: guard.identity.display_name.clone(),
                text,
                created_at: chrono::Utc::now().timestamp(),
            };
            (message, member_ids)
        };
        self.store.append_message(&message)?;
        self.broadcast_to_nodes(
            &member_ids,
            WireMessage::ChatText {
                message: message.clone(),
            },
        )
        .await;
        {
            let guard = self.inner.read().await;
            self.emit_event(&guard, EVENT_MESSAGE, &message);
        }
        Ok(message)
    }

    pub async fn list_messages(&self, group_id: String) -> Result<Vec<ChatMessage>, String> {
        self.store.load_messages(&group_id)
    }

    /// 立即触发一次扫描（UDP 已在后台；此处补端口扫描 + 刷新状态）。
    pub async fn refresh_scan(&self) -> Result<LanCollabStatus, String> {
        {
            let guard = self.inner.read().await;
            if !guard.enabled {
                return Err("请先开启局域网协作".to_string());
            }
        }
        self.run_port_scan_once().await;
        Ok(self.status().await)
    }

    /// 共享本机模型（Key 留在本机；对端通过本机代理零 Key 调用）。
    pub async fn share_model(
        &self,
        model_id: String,
        display_name: String,
        provider_id: String,
        upstream_model: String,
        group_id: Option<String>,
        upstream_base_url: Option<String>,
        upstream_api_key: Option<String>,
    ) -> Result<SharedModelOffer, String> {
        self.ensure_can_share_to_group(group_id.as_deref()).await?;
        {
            let guard = self.inner.read().await;
            if !guard.enabled {
                return Err("请先开启局域网协作".to_string());
            }
        }
        // provider_id：前端供应商实例 id 或 type；上游解析时回落到 ConfigManager。
        // 同时写入 type 侧配置键，保证代理能读到 base_url/api_key。
        let cfg = self
            .model_share
            .share_model(
                model_id,
                display_name,
                provider_id,
                upstream_model,
                group_id,
                upstream_base_url,
                upstream_api_key,
            )
            .await?;
        self.broadcast_local_model_shares().await;
        self.local_offer_from_config(&cfg).await
    }

    pub async fn unshare_model(&self, share_id: String) -> Result<(), String> {
        self.model_share.unshare_model(share_id).await?;
        self.broadcast_local_model_shares().await;
        Ok(())
    }

    pub async fn list_remote_shared_models(&self) -> Vec<SharedModelOffer> {
        self.inner.read().await.remote_shared_models.clone()
    }

    pub async fn list_local_shared_models(&self) -> Vec<SharedModelOffer> {
        self.status().await.local_shared_models
    }

    /// 共享本机知识库（对端可在线检索并按需拉取正文）。
    pub async fn share_knowledge(
        &self,
        title: String,
        group_id: Option<String>,
        source_group: Option<String>,
        domain: Option<String>,
        doc_ids: Vec<String>,
    ) -> Result<SharedKnowledgeOffer, String> {
        self.ensure_can_share_to_group(group_id.as_deref()).await?;
        {
            let guard = self.inner.read().await;
            if !guard.enabled {
                return Err("请先开启局域网协作".to_string());
            }
        }
        let cfg = self
            .knowledge_share
            .share_knowledge(title, group_id, source_group, domain, doc_ids)
            .await?;
        self.broadcast_local_knowledge_shares().await;
        self.local_kb_offer_from_share_id(&cfg.share_id).await
    }

    pub async fn unshare_knowledge(&self, share_id: String) -> Result<(), String> {
        self.knowledge_share.unshare_knowledge(share_id).await?;
        self.broadcast_local_knowledge_shares().await;
        Ok(())
    }

    pub async fn list_local_shared_knowledge(&self) -> Vec<SharedKnowledgeOffer> {
        self.status().await.local_shared_knowledge
    }

    pub async fn list_remote_shared_knowledge(&self) -> Vec<SharedKnowledgeOffer> {
        self.inner.read().await.remote_shared_knowledge.clone()
    }

    /// 共享本机 Skill（对端可发现并主动安装）。
    pub async fn share_skill(
        &self,
        skill_id: String,
        group_id: Option<String>,
    ) -> Result<SharedSkillOffer, String> {
        self.ensure_can_share_to_group(group_id.as_deref()).await?;
        {
            let guard = self.inner.read().await;
            if !guard.enabled {
                return Err("请先开启局域网协作".to_string());
            }
        }
        let cfg = self.skill_share.share_skill(skill_id, group_id).await?;
        self.broadcast_local_skill_shares().await;
        self.local_skill_offer_from_share_id(&cfg.share_id).await
    }

    pub async fn unshare_skill(&self, share_id: String) -> Result<(), String> {
        self.skill_share.unshare_skill(share_id).await?;
        self.broadcast_local_skill_shares().await;
        Ok(())
    }

    pub async fn list_local_shared_skills(&self) -> Vec<SharedSkillOffer> {
        self.status().await.local_shared_skills
    }

    pub async fn list_remote_shared_skills(&self) -> Vec<SharedSkillOffer> {
        self.inner.read().await.remote_shared_skills.clone()
    }

    /// 拉取远端共享 Skill 并安装到本机。
    pub async fn install_remote_skill(
        &self,
        host_node_id: String,
        share_id: String,
        overwrite: Option<bool>,
        force_overwrite: Option<bool>,
    ) -> Result<String, String> {
        let overwrite = overwrite.unwrap_or(false);
        let force_overwrite = force_overwrite.unwrap_or(false);
        {
            let guard = self.inner.read().await;
            if !guard.enabled {
                return Err("请先开启局域网协作".to_string());
            }
            if !guard.sessions.contains_key(&host_node_id) {
                return Err("共享方当前未连接".to_string());
            }
        }

        let request_id = format!("sfetch_{}", Uuid::new_v4().simple());
        let (tx, rx) = oneshot::channel();
        {
            let mut guard = self.inner.write().await;
            guard.pending_skill_fetch.insert(request_id.clone(), tx);
        }
        self.send_to(
            &host_node_id,
            WireMessage::SkillFetchRequest {
                request_id: request_id.clone(),
                share_id: share_id.clone(),
            },
        )
        .await;

        let payload = match tokio::time::timeout(Duration::from_secs(JOIN_TIMEOUT_SECS), rx).await {
            Ok(Ok(Ok(payload))) => payload,
            Ok(Ok(Err(err))) => return Err(err),
            Ok(Err(_)) => return Err("Skill 拉取请求已取消".to_string()),
            Err(_) => {
                let mut guard = self.inner.write().await;
                guard.pending_skill_fetch.remove(&request_id);
                return Err("Skill 拉取超时".to_string());
            }
        };

        let host_display_name = {
            let guard = self.inner.read().await;
            guard
                .remote_shared_skills
                .iter()
                .find(|o| o.host_node_id == host_node_id && o.share_id == share_id)
                .map(|o| o.host_display_name.clone())
                .or_else(|| {
                    guard
                        .peers
                        .iter()
                        .find(|p| p.node_id == host_node_id)
                        .map(|p| p.display_name.clone())
                })
                .unwrap_or_else(|| host_node_id.clone())
        };

        // share_id 在 payload 可能为空（旧协议），用请求参数回填
        let mut payload = payload;
        if payload.share_id.trim().is_empty() {
            payload.share_id = share_id;
        }

        self.skill_share.install_skill_payload(
            &payload,
            &host_node_id,
            &host_display_name,
            None,
            overwrite,
            force_overwrite,
        )
    }

    /// 共享本机 Workflow。
    pub async fn share_workflow(
        &self,
        workflow_name: String,
        group_id: Option<String>,
    ) -> Result<SharedWorkflowOffer, String> {
        self.ensure_can_share_to_group(group_id.as_deref()).await?;
        {
            let guard = self.inner.read().await;
            if !guard.enabled {
                return Err("请先开启局域网协作".to_string());
            }
        }
        let cfg = self
            .workflow_share
            .share_workflow(workflow_name, group_id)
            .await?;
        self.broadcast_local_workflow_shares().await;
        self.local_workflow_offer_from_share_id(&cfg.share_id).await
    }

    pub async fn unshare_workflow(&self, share_id: String) -> Result<(), String> {
        self.workflow_share.unshare_workflow(share_id).await?;
        self.broadcast_local_workflow_shares().await;
        Ok(())
    }

    pub async fn list_local_shared_workflows(&self) -> Vec<SharedWorkflowOffer> {
        self.status().await.local_shared_workflows
    }

    pub async fn list_remote_shared_workflows(&self) -> Vec<SharedWorkflowOffer> {
        self.inner.read().await.remote_shared_workflows.clone()
    }

    /// 拉取远端共享 Workflow 并安装到本机。
    pub async fn install_remote_workflow(
        &self,
        host_node_id: String,
        share_id: String,
        overwrite: Option<bool>,
        force_overwrite: Option<bool>,
        install_as: Option<String>,
    ) -> Result<String, String> {
        let overwrite = overwrite.unwrap_or(false);
        let force_overwrite = force_overwrite.unwrap_or(false);
        {
            let guard = self.inner.read().await;
            if !guard.enabled {
                return Err("请先开启局域网协作".to_string());
            }
            if !guard.sessions.contains_key(&host_node_id) {
                return Err("共享方当前未连接".to_string());
            }
        }

        let request_id = format!("wfetch_{}", Uuid::new_v4().simple());
        let (tx, rx) = oneshot::channel();
        {
            let mut guard = self.inner.write().await;
            guard.pending_workflow_fetch.insert(request_id.clone(), tx);
        }
        self.send_to(
            &host_node_id,
            WireMessage::WorkflowFetchRequest {
                request_id: request_id.clone(),
                share_id: share_id.clone(),
            },
        )
        .await;

        let payload = match tokio::time::timeout(Duration::from_secs(JOIN_TIMEOUT_SECS), rx).await {
            Ok(Ok(Ok(payload))) => payload,
            Ok(Ok(Err(err))) => return Err(err),
            Ok(Err(_)) => return Err("Workflow 拉取请求已取消".to_string()),
            Err(_) => {
                let mut guard = self.inner.write().await;
                guard.pending_workflow_fetch.remove(&request_id);
                return Err("Workflow 拉取超时".to_string());
            }
        };

        let mut payload = payload;
        if payload.share_id.trim().is_empty() {
            payload.share_id = share_id.clone();
        }

        let (host_display_name, group_id) = {
            let guard = self.inner.read().await;
            let offer = guard
                .remote_shared_workflows
                .iter()
                .find(|o| o.host_node_id == host_node_id && o.share_id == share_id);
            let display = offer
                .map(|o| o.host_display_name.clone())
                .or_else(|| {
                    guard
                        .peers
                        .iter()
                        .find(|p| p.node_id == host_node_id)
                        .map(|p| p.display_name.clone())
                })
                .unwrap_or_else(|| host_node_id.clone());
            let gid = offer.and_then(|o| o.group_id.clone());
            (display, gid)
        };

        self.workflow_share.install_workflow_payload(
            &payload,
            &host_node_id,
            &host_display_name,
            group_id,
            overwrite,
            force_overwrite,
            install_as,
        )
    }

    pub async fn list_shareable_knowledge_docs(
        &self,
        source_group: Option<String>,
        domain: Option<String>,
    ) -> Result<Vec<super::types::SharedKnowledgeDocMeta>, String> {
        self.knowledge_share
            .list_shareable_docs(&source_group, &domain, &[])
    }

    /// 检索远端共享知识（P2P 请求共享方）。
    pub async fn search_remote_knowledge(
        &self,
        host_node_id: String,
        share_id: String,
        query: String,
        top_k: Option<usize>,
    ) -> Result<Vec<RemoteKnowledgeHit>, String> {
        let query = query.trim().to_string();
        if query.is_empty() {
            return Err("检索词不能为空".to_string());
        }
        let top_k = top_k.unwrap_or(8).clamp(1, 20);
        {
            let guard = self.inner.read().await;
            if !guard.enabled {
                return Err("请先开启局域网协作".to_string());
            }
            if !guard.sessions.contains_key(&host_node_id) {
                return Err("共享方当前未连接".to_string());
            }
        }

        let request_id = format!("ksearch_{}", Uuid::new_v4().simple());
        let (tx, rx) = oneshot::channel();
        {
            let mut guard = self.inner.write().await;
            guard.pending_kb_search.insert(request_id.clone(), tx);
        }
        self.send_to(
            &host_node_id,
            WireMessage::KnowledgeSearchRequest {
                request_id: request_id.clone(),
                share_id,
                query,
                top_k,
            },
        )
        .await;

        match tokio::time::timeout(Duration::from_secs(JOIN_TIMEOUT_SECS), rx).await {
            Ok(Ok(Ok(hits))) => Ok(hits),
            Ok(Ok(Err(err))) => Err(err),
            Ok(Err(_)) => Err("知识检索请求已取消".to_string()),
            Err(_) => {
                let mut guard = self.inner.write().await;
                guard.pending_kb_search.remove(&request_id);
                Err("知识检索超时".to_string())
            }
        }
    }

    /// 拉取远端共享知识正文。
    pub async fn fetch_remote_knowledge(
        &self,
        host_node_id: String,
        share_id: String,
        doc_id: String,
    ) -> Result<RemoteKnowledgeDoc, String> {
        let doc_id = doc_id.trim().to_string();
        if doc_id.is_empty() {
            return Err("doc_id 不能为空".to_string());
        }
        {
            let guard = self.inner.read().await;
            if !guard.enabled {
                return Err("请先开启局域网协作".to_string());
            }
            if !guard.sessions.contains_key(&host_node_id) {
                return Err("共享方当前未连接".to_string());
            }
        }

        let request_id = format!("kfetch_{}", Uuid::new_v4().simple());
        let (tx, rx) = oneshot::channel();
        {
            let mut guard = self.inner.write().await;
            guard.pending_kb_fetch.insert(request_id.clone(), tx);
        }
        self.send_to(
            &host_node_id,
            WireMessage::KnowledgeFetchRequest {
                request_id: request_id.clone(),
                share_id,
                doc_id,
            },
        )
        .await;

        match tokio::time::timeout(Duration::from_secs(JOIN_TIMEOUT_SECS), rx).await {
            Ok(Ok(Ok(doc))) => Ok(doc),
            Ok(Ok(Err(err))) => Err(err),
            Ok(Err(_)) => Err("知识拉取请求已取消".to_string()),
            Err(_) => {
                let mut guard = self.inner.write().await;
                guard.pending_kb_fetch.remove(&request_id);
                Err("知识拉取超时".to_string())
            }
        }
    }

    async fn start_listener(&self) -> Result<(), String> {
        {
            let guard = self.inner.read().await;
            if guard.enabled {
                return Ok(());
            }
        }

        let mut bound = None;
        for port in DEFAULT_PORT_START..=DEFAULT_PORT_END {
            match TcpListener::bind(("0.0.0.0", port)).await {
                Ok(listener) => {
                    bound = Some((listener, port));
                    break;
                }
                Err(err) => {
                    tracing::debug!("[lan_collab] bind :{port} failed: {err}");
                }
            }
        }
        let (listener, port) = bound.ok_or_else(|| {
            format!("无法绑定端口 {DEFAULT_PORT_START}-{DEFAULT_PORT_END}，请检查占用/防火墙")
        })?;

        let (stop_tx, mut stop_rx) = mpsc::channel::<()>(1);
        let runtime = self.clone();
        let task = tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = stop_rx.recv() => {
                        tracing::info!("[lan_collab] listener stop signal");
                        break;
                    }
                    accepted = listener.accept() => {
                        match accepted {
                            Ok((stream, addr)) => {
                                tracing::info!("[lan_collab] inbound connection from {addr}");
                                let rt = runtime.clone();
                                tokio::spawn(async move {
                                    if let Err(err) = rt.handle_stream(stream, addr, true).await {
                                        tracing::warn!("[lan_collab] inbound session ended: {err}");
                                    }
                                });
                            }
                            Err(err) => {
                                tracing::warn!("[lan_collab] accept error: {err}");
                            }
                        }
                    }
                }
            }
        });

        let mut guard = self.inner.write().await;
        guard.enabled = true;
        guard.bind_port = port;
        guard.listener_task = Some(task);
        guard.stop_tx = Some(stop_tx);
        drop(guard);
        self.refresh_beacon_cache().await;
        tracing::info!(
            "[lan_collab] enabled node_id={} port={}",
            self.inner.read().await.identity.node_id,
            port
        );
        Ok(())
    }

    async fn stop_listener(&self) {
        let (stop_tx, task, sessions) = {
            let mut guard = self.inner.write().await;
            guard.enabled = false;
            let stop_tx = guard.stop_tx.take();
            let task = guard.listener_task.take();
            let sessions = std::mem::take(&mut guard.sessions);
            for peer in &mut guard.peers {
                peer.connected = false;
            }
            for group in &mut guard.groups {
                group.owner_online = group.is_owner;
            }
            let _ = self.store.save_groups(&guard.groups);
            let _ = self.store.save_peers(&guard.peers);
            (stop_tx, task, sessions)
        };
        // 丢弃 sessions 会关闭写通道，对端读循环退出
        drop(sessions);
        if let Some(tx) = stop_tx {
            let _ = tx.send(()).await;
        }
        if let Some(task) = task {
            let _ = task.await;
        }
        tracing::info!("[lan_collab] disabled");
    }

    async fn dial(&self, addr: SocketAddr) -> Result<NearbyPeer, String> {
        let _guard = self.connect_lock.lock().await;
        // 已连接到同一地址/同一节点则直接返回，避免并发互连互相踢掉会话。
        {
            let guard = self.inner.read().await;
            let host = addr.ip().to_string();
            let port = addr.port();
            if let Some(session) = guard.sessions.values().find(|s| {
                (s.address == host && s.port == port)
                    || (is_loopback_ip(&s.address) && is_loopback_ip(&host) && s.port == port)
            }) {
                if let Some(peer) = guard.peers.iter().find(|p| p.node_id == session.node_id) {
                    let mut connected = peer.clone();
                    connected.connected = true;
                    return Ok(connected);
                }
            }
            if let Some(peer) = guard.peers.iter().find(|p| {
                p.address == host
                    && p.port == port
                    && p.trusted
                    && guard.sessions.contains_key(&p.node_id)
            }) {
                let mut connected = peer.clone();
                connected.connected = true;
                return Ok(connected);
            }
        }

        let stream = tokio::time::timeout(
            Duration::from_secs(CONNECT_TIMEOUT_SECS),
            TcpStream::connect(addr),
        )
        .await
        .map_err(|_| format!("连接 {addr} 超时（{CONNECT_TIMEOUT_SECS}s）"))?
        .map_err(|e| format!("连接 {addr} 失败: {e}"))?;
        let _ = stream.set_nodelay(true);
        self.handle_stream(stream, addr, false).await
    }

    /// 完成握手后注册会话并在后台读写；调用方立即拿到 peer（不再阻塞读循环）。
    async fn handle_stream(
        &self,
        stream: TcpStream,
        addr: SocketAddr,
        inbound: bool,
    ) -> Result<NearbyPeer, String> {
        let _ = stream.set_nodelay(true);
        let (mut reader, mut writer) = stream.into_split();
        let (identity, listen_port) = {
            let guard = self.inner.read().await;
            (guard.identity.clone(), guard.bind_port)
        };

        let remote = tokio::time::timeout(Duration::from_secs(HANDSHAKE_TIMEOUT_SECS), async {
            if inbound {
                let first = read_frame(&mut reader).await?;
                let (remote, remote_listen_port) = match first {
                    WireMessage::Hello {
                        node_id,
                        display_name,
                        device_pubkey,
                        created_at,
                        listen_port,
                        ..
                    } => (
                        NodeIdentity {
                            node_id,
                            display_name,
                            device_pubkey,
                            created_at,
                        },
                        listen_port,
                    ),
                    other => return Err(format!("期望 Hello，收到 {other:?}")),
                };
                write_frame(
                    &mut writer,
                    &WireMessage::hello_ok_from(&identity, listen_port),
                )
                .await?;
                Ok::<_, String>((remote, remote_listen_port))
            } else {
                write_frame(
                    &mut writer,
                    &WireMessage::hello_from(&identity, listen_port),
                )
                .await?;
                let first = read_frame(&mut reader).await?;
                match first {
                    WireMessage::HelloOk {
                        node_id,
                        display_name,
                        device_pubkey,
                        created_at,
                        listen_port: remote_listen_port,
                        ..
                    }
                    | WireMessage::Hello {
                        node_id,
                        display_name,
                        device_pubkey,
                        created_at,
                        listen_port: remote_listen_port,
                        ..
                    } => Ok((
                        NodeIdentity {
                            node_id,
                            display_name,
                            device_pubkey,
                            created_at,
                        },
                        remote_listen_port,
                    )),
                    other => Err(format!("期望 HelloOk，收到 {other:?}")),
                }
            }
        })
        .await
        .map_err(|_| "握手超时".to_string())??;

        let (remote_identity, remote_listen_port) = remote;

        if remote_identity.node_id == identity.node_id {
            // 运行时自愈：复制安装后两个实例可能共享 node_id。
            // 优先让“非 Owner / 后启动”一侧换新身份，避免继续互相过滤。
            if self
                .try_heal_duplicate_identity(&remote_identity, remote_listen_port.max(addr.port()))
                .await
            {
                return Err(
                    "检测到复制安装导致的 node_id 冲突，本机已自动换新身份。请再点一次扫描/连接。"
                        .to_string(),
                );
            }
            return Err(
                "对端 node_id 与本机相同（通常是复制安装目录导致）。请重启复制出的客户端，或删除其 codey/lan_collab/identity.json 后重开。".to_string(),
            );
        }

        let peer_port = if remote_listen_port > 0 {
            remote_listen_port
        } else {
            addr.port()
        };
        let peer = NearbyPeer {
            node_id: remote_identity.node_id.clone(),
            display_name: remote_identity.display_name.clone(),
            address: addr.ip().to_string(),
            port: peer_port,
            last_seen_at: chrono::Utc::now().timestamp(),
            trusted: true,
            connected: true,
        };

        let (tx, mut rx) = mpsc::unbounded_channel::<WireMessage>();

        {
            let mut guard = self.inner.write().await;
            // 同 node 重复连接：只保留“较小 node_id 主动拨号”的那条链路。
            // 否则同机双开时双方会同时互拨，各自丢弃入站后把唯一会话互相掐死。
            let prefer_this = should_prefer_connection(&identity.node_id, &peer.node_id, inbound);
            if let Some(existing) = guard.sessions.get(&peer.node_id) {
                if !prefer_this {
                    let mut kept = peer.clone();
                    kept.address = existing.address.clone();
                    kept.port = existing.port;
                    kept.connected = true;
                    kept.last_seen_at = chrono::Utc::now().timestamp();
                    upsert_peer(&mut guard.peers, kept.clone());
                    let _ = self.store.save_peers(&guard.peers);
                    self.emit_event(&guard, EVENT_PEER, &kept);
                    tracing::info!(
                        "[lan_collab] keep existing session for {} (drop duplicate {}:{})",
                        peer.node_id,
                        peer.address,
                        peer.port
                    );
                    return Ok(kept);
                }
                if let Some(old) = guard.sessions.remove(&peer.node_id) {
                    drop(old);
                }
                tracing::info!(
                    "[lan_collab] replace session for {} with preferred {} connection {}:{}",
                    peer.node_id,
                    if inbound { "inbound" } else { "outbound" },
                    peer.address,
                    peer.port
                );
            }
            upsert_peer(&mut guard.peers, peer.clone());
            let _ = self.store.save_peers(&guard.peers);
            for group in &mut guard.groups {
                if group.owner_node_id == peer.node_id {
                    group.owner_online = true;
                }
            }
            let _ = self.store.save_groups(&guard.groups);
            guard.sessions.insert(
                peer.node_id.clone(),
                PeerSession {
                    node_id: peer.node_id.clone(),
                    display_name: peer.display_name.clone(),
                    address: peer.address.clone(),
                    port: peer.port,
                    tx: tx.clone(),
                },
            );
            self.emit_event(&guard, EVENT_PEER, &peer);
        }

        // 写循环（后台）
        tokio::spawn(async move {
            while let Some(msg) = rx.recv().await {
                if write_frame(&mut writer, &msg).await.is_err() {
                    break;
                }
            }
        });

        // 读循环（后台）；结束后清理 session
        let runtime = self.clone();
        let remote_node_id = peer.node_id.clone();
        tokio::spawn(async move {
            let read_result = async {
                loop {
                    let msg = read_frame(&mut reader).await?;
                    runtime.handle_wire(&remote_node_id, msg).await?;
                }
                #[allow(unreachable_code)]
                Ok::<(), String>(())
            }
            .await;

            {
                let mut guard = runtime.inner.write().await;
                guard.sessions.remove(&remote_node_id);
                if let Some(peer) = guard.peers.iter_mut().find(|p| p.node_id == remote_node_id) {
                    peer.connected = false;
                    peer.last_seen_at = chrono::Utc::now().timestamp();
                }
                for group in &mut guard.groups {
                    if group.owner_node_id == remote_node_id {
                        group.owner_online = group.is_owner;
                    }
                }
                // 对端下线：移除其模型共享目录
                let before = guard.remote_shared_models.len();
                guard
                    .remote_shared_models
                    .retain(|offer| offer.host_node_id != remote_node_id);
                let models_changed = guard.remote_shared_models.len() != before;
                let before_kb = guard.remote_shared_knowledge.len();
                guard
                    .remote_shared_knowledge
                    .retain(|offer| offer.host_node_id != remote_node_id);
                let kb_changed = guard.remote_shared_knowledge.len() != before_kb;
                let before_skill = guard.remote_shared_skills.len();
                guard
                    .remote_shared_skills
                    .retain(|offer| offer.host_node_id != remote_node_id);
                let skill_changed = guard.remote_shared_skills.len() != before_skill;
                let before_workflow = guard.remote_shared_workflows.len();
                guard
                    .remote_shared_workflows
                    .retain(|offer| offer.host_node_id != remote_node_id);
                let workflow_changed = guard.remote_shared_workflows.len() != before_workflow;
                let _ = runtime.store.save_groups(&guard.groups);
                let _ = runtime.store.save_peers(&guard.peers);
                if let Some(peer) = guard.peers.iter().find(|p| p.node_id == remote_node_id) {
                    runtime.emit_event(&guard, EVENT_PEER, peer);
                }
                if models_changed {
                    let offers = guard.remote_shared_models.clone();
                    runtime.emit_event(&guard, EVENT_MODEL_SHARE, &offers);
                }
                if kb_changed {
                    let offers = guard.remote_shared_knowledge.clone();
                    runtime.emit_event(&guard, EVENT_KNOWLEDGE_SHARE, &offers);
                }
                if skill_changed {
                    let offers = guard.remote_shared_skills.clone();
                    runtime.emit_event(&guard, EVENT_SKILL_SHARE, &offers);
                }
                if workflow_changed {
                    let offers = guard.remote_shared_workflows.clone();
                    runtime.emit_event(&guard, EVENT_WORKFLOW_SHARE, &offers);
                }
            }

            if let Err(err) = read_result {
                tracing::warn!(
                    "[lan_collab] session {remote_node_id} closed (inbound={inbound}): {err}"
                );
            } else {
                tracing::info!("[lan_collab] session {remote_node_id} closed cleanly");
            }
        });

        // 若本机是某些组的 Owner，且对端已是成员，推送快照收敛
        self.push_owned_snapshots_to(&peer.node_id).await;

        // 握手完成后交换模型共享目录
        self.send_local_model_shares_to(&peer.node_id).await;
        self.send_to(&peer.node_id, WireMessage::ModelShareQuery {})
            .await;
        // 握手完成后交换知识共享目录
        self.send_local_knowledge_shares_to(&peer.node_id).await;
        self.send_to(&peer.node_id, WireMessage::KnowledgeShareQuery {})
            .await;
        // 握手完成后交换 Skill 共享目录
        self.send_local_skill_shares_to(&peer.node_id).await;
        self.send_to(&peer.node_id, WireMessage::SkillShareQuery {})
            .await;
        // 握手完成后交换 Workflow 共享目录
        self.send_local_workflow_shares_to(&peer.node_id).await;
        self.send_to(&peer.node_id, WireMessage::WorkflowShareQuery {})
            .await;
        // 握手完成后交换公开协作组目录
        self.send_group_directory_to(&peer.node_id).await;
        self.send_to(&peer.node_id, WireMessage::GroupDirectoryQuery {})
            .await;

        Ok(peer)
    }

    async fn push_owned_snapshots_to(&self, node_id: &str) {
        let snapshots: Vec<CollabGroup> = {
            let guard = self.inner.read().await;
            guard
                .groups
                .iter()
                .filter(|g| g.is_owner && g.members.iter().any(|m| m.node_id == node_id))
                .cloned()
                .collect()
        };
        for group in snapshots {
            self.send_to(node_id, WireMessage::GroupSnapshot { group })
                .await;
        }
    }

    async fn handle_wire(&self, from_node_id: &str, message: WireMessage) -> Result<(), String> {
        match message {
            WireMessage::Ping { ts } => {
                self.send_to(from_node_id, WireMessage::Pong { ts }).await;
            }
            WireMessage::Pong { .. } => {}
            WireMessage::Hello { .. } | WireMessage::HelloOk { .. } => {
                // 握手阶段外忽略
            }
            WireMessage::GroupJoinRequest {
                request_id,
                invite_code,
                node_id,
                display_name,
            } => {
                self.handle_join_request(
                    from_node_id,
                    request_id,
                    invite_code,
                    node_id,
                    display_name,
                )
                .await;
            }
            WireMessage::GroupJoinAccept { request_id, group } => {
                self.handle_join_accept(request_id, group).await?;
            }
            WireMessage::GroupJoinReject { request_id, reason } => {
                let mut guard = self.inner.write().await;
                if let Some(tx) = guard.pending_joins.remove(&request_id) {
                    let _ = tx.send(Err(reason));
                }
            }
            WireMessage::GroupSnapshot { group } => {
                self.upsert_group_snapshot(group).await?;
            }
            WireMessage::GroupDirectoryAdvert { groups } => {
                self.handle_group_directory_advert(from_node_id, groups)
                    .await;
            }
            WireMessage::GroupDirectoryQuery {} => {
                self.send_group_directory_to(from_node_id).await;
            }
            WireMessage::ChatText { message } => {
                self.handle_remote_chat(message).await?;
            }
            WireMessage::ModelShareAdvert { offers } => {
                self.handle_model_share_advert(from_node_id, offers).await;
            }
            WireMessage::ModelShareQuery {} => {
                self.send_local_model_shares_to(from_node_id).await;
            }
            WireMessage::KnowledgeShareAdvert { offers } => {
                self.handle_knowledge_share_advert(from_node_id, offers)
                    .await;
            }
            WireMessage::KnowledgeShareQuery {} => {
                self.send_local_knowledge_shares_to(from_node_id).await;
            }
            WireMessage::SkillShareAdvert { offers } => {
                self.handle_skill_share_advert(from_node_id, offers).await;
            }
            WireMessage::SkillShareQuery {} => {
                self.send_local_skill_shares_to(from_node_id).await;
            }
            WireMessage::WorkflowShareAdvert { offers } => {
                self.handle_workflow_share_advert(from_node_id, offers)
                    .await;
            }
            WireMessage::WorkflowShareQuery {} => {
                self.send_local_workflow_shares_to(from_node_id).await;
            }
            WireMessage::SkillFetchRequest {
                request_id,
                share_id,
            } => {
                self.handle_skill_fetch_request(from_node_id, request_id, share_id)
                    .await;
            }
            WireMessage::SkillFetchResponse {
                request_id,
                skill_id,
                name,
                content,
                content_hash,
                error,
            } => {
                let mut guard = self.inner.write().await;
                if let Some(tx) = guard.pending_skill_fetch.remove(&request_id) {
                    let _ = tx.send(match (skill_id, name, content, content_hash, error) {
                        (Some(skill_id), Some(name), Some(content), content_hash, _) => {
                            Ok(SharedSkillPayload {
                                share_id: String::new(),
                                skill_id,
                                name,
                                description: String::new(),
                                tags: Vec::new(),
                                content,
                                content_hash: content_hash.unwrap_or_default(),
                            })
                        }
                        (_, _, _, _, Some(err)) => Err(err),
                        _ => Err("空的 Skill 拉取响应".to_string()),
                    });
                }
            }
            WireMessage::WorkflowFetchRequest {
                request_id,
                share_id,
            } => {
                self.handle_workflow_fetch_request(from_node_id, request_id, share_id)
                    .await;
            }
            WireMessage::WorkflowFetchResponse {
                request_id,
                workflow_name,
                title,
                content_hash,
                workflow_json,
                skill_md,
                scripts,
                scripts_manifest_json,
                error,
            } => {
                let mut guard = self.inner.write().await;
                if let Some(tx) = guard.pending_workflow_fetch.remove(&request_id) {
                    let _ = tx.send(match (workflow_name, workflow_json, error) {
                        (Some(workflow_name), Some(workflow_json), _) => {
                            Ok(SharedWorkflowPayload {
                                share_id: String::new(),
                                workflow_name,
                                title: title.unwrap_or_default(),
                                description: String::new(),
                                content_hash: content_hash.unwrap_or_default(),
                                workflow_json,
                                skill_md: skill_md.unwrap_or_default(),
                                scripts: scripts.unwrap_or_default(),
                                scripts_manifest_json,
                            })
                        }
                        (_, _, Some(err)) => Err(err),
                        _ => Err("空的 Workflow 拉取响应".to_string()),
                    });
                }
            }
            WireMessage::KnowledgeSearchRequest {
                request_id,
                share_id,
                query,
                top_k,
            } => {
                self.handle_knowledge_search_request(
                    from_node_id,
                    request_id,
                    share_id,
                    query,
                    top_k,
                )
                .await;
            }
            WireMessage::KnowledgeSearchResponse {
                request_id,
                hits,
                error,
            } => {
                let mut guard = self.inner.write().await;
                if let Some(tx) = guard.pending_kb_search.remove(&request_id) {
                    let _ = tx.send(match (hits, error) {
                        (Some(hits), _) => Ok(hits),
                        (_, Some(err)) => Err(err),
                        _ => Err("空的知识检索响应".to_string()),
                    });
                }
            }
            WireMessage::KnowledgeFetchRequest {
                request_id,
                share_id,
                doc_id,
            } => {
                self.handle_knowledge_fetch_request(from_node_id, request_id, share_id, doc_id)
                    .await;
            }
            WireMessage::KnowledgeFetchResponse {
                request_id,
                doc,
                error,
            } => {
                let mut guard = self.inner.write().await;
                if let Some(tx) = guard.pending_kb_fetch.remove(&request_id) {
                    let _ = tx.send(match (doc, error) {
                        (Some(doc), _) => Ok(doc),
                        (_, Some(err)) => Err(err),
                        _ => Err("空的知识拉取响应".to_string()),
                    });
                }
            }
            WireMessage::Error { message } => {
                tracing::warn!("[lan_collab] remote error from {from_node_id}: {message}");
            }
        }
        // 刷新 last_seen
        {
            let mut guard = self.inner.write().await;
            let connected = guard.sessions.contains_key(from_node_id);
            if let Some(peer) = guard.peers.iter_mut().find(|p| p.node_id == from_node_id) {
                peer.last_seen_at = chrono::Utc::now().timestamp();
                peer.connected = connected;
            }
        }
        Ok(())
    }

    async fn handle_join_request(
        &self,
        from_node_id: &str,
        request_id: String,
        invite_code: String,
        node_id: String,
        display_name: String,
    ) {
        let code = invite_code.trim().to_uppercase();
        let reply = {
            let mut guard = self.inner.write().await;
            if let Some(group) = guard.groups.iter_mut().find(|g| {
                g.is_owner
                    && g.invite_code
                        .as_deref()
                        .map(|c| c.eq_ignore_ascii_case(&code))
                        .unwrap_or(false)
            }) {
                if !group.members.iter().any(|m| m.node_id == node_id) {
                    group.members.push(GroupMember {
                        node_id: node_id.clone(),
                        display_name: display_name.clone(),
                        role: "member".to_string(),
                        joined_at: chrono::Utc::now().timestamp(),
                    });
                    group.snapshot_version = group.snapshot_version.saturating_add(1);
                }
                let snapshot = group.clone();
                let _ = self.store.save_groups(&guard.groups);
                self.emit_event(&guard, EVENT_GROUP, &snapshot);
                Ok(snapshot)
            } else {
                Err("邀请码无效或本机不是该组 Owner".to_string())
            }
        };

        match reply {
            Ok(group) => {
                // 回给请求方
                self.send_to(
                    from_node_id,
                    WireMessage::GroupJoinAccept {
                        request_id,
                        group: group.clone(),
                    },
                )
                .await;
                // 广播快照给其他已连接成员（弱中心收敛）
                let member_ids: Vec<String> =
                    group.members.iter().map(|m| m.node_id.clone()).collect();
                self.broadcast_to_nodes(
                    &member_ids,
                    WireMessage::GroupSnapshot {
                        group: group.clone(),
                    },
                )
                .await;
                tracing::info!(
                    "[lan_collab] join accepted node={node_id} group={}",
                    group.group_id
                );
            }
            Err(reason) => {
                self.send_to(
                    from_node_id,
                    WireMessage::GroupJoinReject { request_id, reason },
                )
                .await;
            }
        }
    }

    async fn handle_join_accept(
        &self,
        request_id: String,
        group: CollabGroup,
    ) -> Result<(), String> {
        let localized = self.upsert_group_snapshot(group).await?;
        let mut guard = self.inner.write().await;
        if let Some(tx) = guard.pending_joins.remove(&request_id) {
            let _ = tx.send(Ok(localized));
        }
        Ok(())
    }

    async fn upsert_group_snapshot(&self, mut group: CollabGroup) -> Result<CollabGroup, String> {
        let mut guard = self.inner.write().await;
        let self_id = guard.identity.node_id.clone();
        group.is_owner = group.owner_node_id == self_id;
        group.owner_online = group.is_owner || guard.sessions.contains_key(&group.owner_node_id);

        if let Some(existing) = guard
            .groups
            .iter_mut()
            .find(|g| g.group_id == group.group_id)
        {
            if group.snapshot_version >= existing.snapshot_version {
                *existing = group.clone();
            } else {
                return Ok(existing.clone());
            }
        } else {
            guard.groups.push(group.clone());
        }
        self.store.save_groups(&guard.groups)?;
        self.emit_event(&guard, EVENT_GROUP, &group);
        Ok(group)
    }

    async fn handle_remote_chat(&self, message: ChatMessage) -> Result<(), String> {
        let (allowed, should_relay, member_ids) = {
            let guard = self.inner.read().await;
            if let Some(group) = guard.groups.iter().find(|g| g.group_id == message.group_id) {
                let is_member = group
                    .members
                    .iter()
                    .any(|m| m.node_id == guard.identity.node_id);
                let should_relay = group.is_owner
                    && group
                        .members
                        .iter()
                        .any(|m| m.node_id == message.from_node_id);
                let member_ids: Vec<String> =
                    group.members.iter().map(|m| m.node_id.clone()).collect();
                (is_member, should_relay, member_ids)
            } else {
                (false, false, Vec::new())
            }
        };
        if !allowed {
            tracing::debug!(
                "[lan_collab] drop chat for unknown/non-member group {}",
                message.group_id
            );
            return Ok(());
        }
        self.store.append_message(&message)?;
        {
            let guard = self.inner.read().await;
            self.emit_event(&guard, EVENT_MESSAGE, &message);
        }
        // Owner 作为弱中心：若成员只连 Owner，由 Owner 转发组聊
        if should_relay {
            let except = message.from_node_id.clone();
            let targets: Vec<String> = member_ids.into_iter().filter(|id| id != &except).collect();
            self.broadcast_to_nodes(
                &targets,
                WireMessage::ChatText {
                    message: message.clone(),
                },
            )
            .await;
        }
        Ok(())
    }

    async fn send_to(&self, node_id: &str, message: WireMessage) {
        let tx = {
            let guard = self.inner.read().await;
            guard.sessions.get(node_id).map(|s| s.tx.clone())
        };
        if let Some(tx) = tx {
            let _ = tx.send(message);
        }
    }

    async fn broadcast_wire(&self, message: WireMessage) {
        let targets: Vec<mpsc::UnboundedSender<WireMessage>> = {
            let guard = self.inner.read().await;
            guard.sessions.values().map(|s| s.tx.clone()).collect()
        };
        for tx in targets {
            let _ = tx.send(message.clone());
        }
    }

    async fn broadcast_to_nodes(&self, node_ids: &[String], message: WireMessage) {
        let self_id = {
            let guard = self.inner.read().await;
            guard.identity.node_id.clone()
        };
        let targets: Vec<mpsc::UnboundedSender<WireMessage>> = {
            let guard = self.inner.read().await;
            node_ids
                .iter()
                .filter(|id| id.as_str() != self_id)
                .filter_map(|id| guard.sessions.get(id).map(|s| s.tx.clone()))
                .collect()
        };
        for tx in targets {
            let _ = tx.send(message.clone());
        }
    }

    async fn ensure_can_share_to_group(&self, group_id: Option<&str>) -> Result<(), String> {
        let Some(group_id) = group_id.map(str::trim).filter(|s| !s.is_empty()) else {
            return Err("请选择要共享到的协作组（未选组的内容保持私有）".to_string());
        };
        let guard = self.inner.read().await;
        if !guard.enabled {
            return Err("请先开启局域网协作".to_string());
        }
        let self_id = guard.identity.node_id.clone();
        let ok = guard.groups.iter().any(|g| {
            g.group_id == group_id && (g.is_owner || g.members.iter().any(|m| m.node_id == self_id))
        });
        if !ok {
            return Err("只能共享到本机已加入的协作组".to_string());
        }
        Ok(())
    }

    fn group_ids_for_peer_locked(guard: &RuntimeInner, node_id: &str) -> HashSet<String> {
        guard
            .groups
            .iter()
            .filter(|g| {
                g.owner_node_id == node_id || g.members.iter().any(|m| m.node_id == node_id)
            })
            .map(|g| g.group_id.clone())
            .collect()
    }

    async fn group_ids_for_peer(&self, node_id: &str) -> HashSet<String> {
        let guard = self.inner.read().await;
        Self::group_ids_for_peer_locked(&guard, node_id)
    }

    async fn ensure_peer_can_access_share(
        &self,
        peer_node_id: &str,
        group_id: Result<Option<String>, String>,
    ) -> Result<(), String> {
        let group_id = group_id?;
        let Some(group_id) = group_id.filter(|s| !s.trim().is_empty()) else {
            return Err("该共享未绑定协作组，已保持私有".to_string());
        };
        let allowed = self.group_ids_for_peer(peer_node_id).await;
        if allowed.contains(&group_id) {
            Ok(())
        } else {
            Err("无权访问该共享（不在授权协作组内）".to_string())
        }
    }

    async fn local_offer_from_config(
        &self,
        cfg: &SharedModelConfig,
    ) -> Result<SharedModelOffer, String> {
        let (identity, host_addr) = {
            let guard = self.inner.read().await;
            let host_addr = if guard.enabled {
                format!(
                    "{}:{}",
                    local_ip_hint().unwrap_or_else(|| "127.0.0.1".to_string()),
                    guard.bind_port
                )
            } else {
                "127.0.0.1".to_string()
            };
            (guard.identity.clone(), host_addr)
        };
        let offers = self
            .model_share
            .local_offers(&identity.node_id, &identity.display_name, &host_addr, None)
            .await;
        offers
            .into_iter()
            .find(|o| o.share_id == cfg.share_id)
            .ok_or_else(|| "共享项已创建但代理尚未就绪".to_string())
    }

    async fn current_local_offers(
        &self,
        allowed_group_ids: Option<&HashSet<String>>,
    ) -> Vec<SharedModelOffer> {
        let (enabled, identity, host_addr) = {
            let guard = self.inner.read().await;
            let host_addr = if guard.enabled {
                format!(
                    "{}:{}",
                    local_ip_hint().unwrap_or_else(|| "127.0.0.1".to_string()),
                    guard.bind_port
                )
            } else {
                "127.0.0.1".to_string()
            };
            (guard.enabled, guard.identity.clone(), host_addr)
        };
        if !enabled {
            return Vec::new();
        }
        self.model_share
            .local_offers(
                &identity.node_id,
                &identity.display_name,
                &host_addr,
                allowed_group_ids,
            )
            .await
    }

    async fn send_local_model_shares_to(&self, node_id: &str) {
        let allowed = self.group_ids_for_peer(node_id).await;
        let offers = self.current_local_offers(Some(&allowed)).await;
        self.send_to(node_id, WireMessage::ModelShareAdvert { offers })
            .await;
    }

    async fn broadcast_local_model_shares(&self) {
        let full = self.current_local_offers(None).await;
        {
            let guard = self.inner.read().await;
            self.emit_event(&guard, EVENT_MODEL_SHARE, &full);
        }
        let peer_ids: Vec<String> = {
            let guard = self.inner.read().await;
            guard.sessions.keys().cloned().collect()
        };
        for peer_id in peer_ids {
            let allowed = self.group_ids_for_peer(&peer_id).await;
            let offers = full
                .iter()
                .filter(|o| match &o.group_id {
                    Some(gid) => allowed.contains(gid),
                    None => false,
                })
                .cloned()
                .collect::<Vec<_>>();
            self.send_to(&peer_id, WireMessage::ModelShareAdvert { offers })
                .await;
        }
    }

    async fn handle_model_share_advert(
        &self,
        from_node_id: &str,
        mut offers: Vec<SharedModelOffer>,
    ) {
        // 只接受声明来自发送方的条目，防止伪造
        offers.retain(|o| o.host_node_id == from_node_id);
        // 用对端已知会话地址校正 host_address（避免 127.0.0.1 广播）
        let peer_addr = {
            let guard = self.inner.read().await;
            guard
                .peers
                .iter()
                .find(|p| p.node_id == from_node_id)
                .map(|p| p.address.clone())
        };
        if let Some(addr) = peer_addr {
            for offer in &mut offers {
                offer.host_address = addr.clone();
                offer.online = true;
            }
        } else {
            for offer in &mut offers {
                offer.online = true;
            }
        }

        let mut guard = self.inner.write().await;
        guard
            .remote_shared_models
            .retain(|o| o.host_node_id != from_node_id);
        guard.remote_shared_models.extend(offers);
        let snapshot = guard.remote_shared_models.clone();
        self.emit_event(&guard, EVENT_MODEL_SHARE, &snapshot);
        tracing::info!(
            "[lan_collab] model share advert from {from_node_id}: {} offers",
            snapshot
                .iter()
                .filter(|o| o.host_node_id == from_node_id)
                .count()
        );
    }

    async fn local_kb_offer_from_share_id(
        &self,
        share_id: &str,
    ) -> Result<SharedKnowledgeOffer, String> {
        let offers = self.current_local_knowledge_offers(None).await;
        offers
            .into_iter()
            .find(|o| o.share_id == share_id)
            .ok_or_else(|| "知识共享项已创建但目录尚未就绪".to_string())
    }

    async fn current_local_knowledge_offers(
        &self,
        allowed_group_ids: Option<&HashSet<String>>,
    ) -> Vec<SharedKnowledgeOffer> {
        let (enabled, identity) = {
            let guard = self.inner.read().await;
            (guard.enabled, guard.identity.clone())
        };
        if !enabled {
            return Vec::new();
        }
        self.knowledge_share
            .local_offers(&identity.node_id, &identity.display_name, allowed_group_ids)
            .await
    }

    async fn send_local_knowledge_shares_to(&self, node_id: &str) {
        let allowed = self.group_ids_for_peer(node_id).await;
        let offers = self.current_local_knowledge_offers(Some(&allowed)).await;
        self.send_to(node_id, WireMessage::KnowledgeShareAdvert { offers })
            .await;
    }

    async fn broadcast_local_knowledge_shares(&self) {
        let full = self.current_local_knowledge_offers(None).await;
        {
            let guard = self.inner.read().await;
            self.emit_event(&guard, EVENT_KNOWLEDGE_SHARE, &full);
        }
        let peer_ids: Vec<String> = {
            let guard = self.inner.read().await;
            guard.sessions.keys().cloned().collect()
        };
        for peer_id in peer_ids {
            let allowed = self.group_ids_for_peer(&peer_id).await;
            let offers = full
                .iter()
                .filter(|o| match &o.group_id {
                    Some(gid) => allowed.contains(gid),
                    None => false,
                })
                .cloned()
                .collect::<Vec<_>>();
            self.send_to(&peer_id, WireMessage::KnowledgeShareAdvert { offers })
                .await;
        }
    }

    async fn handle_knowledge_share_advert(
        &self,
        from_node_id: &str,
        mut offers: Vec<SharedKnowledgeOffer>,
    ) {
        offers.retain(|o| o.host_node_id == from_node_id);
        for offer in &mut offers {
            offer.online = true;
        }
        let mut guard = self.inner.write().await;
        guard
            .remote_shared_knowledge
            .retain(|o| o.host_node_id != from_node_id);
        guard.remote_shared_knowledge.extend(offers);
        let snapshot = guard.remote_shared_knowledge.clone();
        self.emit_event(&guard, EVENT_KNOWLEDGE_SHARE, &snapshot);
        tracing::info!(
            "[lan_collab] knowledge share advert from {from_node_id}: {} offers",
            snapshot
                .iter()
                .filter(|o| o.host_node_id == from_node_id)
                .count()
        );
    }

    async fn local_skill_offer_from_share_id(
        &self,
        share_id: &str,
    ) -> Result<SharedSkillOffer, String> {
        let offers = self.current_local_skill_offers(None).await;
        offers
            .into_iter()
            .find(|o| o.share_id == share_id)
            .ok_or_else(|| "Skill 共享项已创建但目录尚未就绪".to_string())
    }

    async fn current_local_skill_offers(
        &self,
        allowed_group_ids: Option<&HashSet<String>>,
    ) -> Vec<SharedSkillOffer> {
        let (enabled, identity) = {
            let guard = self.inner.read().await;
            (guard.enabled, guard.identity.clone())
        };
        if !enabled {
            return Vec::new();
        }
        self.skill_share
            .local_offers(&identity.node_id, &identity.display_name, allowed_group_ids)
            .await
    }

    async fn send_local_skill_shares_to(&self, node_id: &str) {
        let allowed = self.group_ids_for_peer(node_id).await;
        let offers = self.current_local_skill_offers(Some(&allowed)).await;
        self.send_to(node_id, WireMessage::SkillShareAdvert { offers })
            .await;
    }

    async fn broadcast_local_skill_shares(&self) {
        let full = self.current_local_skill_offers(None).await;
        {
            let guard = self.inner.read().await;
            self.emit_event(&guard, EVENT_SKILL_SHARE, &full);
        }
        let peer_ids: Vec<String> = {
            let guard = self.inner.read().await;
            guard.sessions.keys().cloned().collect()
        };
        for peer_id in peer_ids {
            let allowed = self.group_ids_for_peer(&peer_id).await;
            let offers = full
                .iter()
                .filter(|o| match &o.group_id {
                    Some(gid) => allowed.contains(gid),
                    None => false,
                })
                .cloned()
                .collect::<Vec<_>>();
            self.send_to(&peer_id, WireMessage::SkillShareAdvert { offers })
                .await;
        }
    }

    async fn handle_skill_share_advert(
        &self,
        from_node_id: &str,
        mut offers: Vec<SharedSkillOffer>,
    ) {
        offers.retain(|o| o.host_node_id == from_node_id);
        for offer in &mut offers {
            offer.online = true;
        }
        let mut guard = self.inner.write().await;
        guard
            .remote_shared_skills
            .retain(|o| o.host_node_id != from_node_id);
        guard.remote_shared_skills.extend(offers);
        let snapshot = guard.remote_shared_skills.clone();
        self.emit_event(&guard, EVENT_SKILL_SHARE, &snapshot);
        tracing::info!(
            "[lan_collab] skill share advert from {from_node_id}: {} offers",
            snapshot
                .iter()
                .filter(|o| o.host_node_id == from_node_id)
                .count()
        );
    }

    async fn handle_skill_fetch_request(
        &self,
        from_node_id: &str,
        request_id: String,
        share_id: String,
    ) {
        let result = match self
            .ensure_peer_can_access_share(
                from_node_id,
                self.skill_share.share_group_id(&share_id).await,
            )
            .await
        {
            Ok(()) => self.skill_share.fetch_share(&share_id).await,
            Err(err) => Err(err),
        };
        let (skill_id, name, content, content_hash, error) = match result {
            Ok(payload) => (
                Some(payload.skill_id),
                Some(payload.name),
                Some(payload.content),
                Some(payload.content_hash),
                None,
            ),
            Err(err) => (None, None, None, None, Some(err)),
        };
        self.send_to(
            from_node_id,
            WireMessage::SkillFetchResponse {
                request_id,
                skill_id,
                name,
                content,
                content_hash,
                error,
            },
        )
        .await;
    }

    async fn local_workflow_offer_from_share_id(
        &self,
        share_id: &str,
    ) -> Result<SharedWorkflowOffer, String> {
        self.current_local_workflow_offers(None)
            .await
            .into_iter()
            .find(|o| o.share_id == share_id)
            .ok_or_else(|| "Workflow 共享项已创建但目录尚未就绪".to_string())
    }

    async fn current_local_workflow_offers(
        &self,
        allowed_group_ids: Option<&HashSet<String>>,
    ) -> Vec<SharedWorkflowOffer> {
        let (enabled, identity) = {
            let guard = self.inner.read().await;
            (guard.enabled, guard.identity.clone())
        };
        if !enabled {
            return Vec::new();
        }
        self.workflow_share
            .local_offers(&identity.node_id, &identity.display_name, allowed_group_ids)
            .await
    }

    async fn send_local_workflow_shares_to(&self, node_id: &str) {
        let allowed = self.group_ids_for_peer(node_id).await;
        let offers = self.current_local_workflow_offers(Some(&allowed)).await;
        self.send_to(node_id, WireMessage::WorkflowShareAdvert { offers })
            .await;
    }

    async fn broadcast_local_workflow_shares(&self) {
        let full = self.current_local_workflow_offers(None).await;
        {
            let guard = self.inner.read().await;
            self.emit_event(&guard, EVENT_WORKFLOW_SHARE, &full);
        }
        let peer_ids: Vec<String> = {
            let guard = self.inner.read().await;
            guard.sessions.keys().cloned().collect()
        };
        for peer_id in peer_ids {
            let allowed = self.group_ids_for_peer(&peer_id).await;
            let offers = full
                .iter()
                .filter(|o| match &o.group_id {
                    Some(gid) => allowed.contains(gid),
                    None => false,
                })
                .cloned()
                .collect::<Vec<_>>();
            self.send_to(&peer_id, WireMessage::WorkflowShareAdvert { offers })
                .await;
        }
    }

    async fn handle_workflow_share_advert(
        &self,
        from_node_id: &str,
        mut offers: Vec<SharedWorkflowOffer>,
    ) {
        offers.retain(|o| o.host_node_id == from_node_id);
        for offer in &mut offers {
            offer.online = true;
        }
        let mut guard = self.inner.write().await;
        guard
            .remote_shared_workflows
            .retain(|o| o.host_node_id != from_node_id);
        guard.remote_shared_workflows.extend(offers);
        let snapshot = guard.remote_shared_workflows.clone();
        self.emit_event(&guard, EVENT_WORKFLOW_SHARE, &snapshot);
        tracing::info!(
            "[lan_collab] workflow share advert from {from_node_id}: {} offers",
            snapshot
                .iter()
                .filter(|o| o.host_node_id == from_node_id)
                .count()
        );
    }

    async fn handle_workflow_fetch_request(
        &self,
        from_node_id: &str,
        request_id: String,
        share_id: String,
    ) {
        let result = match self
            .ensure_peer_can_access_share(
                from_node_id,
                self.workflow_share.share_group_id(&share_id).await,
            )
            .await
        {
            Ok(()) => self.workflow_share.fetch_share(&share_id).await,
            Err(err) => Err(err),
        };
        let msg = match result {
            Ok(payload) => WireMessage::WorkflowFetchResponse {
                request_id,
                workflow_name: Some(payload.workflow_name),
                title: Some(payload.title),
                content_hash: Some(payload.content_hash),
                workflow_json: Some(payload.workflow_json),
                skill_md: Some(payload.skill_md),
                scripts: Some(payload.scripts),
                scripts_manifest_json: payload.scripts_manifest_json,
                error: None,
            },
            Err(err) => WireMessage::WorkflowFetchResponse {
                request_id,
                workflow_name: None,
                title: None,
                content_hash: None,
                workflow_json: None,
                skill_md: None,
                scripts: None,
                scripts_manifest_json: None,
                error: Some(err),
            },
        };
        self.send_to(from_node_id, msg).await;
    }

    async fn handle_knowledge_search_request(
        &self,
        from_node_id: &str,
        request_id: String,
        share_id: String,
        query: String,
        top_k: usize,
    ) {
        let result = match self
            .ensure_peer_can_access_share(
                from_node_id,
                self.knowledge_share.share_group_id(&share_id).await,
            )
            .await
        {
            Ok(()) => {
                self.knowledge_share
                    .search_share(&share_id, &query, top_k)
                    .await
            }
            Err(err) => Err(err),
        };
        let (hits, error) = match result {
            Ok(mut hits) => {
                let host = {
                    let guard = self.inner.read().await;
                    guard.identity.clone()
                };
                for hit in &mut hits {
                    hit.host_node_id = host.node_id.clone();
                    hit.host_display_name = host.display_name.clone();
                    hit.share_id = share_id.clone();
                }
                (Some(hits), None)
            }
            Err(err) => (None, Some(err)),
        };
        self.send_to(
            from_node_id,
            WireMessage::KnowledgeSearchResponse {
                request_id,
                hits,
                error,
            },
        )
        .await;
    }

    async fn handle_knowledge_fetch_request(
        &self,
        from_node_id: &str,
        request_id: String,
        share_id: String,
        doc_id: String,
    ) {
        let result = match self
            .ensure_peer_can_access_share(
                from_node_id,
                self.knowledge_share.share_group_id(&share_id).await,
            )
            .await
        {
            Ok(()) => self.knowledge_share.fetch_doc(&share_id, &doc_id).await,
            Err(err) => Err(err),
        };
        let (doc, error) = match result {
            Ok(mut doc) => {
                let host = {
                    let guard = self.inner.read().await;
                    guard.identity.clone()
                };
                doc.host_node_id = host.node_id;
                doc.host_display_name = host.display_name;
                doc.share_id = share_id;
                (Some(doc), None)
            }
            Err(err) => (None, Some(err)),
        };
        self.send_to(
            from_node_id,
            WireMessage::KnowledgeFetchResponse {
                request_id,
                doc,
                error,
            },
        )
        .await;
    }

    async fn start_discovery(&self) -> Result<(), String> {
        {
            let handle_guard = self.discovery_handle.lock().await;
            if handle_guard.is_some() {
                return Ok(());
            }
        }

        self.refresh_beacon_cache().await;

        {
            let mut guard = self.inner.write().await;
            guard.discovery_scanning = true;
        }

        let runtime = self.clone();
        let local_node_id = {
            let guard = self.inner.read().await;
            guard.identity.node_id.clone()
        };
        let listen_port = {
            let guard = self.inner.read().await;
            guard.bind_port
        };

        let beacon_cache = self.beacon_cache.clone();
        let get_beacon = Arc::new(move || {
            beacon_cache.read().map(|b| b.clone()).unwrap_or_else(|_| {
                PresenceBeacon::new(String::new(), String::new(), listen_port, Vec::new())
            })
        });

        let on_discovered: discovery::DiscoveryCallback = Arc::new(move |endpoint| {
            let runtime = runtime.clone();
            tauri::async_runtime::spawn(async move {
                runtime.on_discovered_endpoint(endpoint).await;
            });
        });

        let handle =
            discovery::start_discovery(local_node_id, listen_port, get_beacon, on_discovered)
                .await?;
        {
            let mut handle_guard = self.discovery_handle.lock().await;
            *handle_guard = Some(handle);
        }

        // 启动后立即补一轮端口扫描（UDP 可能被禁）
        let runtime = self.clone();
        tokio::spawn(async move {
            runtime.run_port_scan_once().await;
        });

        Ok(())
    }

    async fn stop_discovery(&self) {
        let handle = {
            let mut guard = self.discovery_handle.lock().await;
            guard.take()
        };
        if let Some(handle) = handle {
            handle.stop().await;
        }
        let mut guard = self.inner.write().await;
        guard.discovery_scanning = false;
    }

    async fn refresh_beacon_cache(&self) {
        let (identity, bind_port, owned_groups) = {
            let guard = self.inner.read().await;
            let host_addr = local_ip_hint().unwrap_or_else(|| "127.0.0.1".to_string());
            let owned = guard
                .groups
                .iter()
                .filter(|g| g.is_owner)
                .map(|g| DiscoveredGroupSummary {
                    group_id: g.group_id.clone(),
                    name: g.name.clone(),
                    owner_node_id: g.owner_node_id.clone(),
                    owner_display_name: guard.identity.display_name.clone(),
                    member_count: g.members.len(),
                    has_invite: g.invite_code.is_some(),
                    owner_address: host_addr.clone(),
                    owner_port: guard.bind_port,
                    last_seen_at: chrono::Utc::now().timestamp(),
                })
                .collect::<Vec<_>>();
            (guard.identity.clone(), guard.bind_port, owned)
        };
        if let Ok(mut cache) = self.beacon_cache.write() {
            *cache = PresenceBeacon::new(
                identity.node_id,
                identity.display_name,
                bind_port,
                owned_groups,
            );
        }
    }

    async fn on_discovered_endpoint(&self, endpoint: DiscoveredEndpoint) {
        let now = chrono::Utc::now().timestamp();
        let mut should_connect = false;
        let connect_key = format!("{}:{}", endpoint.address, endpoint.port);

        {
            let mut guard = self.inner.write().await;
            if !guard.enabled {
                return;
            }
            if endpoint.node_id == guard.identity.node_id {
                // 复制安装会让两个实例共享 node_id：不能当普通对端处理，
                // 但可以尝试让本机自动换新身份，否则永远扫不到对方的组。
                let self_port = guard.bind_port;
                let owns_groups = guard.groups.iter().any(|g| g.is_owner);
                drop(guard);
                if endpoint.port != 0 && endpoint.port != self_port {
                    tracing::warn!(
                        "[lan_collab] discovered peer with same node_id={} at {}:{} (copied install? owns_groups={owns_groups})",
                        endpoint.node_id,
                        endpoint.address,
                        endpoint.port
                    );
                    if !owns_groups {
                        let _ = self.try_heal_duplicate_identity_local(endpoint.port).await;
                    }
                }
                return;
            }

            guard.discovery_last_scan_at = Some(now);

            // 合并发现到的协作组
            for mut group in endpoint.groups {
                group.last_seen_at = now;
                if group.owner_address.is_empty() {
                    group.owner_address = endpoint.address.clone();
                }
                if group.owner_port == 0 {
                    group.owner_port = endpoint.port;
                }
                upsert_discovered_group(&mut guard.discovered_groups, group);
            }

            // 信标来源：更新/插入 peer 摘要（扫描候选用临时 node_id）
            if endpoint.from_beacon && !endpoint.node_id.starts_with("scan:") {
                let peer = NearbyPeer {
                    node_id: endpoint.node_id.clone(),
                    display_name: endpoint.display_name.clone(),
                    address: endpoint.address.clone(),
                    port: endpoint.port,
                    last_seen_at: now,
                    trusted: guard
                        .peers
                        .iter()
                        .any(|p| p.node_id == endpoint.node_id && p.trusted),
                    connected: guard.sessions.contains_key(&endpoint.node_id),
                };
                upsert_peer(&mut guard.peers, peer.clone());
                let _ = self.store.save_peers(&guard.peers);
                self.emit_event(&guard, EVENT_PEER, &peer);

                // 仅较小 node_id 主动拨号，避免双方同时互连。
                let local_node_id = guard.identity.node_id.clone();
                if should_dial_peer(&local_node_id, &endpoint.node_id)
                    && !guard.sessions.contains_key(&endpoint.node_id)
                    && !guard.auto_connect_inflight.contains(&connect_key)
                {
                    // 同 node 的其他地址也算 in-flight，减少 127.0.0.1 / 局域网 IP 双拨。
                    let node_key = format!("node:{}", endpoint.node_id);
                    if !guard.auto_connect_inflight.contains(&node_key) {
                        guard.auto_connect_inflight.insert(connect_key.clone());
                        guard.auto_connect_inflight.insert(node_key);
                        should_connect = true;
                    }
                }
            } else if !endpoint.from_beacon {
                // 端口扫描候选：若尚未连接该地址则自动拨号
                let already_connected = guard.sessions.values().any(|s| {
                    (s.address == endpoint.address && s.port == endpoint.port)
                        || (is_loopback_ip(&s.address)
                            && is_loopback_ip(&endpoint.address)
                            && s.port == endpoint.port)
                }) || guard.peers.iter().any(|p| {
                    p.address == endpoint.address
                        && p.port == endpoint.port
                        && guard.sessions.contains_key(&p.node_id)
                }) || guard.peers.iter().any(|p| {
                    // 已通过真实 node_id 连上同一端口时，跳过 scan: 重复拨号
                    !p.node_id.starts_with("scan:")
                        && p.port == endpoint.port
                        && (p.address == endpoint.address
                            || (is_loopback_ip(&p.address) && is_loopback_ip(&endpoint.address)))
                        && guard.sessions.contains_key(&p.node_id)
                });
                // 扫描候选尚不知对端 node_id：用端口大小做单向拨号，降低双连概率。
                let local_port = guard.bind_port;
                if !already_connected
                    && local_port < endpoint.port
                    && !guard.auto_connect_inflight.contains(&connect_key)
                {
                    let port_key = format!("port:{}", endpoint.port);
                    if !guard.auto_connect_inflight.contains(&port_key) {
                        guard.auto_connect_inflight.insert(connect_key.clone());
                        guard.auto_connect_inflight.insert(port_key);
                        should_connect = true;
                    }
                }
            }

            // 扫描候选也先展示到附近设备，避免“扫到了但列表空白”
            if !endpoint.from_beacon {
                let probe = NearbyPeer {
                    node_id: endpoint.node_id.clone(),
                    display_name: endpoint.display_name.clone(),
                    address: endpoint.address.clone(),
                    port: endpoint.port,
                    last_seen_at: now,
                    trusted: false,
                    connected: false,
                };
                let already_listed = guard.peers.iter().any(|p| same_peer_record(p, &probe));
                if !already_listed {
                    upsert_peer(&mut guard.peers, probe.clone());
                    let _ = self.store.save_peers(&guard.peers);
                    self.emit_event(&guard, EVENT_PEER, &probe);
                }
            }

            let groups = guard.discovered_groups.clone();
            self.emit_event(&guard, EVENT_DISCOVERY, &groups);
        }

        if should_connect {
            let runtime = self.clone();
            let host = endpoint.address.clone();
            let port = endpoint.port;
            let node_id = endpoint.node_id.clone();
            tokio::spawn(async move {
                let result = runtime.connect_peer(host.clone(), port).await;
                {
                    let mut guard = runtime.inner.write().await;
                    guard
                        .auto_connect_inflight
                        .remove(&format!("{host}:{port}"));
                    if !node_id.is_empty() {
                        guard
                            .auto_connect_inflight
                            .remove(&format!("node:{node_id}"));
                    }
                    guard.auto_connect_inflight.remove(&format!("port:{port}"));
                }
                if let Err(err) = result {
                    tracing::debug!("[lan_collab] auto-connect {host}:{port} failed: {err}");
                }
            });
        }
    }

    async fn run_port_scan_once(&self) {
        let (enabled, bind_port) = {
            let guard = self.inner.read().await;
            (guard.enabled, guard.bind_port)
        };
        if !enabled {
            return;
        }
        {
            let mut guard = self.inner.write().await;
            guard.discovery_scanning = true;
        }
        let found = discovery::scan_now(bind_port).await;
        let now = chrono::Utc::now().timestamp();
        {
            let mut guard = self.inner.write().await;
            guard.discovery_last_scan_at = Some(now);
            guard.discovery_scanning = true;
        }
        for (ip, port) in found {
            self.on_discovered_endpoint(DiscoveredEndpoint {
                node_id: format!("scan:{ip}:{port}"),
                display_name: format!("{ip}:{port}"),
                address: ip,
                port,
                groups: Vec::new(),
                from_beacon: false,
            })
            .await;
        }
    }

    async fn owned_group_directory(&self) -> Vec<DiscoveredGroupSummary> {
        let guard = self.inner.read().await;
        let host_addr = local_ip_hint().unwrap_or_else(|| "127.0.0.1".to_string());
        let now = chrono::Utc::now().timestamp();
        guard
            .groups
            .iter()
            .filter(|g| g.is_owner)
            .map(|g| DiscoveredGroupSummary {
                group_id: g.group_id.clone(),
                name: g.name.clone(),
                owner_node_id: g.owner_node_id.clone(),
                owner_display_name: guard.identity.display_name.clone(),
                member_count: g.members.len(),
                has_invite: g.invite_code.is_some(),
                owner_address: host_addr.clone(),
                owner_port: guard.bind_port,
                last_seen_at: now,
            })
            .collect()
    }

    async fn send_group_directory_to(&self, node_id: &str) {
        let groups = self.owned_group_directory().await;
        if groups.is_empty() {
            // 仍发送空目录，便于对端清理过期项
            self.send_to(
                node_id,
                WireMessage::GroupDirectoryAdvert { groups: Vec::new() },
            )
            .await;
            return;
        }
        self.send_to(node_id, WireMessage::GroupDirectoryAdvert { groups })
            .await;
    }

    async fn handle_group_directory_advert(
        &self,
        from_node_id: &str,
        groups: Vec<DiscoveredGroupSummary>,
    ) {
        let now = chrono::Utc::now().timestamp();
        let peer_addr = {
            let guard = self.inner.read().await;
            guard
                .peers
                .iter()
                .find(|p| p.node_id == from_node_id)
                .map(|p| (p.address.clone(), p.port))
        };
        let mut guard = self.inner.write().await;
        if groups.is_empty() {
            // 对端无公开组：移除其旧目录
            guard
                .discovered_groups
                .retain(|g| g.owner_node_id != from_node_id);
        } else {
            for mut group in groups {
                group.last_seen_at = now;
                if group.owner_address.is_empty() {
                    if let Some((addr, port)) = &peer_addr {
                        group.owner_address = addr.clone();
                        if group.owner_port == 0 {
                            group.owner_port = *port;
                        }
                    }
                }
                if group.owner_node_id.is_empty() {
                    group.owner_node_id = from_node_id.to_string();
                }
                upsert_discovered_group(&mut guard.discovered_groups, group);
            }
        }
        let snapshot = guard.discovered_groups.clone();
        self.emit_event(&guard, EVENT_DISCOVERY, &snapshot);
    }

    fn emit_event<T: serde::Serialize + Clone>(
        &self,
        guard: &RuntimeInner,
        event: &str,
        payload: &T,
    ) {
        if let Some(app) = guard.app_handle.as_ref() {
            let _ = app.emit(event, payload.clone());
        }
    }
}

fn upsert_peer(peers: &mut Vec<NearbyPeer>, peer: NearbyPeer) {
    // 同机双开时，同一节点常同时被 127.0.0.1 与局域网 IP 发现。
    // 这里按 node_id / 同端口 loopback+LAN / scan 候选合并，避免 UI 来回闪地址。
    if let Some(idx) = peers.iter().position(|p| same_peer_record(p, &peer)) {
        let existing = peers[idx].clone();
        peers[idx] = merge_peer_record(existing, peer);
        // 合并后可能把 scan: 候选与真实 node 合成一条，清理残留的同端口占位项。
        dedupe_peers(peers);
    } else {
        peers.push(peer);
        dedupe_peers(peers);
    }
}

fn same_peer_record(a: &NearbyPeer, b: &NearbyPeer) -> bool {
    if a.node_id == b.node_id {
        return true;
    }
    // 扫描占位项与真实节点：同一端口且地址等价（含 loopback/本机 LAN）
    if a.port == b.port
        && same_host_endpoint(&a.address, &b.address)
        && (a.node_id.starts_with("scan:")
            || b.node_id.starts_with("scan:")
            || a.display_name == format!("{}:{}", a.address, a.port)
            || b.display_name == format!("{}:{}", b.address, b.port))
    {
        return true;
    }
    // 真实节点偶发只改地址：同 node 前缀已覆盖；这里兜底同端口 + 同主机族
    !a.node_id.starts_with("scan:")
        && !b.node_id.starts_with("scan:")
        && a.port == b.port
        && same_host_endpoint(&a.address, &b.address)
}

fn merge_peer_record(existing: NearbyPeer, incoming: NearbyPeer) -> NearbyPeer {
    let prefer_incoming = prefer_peer_record(&incoming, &existing);
    let (primary, secondary) = if prefer_incoming {
        (incoming, existing)
    } else {
        (existing, incoming)
    };
    NearbyPeer {
        node_id: if primary.node_id.starts_with("scan:") && !secondary.node_id.starts_with("scan:")
        {
            secondary.node_id
        } else {
            primary.node_id
        },
        display_name: if primary.display_name.contains(':')
            && !secondary.display_name.contains(':')
            && !secondary.display_name.is_empty()
        {
            secondary.display_name
        } else if !primary.display_name.is_empty() {
            primary.display_name
        } else {
            secondary.display_name
        },
        address: prefer_display_address(&primary.address, &secondary.address),
        port: primary.port,
        last_seen_at: primary.last_seen_at.max(secondary.last_seen_at),
        trusted: primary.trusted || secondary.trusted,
        connected: primary.connected || secondary.connected,
    }
}

fn prefer_peer_record(candidate: &NearbyPeer, current: &NearbyPeer) -> bool {
    let candidate_real = !candidate.node_id.starts_with("scan:");
    let current_real = !current.node_id.starts_with("scan:");
    match (candidate_real, current_real) {
        (true, false) => true,
        (false, true) => false,
        _ => {
            // 优先保留已连接 / 可信会话地址；否则优先非 loopback，减少 127.0.0.1 闪烁。
            if candidate.connected != current.connected {
                return candidate.connected;
            }
            if candidate.trusted != current.trusted {
                return candidate.trusted;
            }
            match (
                is_loopback_ip(&candidate.address),
                is_loopback_ip(&current.address),
            ) {
                (false, true) => true,
                (true, false) => false,
                _ => candidate.last_seen_at >= current.last_seen_at,
            }
        }
    }
}

fn prefer_display_address(primary: &str, secondary: &str) -> String {
    if is_loopback_ip(primary) && !is_loopback_ip(secondary) {
        secondary.to_string()
    } else if primary.is_empty() {
        secondary.to_string()
    } else {
        primary.to_string()
    }
}

fn same_host_endpoint(a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    if is_loopback_ip(a) && is_loopback_ip(b) {
        return true;
    }
    // 同机双开：一端扫到 127.0.0.1，另一端扫到本机局域网 IP，视为同一主机。
    let local_lan = local_ip_hint();
    match local_lan.as_deref() {
        Some(lan) => {
            (is_loopback_ip(a) && b == lan)
                || (is_loopback_ip(b) && a == lan)
                || (a == lan && b == lan)
        }
        None => false,
    }
}

fn dedupe_peers(peers: &mut Vec<NearbyPeer>) {
    let mut kept: Vec<NearbyPeer> = Vec::with_capacity(peers.len());
    for peer in peers.drain(..) {
        if let Some(idx) = kept.iter().position(|p| same_peer_record(p, &peer)) {
            let existing = kept[idx].clone();
            kept[idx] = merge_peer_record(existing, peer);
        } else {
            kept.push(peer);
        }
    }
    *peers = kept;
}

fn upsert_discovered_group(
    groups: &mut Vec<DiscoveredGroupSummary>,
    group: DiscoveredGroupSummary,
) {
    if let Some(existing) = groups.iter_mut().find(|g| g.group_id == group.group_id) {
        *existing = group;
    } else {
        groups.push(group);
    }
}

fn make_invite_code(name: &str) -> String {
    let prefix: String = name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .take(4)
        .collect::<String>()
        .to_uppercase();
    let prefix = if prefix.is_empty() {
        "LAN".to_string()
    } else {
        prefix
    };
    let suffix = &Uuid::new_v4().simple().to_string()[..4].to_uppercase();
    format!("{prefix}-{suffix}")
}

fn is_loopback_ip(ip: &str) -> bool {
    ip == "127.0.0.1" || ip == "::1" || ip == "localhost" || ip.starts_with("127.")
}

/// 较小 node_id 的节点负责主动拨号。
fn should_dial_peer(local_node_id: &str, remote_node_id: &str) -> bool {
    !remote_node_id.is_empty()
        && !remote_node_id.starts_with("scan:")
        && local_node_id < remote_node_id
}

/// 重复连接裁决：只保留“较小 node_id 拨号”的那条连接。
fn should_prefer_connection(local_node_id: &str, remote_node_id: &str, inbound: bool) -> bool {
    if inbound {
        // 对端拨入：对端 node_id 更小时保留
        remote_node_id < local_node_id
    } else {
        // 本机拨出：本机 node_id 更小时保留
        local_node_id < remote_node_id
    }
}

fn local_ip_hint() -> Option<String> {
    local_ip_address::local_ip().ok().map(|ip| ip.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config_system::ConfigManager;
    use axum::routing::post;
    use axum::{Json, Router};
    use serde_json::{Value, json};
    use std::fs;
    use std::path::Path;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::net::TcpListener;

    fn write_kb_doc(kdir: &Path, doc_id: &str, title: &str, body: &str) {
        let docs = kdir.join("docs");
        fs::create_dir_all(&docs).unwrap();
        let content =
            format!("---\ntype: Knowledge\ntitle: {title}\ndomain: test\n---\n\n{body}\n");
        fs::write(docs.join(format!("{doc_id}.md")), content).unwrap();
    }

    fn write_kb_index(kdir: &Path, entries: &[(&str, &str)]) {
        let index = serde_json::json!({
            "version": 1,
            "entries": entries.iter().map(|(id, title)| serde_json::json!({
                "doc_id": id,
                "source_file": format!("{id}.md"),
                "source_type": "markdown",
                "title": title,
                "added_at": 1,
                "chunk_count": 1,
                "categories": [],
                "domain": "test",
            })).collect::<Vec<_>>()
        });
        fs::create_dir_all(kdir).unwrap();
        fs::write(
            kdir.join("index.json"),
            serde_json::to_string_pretty(&index).unwrap(),
        )
        .unwrap();
    }

    async fn wait_until<F, Fut>(mut cond: F, timeout_ms: u64) -> bool
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = bool>,
    {
        let deadline = tokio::time::Instant::now() + Duration::from_millis(timeout_ms);
        while tokio::time::Instant::now() < deadline {
            if cond().await {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        false
    }

    #[tokio::test]
    async fn dual_node_connect_chat_model_and_knowledge_smoke() {
        // 假上游 OpenAI 兼容接口：验证模型代理 token 与转发
        let upstream_hits = Arc::new(AtomicUsize::new(0));
        let upstream_hits_clone = upstream_hits.clone();
        let upstream_app = Router::new().route(
            "/v1/chat/completions",
            post(move |Json(body): Json<Value>| {
                let hits = upstream_hits_clone.clone();
                async move {
                    hits.fetch_add(1, Ordering::SeqCst);
                    let model = body
                        .get("model")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown")
                        .to_string();
                    Json(json!({
                        "id": "chatcmpl-test",
                        "object": "chat.completion",
                        "choices": [{
                            "index": 0,
                            "message": {
                                "role": "assistant",
                                "content": format!("ok:{model}")
                            },
                            "finish_reason": "stop"
                        }]
                    }))
                }
            }),
        );
        let upstream_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let upstream_addr = upstream_listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(upstream_listener, upstream_app).await.unwrap();
        });

        let tmp_a = tempfile::tempdir().unwrap();
        let tmp_b = tempfile::tempdir().unwrap();
        let ws_a = tmp_a.path().to_path_buf();
        let ws_b = tmp_b.path().to_path_buf();

        // A 侧准备本地知识库
        let kdir = ws_a.join("memories").join("knowledge");
        write_kb_index(
            &kdir,
            &[("doc_alpha", "Alpha Doc"), ("doc_beta", "Beta Doc")],
        );
        write_kb_doc(
            &kdir,
            "doc_alpha",
            "Alpha Doc",
            "alpha secret knowledge for lan collab",
        );
        write_kb_doc(
            &kdir,
            "doc_beta",
            "Beta Doc",
            "beta knowledge should remain unshared",
        );

        let config_a = ConfigManager::new(ws_a.join("config.toml"));
        let config_b = ConfigManager::new(ws_b.join("config.toml"));
        let node_a =
            LanCollabRuntime::open(ws_a.join("lan_collab"), config_a, ws_a.clone()).unwrap();
        let node_b =
            LanCollabRuntime::open(ws_b.join("lan_collab"), config_b, ws_b.clone()).unwrap();

        node_a.set_display_name("OwnerA".into()).await.unwrap();
        node_b.set_display_name("MemberB".into()).await.unwrap();

        let status_a = node_a.set_enabled(true).await.unwrap();
        let status_b = node_b.set_enabled(true).await.unwrap();
        assert!(status_a.enabled);
        assert!(status_b.enabled);
        assert!(status_a.bind_port >= 47800);

        // B 手动连接 A
        let peer = node_b
            .connect_peer("127.0.0.1".into(), status_a.bind_port)
            .await
            .unwrap();
        assert!(peer.connected);
        assert_eq!(peer.display_name, "OwnerA");

        let connected = wait_until(
            || async {
                let a = node_a.status().await;
                let b = node_b.status().await;
                a.connected_peer_count >= 1 && b.connected_peer_count >= 1
            },
            3000,
        )
        .await;
        assert!(connected, "双端会话未建立");

        // 建组 + 入组 + 聊天
        let group = node_a.create_group("Smoke Team".into()).await.unwrap();
        assert!(group.is_owner);
        let invite = group.invite_code.clone().expect("invite code");
        let joined = node_b.join_group(invite).await.unwrap();
        assert_eq!(joined.group_id, group.group_id);
        assert_eq!(joined.members.len(), 2);

        let msg = node_a
            .send_message(group.group_id.clone(), "hello from A".into())
            .await
            .unwrap();
        assert_eq!(msg.text, "hello from A");

        let chat_synced = wait_until(
            || async {
                match node_b.list_messages(group.group_id.clone()).await {
                    Ok(list) => list.iter().any(|m| m.message_id == msg.message_id),
                    Err(_) => false,
                }
            },
            3000,
        )
        .await;
        assert!(chat_synced, "B 未收到组内消息");

        // 模型共享：A 共享本地假上游，B 通过代理零 Key 调用
        let offer = node_a
            .share_model(
                "demo-model".into(),
                "Demo Shared".into(),
                "custom".into(),
                "demo-model".into(),
                Some(group.group_id.clone()),
                Some(format!("http://{upstream_addr}/v1")),
                Some("host-secret-key".into()),
            )
            .await
            .unwrap();
        assert!(offer.proxy_port >= 47900);
        assert!(!offer.access_token.is_empty());

        let remote_model_ready = wait_until(
            || async {
                node_b
                    .list_remote_shared_models()
                    .await
                    .iter()
                    .any(|o| o.share_id == offer.share_id)
            },
            3000,
        )
        .await;
        assert!(remote_model_ready, "B 未收到模型共享目录");

        let remote_offer = node_b
            .list_remote_shared_models()
            .await
            .into_iter()
            .find(|o| o.share_id == offer.share_id)
            .unwrap();
        // host_address 可能是 127.0.0.1 或本机网卡 IP；测试连本机回环代理
        let proxy_url = format!(
            "http://127.0.0.1:{}/v1/chat/completions",
            remote_offer.proxy_port
        );
        let client = reqwest::Client::new();
        let resp = client
            .post(&proxy_url)
            .header(
                "Authorization",
                format!("Bearer {}", remote_offer.access_token),
            )
            .json(&json!({
                "model": remote_offer.share_id,
                "messages": [{"role":"user","content":"ping"}],
                "stream": false
            }))
            .send()
            .await
            .unwrap();
        assert!(
            resp.status().is_success(),
            "模型代理调用失败: {}",
            resp.status()
        );
        let body: Value = resp.json().await.unwrap();
        let content = body["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or_default();
        assert!(
            content.contains("ok:demo-model"),
            "unexpected content: {content}"
        );
        assert_eq!(upstream_hits.load(Ordering::SeqCst), 1);

        // 知识共享：A 仅共享 alpha，B 可拉取；beta 被拒绝；撤销后失败
        let kb_offer = node_a
            .share_knowledge(
                "Team KB".into(),
                Some(group.group_id.clone()),
                None,
                None,
                vec!["doc_alpha".into()],
            )
            .await
            .unwrap();

        let remote_kb_ready = wait_until(
            || async {
                node_b
                    .list_remote_shared_knowledge()
                    .await
                    .iter()
                    .any(|o| o.share_id == kb_offer.share_id)
            },
            3000,
        )
        .await;
        assert!(remote_kb_ready, "B 未收到知识共享目录");

        let fetched = node_b
            .fetch_remote_knowledge(
                node_a.status().await.identity.node_id,
                kb_offer.share_id.clone(),
                "doc_alpha".into(),
            )
            .await
            .unwrap();
        assert!(fetched.content.contains("alpha secret knowledge"));

        let denied = node_b
            .fetch_remote_knowledge(
                node_a.status().await.identity.node_id,
                kb_offer.share_id.clone(),
                "doc_beta".into(),
            )
            .await;
        assert!(denied.is_err(), "未共享文档不应被拉取");

        node_a
            .unshare_knowledge(kb_offer.share_id.clone())
            .await
            .unwrap();
        let after_unshare = node_b
            .fetch_remote_knowledge(
                node_a.status().await.identity.node_id,
                kb_offer.share_id.clone(),
                "doc_alpha".into(),
            )
            .await;
        assert!(after_unshare.is_err(), "撤销共享后仍可拉取");

        // 清理：关闭监听，避免端口占用影响后续测试
        let _ = node_a.set_enabled(false).await;
        let _ = node_b.set_enabled(false).await;
    }

    #[test]
    fn merge_loopback_and_lan_peer_into_one() {
        let mut peers = Vec::new();

        // 先以局域网地址建立真实节点，再反复用 loopback 刷新：
        // 应始终合并为 1 条，并稳定展示非 loopback 地址。
        upsert_peer(
            &mut peers,
            NearbyPeer {
                node_id: "node_owner".into(),
                display_name: "OwnerA".into(),
                address: "192.168.0.108".into(),
                port: 47800,
                last_seen_at: 1,
                trusted: true,
                connected: true,
            },
        );
        upsert_peer(
            &mut peers,
            NearbyPeer {
                node_id: "node_owner".into(),
                display_name: "OwnerA".into(),
                address: "127.0.0.1".into(),
                port: 47800,
                last_seen_at: 2,
                trusted: true,
                connected: true,
            },
        );
        upsert_peer(
            &mut peers,
            NearbyPeer {
                node_id: "scan:127.0.0.1:47800".into(),
                display_name: "127.0.0.1:47800".into(),
                address: "127.0.0.1".into(),
                port: 47800,
                last_seen_at: 3,
                trusted: false,
                connected: false,
            },
        );

        assert_eq!(peers.len(), 1, "same node must collapse to one row");
        assert_eq!(peers[0].node_id, "node_owner");
        assert_eq!(peers[0].display_name, "OwnerA");
        assert_eq!(peers[0].address, "192.168.0.108");
        assert!(peers[0].trusted);
        assert!(peers[0].connected);
    }
}
