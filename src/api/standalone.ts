import { invoke } from "@tauri-apps/api/core";
import type { BinaryAttachedFile } from "../types/provider";

export interface ServerStatus {
  initialized: boolean;
  currentThreadId: string | null;
  cwd: string;
  locale: string;
  configDir: string;
  configPath: string;
}

export async function getServerStatus(): Promise<ServerStatus> {
  return invoke("get_server_status");
}

export async function standaloneInit(): Promise<string> {
  return invoke("standalone_init");
}

export async function standaloneConfigRead(): Promise<{
  config: Record<string, unknown>;
  filePath: string;
}> {
  return invoke("standalone_config_read");
}

export async function standaloneConfigWrite(
  edits: Array<{ keyPath: string; value: unknown; mergeStrategy?: string }>,
): Promise<{ status: string; filePath: string }> {
  return invoke("standalone_config_write", { edits });
}

export interface RemoteProviderModel {
  id: string;
  label: string;
  supportsVision: boolean;
  contextLength?: number;
  maxOutputTokens?: number;
}

export interface FetchProviderModelsResult {
  supported: boolean;
  models: RemoteProviderModel[];
}

export interface ProviderCapabilityProbeResult {
  success: boolean;
  cached: boolean;
  fingerprint: string;
  probedAt: number;
  latencyMs?: number;
  capabilities: {
    structuredTools: boolean | null;
    streaming: boolean | null;
    reasoning: boolean | null;
    usage: boolean | null;
    parallelToolCalls: boolean | null;
    recommendedWireApi?: string;
  };
  error?: string;
}

export async function probeModelCapabilities(params: {
  baseUrl: string;
  apiKey: string;
  model: string;
  wireApi: string;
  providerKey?: string;
  forceRefresh?: boolean;
}): Promise<ProviderCapabilityProbeResult> {
  return invoke("probe_model_capabilities", params);
}

export async function fetchProviderModels(params: {
  baseUrl: string;
  apiKey: string;
  wireApi: string;
}): Promise<FetchProviderModelsResult> {
  return invoke("fetch_provider_models", params);
}

export interface PlaywrightMcpEnableResult {
  status: string;
  filePath: string;
  serverName: string;
  configured: boolean;
  installStarted: boolean;
  installStatus: "succeeded" | "failed";
  detail?: string | null;
  error?: string | null;
}

export async function standaloneMcpEnablePlaywright(): Promise<PlaywrightMcpEnableResult> {
  return invoke("standalone_mcp_enable_playwright");
}

export async function standaloneThreadCreate(): Promise<{
  thread: { id: string };
}> {
  return invoke("standalone_thread_create");
}

export async function standaloneThreadList(): Promise<{
  data: Array<{
    id: string;
    name?: string;
    preview?: string;
    updatedAt?: number;
  }>;
}> {
  return invoke("standalone_thread_list");
}

export async function threadArchive(threadId: string): Promise<{ status: string }> {
  return invoke("thread_archive", { params: { threadId } });
}

export async function standaloneThreadPeekGoal(
  threadId: string,
): Promise<{ goal: ThreadGoal | null }> {
  return invoke("standalone_thread_peek_goal", { threadId });
}

export async function standaloneThreadRead(threadId: string): Promise<{
  thread: {
    id: string;
    name?: string;
    goal?: ThreadGoal | null;
    robotState?: RobotWorkflowProgress | null;
    activePlan?: {
      path: string;
      content: string;
      revision?: number;
      updatedAt?: number;
    } | null;
    turns?: Array<{
      id: string;
      items?: Array<{
        type: string;
        id?: string;
        text?: string;
        content?: Array<{ type?: string; text?: string }>;
      }>;
      startedAt?: number;
      completedAt?: number;
      mode?: "chat" | "plan" | "goal";
      durationMs?: number;
      changedFiles?: Array<{ path: string; action: string }>;
      changedFileSnapshots?: Array<{
        path: string;
        action: string;
        beforeContent?: string;
        afterContent?: string;
      }>;
      usage?: {
        promptTokens: number;
        completionTokens: number;
        totalTokens: number;
        callCount?: number;
        lastSinglePromptTokens?: number;
      };
      goalBudgetTokens?: number;
      budgetLimited?: boolean;
    }>;
  };
}> {
  return invoke("standalone_thread_read", { threadId });
}

export type ThreadGoalStatus =
  | "active"
  | "paused"
  | "blocked"
  | "usageLimited"
  | "budgetLimited"
  | "complete";

export interface RobotWorkflowProgress {
  robotId?: string;
  currentNodeIndex: number;
  rootObjective?: string;
  runtimeNodes: string[];
  nodeDeliveries?: string[];
  completed?: boolean;
}

export interface ThreadGoal {
  objective: string;
  status: ThreadGoalStatus;
  tokenBudget?: number | null;
  tokensUsed?: number;
  createdAt?: number;
  updatedAt?: number;
  workflowProgress?: RobotWorkflowProgress;
}

export async function standaloneThreadGoalSet(
  threadId: string,
  objective: string,
  status?: ThreadGoalStatus,
  goalBudgetTokens?: number,
): Promise<{ goal: ThreadGoal }> {
  return invoke("standalone_thread_goal_set", {
    threadId,
    objective,
    status,
    goalBudgetTokens,
  });
}

export async function standaloneThreadGoalStatus(
  threadId: string,
  status: ThreadGoalStatus,
): Promise<{ goal: ThreadGoal }> {
  return invoke("standalone_thread_goal_status", {
    threadId,
    status,
  });
}

export async function standaloneThreadGoalEdit(
  threadId: string,
  objective: string,
  goalBudgetTokens?: number,
): Promise<{ goal: ThreadGoal }> {
  return invoke("standalone_thread_goal_edit", {
    threadId,
    objective,
    goalBudgetTokens,
  });
}

export async function standaloneThreadGoalClear(
  threadId: string,
): Promise<{ goal: null }> {
  return invoke("standalone_thread_goal_clear", { threadId });
}

export interface StandaloneChatOverrides {
  provider?: StandaloneChatProviderOverride | null;
  smartbrainEnabled?: boolean | null;
}

/** 仅本对话生效的供应商/模型覆盖快照（不写全局 config.toml） */
export interface StandaloneChatProviderOverride {
  providerKey: string;
  baseUrl?: string | null;
  apiKey?: string | null;
  wireApi?: string | null;
  requiresOpenAIAuth?: boolean | null;
  modelId?: string | null;
  modelContextWindow?: number | null;
  maxOutputTokens?: number | null;
  modelSupportsVision?: boolean | null;
  visionFallbackKind?: string | null;
  visionFallbackProvider?: string | null;
  visionFallbackModel?: string | null;
  modelEndpoints?: Array<{
    url: string;
    label?: string;
    model?: string;
    apiKey?: string;
    wireApi?: string;
  }> | null;
  activeEndpointIndex?: number | null;
}

export async function standaloneChat(
  threadId: string,
  message: string,
  cwd?: string,
  mode?: "chat" | "plan" | "goal" | "robot-create" | "robot-modify",
  attachments?: BinaryAttachedFile[],
  goalBudgetTokens?: number,
  robotId?: string,
  overrides?: StandaloneChatOverrides,
  clientMessageId?: string,
): Promise<{ status: string }> {
  return invoke("standalone_chat", {
    threadId,
    message,
    cwd,
    mode,
    attachments,
    goalBudgetTokens,
    robotId,
    provider: overrides?.provider ?? null,
    smartbrainEnabled: overrides?.smartbrainEnabled ?? null,
    clientMessageId: clientMessageId ?? null,
  });
}

/** 编辑重发：删除指定用户消息及其后的所有回复。 */
export async function standaloneThreadTruncateBefore(
  threadId: string,
  messageId: string,
  messageContent?: string,
): Promise<{ status: string; keptCount: number }> {
  return invoke("standalone_thread_truncate_before", {
    threadId,
    messageId,
    messageContent: messageContent ?? null,
  });
}

export async function standaloneTurnInterrupt(
  threadId?: string | null,
): Promise<{ status: string }> {
  return invoke("standalone_turn_interrupt", {
    threadId: threadId ?? null,
  });
}

export async function standalonePlanOpen(path: string): Promise<{ status: string }> {
  return invoke("standalone_plan_open", { path });
}

export interface FortuneDetailStreamStartParams {
  baseUrl: string;
  apiKey: string;
  model: string;
  wireApi: string;
  prompt: string;
  requestId?: string;
}

export async function fortuneDetailStreamStart(
  params: FortuneDetailStreamStartParams,
): Promise<{ requestId: string }> {
  return invoke("fortune_detail_stream_start", { ...params });
}
