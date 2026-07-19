import { invoke } from "@tauri-apps/api/core";

export interface NodeIdentity {
  nodeId: string;
  displayName: string;
  devicePubkey: string;
  createdAt: number;
}

export interface NearbyPeer {
  nodeId: string;
  displayName: string;
  address: string;
  port: number;
  lastSeenAt: number;
  trusted: boolean;
  connected?: boolean;
}

export interface GroupMember {
  nodeId: string;
  displayName: string;
  role: string;
  joinedAt: number;
}

export interface CollabGroup {
  groupId: string;
  name: string;
  ownerNodeId: string;
  createdAt: number;
  snapshotVersion: number;
  inviteCode?: string | null;
  members: GroupMember[];
  isOwner: boolean;
  ownerOnline: boolean;
}

export interface ChatMessage {
  messageId: string;
  groupId: string;
  fromNodeId: string;
  fromDisplayName: string;
  text: string;
  createdAt: number;
}

export interface DiscoveredGroupSummary {
  groupId: string;
  name: string;
  ownerNodeId: string;
  ownerDisplayName: string;
  memberCount: number;
  hasInvite?: boolean;
  ownerAddress: string;
  ownerPort: number;
  lastSeenAt: number;
}

export interface DiscoveryStatus {
  scanning: boolean;
  lastScanAt?: number | null;
  discoveredPeerCount: number;
}

export interface LanCollabStatus {
  enabled: boolean;
  identity: NodeIdentity;
  bindPort: number;
  localAddress?: string | null;
  peers: NearbyPeer[];
  groups: CollabGroup[];
  discoveredGroups?: DiscoveredGroupSummary[];
  discovery?: DiscoveryStatus;
  connectedPeerCount: number;
  localSharedModels?: SharedModelOffer[];
  remoteSharedModels?: SharedModelOffer[];
  localSharedKnowledge?: SharedKnowledgeOffer[];
  remoteSharedKnowledge?: SharedKnowledgeOffer[];
  localSharedSkills?: SharedSkillOffer[];
  remoteSharedSkills?: SharedSkillOffer[];
  localSharedWorkflows?: SharedWorkflowOffer[];
  remoteSharedWorkflows?: SharedWorkflowOffer[];
  architecture: string;
  note: string;
}

export interface SharedModelOffer {
  shareId: string;
  hostNodeId: string;
  hostDisplayName: string;
  hostAddress: string;
  proxyPort: number;
  modelId: string;
  displayName: string;
  providerId: string;
  upstreamModel: string;
  groupId?: string | null;
  accessToken: string;
  online: boolean;
}

export interface SharedKnowledgeDocMeta {
  docId: string;
  title: string;
  domain?: string | null;
  sourceGroup?: string | null;
  addedAt: number;
  chunkCount: number;
}

export interface SharedKnowledgeOffer {
  shareId: string;
  hostNodeId: string;
  hostDisplayName: string;
  title: string;
  groupId?: string | null;
  permission: string;
  sourceGroup?: string | null;
  domain?: string | null;
  docCount: number;
  docs?: SharedKnowledgeDocMeta[];
  online: boolean;
}

export interface RemoteKnowledgeHit {
  shareId: string;
  hostNodeId: string;
  hostDisplayName: string;
  docId: string;
  title: string;
  score: number;
  domain?: string | null;
  sourceGroup?: string | null;
  tags?: string[];
  isChunk?: boolean;
  chunkIndex?: number | null;
}

export interface RemoteKnowledgeDoc {
  shareId: string;
  hostNodeId: string;
  hostDisplayName: string;
  docId: string;
  title: string;
  content: string;
  domain?: string | null;
  tags?: string[];
  sourceGroup?: string | null;
}

export interface SharedSkillOffer {
  shareId: string;
  hostNodeId: string;
  hostDisplayName: string;
  skillId: string;
  name: string;
  description: string;
  tags?: string[];
  groupId?: string | null;
  online: boolean;
  contentHash?: string;
  updatedAt?: number | null;
}

export interface SharedWorkflowOffer {
  shareId: string;
  hostNodeId: string;
  hostDisplayName: string;
  workflowName: string;
  title: string;
  description: string;
  nodeCount: number;
  contentHash?: string;
  groupId?: string | null;
  online: boolean;
}

export interface WorkflowOriginSummary {
  workflowName: string;
  title: string;
  sourceHostDisplayName: string;
  sourceShareId: string;
  sourceHostNodeId: string;
  installedContentHash: string;
  currentContentHash?: string | null;
  localModified: boolean;
}

export function lanCollabStatus() {
  return invoke<LanCollabStatus>("lan_collab_status");
}

export function lanCollabSetEnabled(enabled: boolean) {
  return invoke<LanCollabStatus>("lan_collab_set_enabled", { enabled });
}

export function lanCollabSetDisplayName(displayName: string) {
  return invoke<NodeIdentity>("lan_collab_set_display_name", { displayName });
}

export function lanCollabListPeers() {
  return invoke<NearbyPeer[]>("lan_collab_list_peers");
}

export function lanCollabConnectPeer(host: string, port: number) {
  return invoke<NearbyPeer>("lan_collab_connect_peer", { host, port });
}

export function lanCollabRefreshScan() {
  return invoke<LanCollabStatus>("lan_collab_refresh_scan");
}

export function lanCollabCreateGroup(name: string) {
  return invoke<CollabGroup>("lan_collab_create_group", { name });
}

export function lanCollabJoinGroup(inviteCode: string) {
  return invoke<CollabGroup>("lan_collab_join_group", { inviteCode });
}

export function lanCollabListGroups() {
  return invoke<CollabGroup[]>("lan_collab_list_groups");
}

export function lanCollabSendMessage(groupId: string, text: string) {
  return invoke<ChatMessage>("lan_collab_send_message", { groupId, text });
}

export function lanCollabListMessages(groupId: string) {
  return invoke<ChatMessage[]>("lan_collab_list_messages", { groupId });
}

export function lanCollabShareModel(params: {
  modelId: string;
  displayName: string;
  providerId: string;
  upstreamModel: string;
  groupId?: string | null;
  upstreamBaseUrl?: string | null;
  upstreamApiKey?: string | null;
}) {
  return invoke<SharedModelOffer>("lan_collab_share_model", {
    modelId: params.modelId,
    displayName: params.displayName,
    providerId: params.providerId,
    upstreamModel: params.upstreamModel,
    groupId: params.groupId ?? null,
    upstreamBaseUrl: params.upstreamBaseUrl ?? null,
    upstreamApiKey: params.upstreamApiKey ?? null,
  });
}

export function lanCollabUnshareModel(shareId: string) {
  return invoke<void>("lan_collab_unshare_model", { shareId });
}

export function lanCollabListLocalSharedModels() {
  return invoke<SharedModelOffer[]>("lan_collab_list_local_shared_models");
}

export function lanCollabListRemoteSharedModels() {
  return invoke<SharedModelOffer[]>("lan_collab_list_remote_shared_models");
}

export function lanCollabShareKnowledge(params: {
  title: string;
  groupId?: string | null;
  sourceGroup?: string | null;
  domain?: string | null;
  docIds?: string[];
}) {
  return invoke<SharedKnowledgeOffer>("lan_collab_share_knowledge", {
    title: params.title,
    groupId: params.groupId ?? null,
    sourceGroup: params.sourceGroup ?? null,
    domain: params.domain ?? null,
    docIds: params.docIds ?? [],
  });
}

export function lanCollabUnshareKnowledge(shareId: string) {
  return invoke<void>("lan_collab_unshare_knowledge", { shareId });
}

export function lanCollabListLocalSharedKnowledge() {
  return invoke<SharedKnowledgeOffer[]>("lan_collab_list_local_shared_knowledge");
}

export function lanCollabListRemoteSharedKnowledge() {
  return invoke<SharedKnowledgeOffer[]>("lan_collab_list_remote_shared_knowledge");
}

export function lanCollabListShareableKnowledgeDocs(params?: {
  sourceGroup?: string | null;
  domain?: string | null;
}) {
  return invoke<SharedKnowledgeDocMeta[]>("lan_collab_list_shareable_knowledge_docs", {
    sourceGroup: params?.sourceGroup ?? null,
    domain: params?.domain ?? null,
  });
}

export function lanCollabSearchRemoteKnowledge(params: {
  hostNodeId: string;
  shareId: string;
  query: string;
  topK?: number;
}) {
  return invoke<RemoteKnowledgeHit[]>("lan_collab_search_remote_knowledge", {
    hostNodeId: params.hostNodeId,
    shareId: params.shareId,
    query: params.query,
    topK: params.topK ?? 8,
  });
}

export function lanCollabFetchRemoteKnowledge(params: {
  hostNodeId: string;
  shareId: string;
  docId: string;
}) {
  return invoke<RemoteKnowledgeDoc>("lan_collab_fetch_remote_knowledge", {
    hostNodeId: params.hostNodeId,
    shareId: params.shareId,
    docId: params.docId,
  });
}

export function lanCollabShareSkill(params: {
  skillId: string;
  groupId?: string | null;
}) {
  return invoke<SharedSkillOffer>("lan_collab_share_skill", {
    skillId: params.skillId,
    groupId: params.groupId ?? null,
  });
}

export function lanCollabUnshareSkill(shareId: string) {
  return invoke<void>("lan_collab_unshare_skill", { shareId });
}

export function lanCollabListLocalSharedSkills() {
  return invoke<SharedSkillOffer[]>("lan_collab_list_local_shared_skills");
}

export function lanCollabListRemoteSharedSkills() {
  return invoke<SharedSkillOffer[]>("lan_collab_list_remote_shared_skills");
}

export function lanCollabInstallRemoteSkill(params: {
  hostNodeId: string;
  shareId: string;
  overwrite?: boolean;
  forceOverwrite?: boolean;
}) {
  return invoke<string>("lan_collab_install_remote_skill", {
    hostNodeId: params.hostNodeId,
    shareId: params.shareId,
    overwrite: params.overwrite ?? false,
    forceOverwrite: params.forceOverwrite ?? false,
  });
}

export function lanCollabShareWorkflow(params: {
  workflowName: string;
  groupId?: string | null;
}) {
  return invoke<SharedWorkflowOffer>("lan_collab_share_workflow", {
    workflowName: params.workflowName,
    groupId: params.groupId ?? null,
  });
}

export function lanCollabUnshareWorkflow(shareId: string) {
  return invoke<void>("lan_collab_unshare_workflow", { shareId });
}

export function lanCollabListLocalSharedWorkflows() {
  return invoke<SharedWorkflowOffer[]>("lan_collab_list_local_shared_workflows");
}

export function lanCollabListRemoteSharedWorkflows() {
  return invoke<SharedWorkflowOffer[]>("lan_collab_list_remote_shared_workflows");
}

export function lanCollabInstallRemoteWorkflow(params: {
  hostNodeId: string;
  shareId: string;
  overwrite?: boolean;
  forceOverwrite?: boolean;
  installAs?: string | null;
}) {
  return invoke<string>("lan_collab_install_remote_workflow", {
    hostNodeId: params.hostNodeId,
    shareId: params.shareId,
    overwrite: params.overwrite ?? false,
    forceOverwrite: params.forceOverwrite ?? false,
    installAs: params.installAs ?? null,
  });
}

export function lanCollabListWorkflowShareOrigins() {
  return invoke<WorkflowOriginSummary[]>("lan_collab_list_workflow_share_origins");
}

/** 根据共享 offer 生成接收方 OpenAI 兼容 baseUrl。 */
export function sharedModelBaseUrl(offer: SharedModelOffer): string {
  const host = (offer.hostAddress || "").split(":")[0] || offer.hostAddress;
  return `http://${host}:${offer.proxyPort}/v1`;
}

/** 解析 `host:port` 或 `host port` 输入。 */
export function parseHostPort(input: string): { host: string; port: number } | null {
  const raw = input.trim();
  if (!raw) return null;

  // IPv6: [addr]:port
  if (raw.startsWith("[")) {
    const end = raw.indexOf("]");
    if (end > 1 && raw[end + 1] === ":") {
      const host = raw.slice(1, end);
      const port = Number(raw.slice(end + 2));
      if (host && Number.isInteger(port) && port > 0 && port <= 65535) {
        return { host, port };
      }
    }
    return null;
  }

  const colon = raw.lastIndexOf(":");
  if (colon > 0) {
    const host = raw.slice(0, colon).trim();
    const port = Number(raw.slice(colon + 1).trim());
    if (host && Number.isInteger(port) && port > 0 && port <= 65535) {
      return { host, port };
    }
  }

  const parts = raw.split(/\s+/);
  if (parts.length === 2) {
    const host = parts[0];
    const port = Number(parts[1]);
    if (host && Number.isInteger(port) && port > 0 && port <= 65535) {
      return { host, port };
    }
  }

  return null;
}
