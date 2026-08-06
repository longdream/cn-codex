import { create } from "zustand";
import {
  standaloneConfigWrite,
  standaloneThreadCreate,
  standaloneThreadList,
  standaloneThreadRead,
  threadArchive,
} from "../api";
import type {
  ThreadGoal as ApiThreadGoal,
  ThreadGoalStatus,
} from "../api";
import type {
  AttachedFile,
  BinaryAttachedFile,
  PoolModelEndpoint,
  ProviderConfig,
  ProviderModel,
  ProviderPreset,
  VisionFallbackKind,
} from "../types/provider";
import type { ActiveSubagentView } from "../utils/subagentStatus";
import {
  normalizeReportedFileChanges,
  normalizeReportedFilePath,
} from "../utils/reportedFilePath";

export const GENERAL_PROJECT_ID = "__general__";

export type ChatMode = "chat" | "plan" | "goal";
export type GoalStatus = ThreadGoalStatus;
export type ThreadGoal = ApiThreadGoal;

/** 对话框可选的推理强度级别（写入 config.toml model_reasoning_effort） */
export const REASONING_EFFORT_OPTIONS = [
  "none",
  "minimal",
  "low",
  "medium",
  "high",
  "xhigh",
] as const;
export type ReasoningEffortLevel = (typeof REASONING_EFFORT_OPTIONS)[number];

export function normalizeReasoningEffort(value: unknown): ReasoningEffortLevel {
  if (typeof value !== "string") {
    return "medium";
  }
  const trimmed = value.trim().toLowerCase();
  if (!trimmed) {
    return "medium";
  }
  switch (trimmed) {
    case "none":
    case "off":
    case "disable":
    case "disabled":
    case "false":
    case "0":
      return "none";
    case "minimal":
    case "min":
      return "minimal";
    case "low":
      return "low";
    case "medium":
    case "med":
    case "default":
    case "normal":
      return "medium";
    case "high":
      return "high";
    case "xhigh":
    case "x-high":
    case "extra_high":
    case "extra-high":
    case "max":
    case "highest":
      return "xhigh";
    default:
      return "medium";
  }
}

/** 机器人提问倒计时等待状态 */
export interface RobotWaitCountdown {
  /** 等待的唯一标识，对应后端 callId */
  callId: string;
  /** 关联的线程 ID */
  threadId: string;
  /** 总倒计时毫秒数 */
  countdownMs: number;
  /** 倒计时开始的时间戳（Date.now()） */
  startedAt: number;
  /** AI 提出的问题文本 */
  assistantText: string;
}

export interface ChatSendOptions {
  goalBudgetTokens?: number;
}

interface StreamingConvergeOptions {
  commitStreamingText?: boolean;
}

export interface FileChange {
  path: string;
  action: string;
}

export interface FileChangeSnapshot {
  path: string;
  action: string;
  beforeContent?: string;
  afterContent?: string;
}

export interface PatchProgressChange {
  path: string;
  action: string;
  moveTo?: string;
  additions?: number;
  deletions?: number;
}

export interface TokenUsage {
  promptTokens: number;
  completionTokens: number;
  totalTokens: number;
  // 缓存命中 token 数
  cachedTokens?: number;
  // 缓存写入 token 数
  cacheCreationTokens?: number;
  // 思考（reasoning）token 数
  reasoningTokens?: number;
  callCount?: number;
  lastSinglePromptTokens?: number;
  // 上下文窗口大小（tokens）。用于让前端展示口径与后端运行时配置保持一致。
  contextWindowTokens?: number;
}

export interface PendingFileReviewFile {
  path: string;
  action: string;
  moveTo?: string;
  baseContent?: string;
  candidateContent?: string;
  editedContent?: string;
  keep: boolean;
}

export interface PendingFileReview {
  threadId: string;
  callId: string;
  rawPatch: string;
  createdAtMs: number;
  updatedAtMs: number;
  files: PendingFileReviewFile[];
  selectedPath: string | null;
  keepAll: boolean;
  status: "pending" | "applying" | "applied" | "cancelled" | "failed";
  error?: string;
}

export interface RunSummary {
  turnId: string;
  mode: ChatMode;
  cwd?: string;
  startedAt?: number;
  completedAt?: number;
  durationMs?: number;
  changedFiles: FileChange[];
  changedFileSnapshots?: FileChangeSnapshot[];
  usage?: TokenUsage;
  goalBudgetTokens?: number;
  budgetLimited?: boolean;
}

export interface ToolCallItem {
  id: string;
  name: string;
  arguments: string;
  status: "running" | "success" | "failed";
  displayLabel: string;
  output?: string;
  patchProgress?: PatchProgressChange[];
}

export interface PlanFile {
  path: string;
  content: string;
  revision?: number;
  updatedAt?: number;
  synthetic?: boolean;
}

export interface ChatMessage {
  id: string;
  role: "user" | "assistant" | "system";
  content: string;
  timestamp: number;
  attachments?: BinaryAttachedFile[];
  toolCalls?: ToolCallItem[];
  commandStatus?: string;
  fileChanges?: { path: string; action: string }[];
  runSummary?: RunSummary;
  planFile?: PlanFile;
}

/** 按 id 合并工具调用列表，保留既有顺序，新项追加到末尾。 */
export function mergeToolCallItems(
  existing: ToolCallItem[],
  incoming: ToolCallItem[],
): ToolCallItem[] {
  if (incoming.length === 0) {
    return existing.slice();
  }
  const merged = existing.slice();
  const indexById = new Map(merged.map((item, index) => [item.id, index]));
  for (const item of incoming) {
    const existingIndex = indexById.get(item.id);
    if (existingIndex === undefined) {
      indexById.set(item.id, merged.length);
      merged.push(item);
      continue;
    }
    const prev = merged[existingIndex];
    merged[existingIndex] = {
      ...prev,
      ...item,
      // 已有输出优先保留，除非新项显式提供 output。
      output: item.output !== undefined ? item.output : prev.output,
      patchProgress: item.patchProgress ?? prev.patchProgress,
    };
  }
  return merged;
}

/**
 * 查找当前用户轮次内可继续追加的工具调用卡片。
 * 仅当“最近一条 user 之后的最后一条消息”本身就是工具卡时才复用，
 * 避免把后续工具合并到中间文本之前的旧卡片上。
 */
export function findOpenToolCallGroupIndex(messages: ChatMessage[]): number {
  let lastUserIdx = -1;
  for (let i = messages.length - 1; i >= 0; i--) {
    if (messages[i].role === "user") {
      lastUserIdx = i;
      break;
    }
  }
  const lastIdx = messages.length - 1;
  if (lastIdx <= lastUserIdx) return -1;
  const last = messages[lastIdx];
  if (last.toolCalls && last.toolCalls.length > 0) {
    return lastIdx;
  }
  return -1;
}

/**
 * 将同一用户轮次内“连续”的多张工具调用卡合并为一张。
 * 若中间插入了助手文本/摘要等内容，则保留时间线，不跨内容合并。
 */
export function collapseToolCallCardsPerUserTurn(messages: ChatMessage[]): ChatMessage[] {
  const result: ChatMessage[] = [];
  let currentTurnToolMsgIdx = -1;

  for (const message of messages) {
    if (message.role === "user") {
      result.push(message);
      currentTurnToolMsgIdx = -1;
      continue;
    }

    if (message.toolCalls && message.toolCalls.length > 0) {
      if (currentTurnToolMsgIdx >= 0) {
        const existing = result[currentTurnToolMsgIdx];
        result[currentTurnToolMsgIdx] = {
          ...existing,
          toolCalls: mergeToolCallItems(existing.toolCalls ?? [], message.toolCalls),
        };
        continue;
      }
      currentTurnToolMsgIdx = result.length;
      result.push(message);
      continue;
    }

    // 非工具内容打断当前开放工具组，后续工具卡应出现在该内容之后。
    if (shouldBreakToolCallGroup(message)) {
      currentTurnToolMsgIdx = -1;
    }

    result.push(message);
  }

  return result;
}

/** 判断消息是否应打断连续工具卡合并。 */
function shouldBreakToolCallGroup(message: ChatMessage): boolean {
  if (message.runSummary || message.planFile) return true;
  if (message.role === "assistant") return true;
  if (message.role === "system" && message.content.trim().length > 0) return true;
  return false;
}

export interface ThreadSummary {
  id: string;
  name?: string;
  preview: string;
  updatedAt: number;
  projectId?: string;
}

export interface Project {
  id: string;
  name: string;
  cwd: string;
}

export interface ModelEntry {
  id: string;
  provider: string;
  model: string;
  label: string;
  supportsVision: boolean;
}

export interface QueuedMessage {
  id: string;
  text: string;
  mode: ChatMode;
  attachments: BinaryAttachedFile[];
  options?: {
    goalBudgetTokens?: number;
    robotId?: string;
    robotCreateMode?: boolean;
    robotModifyMode?: boolean;
  };
  timestamp: number;
}

/**
 * 单个对话的运行时状态快照。
 *
 * 顶层 store 字段始终反映"活跃线程"的视图，而该结构用于保存非活跃
 * （后台）线程的对话上下文，使切换对话时不丢失队列、流式状态、消息等。
 */
export interface ThreadRuntimeState {
  messages: ChatMessage[];
  streamingText: string;
  streamingLabel: string;
  isStreaming: boolean;
  liveTurnUsage: TokenUsage | null;
  currentTurnId: string | null;
  pendingMessageQueue: QueuedMessage[];
  pendingFileReviews: Record<string, PendingFileReview>;
  currentGoal: ThreadGoal | null;
  activePlan: PlanFile | null;
  latestPlanContent: string | null;
  chatMode: ChatMode;
  selectedRobotId: string | null;
  robotCreateMode: boolean;
  robotWaitCountdown: RobotWaitCountdown | null;
  /** 当前线程的实时子智能体状态（subagent-status 事件） */
  liveSubagents: Record<string, ActiveSubagentView>;
  reasoningText: string;
  /** 浏览器状态 — 每个对话独立维护 */
  browserPanelUrl: string | null;
  browserPanelTitle: string | null;
  browserPanelStatus: "idle" | "running" | "success" | "failed";
  browserCanGoBack: boolean;
  browserCanGoForward: boolean;
  browserActive: boolean;
  browserDetached: boolean;
  /** 本对话覆盖的供应商 ID；null 表示继承全局 activeProviderId */
  overrideProviderId: string | null;
  /** 本对话覆盖的模型 ID；null 表示继承全局 */
  overrideModelId: string | null;
  /** 本对话是否启用本地知识库 */
  smartbrainEnabled: boolean;
  /** 本对话是否启用子智能体工具 */
  subagentEnabled: boolean;
  updatedAt: number;
}

/** 后台线程运行时状态映射的最大条目数，超出时淘汰最旧条目。 */
const MAX_THREAD_RUNTIME_STATES = 20;

function createDefaultThreadRuntimeState(): ThreadRuntimeState {
  return {
    messages: [],
    streamingText: "",
    streamingLabel: "",
    isStreaming: false,
    liveTurnUsage: null,
    currentTurnId: null,
    pendingMessageQueue: [],
    pendingFileReviews: {},
    currentGoal: null,
    activePlan: null,
    latestPlanContent: null,
    chatMode: "chat",
    selectedRobotId: null,
    robotCreateMode: false,
    robotWaitCountdown: null,
    liveSubagents: {},
    reasoningText: "",
    browserPanelUrl: null,
    browserPanelTitle: null,
    browserPanelStatus: "idle",
    browserCanGoBack: false,
    browserCanGoForward: false,
    browserActive: false,
    browserDetached: false,
    overrideProviderId: null,
    overrideModelId: null,
    smartbrainEnabled: false,
    subagentEnabled: false,
    updatedAt: Date.now(),
  };
}

export type RightPanelTab = "browser" | "project" | "terminal" | "git" | "miniapp";
export type SidebarTab = "chats" | "projects";
export interface SmartbrainExtractionProgress {
  current: number;
  total: number;
}

export interface ImageGenerationSettings {
  enabled: boolean;
  model: string;
  baseUrl: string;
  apiKey: string;
}

// 左侧栏宽度边界：保持目录可读，并避免过宽挤占聊天区。
export const SIDEBAR_WIDTH_MIN = 220;
export const SIDEBAR_WIDTH_MAX = 520;
export const DEFAULT_SIDEBAR_WIDTH = 256;

// 右侧栏宽度边界：支持 Git/终端面板信息展示，同时控制总体占比。
export const RIGHT_PANEL_WIDTH_MIN = 320;
export const RIGHT_PANEL_WIDTH_MAX = 760;
export const DEFAULT_RIGHT_PANEL_WIDTH = 384;
export const DEFAULT_MODEL_CONTEXT_LENGTH = 128000;
export const DEFAULT_MODEL_MAX_OUTPUT_TOKENS = 65535;
export const DEFAULT_IMAGE_GENERATION_MODEL = "gpt-image-2";
export const DEFAULT_IMAGE_GENERATION_BASE_URL = "https://api.openai.com/v1";
const LAYOUT_SNAPSHOT_STORAGE_KEY = "cn-codex:layout-snapshot";
export const VISION_FALLBACK_KIND_MULTIMODAL: VisionFallbackKind = "multimodal";
export const VISION_FALLBACK_KIND_LOCAL_OCR: VisionFallbackKind = "local_ocr";

interface LayoutSnapshot {
  sidebarWidth?: number;
  rightPanelWidth?: number;
}

function readLayoutSnapshotFromStorage(): LayoutSnapshot | null {
  if (typeof window === "undefined") {
    return null;
  }
  try {
    const raw = window.localStorage.getItem(LAYOUT_SNAPSHOT_STORAGE_KEY);
    if (!raw) {
      return null;
    }
    const parsed = JSON.parse(raw) as LayoutSnapshot;
    return parsed && typeof parsed === "object" ? parsed : null;
  } catch {
    return null;
  }
}

function writeLayoutSnapshotToStorage(next: LayoutSnapshot): void {
  if (typeof window === "undefined") {
    return;
  }
  try {
    window.localStorage.setItem(LAYOUT_SNAPSHOT_STORAGE_KEY, JSON.stringify(next));
  } catch {
    // Ignore localStorage failures and keep SQLite as source of truth.
  }
}

function mergeLayoutSnapshotToStorage(patch: LayoutSnapshot): void {
  const current = readLayoutSnapshotFromStorage() ?? {};
  writeLayoutSnapshotToStorage({
    ...current,
    ...patch,
  });
}

const initialLayoutSnapshot = readLayoutSnapshotFromStorage();

export function normalizeImageGenerationSettings(
  value?: Partial<ImageGenerationSettings> | null,
): ImageGenerationSettings {
  const enabled = typeof value?.enabled === "boolean" ? value.enabled : true;
  const model = typeof value?.model === "string" ? value.model.trim() : "";
  const baseUrl = typeof value?.baseUrl === "string" ? value.baseUrl.trim() : "";
  const apiKey = typeof value?.apiKey === "string" ? value.apiKey.trim() : "";
  return {
    enabled,
    model: model || DEFAULT_IMAGE_GENERATION_MODEL,
    baseUrl: (baseUrl || DEFAULT_IMAGE_GENERATION_BASE_URL).replace(/\/+$/, ""),
    apiKey,
  };
}

interface RawToolCallInfo {
  id: string;
  name: string;
  arguments: string;
}

interface RawThreadItem {
  type: string;
  id?: string;
  text?: string;
  content?: Array<{ type?: string; text?: string }>;
  attachments?: Array<{
    name?: string;
    type?: string;
    dataUrl?: string;
    size?: number;
  }>;
  toolName?: string;
  toolCallId?: string;
  calls?: RawToolCallInfo[];
}

interface RawTurn {
  id: string;
  items?: RawThreadItem[];
  cwd?: string | null;
  startedAt?: number | null;
  completedAt?: number | null;
  mode?: ChatMode | null;
  durationMs?: number | null;
  changedFiles?: FileChange[];
  changedFileSnapshots?: FileChangeSnapshot[];
  usage?: TokenUsage | null;
  goalBudgetTokens?: number | null;
  budgetLimited?: boolean | null;
}

interface RawThread {
  id: string;
  name?: string;
  goal?: ApiThreadGoal | null;
  robotState?: ApiThreadGoal["workflowProgress"] | null;
  activePlan?: {
    path?: string;
    content?: string;
    revision?: number | null;
    updatedAt?: number | null;
  } | null;
  turns?: RawTurn[];
}

function toMillis(value?: number | null): number {
  if (!value) {
    return Date.now();
  }
  return value > 10_000_000_000 ? value : value * 1000;
}

function toolDisplayLabelFromArgs(name: string, args: string): string {
  if (name === "apply_patch") {
    const path = firstPatchPath(args);
    if (path) {
      return path;
    }
  }

  try {
    const parsed = JSON.parse(args);
    if (name.startsWith("mcp__")) {
      return mcpDirectDisplayLabel(name);
    }
    switch (name) {
      case "shell":
      case "shell_command":
        return Array.isArray(parsed.command) ? parsed.command.join(" ") : String(parsed.command ?? name);
      case "exec_command":
        return String(parsed.cmd ?? "exec_command");
      case "write_stdin":
      case "close_exec_session":
        return parsed.session_id != null ? `session ${parsed.session_id}` : name;
      case "read_file":
        return parsed.path ?? "read_file";
      case "write_file":
        return parsed.path ?? "write_file";
      case "tool_search":
        return parsed.query ?? "tool_search";
      case "code_review":
        return parsed.base_ref
          ? `vs ${parsed.base_ref}`
          : Array.isArray(parsed.paths) && parsed.paths.length > 0
            ? `${parsed.paths.length} paths`
            : "working tree";
      case "apply_patch":
        return firstPatchPath(parsed.patch ?? parsed.command) ?? "apply_patch";
      case "list_directory":
        return parsed.path ?? ".";
      case "update_plan":
        return Array.isArray(parsed.plan) ? `${parsed.plan.length} steps` : "update_plan";
      case "request_user_input":
        return Array.isArray(parsed.questions) ? `${parsed.questions.length} question(s)` : "request_user_input";
      case "request_permissions":
        return parsed.reason ?? "permissions";
      case "view_image":
        return parsed.path ?? "view_image";
      case "ocr_image":
        return parsed.path ?? "ocr_image";
      case "image_generate":
        return parsed.output_path ?? promptPreview(parsed.prompt) ?? "image_generate";
      case "echarts_report": {
        const title = typeof parsed.title === "string" ? parsed.title.trim() : "";
        if (title) {
          return title;
        }
        const chartType = typeof parsed.chart_type === "string" ? parsed.chart_type.trim() : "";
        return chartType || "echarts_report";
      }
      case "memory_list":
        return parsed.path ?? ".";
      case "memory_read":
      case "memory_write":
      case "memory_update":
      case "memory_forget":
        return parsed.path ?? name;
      case "memory_search":
        return parsed.query ?? "memory_search";
      case "mcp_list_servers":
        return "MCP servers";
      case "mcp_status":
        return parsed.server ?? "all";
      case "mcp_list_tools":
      case "mcp_list_resources":
      case "mcp_list_resource_templates":
      case "mcp_list_prompts":
        return parsed.server ?? "all";
      case "mcp_call_tool":
        return parsed.server && parsed.tool ? `${parsed.server}:${parsed.tool}` : "mcp_call_tool";
      case "mcp_read_resource":
        return parsed.server && parsed.uri ? `${parsed.server}:${parsed.uri}` : "mcp_read_resource";
      case "mcp_get_prompt":
        return parsed.server && parsed.prompt ? `${parsed.server}:${parsed.prompt}` : "mcp_get_prompt";
      case "apps_list":
        return parsed.connector_id ?? "all";
      case "list_available_plugins_to_install":
        return parsed.query ?? "plugins";
      case "request_plugin_install":
        return parsed.tool_id ?? parsed.name ?? "plugin";
      case "plugin_manage":
        return parsed.plugin_id ?? parsed.id ?? parsed.action ?? "plugins";
      case "browser_run": {
        const url = typeof parsed.url === "string" && parsed.url.trim() ? parsed.url : "browser";
        const actionCount = Array.isArray(parsed.actions) ? parsed.actions.length : 0;
        return actionCount > 0 ? `${url} (${actionCount} actions)` : url;
      }
      case "spawn_agent":
        return parsed.role ?? promptPreview(parsed.prompt) ?? "agent";
      case "wait_agent":
        return parsed.agent_id ?? (Array.isArray(parsed.agent_ids) ? `${parsed.agent_ids.length} agents` : "agents");
      case "send_input":
        return parsed.target ?? parsed.agent_id ?? parsed.id ?? "agent";
      case "resume_agent":
        return parsed.id ?? parsed.target ?? parsed.agent_id ?? "agent";
      case "list_agents":
        return parsed.status ?? "agents";
      case "close_agent":
        return parsed.target ?? parsed.agent_id ?? parsed.id ?? "agent";
      case "web_search":
        return parsed.query ?? "web_search";
      case "web_fetch":
        return parsed.url ?? "web_fetch";
      default:
        return name;
    }
  } catch {
    if (name === "apply_patch") {
      return firstPatchPath(args) ?? "apply_patch";
    }
    return name.startsWith("mcp__") ? mcpDirectDisplayLabel(name) : name;
  }
}

function promptPreview(value: unknown): string | null {
  if (typeof value !== "string") {
    return null;
  }
  const normalized = value.replace(/\s+/g, " ").trim();
  if (!normalized) {
    return null;
  }
  return normalized.length > 48 ? `${normalized.slice(0, 48)}...` : normalized;
}

function mcpDirectDisplayLabel(name: string): string {
  const parts = name.slice("mcp__".length).split("__");
  return parts.length >= 2 ? `${parts[0]}:${parts.slice(1).join("__")}` : name;
}

function firstPatchPath(value: unknown): string | null {
  if (typeof value !== "string") {
    return null;
  }

  for (const line of value.split(/\r?\n/)) {
    const match = line.match(/^\*\*\* (?:Add|Update|Delete) File: (.+)$/);
    if (match) {
      return match[1].trim();
    }
  }

  return null;
}

export function createRunSummaryMessage(summary: RunSummary): ChatMessage {
  return {
    id: `summary-${summary.turnId}-${crypto.randomUUID()}`,
    role: "system",
    content: "",
    timestamp: summary.completedAt ?? Date.now(),
    runSummary: summary,
  };
}

function normalizeRunSummary(turn: RawTurn): RunSummary | null {
  if (turn.durationMs == null && !(turn.changedFiles?.length) && turn.mode !== "goal") {
    return null;
  }

  return {
    turnId: turn.id,
    mode: turn.mode === "goal" ? "goal" : turn.mode === "plan" ? "plan" : "chat",
    cwd: turn.cwd ?? undefined,
    startedAt: turn.startedAt ? toMillis(turn.startedAt) : undefined,
    completedAt: turn.completedAt ? toMillis(turn.completedAt) : undefined,
    durationMs: turn.durationMs ?? undefined,
    changedFiles: normalizeReportedFileChanges(turn.changedFiles),
    changedFileSnapshots: normalizeFileChangeSnapshots(turn.changedFileSnapshots),
    usage: normalizeTokenUsage(turn.usage),
    goalBudgetTokens: normalizeTokenBudget(turn.goalBudgetTokens),
    budgetLimited: Boolean(turn.budgetLimited),
  };
}

function normalizeFileChangeSnapshots(
  snapshots?: FileChangeSnapshot[] | null,
): FileChangeSnapshot[] | undefined {
  if (!Array.isArray(snapshots) || snapshots.length === 0) {
    return undefined;
  }

  const normalized = snapshots
    .map((snapshot) => ({
      path: normalizeReportedFilePath(String(snapshot.path ?? "").trim()),
      action: String(snapshot.action ?? "modified").trim() || "modified",
      beforeContent:
        typeof snapshot.beforeContent === "string" ? snapshot.beforeContent : undefined,
      afterContent:
        typeof snapshot.afterContent === "string" ? snapshot.afterContent : undefined,
    }))
    .filter((snapshot) => snapshot.path.length > 0);

  return normalized.length > 0 ? normalized : undefined;
}

function normalizeTokenUsage(usage?: TokenUsage | null): TokenUsage | undefined {
  if (!usage) {
    return undefined;
  }

  const promptTokens = Number(usage.promptTokens ?? 0);
  const completionTokens = Number(usage.completionTokens ?? 0);
  const totalTokens = Number(usage.totalTokens ?? promptTokens + completionTokens);
  const callCount = Number(usage.callCount ?? 0);
  const lastSinglePromptTokens = Number(usage.lastSinglePromptTokens ?? 0);
  const contextWindowTokens = Number(usage.contextWindowTokens ?? 0);
  const cachedTokens = Number(usage.cachedTokens ?? 0);
  const cacheCreationTokens = Number(usage.cacheCreationTokens ?? 0);
  const reasoningTokens = Number(usage.reasoningTokens ?? 0);
  if (
    promptTokens <= 0 &&
    completionTokens <= 0 &&
    totalTokens <= 0 &&
    callCount <= 0 &&
    lastSinglePromptTokens <= 0 &&
    contextWindowTokens <= 0
  ) {
    return undefined;
  }

  return {
    promptTokens: Math.max(0, promptTokens),
    completionTokens: Math.max(0, completionTokens),
    totalTokens: Math.max(0, totalTokens),
    ...(cachedTokens > 0 ? { cachedTokens: Math.max(0, Math.round(cachedTokens)) } : {}),
    ...(cacheCreationTokens > 0
      ? { cacheCreationTokens: Math.max(0, Math.round(cacheCreationTokens)) }
      : {}),
    ...(reasoningTokens > 0 ? { reasoningTokens: Math.max(0, Math.round(reasoningTokens)) } : {}),
    ...(callCount > 0 ? { callCount: Math.max(0, Math.round(callCount)) } : {}),
    ...(lastSinglePromptTokens > 0
      ? { lastSinglePromptTokens: Math.max(0, Math.round(lastSinglePromptTokens)) }
      : {}),
    ...(contextWindowTokens > 0
      ? { contextWindowTokens: Math.max(1, Math.round(contextWindowTokens)) }
      : {}),
  };
}

function normalizeTokenBudget(value?: number | null): number | undefined {
  const budget = Number(value ?? 0);
  if (!Number.isFinite(budget) || budget <= 0) {
    return undefined;
  }
  return Math.floor(budget);
}

function normalizePendingFileReviewFiles(files: PendingFileReviewFile[]): PendingFileReviewFile[] {
  return files.map((file) => ({
    path: file.path.trim(),
    action: file.action || "modified",
    moveTo: file.moveTo,
    baseContent: file.baseContent,
    candidateContent: file.candidateContent,
    editedContent: file.editedContent,
    keep: file.keep !== false,
  })).filter((file) => file.path.length > 0);
}

function normalizePendingFileReview(review: PendingFileReview): PendingFileReview {
  const files = normalizePendingFileReviewFiles(review.files);
  const keepAll = files.length > 0 && files.every((file) => file.keep);
  const selectedPath = review.selectedPath && files.some((file) => file.path === review.selectedPath)
    ? review.selectedPath
    : files[0]?.path ?? null;
  return {
    ...review,
    files,
    selectedPath,
    keepAll,
    status: review.status ?? "pending",
    error: review.error,
  };
}

function normalizeWorkflowProgress(
  progress?: ApiThreadGoal["workflowProgress"] | null,
): ApiThreadGoal["workflowProgress"] | undefined {
  if (!progress || !Array.isArray(progress.runtimeNodes) || progress.runtimeNodes.length === 0) {
    return undefined;
  }
  const runtimeNodes = progress.runtimeNodes
    .map((node) => typeof node === "string" ? node.trim() : "")
    .filter(Boolean);
  if (runtimeNodes.length === 0) {
    return undefined;
  }
  const rawIndex = Number(progress.currentNodeIndex);
  const nodeDeliveries = Array.isArray(progress.nodeDeliveries)
    ? progress.nodeDeliveries
        .slice(0, runtimeNodes.length)
        .map((delivery) => typeof delivery === "string" ? delivery.trim() : "")
    : [];
  const robotId = typeof progress.robotId === "string" ? progress.robotId.trim() : "";
  const rootObjective = typeof progress.rootObjective === "string"
    ? progress.rootObjective.trim()
    : "";
  return {
    currentNodeIndex: Math.min(
      runtimeNodes.length - 1,
      Math.max(0, Number.isFinite(rawIndex) ? Math.trunc(rawIndex) : 0),
    ),
    runtimeNodes,
    ...(robotId ? { robotId } : {}),
    ...(rootObjective ? { rootObjective } : {}),
    ...(nodeDeliveries.length > 0 ? { nodeDeliveries } : {}),
    ...(progress.completed ? { completed: true } : {}),
  };
}

function normalizeThreadGoal(
  goal?: ApiThreadGoal | null,
  fallbackWorkflowProgress?: ApiThreadGoal["workflowProgress"],
): ThreadGoal | null {
  if (!goal?.objective?.trim()) {
    return null;
  }

  const hasWorkflowProgress = Object.prototype.hasOwnProperty.call(goal, "workflowProgress");
  const workflowProgress = normalizeWorkflowProgress(
    hasWorkflowProgress ? goal.workflowProgress : fallbackWorkflowProgress,
  );

  return {
    objective: goal.objective,
    status: goal.status ?? "active",
    tokenBudget: normalizeTokenBudget(goal.tokenBudget) ?? null,
    tokensUsed: Math.max(0, Number(goal.tokensUsed ?? 0)),
    createdAt: goal.createdAt,
    updatedAt: goal.updatedAt,
    ...(workflowProgress ? { workflowProgress } : {}),
  };
}

function normalizePlanFile(
  plan?: {
    path?: string;
    content?: string;
    revision?: number | null;
    updatedAt?: number | null;
  } | null,
): PlanFile | null {
  const path = typeof plan?.path === "string" ? plan.path.trim() : "";
  if (!path) {
    return null;
  }
  const content = typeof plan?.content === "string" ? plan.content : "";
  const revision = Number(plan?.revision ?? 0);
  const updatedAt = Number(plan?.updatedAt ?? 0);

  return {
    path,
    content,
    ...(Number.isFinite(revision) && revision > 0
      ? { revision: Math.floor(revision) }
      : {}),
    ...(Number.isFinite(updatedAt) && updatedAt > 0
      ? { updatedAt }
      : {}),
  };
}

const PROPOSED_PLAN_OPEN_TAG = "<proposed_plan>";
const PROPOSED_PLAN_CLOSE_TAG = "</proposed_plan>";
const TAGGED_PLAN_PATH = "conversation://proposed-plan";

function extractTaggedPlanContent(text: string): string | null {
  const start = text.indexOf(PROPOSED_PLAN_OPEN_TAG);
  if (start < 0) return null;
  const afterOpen = text.slice(start + PROPOSED_PLAN_OPEN_TAG.length);
  const end = afterOpen.indexOf(PROPOSED_PLAN_CLOSE_TAG);
  if (end < 0) return null;
  const content = afterOpen.slice(0, end).trim();
  return content.length > 0 ? content : null;
}

function stripTaggedPlanBlocks(text: string): string {
  let visible = "";
  let rest = text;

  while (rest.length > 0) {
    const start = rest.indexOf(PROPOSED_PLAN_OPEN_TAG);
    if (start < 0) {
      visible += rest;
      break;
    }

    visible += rest.slice(0, start);
    const afterOpen = rest.slice(start + PROPOSED_PLAN_OPEN_TAG.length);
    const end = afterOpen.indexOf(PROPOSED_PLAN_CLOSE_TAG);
    if (end < 0) {
      // 历史异常兜底：缺少闭合标签时至少去掉起始标签，避免标签外露。
      visible += afterOpen;
      break;
    }
    rest = afterOpen.slice(end + PROPOSED_PLAN_CLOSE_TAG.length);
  }

  return visible;
}

function mapTurnsToMessages(
  turns: RawTurn[],
  activePlan: PlanFile | null,
): { messages: ChatMessage[]; activePlan: PlanFile | null } {
  const messages: ChatMessage[] = [];
  const toolResultMap = new Map<string, string>();
  let latestTaggedPlanContent: string | null = null;
  let latestTaggedPlanTimestamp: number | null = null;

  for (const turn of turns) {
    for (const item of turn.items ?? []) {
      if (item.type === "toolResult" && item.toolCallId) {
        toolResultMap.set(item.toolCallId, item.text ?? "");
      }
    }
  }

  for (const turn of turns) {
    for (const item of turn.items ?? []) {
      if (item.type === "userMessage") {
        const attachments: BinaryAttachedFile[] = (item.attachments ?? [])
          .filter((attachment) =>
            typeof attachment?.name === "string"
            && typeof attachment?.type === "string"
            && typeof attachment?.dataUrl === "string"
            && attachment.name.trim().length > 0
            && attachment.type.trim().length > 0
            && attachment.dataUrl.trim().length > 0)
          .map((attachment) => ({
            kind: "binary",
            name: attachment.name!.trim(),
            type: attachment.type!.trim(),
            dataUrl: attachment.dataUrl!,
            size: Number.isFinite(attachment.size) ? Number(attachment.size) : 0,
          }));
        const text = (item.content ?? [])
          .filter((content) => content.type === "text" && content.text)
          .map((content) => content.text?.trim() ?? "")
          .filter(Boolean)
          .join("\n\n");

        if (text || attachments.length > 0) {
          messages.push({
            id: item.id ?? crypto.randomUUID(),
            role: "user",
            content: text,
            timestamp: toMillis(turn.startedAt),
            ...(attachments.length > 0 ? { attachments } : {}),
          });
        }
      }

      if (item.type === "agentMessage" && typeof item.text === "string") {
        const agentText = item.text ?? "";
        const extractedPlan = extractTaggedPlanContent(agentText);
        if (extractedPlan) {
          latestTaggedPlanContent = extractedPlan;
          latestTaggedPlanTimestamp = toMillis(turn.completedAt ?? turn.startedAt);
        }
        const visibleContent = stripTaggedPlanBlocks(agentText).trim();
        if (visibleContent) {
          messages.push({
            id: item.id ?? crypto.randomUUID(),
            role: "assistant",
            content: visibleContent,
            timestamp: toMillis(turn.completedAt ?? turn.startedAt),
          });
        }
      }

      if (item.type === "toolUse" && item.calls && item.calls.length > 0) {
        const toolItems: ToolCallItem[] = item.calls.map((c) => ({
          id: c.id,
          name: c.name,
          arguments: c.arguments,
          status: "success" as const,
          displayLabel: toolDisplayLabelFromArgs(c.name, c.arguments),
          output: toolResultMap.get(c.id),
        }));
        messages.push({
          id: item.id ?? `tcg-${crypto.randomUUID()}`,
          role: "system",
          content: "",
          timestamp: toMillis(turn.startedAt),
          toolCalls: toolItems,
        });
      }

      if (item.type === "toolResult" && item.toolName && !item.toolCallId) {
        messages.push({
          id: item.id ?? crypto.randomUUID(),
          role: "system",
          content: item.toolName,
          timestamp: toMillis(turn.startedAt),
          commandStatus: "success",
        });
      }
    }

    const summary = normalizeRunSummary(turn);
    if (summary) {
      messages.push(createRunSummaryMessage(summary));
    }
  }

  const hydratedActivePlan = latestTaggedPlanContent
    ? activePlan
      ? {
        ...activePlan,
        content: latestTaggedPlanContent,
      }
      : {
        path: TAGGED_PLAN_PATH,
        content: latestTaggedPlanContent,
        synthetic: true,
      }
    : activePlan;

  if (hydratedActivePlan?.content.trim()) {
    const duplicated = messages.some(
      (message) =>
        message.planFile?.path === hydratedActivePlan.path
        && message.planFile?.content === hydratedActivePlan.content
        && message.planFile?.revision === hydratedActivePlan.revision,
    );
    if (!duplicated) {
      messages.push({
        id: `plan-${crypto.randomUUID()}`,
        role: "assistant",
        content: "",
        timestamp: latestTaggedPlanTimestamp
          ?? (hydratedActivePlan.updatedAt ? toMillis(hydratedActivePlan.updatedAt) : Date.now()),
        planFile: hydratedActivePlan,
      });
    }
  }

  return {
    // 同一用户轮次内连续的多批 toolUse 合并为一张工具调用卡。
    messages: collapseToolCallCardsPerUserTurn(messages),
    activePlan: hydratedActivePlan,
  };
}

import { appStateSet, appStateDelete } from "../api/app_state";

const PROJECTS_KEY = "projects";
const THREAD_PROJECT_KEY = "thread-projects";
const ACTIVE_PROJECT_KEY = "active-project";
const CONFIGURED_MODELS_KEY = "configured-models";
const ACTIVE_MODEL_KEY = "active-model";
const PROVIDERS_KEY = "providers";
const ACTIVE_PROVIDER_KEY = "active-provider";
const AUTO_APPROVE_KEY = "auto-approve";
const IMAGE_GENERATION_SETTINGS_KEY = "image-generation-settings";
const SIDEBAR_WIDTH_KEY = "sidebar-width";
const RIGHT_PANEL_WIDTH_KEY = "right-panel-width";
const THREAD_PREFERENCES_KEY = "thread-preferences";

type PersistedProviderRecord = Omit<ProviderConfig, "models"> & {
  models?: Array<Partial<ProviderModel>>;
  maxOutputTokens?: unknown;
};

/** 跨会话持久化的对话级偏好（模型覆盖 + 本地知识库/子智能体开关 + 小程序绑定） */
export interface ThreadPreference {
  overrideProviderId?: string | null;
  overrideModelId?: string | null;
  smartbrainEnabled?: boolean;
  subagentEnabled?: boolean;
  /** 小程序编辑对话绑定：小程序 slug；非空表示该线程是小程序专属编辑对话 */
  miniappSlug?: string | null;
  /** 小程序显示名（用于侧边栏展示） */
  miniappName?: string | null;
  /** 小程序根目录：作为该线程的专属工作目录，优先级高于全局 workspaceCwd */
  miniappRootPath?: string | null;
}

function normalizePositiveInt(value: unknown, fallback: number): number {
  const parsed = typeof value === "number" ? value : Number(value);
  if (!Number.isFinite(parsed) || parsed <= 0) {
    return fallback;
  }
  return Math.floor(parsed);
}

function normalizeContextLength(value: unknown): number {
  return normalizePositiveInt(value, DEFAULT_MODEL_CONTEXT_LENGTH);
}

function normalizeModelMaxOutputTokens(
  value: unknown,
  fallback = DEFAULT_MODEL_MAX_OUTPUT_TOKENS,
): number {
  return normalizePositiveInt(value, fallback);
}

function normalizeVisionFallbackKind(value: unknown): VisionFallbackKind | undefined {
  if (typeof value !== "string") {
    return undefined;
  }
  if (
    value === VISION_FALLBACK_KIND_MULTIMODAL
    || value === VISION_FALLBACK_KIND_LOCAL_OCR
  ) {
    return value;
  }
  return undefined;
}

function normalizeEndpointModelName(value: unknown, fallbackModelId: string): string {
  const candidate = typeof value === "string" ? value.trim() : "";
  if (candidate.length > 0) {
    return candidate;
  }
  return fallbackModelId.trim();
}

function normalizePoolModelEndpoint(
  endpoint: Partial<PoolModelEndpoint>,
  fallbackModelId: string,
): PoolModelEndpoint {
  const id = typeof endpoint.id === "string" && endpoint.id.trim()
    ? endpoint.id
    : crypto.randomUUID();
  const url = typeof endpoint.url === "string" ? endpoint.url.trim() : "";
  const label = typeof endpoint.label === "string" ? endpoint.label.trim() : "";
  const model = normalizeEndpointModelName(endpoint.model, fallbackModelId);

  return {
    id,
    url,
    model,
    label,
    enabled: endpoint.enabled !== false,
    ...(typeof endpoint.apiKey === "string" && endpoint.apiKey.trim()
      ? { apiKey: endpoint.apiKey.trim() }
      : {}),
    ...(typeof endpoint.wireApi === "string" && endpoint.wireApi.trim()
      ? { wireApi: endpoint.wireApi.trim() }
      : {}),
  };
}

function normalizeProviderModel(
  model: Partial<ProviderModel>,
  fallbackMaxOutputTokens = DEFAULT_MODEL_MAX_OUTPUT_TOKENS,
): ProviderModel {
  const id = typeof model.id === "string" ? model.id : "";
  const label = typeof model.label === "string" && model.label.trim() ? model.label : id;
  const requestedFallbackKind = normalizeVisionFallbackKind(model.visionFallbackKind);
  const visionFallbackProviderId = typeof model.visionFallbackProviderId === "string"
    ? model.visionFallbackProviderId.trim()
    : "";
  const visionFallbackModelId = typeof model.visionFallbackModelId === "string"
    ? model.visionFallbackModelId.trim()
    : "";
  const hasMultimodalFallback = Boolean(visionFallbackProviderId && visionFallbackModelId);
  const fallbackKind = requestedFallbackKind === VISION_FALLBACK_KIND_LOCAL_OCR
    ? VISION_FALLBACK_KIND_LOCAL_OCR
    : hasMultimodalFallback
      ? VISION_FALLBACK_KIND_MULTIMODAL
      : undefined;
  const normalizedEndpoints = Array.isArray(model.endpoints)
    ? model.endpoints.map((endpoint) => normalizePoolModelEndpoint(endpoint, id))
    : undefined;
  const capabilities = model.capabilities && typeof model.capabilities === "object"
    ? {
      structuredTools: model.capabilities.structuredTools === true
        ? true : model.capabilities.structuredTools === false ? false : null,
      streaming: model.capabilities.streaming === true
        ? true : model.capabilities.streaming === false ? false : null,
      reasoning: model.capabilities.reasoning === true
        ? true : model.capabilities.reasoning === false ? false : null,
      usage: model.capabilities.usage === true
        ? true : model.capabilities.usage === false ? false : null,
      parallelToolCalls: model.capabilities.parallelToolCalls === true
        ? true : model.capabilities.parallelToolCalls === false ? false : null,
      probedAt: typeof model.capabilities.probedAt === "number" ? model.capabilities.probedAt : 0,
      fingerprint: typeof model.capabilities.fingerprint === "string" ? model.capabilities.fingerprint : "",
      wireApi: typeof model.capabilities.wireApi === "string" ? model.capabilities.wireApi : "",
    }
    : undefined;
  return {
    id,
    label,
    supportsVision: Boolean(model.supportsVision),
    ...(fallbackKind === VISION_FALLBACK_KIND_LOCAL_OCR
      ? { visionFallbackKind: VISION_FALLBACK_KIND_LOCAL_OCR }
      : {}),
    ...(fallbackKind === VISION_FALLBACK_KIND_MULTIMODAL
      ? {
        visionFallbackKind: VISION_FALLBACK_KIND_MULTIMODAL,
        visionFallbackProviderId,
        visionFallbackModelId,
      }
      : {}),
    contextLength: normalizeContextLength(model.contextLength),
    maxOutputTokens: normalizeModelMaxOutputTokens(model.maxOutputTokens, fallbackMaxOutputTokens),
    ...(capabilities ? { capabilities } : {}),
    ...(normalizedEndpoints !== undefined ? { endpoints: normalizedEndpoints } : {}),
  };
}

function buildLocalPoolModelEndpoints(
  provider: ProviderConfig | null | undefined,
  model: ProviderModel | null | undefined,
): Array<{
  url: string;
  label?: string;
  model: string;
  api_key?: string;
  wire_api?: string;
}> {
  if (provider?.type !== "local-pool" || !model?.endpoints?.length) {
    return [];
  }

  return model.endpoints
    .filter((ep) => ep.enabled)
    .map((ep) => ({
      url: ep.url.trim(),
      label: ep.label.trim() || undefined,
      model: normalizeEndpointModelName(ep.model, model.id),
      api_key: ep.apiKey?.trim() || undefined,
      wire_api: ep.wireApi?.trim() || undefined,
    }))
    .filter((ep) => ep.url.length > 0 && ep.model.length > 0);
}

function buildThreadChatProviderOverrideSnapshot(
  providers: ProviderConfig[],
  overrideProviderId: string | null | undefined,
  overrideModelId: string | null | undefined,
): {
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
} | null {
  const providerId = overrideProviderId?.trim() || null;
  const modelId = overrideModelId?.trim() || null;
  if (!providerId && !modelId) {
    return null;
  }

  const provider = providerId
    ? providers.find((item) => item.id === providerId || item.type === providerId) ?? null
    : null;
  if (!provider) {
    // 没有找到供应商实例时，至少把模型名传下去，避免完全失效。
    return modelId
      ? {
          providerKey: providerId || "custom",
          modelId,
        }
      : null;
  }

  const selectedModel = modelId
    ? provider.models.find((model) => model.id === modelId) ?? null
    : provider.models[0] ?? null;
  const modelEndpoints = buildLocalPoolModelEndpoints(provider, selectedModel);
  const visionFallback = resolveVisionFallbackConfig(selectedModel, providers);
  const providerKey = provider.type || provider.id || "custom";

  return {
    providerKey,
    baseUrl: provider.baseUrl || null,
    apiKey: provider.apiKey || null,
    wireApi: provider.wireApi || "chat",
    requiresOpenAIAuth: provider.requiresOpenAIAuth,
    modelId: selectedModel?.id ?? modelId,
    modelContextWindow: modelContextLengthOrDefault(selectedModel),
    maxOutputTokens: modelMaxOutputTokensOrDefault(selectedModel),
    modelSupportsVision: visionFallback.modelSupportsVision,
    visionFallbackKind: visionFallback.fallbackKind,
    visionFallbackProvider: visionFallback.fallbackProviderKey,
    visionFallbackModel: visionFallback.fallbackModelId,
    modelEndpoints: modelEndpoints.length > 0
      ? modelEndpoints.map((endpoint) => ({
          url: endpoint.url,
          label: endpoint.label,
          model: endpoint.model,
          apiKey: endpoint.api_key,
          wireApi: endpoint.wire_api,
        }))
      : [],
    activeEndpointIndex: modelEndpoints.length > 0 ? 0 : null,
  };
}

function resolveVisionFallbackConfig(
  model: ProviderModel | null | undefined,
  providers: ProviderConfig[],
): {
  modelSupportsVision: boolean;
  fallbackKind: VisionFallbackKind | null;
  fallbackProviderKey: string | null;
  fallbackModelId: string | null;
} {
  const modelSupportsVision = Boolean(model?.supportsVision);
  if (modelSupportsVision) {
    return {
      modelSupportsVision,
      fallbackKind: null,
      fallbackProviderKey: null,
      fallbackModelId: null,
    };
  }

  if (normalizeVisionFallbackKind(model?.visionFallbackKind) === VISION_FALLBACK_KIND_LOCAL_OCR) {
    return {
      modelSupportsVision,
      fallbackKind: VISION_FALLBACK_KIND_LOCAL_OCR,
      fallbackProviderKey: null,
      fallbackModelId: null,
    };
  }

  const fallbackProviderId = model?.visionFallbackProviderId?.trim();
  const fallbackModelId = model?.visionFallbackModelId?.trim();
  if (!fallbackProviderId || !fallbackModelId) {
    return {
      modelSupportsVision,
      fallbackKind: null,
      fallbackProviderKey: null,
      fallbackModelId: null,
    };
  }

  const fallbackProvider = providers.find((provider) =>
    provider.id === fallbackProviderId || provider.type === fallbackProviderId
  );
  if (!fallbackProvider) {
    return {
      modelSupportsVision,
      fallbackKind: null,
      fallbackProviderKey: null,
      fallbackModelId: null,
    };
  }

  const fallbackModel = fallbackProvider.models.find((candidate) =>
    candidate.id === fallbackModelId && candidate.supportsVision
  );
  if (!fallbackModel) {
    return {
      modelSupportsVision,
      fallbackKind: null,
      fallbackProviderKey: null,
      fallbackModelId: null,
    };
  }

  return {
    modelSupportsVision,
    fallbackKind: VISION_FALLBACK_KIND_MULTIMODAL,
    fallbackProviderKey: fallbackProvider.type || fallbackProvider.id,
    fallbackModelId: fallbackModel.id,
  };
}

function buildVisionFallbackConfigEdits(
  visionFallback: ReturnType<typeof resolveVisionFallbackConfig>,
): Array<{ keyPath: string; value: unknown; mergeStrategy: string }> {
  return [
    {
      keyPath: "model_supports_vision",
      value: visionFallback.modelSupportsVision,
      mergeStrategy: "replace",
    },
    {
      keyPath: "vision_fallback_kind",
      value: visionFallback.fallbackKind,
      mergeStrategy: "replace",
    },
    {
      keyPath: "vision_fallback_provider",
      value: visionFallback.fallbackProviderKey,
      mergeStrategy: "replace",
    },
    {
      keyPath: "vision_fallback_model",
      value: visionFallback.fallbackModelId,
      mergeStrategy: "replace",
    },
  ];
}

function resolveProviderModelByEntry(
  providers: ProviderConfig[],
  entry: ModelEntry | undefined,
  modelName: string,
): { provider: ProviderConfig | null; model: ProviderModel | null } {
  if (entry) {
    const matchedProvider = providers.find(
      (provider) => provider.id === entry.provider || provider.type === entry.provider,
    );
    if (matchedProvider) {
      const matchedModel = matchedProvider.models.find((model) => model.id === modelName);
      if (matchedModel) {
        return { provider: matchedProvider, model: matchedModel };
      }
    }
  }

  for (const provider of providers) {
    const matchedModel = provider.models.find((model) => model.id === modelName);
    if (matchedModel) {
      return { provider, model: matchedModel };
    }
  }

  return { provider: null, model: null };
}

function modelMaxOutputTokensOrDefault(model: ProviderModel | null | undefined): number {
  return normalizeModelMaxOutputTokens(model?.maxOutputTokens);
}

function modelContextLengthOrDefault(model: ProviderModel | null | undefined): number {
  return normalizeContextLength(model?.contextLength);
}

/**
 * 供应商预设模板列表
 * 仅作为"创建实例"的模板，不直接存储用户数据。
 * 用户从预设中选择一个来创建新的 ProviderConfig 实例。
 */
export const PROVIDER_PRESETS: ProviderPreset[] = [
  {
    type: "openai", name: "OpenAI", category: "global",
    defaultBaseUrl: "https://api.openai.com/v1", defaultWireApi: "chat", requiresOpenAIAuth: true,
    signupUrl: "https://platform.openai.com/api-keys",
    defaultModels: [
      { id: "gpt-4o", label: "GPT-4o", supportsVision: true, contextLength: 128000 },
      { id: "gpt-4o-mini", label: "GPT-4o Mini", supportsVision: true, contextLength: 128000 },
      { id: "o3-mini", label: "o3-mini", supportsVision: false, contextLength: 200000 },
    ],
  },
  {
    type: "anthropic", name: "Anthropic", category: "global",
    defaultBaseUrl: "https://api.anthropic.com/v1", defaultWireApi: "anthropic", requiresOpenAIAuth: false,
    signupUrl: "https://console.anthropic.com/settings/keys",
    defaultModels: [
      { id: "claude-sonnet-4-20250514", label: "Claude Sonnet 4", supportsVision: true, contextLength: 200000 },
      { id: "claude-opus-4-20250514", label: "Claude Opus 4", supportsVision: true, contextLength: 200000 },
    ],
  },
  {
    type: "google", name: "Google Gemini", category: "global",
    defaultBaseUrl: "https://generativelanguage.googleapis.com/v1beta/openai", defaultWireApi: "chat", requiresOpenAIAuth: false,
    signupUrl: "https://aistudio.google.com/apikey",
    defaultModels: [{ id: "gemini-2.5-pro", label: "Gemini 2.5 Pro", supportsVision: true, contextLength: 1000000 }],
  },
  {
    type: "deepseek", name: "DeepSeek", category: "china",
    defaultBaseUrl: "https://api.deepseek.com/v1", defaultWireApi: "chat", requiresOpenAIAuth: false,
    signupUrl: "https://platform.deepseek.com/api_keys",
    defaultModels: [
      { id: "deepseek-chat", label: "DeepSeek Chat", supportsVision: false, contextLength: 128000 },
      { id: "deepseek-reasoner", label: "DeepSeek Reasoner", supportsVision: false, contextLength: 128000 },
    ],
  },
  {
    type: "volcengine", name: "provider.preset.volcengine", category: "china",
    defaultBaseUrl: "https://ark.cn-beijing.volces.com/api/v3", defaultWireApi: "chat", requiresOpenAIAuth: false,
    signupUrl: "https://console.volcengine.com/ark/region:ark+cn-beijing/apiKey",
    defaultModels: [{ id: "deepseek-v4-pro-260425", label: "DeepSeek V4 Pro", supportsVision: false, contextLength: 128000 }],
  },
  {
    type: "qwen", name: "provider.preset.qwen", category: "china",
    defaultBaseUrl: "https://dashscope.aliyuncs.com/compatible-mode/v1", defaultWireApi: "chat", requiresOpenAIAuth: false,
    signupUrl: "https://dashscope.console.aliyun.com/apiKey",
    defaultModels: [{ id: "qwen-max", label: "Qwen Max", supportsVision: true, contextLength: 32768 }],
  },
  {
    type: "zhipu", name: "provider.preset.zhipu", category: "china",
    defaultBaseUrl: "https://open.bigmodel.cn/api/paas/v4", defaultWireApi: "chat", requiresOpenAIAuth: false,
    signupUrl: "https://open.bigmodel.cn/usercenter/apikeys",
    defaultModels: [{ id: "glm-4-plus", label: "GLM-4 Plus", supportsVision: true, contextLength: 128000 }],
  },
  {
    type: "moonshot", name: "Moonshot AI", category: "china",
    defaultBaseUrl: "https://api.moonshot.cn/v1", defaultWireApi: "chat", requiresOpenAIAuth: false,
    signupUrl: "https://platform.moonshot.cn/console/api-keys",
    defaultModels: [{ id: "moonshot-v1-128k", label: "Moonshot V1 128K", supportsVision: false, contextLength: 128000 }],
  },
  {
    type: "siliconflow", name: "SiliconFlow", category: "china",
    defaultBaseUrl: "https://api.siliconflow.cn/v1", defaultWireApi: "chat", requiresOpenAIAuth: false,
    signupUrl: "https://cloud.siliconflow.cn/account/ak",
    defaultModels: [{ id: "deepseek-ai/DeepSeek-V3", label: "DeepSeek V3", supportsVision: false, contextLength: 128000 }],
  },
  {
    type: "rightcode", name: "provider.preset.rightcode", category: "china",
    defaultBaseUrl: "https://right.codes/codex/v1", defaultWireApi: "chat", requiresOpenAIAuth: false,
    signupUrl: "https://www.right.codes/",
    defaultModels: [{ id: "gpt-5.2", label: "GPT-5.2", supportsVision: false, contextLength: 128000 }],
  },
  {
    type: "baichuan", name: "provider.preset.baichuan", category: "china",
    defaultBaseUrl: "https://api.baichuan-ai.com/v1", defaultWireApi: "chat", requiresOpenAIAuth: false,
    signupUrl: "https://platform.baichuan-ai.com/console/apikey",
    defaultModels: [{ id: "Baichuan4", label: "Baichuan 4", supportsVision: false, contextLength: 32768 }],
  },
  {
    type: "codebuddy", name: "CodeBuddy", category: "china",
    defaultBaseUrl: "https://copilot.tencent.com/v2", defaultWireApi: "chat", requiresOpenAIAuth: false,
    signupUrl: "https://copilot.tencent.com/profile/keys",
    defaultModels: [
      { id: "deepseek-v3", label: "DeepSeek V3", supportsVision: false, contextLength: 128000 },
      { id: "deepseek-r1", label: "DeepSeek R1", supportsVision: false, contextLength: 128000 },
      { id: "glm-5.1", label: "GLM 5.1", supportsVision: false, contextLength: 128000 },
      { id: "hy3", label: "Hunyuan Hy3", supportsVision: false, contextLength: 128000 },
    ],
  },
  {
    type: "ollama", name: "Ollama", category: "local",
    defaultBaseUrl: "http://localhost:11434/v1", defaultWireApi: "chat", requiresOpenAIAuth: false,
    signupUrl: "https://ollama.com/download",
    defaultModels: [{ id: "qwen2.5-coder:7b", label: "Qwen 2.5 Coder 7B", supportsVision: false, contextLength: 32768 }],
  },
  {
    type: "lmstudio", name: "LM Studio", category: "local",
    defaultBaseUrl: "http://localhost:1234/v1", defaultWireApi: "chat", requiresOpenAIAuth: false,
    signupUrl: "https://lmstudio.ai/",
    defaultModels: [{ id: "local-model", label: "Local Model", supportsVision: false, contextLength: 128000 }],
  },
  {
    type: "local-pool", name: "provider.preset.localPool", category: "local",
    defaultBaseUrl: "", defaultWireApi: "chat", requiresOpenAIAuth: false,
    defaultModels: [],
  },
  {
    type: "custom", name: "provider.preset.custom", category: "other",
    defaultBaseUrl: "", defaultWireApi: "chat", requiresOpenAIAuth: false,
    defaultModels: [],
  },
];

/** 从预设模板创建一个新的供应商实例 */
export function createProviderFromPreset(
  preset: ProviderPreset,
  overrides?: Partial<Pick<ProviderConfig, "name" | "baseUrl" | "apiKey">>,
): ProviderConfig {
  return {
    id: crypto.randomUUID(),
    type: preset.type,
    name: overrides?.name ?? preset.name,
    category: preset.category,
    baseUrl: overrides?.baseUrl ?? preset.defaultBaseUrl,
    apiKey: overrides?.apiKey ?? "",
    wireApi: preset.defaultWireApi,
    requiresOpenAIAuth: preset.requiresOpenAIAuth,
    models: preset.defaultModels.map((model) =>
      normalizeProviderModel(model, DEFAULT_MODEL_MAX_OUTPUT_TOKENS),
    ),
    isCustom: preset.type === "custom",
    createdAt: Date.now(),
  };
}


/**
 * 解析全局默认供应商下应使用的模型：
 * 1) 若旧版 activeModel 属于该供应商，优先沿用；
 * 2) 若 currentModel 仍在该供应商模型列表中，沿用；
 * 3) 否则回退到供应商模型列表第一项。
 */
function resolveGlobalDefaultModel(
  provider: ProviderConfig | null | undefined,
  options?: {
    preferredModelId?: string | null;
    currentModel?: string | null;
  },
): ProviderModel | null {
  if (!provider || provider.models.length === 0) {
    return null;
  }
  const preferred = options?.preferredModelId?.trim();
  if (preferred) {
    const matchedPreferred = provider.models.find((model) => model.id === preferred);
    if (matchedPreferred) {
      return matchedPreferred;
    }
  }
  const current = options?.currentModel?.trim();
  if (current) {
    const matchedCurrent = provider.models.find((model) => model.id === current);
    if (matchedCurrent) {
      return matchedCurrent;
    }
  }
  return provider.models[0] ?? null;
}

function saveProviders(providers: ProviderConfig[]) {
  void appStateSet(PROVIDERS_KEY, JSON.stringify(providers));
}

function saveActiveProviderId(id: string | null) {
  if (id) {
    void appStateSet(ACTIVE_PROVIDER_KEY, id);
  } else {
    void appStateDelete(ACTIVE_PROVIDER_KEY);
  }
}

function saveConfiguredModels(models: ModelEntry[]) {
  void appStateSet(CONFIGURED_MODELS_KEY, JSON.stringify(models));
}

function saveActiveModelId(id: string | null) {
  if (id) {
    void appStateSet(ACTIVE_MODEL_KEY, id);
  } else {
    void appStateDelete(ACTIVE_MODEL_KEY);
  }
}

function saveThreadPreferences(map: Record<string, ThreadPreference>) {
  void appStateSet(THREAD_PREFERENCES_KEY, JSON.stringify(map));
}

function readThreadPreference(
  map: Record<string, ThreadPreference>,
  threadId: string | null | undefined,
): ThreadPreference {
  if (!threadId) {
    return {};
  }
  return map[threadId] ?? {};
}

function buildThreadPreferencePatch(
  existing: ThreadPreference | undefined,
  patch: ThreadPreference,
): ThreadPreference | null {
  const next: ThreadPreference = {
    overrideProviderId:
      patch.overrideProviderId !== undefined
        ? patch.overrideProviderId
        : (existing?.overrideProviderId ?? null),
    overrideModelId:
      patch.overrideModelId !== undefined
        ? patch.overrideModelId
        : (existing?.overrideModelId ?? null),
    smartbrainEnabled:
      patch.smartbrainEnabled !== undefined
        ? patch.smartbrainEnabled
        : (existing?.smartbrainEnabled ?? false),
    subagentEnabled:
      patch.subagentEnabled !== undefined
        ? patch.subagentEnabled
        : (existing?.subagentEnabled ?? false),
    miniappSlug:
      patch.miniappSlug !== undefined
        ? patch.miniappSlug
        : (existing?.miniappSlug ?? null),
    miniappName:
      patch.miniappName !== undefined
        ? patch.miniappName
        : (existing?.miniappName ?? null),
    miniappRootPath:
      patch.miniappRootPath !== undefined
        ? patch.miniappRootPath
        : (existing?.miniappRootPath ?? null),
  };
  const isEmpty =
    !next.overrideProviderId &&
    !next.overrideModelId &&
    !next.smartbrainEnabled &&
    !next.subagentEnabled &&
    !next.miniappSlug &&
    !next.miniappName &&
    !next.miniappRootPath;
  return isEmpty ? null : next;
}

function saveProjects(projects: Project[]) {
  void appStateSet(PROJECTS_KEY, JSON.stringify(projects));
}

function saveThreadProjectMap(map: Record<string, string>) {
  void appStateSet(THREAD_PROJECT_KEY, JSON.stringify(map));
}

function saveActiveProject(id: string | null, cwd: string | null) {
  if (id && cwd) {
    void appStateSet(ACTIVE_PROJECT_KEY, JSON.stringify({ id, cwd }));
  } else {
    void appStateDelete(ACTIVE_PROJECT_KEY);
  }
}

function clampPanelWidth(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value));
}

function saveSidebarWidth(width: number) {
  const clamped = clampPanelWidth(width, SIDEBAR_WIDTH_MIN, SIDEBAR_WIDTH_MAX);
  void appStateSet(SIDEBAR_WIDTH_KEY, String(clamped));
  mergeLayoutSnapshotToStorage({ sidebarWidth: clamped });
}

function saveRightPanelWidth(width: number) {
  const clamped = clampPanelWidth(width, RIGHT_PANEL_WIDTH_MIN, RIGHT_PANEL_WIDTH_MAX);
  void appStateSet(
    RIGHT_PANEL_WIDTH_KEY,
    String(clamped),
  );
  mergeLayoutSnapshotToStorage({ rightPanelWidth: clamped });
}

function saveImageGenerationSettings(settings: ImageGenerationSettings) {
  void appStateSet(
    IMAGE_GENERATION_SETTINGS_KEY,
    JSON.stringify(settings),
  );
}

function getLeafName(path: string): string {
  const parts = path.replace(/\\/g, "/").split("/");
  return parts[parts.length - 1] || path;
}

interface AppState {
  initialized: boolean;
  initError: string | null;
  currentThreadId: string | null;
  currentTurnId: string | null;
  currentModel: string | null;
  /** 全局模型推理强度（写入 config.toml model_reasoning_effort） */
  reasoningEffort: string;
  workspaceCwd: string | null;
  configDir: string | null;
  configPath: string | null;

  configuredModels: ModelEntry[];
  activeModelId: string | null;

  /** 供应商列表 */
  providers: ProviderConfig[];
  /** 当前激活供应商的 ID */
  activeProviderId: string | null;
  /** 当前正在使用的资源池端点索引 */
  activeEndpointIndex: number | null;
  /** 文生图内置工具的独立配置 */
  imageGenerationSettings: ImageGenerationSettings;

  /** 输入框附件列表 */
  attachedFiles: AttachedFile[];
  /** 由外部面板注入到输入框的待插入文本（例如代码片段） */
  pendingComposerInsert: string | null;
  /** AI 回复期间排队等待发送的消息队列 */
  pendingMessageQueue: QueuedMessage[];
  /** apply_patch 的“写盘前审阅”会话，按 callId 建索引。 */
  pendingFileReviews: Record<string, PendingFileReview>;
  /** 后台线程运行时状态映射，key 为 threadId。切换离开的线程状态保存在这里。 */
  threadRuntimeStates: Record<string, ThreadRuntimeState>;

  projects: Project[];
  currentProjectId: string | null;
  threadProjectMap: Record<string, string>;

  threads: ThreadSummary[];
  messages: ChatMessage[];
  streamingText: string;
  streamingLabel: string;
  isStreaming: boolean;
  liveTurnUsage: TokenUsage | null;
  chatMode: ChatMode;
  latestPlanContent: string | null;
  activePlan: PlanFile | null;
  currentGoal: ThreadGoal | null;
  showSettings: boolean;
  settingsInitialTab: string | null;
  rightPanelVisible: boolean;
  rightPanelTab: RightPanelTab;
  sidebarWidth: number;
  rightPanelWidth: number;
  browserPanelUrl: string | null;
  browserPanelTitle: string | null;
  browserPanelStatus: "idle" | "running" | "success" | "failed";
  browserCanGoBack: boolean;
  browserCanGoForward: boolean;
  smartbrainExtractionRunning: boolean;
  smartbrainExtractionLabel: string | null;
  smartbrainExtractionProgress: SmartbrainExtractionProgress | null;
  browserSyncTrigger: number;
  browserActive: boolean;
  browserDetached: boolean;
  /** 本对话覆盖的供应商 ID；null 表示继承全局 */
  overrideProviderId: string | null;
  /** 本对话覆盖的模型 ID；null 表示继承全局 */
  overrideModelId: string | null;
  /** 本对话是否启用本地知识库 */
  smartbrainEnabled: boolean;
  /** 本对话是否启用子智能体工具 */
  subagentEnabled: boolean;
  /** 各对话持久化偏好，key 为 threadId */
  threadPreferences: Record<string, ThreadPreference>;
  autoApprove: boolean;
  sidebarTab: SidebarTab;

  selectedRobotId: string | null;
  robotCreateMode: boolean;

  /** 机器人提问倒计时等待状态 */
  robotWaitCountdown: RobotWaitCountdown | null;
  setRobotWaitCountdown: (v: RobotWaitCountdown | null) => void;
  /** 当前线程实时子智能体状态 */
  liveSubagents: Record<string, ActiveSubagentView>;

  /** Workflow 提取：当设置为非空 threadId 时弹出提取对话框 */
  workflowExtractThreadId: string | null;

  setInitialized: (v: boolean) => void;
  setInitError: (err: string | null) => void;
  retryInit: (() => void) | null;
  setRetryInit: (fn: (() => void) | null) => void;
  setCurrentThread: (id: string | null) => void;
  startNewThreadWithMessage: (threadId: string, message: ChatMessage) => void;
  setCurrentTurnId: (id: string | null) => void;
  setCurrentModel: (model: string | null) => void;
  setReasoningEffort: (effort: string) => void;
  setConfiguredModels: (models: ModelEntry[]) => void;
  setActiveModelId: (id: string | null) => void;
  getActiveModel: () => ModelEntry | null;
  setServerRuntime: (runtime: {
    cwd: string;
    configDir: string;
    configPath: string;
  }) => void;
  setWorkspaceCwd: (cwd: string | null) => void;

  /** 供应商管理 */
  setProviders: (providers: ProviderConfig[]) => void;
  setImageGenerationSettings: (settings: Partial<ImageGenerationSettings>) => void;
  activateProvider: (providerId: string) => void;
  updateProvider: (providerId: string, updates: Partial<ProviderConfig>) => void;
  addCustomProvider: (provider: ProviderConfig) => void;
  removeCustomProvider: (providerId: string) => void;
  addProviderModel: (providerId: string, model: ProviderModel) => void;
  updateProviderModel: (providerId: string, modelId: string, updates: Partial<ProviderModel>) => void;
  removeProviderModel: (providerId: string, modelId: string) => void;
  getActiveProvider: () => ProviderConfig | null;

  /** 附件管理 */
  setAttachedFiles: (files: AttachedFile[]) => void;
  addAttachedFile: (file: AttachedFile) => void;
  removeAttachedFile: (index: number) => void;
  clearAttachedFiles: () => void;
  queueComposerInsert: (text: string) => void;
  consumeComposerInsert: () => string | null;

  /** 消息排队：在 AI 回复期间将消息加入待发送队列 */
  enqueueMessage: (msg: QueuedMessage) => void;
  dequeueMessage: () => QueuedMessage | null;
  removeQueuedMessage: (id: string) => void;
  clearMessageQueue: () => void;

  userHomeDir: string | null;
  setUserHomeDir: (dir: string) => void;

  projectRoot: string | null;

  addProject: (cwd: string) => string;
  selectProject: (projectId: string) => void;
  selectGeneralMode: () => void;
  removeProject: (projectId: string) => void;

  setThreads: (threads: ThreadSummary[]) => void;
  addThread: (thread: ThreadSummary) => void;
  /**
   * 按 threadId 局部更新会话摘要字段。
   * 只允许更新侧边栏展示所需的轻量字段，避免误覆盖消息等重数据。
   */
  updateThreadSummary: (
    threadId: string,
    patch: Partial<Pick<ThreadSummary, "name" | "preview" | "updatedAt">>,
  ) => void;
  /** 删除指定对话 */
  deleteThread: (threadId: string) => void;
  setMessages: (messages: ChatMessage[]) => void;
  addMessage: (message: ChatMessage) => void;
  /** 截断消息列表：保留 index 之前的消息（不含 index） */
  truncateMessagesFrom: (messageId: string) => void;
  setLiveTurnUsage: (usage: TokenUsage | null) => void;
  updateToolCallStatus: (toolId: string, status: "success" | "failed", output?: string) => void;
  updateToolCallPatchProgress: (toolId: string, changes: PatchProgressChange[]) => void;
  upsertPendingFileReview: (review: PendingFileReview) => void;
  removePendingFileReview: (callId: string) => void;
  setPendingFileReviewSelectedPath: (callId: string, path: string) => void;
  setPendingFileReviewFileKeep: (callId: string, path: string, keep: boolean) => void;
  setPendingFileReviewKeepAll: (callId: string, keepAll: boolean) => void;
  setPendingFileReviewEditedContent: (callId: string, path: string, content: string) => void;
  setPendingFileReviewStatus: (
    callId: string,
    status: PendingFileReview["status"],
    error?: string,
  ) => void;
  markRunningToolCallsInterrupted: (reason?: string) => void;
  appendStreamingText: (delta: string) => void;
  clearStreamingText: () => void;
  setStreaming: (v: boolean) => void;
  flushAndStopStreaming: (options?: StreamingConvergeOptions) => void;
  setStreamingLabel: (label: string) => void;
  setChatMode: (mode: ChatMode) => void;
  setLatestPlanContent: (content: string | null) => void;
  setActivePlan: (plan: PlanFile | null) => void;
  setCurrentGoal: (goal: ThreadGoal | null) => void;
  setShowSettings: (v: boolean, options?: { tab?: string }) => void;
  setRightPanelVisible: (v: boolean) => void;
  toggleRightPanel: () => void;
  setRightPanelTab: (tab: RightPanelTab) => void;
  setSidebarWidth: (width: number) => void;
  setRightPanelWidth: (width: number) => void;
  setAutoApprove: (v: boolean) => void;
  setSidebarTab: (tab: SidebarTab) => void;
  setSelectedRobotId: (id: string | null) => void;
  setRobotCreateMode: (v: boolean) => void;
  setWorkflowExtractThreadId: (id: string | null) => void;
  setThreadModelOverride: (providerId: string | null, modelId: string | null) => void;
  setThreadSmartbrainEnabled: (enabled: boolean) => void;
  setThreadSubagentEnabled: (enabled: boolean) => void;
  /** 绑定/解绑线程的小程序编辑上下文（cwd 绑定到小程序根目录，线程级隔离）；传 null 解除绑定 */
  bindThreadMiniapp: (
    threadId: string,
    binding: { slug: string; name: string; rootPath: string } | null,
  ) => void;
  buildThreadChatProviderOverride: (
    providerId?: string | null,
    modelId?: string | null,
  ) => ReturnType<typeof buildThreadChatProviderOverrideSnapshot>;
  setSmartbrainExtractionStatus: (state: Partial<{
    running: boolean;
    label: string | null;
    progress: SmartbrainExtractionProgress | null;
  }>) => void;
  setBrowserPanelState: (state: Partial<{
    url: string | null;
    title: string | null;
    status: "idle" | "running" | "success" | "failed";
    canGoBack: boolean;
    canGoForward: boolean;
  }>) => void;
  triggerBrowserSync: () => void;
  setBrowserActive: (v: boolean) => void;
  setBrowserDetached: (v: boolean) => void;
  /** 按 thread 解析执行 cwd：小程序绑定 > 所属项目 > 当前 UI 工作区 fallback */
  resolveThreadCwd: (threadId: string | null) => string | null;
  /** 打开对话时同步 UI 项目焦点（不清空 messages/runtime） */
  syncWorkspaceForThread: (threadId: string) => void;
  createThread: () => Promise<string | null>;
  loadThreads: () => Promise<void>;
  loadThread: (threadId: string) => Promise<void>;

  // ─── Per-thread 运行时状态管理 ──────────────────────────────
  /** 将当前活跃线程的顶层字段快照到 threadRuntimeStates 映射。 */
  saveCurrentThreadRuntimeState: () => void;
  /** 从映射恢复指定线程状态到顶层字段，返回是否命中缓存。 */
  restoreThreadRuntimeState: (threadId: string) => boolean;
  /** 统一更新入口：活跃线程更新顶层，后台线程更新映射。updater 返回需要合并的部分状态。 */
  applyToThread: (
    threadId: string,
    updater: (draft: ThreadRuntimeState) => Partial<ThreadRuntimeState>,
  ) => Partial<ThreadRuntimeState>;
  /** 获取指定线程的运行时状态快照（活跃线程从顶层字段组装，后台线程从映射读取）。 */
  getThreadRuntimeState: (threadId: string) => ThreadRuntimeState | null;
  /** 删除指定线程的运行时状态（用于删除对话时清理）。 */
  deleteThreadRuntimeState: (threadId: string) => void;
  /** 检查指定线程是否正在流式（活跃线程看顶层 isStreaming，后台看映射）。 */
  isThreadStreaming: (threadId: string) => boolean;

  // ─── Per-thread 方法族（事件处理器调用） ─────────────────────
  addMessageToThread: (threadId: string, message: ChatMessage) => void;
  /**
   * 将工具调用并入当前用户轮次的开放工具卡。
   * 若当前轮次尚无工具卡，则新建一张。
   * 返回该工具卡消息 id。
   */
  appendToolCallsToThread: (threadId: string, items: ToolCallItem[]) => string | null;
  setStreamingForThread: (threadId: string, v: boolean) => void;
  appendStreamingTextForThread: (threadId: string, delta: string) => void;
  clearStreamingTextForThread: (threadId: string) => void;
  setStreamingLabelForThread: (threadId: string, label: string) => void;
  flushAndStopStreamingForThread: (threadId: string, options?: StreamingConvergeOptions) => void;
  setCurrentTurnIdForThread: (threadId: string, id: string | null) => void;
  setLiveTurnUsageForThread: (threadId: string, usage: TokenUsage | null) => void;
  setCurrentGoalForThread: (threadId: string, goal: ThreadGoal | null) => void;
  updateToolCallStatusForThread: (
    threadId: string,
    toolId: string,
    status: "success" | "failed",
    output?: string,
  ) => void;
  updateToolCallPatchProgressForThread: (
    threadId: string,
    toolId: string,
    changes: PatchProgressChange[],
  ) => void;
  markRunningToolCallsInterruptedForThread: (threadId: string, reason?: string) => void;
  upsertPendingFileReviewForThread: (threadId: string, review: PendingFileReview) => void;
  setPendingFileReviewStatusForThread: (
    threadId: string,
    callId: string,
    status: PendingFileReview["status"],
    error?: string,
  ) => void;
  removePendingFileReviewForThread: (threadId: string, callId: string) => void;
  dequeueMessageForThread: (threadId: string) => QueuedMessage | null;
  enqueueMessageToThread: (threadId: string, msg: QueuedMessage) => void;
  setRobotWaitCountdownForThread: (threadId: string, v: RobotWaitCountdown | null) => void;
  upsertLiveSubagentForThread: (threadId: string, agent: ActiveSubagentView) => void;
  clearLiveSubagentsForThread: (threadId: string) => void;
  removeLiveSubagentForThread: (threadId: string, agentId: string) => void;
  setActivePlanForThread: (threadId: string, plan: PlanFile | null) => void;
  setLatestPlanContentForThread: (threadId: string, content: string | null) => void;
}

/** 从顶层 store 字段组装 ThreadRuntimeState 快照。 */
function assembleRuntimeStateFromStore(
  state: Pick<
    AppState,
    | "messages"
    | "streamingText"
    | "streamingLabel"
    | "isStreaming"
    | "liveTurnUsage"
    | "currentTurnId"
    | "pendingMessageQueue"
    | "pendingFileReviews"
    | "currentGoal"
    | "activePlan"
    | "latestPlanContent"
    | "chatMode"
    | "selectedRobotId"
    | "robotCreateMode"
    | "robotWaitCountdown"
    | "liveSubagents"
    | "browserPanelUrl"
    | "browserPanelTitle"
    | "browserPanelStatus"
    | "browserCanGoBack"
    | "browserCanGoForward"
    | "browserActive"
    | "browserDetached"
    | "overrideProviderId"
    | "overrideModelId"
    | "smartbrainEnabled"
    | "subagentEnabled"
  >,
): ThreadRuntimeState {
  return {
    messages: state.messages,
    streamingText: state.streamingText,
    streamingLabel: state.streamingLabel,
    isStreaming: state.isStreaming,
    liveTurnUsage: state.liveTurnUsage,
    currentTurnId: state.currentTurnId,
    pendingMessageQueue: state.pendingMessageQueue,
    pendingFileReviews: state.pendingFileReviews,
    currentGoal: state.currentGoal,
    activePlan: state.activePlan,
    latestPlanContent: state.latestPlanContent,
    chatMode: state.chatMode,
    selectedRobotId: state.selectedRobotId,
    robotCreateMode: state.robotCreateMode,
    robotWaitCountdown: state.robotWaitCountdown,
    liveSubagents: state.liveSubagents,
    reasoningText: "",
    browserPanelUrl: state.browserPanelUrl,
    browserPanelTitle: state.browserPanelTitle,
    browserPanelStatus: state.browserPanelStatus,
    browserCanGoBack: state.browserCanGoBack,
    browserCanGoForward: state.browserCanGoForward,
    browserActive: state.browserActive,
    browserDetached: state.browserDetached,
    overrideProviderId: state.overrideProviderId,
    overrideModelId: state.overrideModelId,
    smartbrainEnabled: state.smartbrainEnabled,
    subagentEnabled: state.subagentEnabled,
    updatedAt: Date.now(),
  };
}

/** LRU 淘汰：若映射超过上限则移除 updatedAt 最旧的条目。 */
function pruneThreadRuntimeStates(
  map: Record<string, ThreadRuntimeState>,
): Record<string, ThreadRuntimeState> {
  const keys = Object.keys(map);
  if (keys.length <= MAX_THREAD_RUNTIME_STATES) {
    return map;
  }
  const sorted = keys.sort(
    (a, b) => (map[a].updatedAt ?? 0) - (map[b].updatedAt ?? 0),
  );
  const next = { ...map };
  const toRemove = sorted.slice(0, keys.length - MAX_THREAD_RUNTIME_STATES);
  for (const key of toRemove) {
    delete next[key];
  }
  return next;
}

export const useAppStore = create<AppState>((set, get) => ({
  initialized: false,
  initError: null,
  currentThreadId: null,
  currentTurnId: null,
  currentModel: null,
  reasoningEffort: "medium",
  configuredModels: [],
  activeModelId: null,
  providers: [],
  activeProviderId: null,
  activeEndpointIndex: null,
  imageGenerationSettings: normalizeImageGenerationSettings(),
  attachedFiles: [],
  pendingComposerInsert: null,
  pendingMessageQueue: [],
  pendingFileReviews: {},
  threadRuntimeStates: {},
  workspaceCwd: null,
  configDir: null,
  configPath: null,

  userHomeDir: null,
  projectRoot: null,

  projects: [],
  currentProjectId: null,
  threadProjectMap: {},

  threads: [],
  messages: [],
  streamingText: "",
  streamingLabel: "",
  isStreaming: false,
  liveTurnUsage: null,
  chatMode: "chat",
  latestPlanContent: null,
  activePlan: null,
  currentGoal: null,
  showSettings: false,
  settingsInitialTab: null,
  autoApprove: false,
  sidebarTab: "chats",
  selectedRobotId: null,
  robotCreateMode: false,
  robotWaitCountdown: null,
  setRobotWaitCountdown: (v) => set({ robotWaitCountdown: v }),
  liveSubagents: {},
  workflowExtractThreadId: null,
  rightPanelVisible: false,
  rightPanelTab: "browser",
  sidebarWidth: clampPanelWidth(
    initialLayoutSnapshot?.sidebarWidth ?? DEFAULT_SIDEBAR_WIDTH,
    SIDEBAR_WIDTH_MIN,
    SIDEBAR_WIDTH_MAX,
  ),
  rightPanelWidth: clampPanelWidth(
    initialLayoutSnapshot?.rightPanelWidth ?? DEFAULT_RIGHT_PANEL_WIDTH,
    RIGHT_PANEL_WIDTH_MIN,
    RIGHT_PANEL_WIDTH_MAX,
  ),
  browserPanelUrl: null,
  browserPanelTitle: null,
  browserPanelStatus: "idle",
  browserCanGoBack: false,
  browserCanGoForward: false,
  smartbrainExtractionRunning: false,
  smartbrainExtractionLabel: null,
  smartbrainExtractionProgress: null,
  browserSyncTrigger: 0,
  browserActive: false,
  browserDetached: false,
  overrideProviderId: null,
  overrideModelId: null,
  smartbrainEnabled: false,
  subagentEnabled: false,
  threadPreferences: {},

  setInitialized: (v) => set({ initialized: v }),
  setInitError: (err) => set({ initError: err }),
  retryInit: null,
  setRetryInit: (fn) => set({ retryInit: fn }),
  setCurrentThread: (id) => {
    // 切换前保存当前线程的运行时状态，避免队列/流式等上下文丢失。
    get().saveCurrentThreadRuntimeState();
    if (id && get().restoreThreadRuntimeState(id)) {
      // 命中缓存：恢复成功，仅需更新 currentThreadId。
      set({ currentThreadId: id });
      return;
    }
    // 未命中缓存：重置为干净的初始状态（新线程或首次打开）。
    const pref = readThreadPreference(get().threadPreferences, id);
    set({
      currentThreadId: id,
      messages: [],
      streamingText: "",
      streamingLabel: "",
      isStreaming: false,
      liveTurnUsage: null,
      currentTurnId: null,
      currentGoal: null,
      latestPlanContent: null,
      activePlan: null,
      selectedRobotId: null,
      robotCreateMode: false,
      pendingComposerInsert: null,
      pendingMessageQueue: [],
      pendingFileReviews: {},
      liveSubagents: {},
      browserPanelUrl: null,
      browserPanelTitle: null,
      browserPanelStatus: "idle",
      browserCanGoBack: false,
      browserCanGoForward: false,
      browserActive: false,
      browserDetached: false,
      overrideProviderId: pref.overrideProviderId ?? null,
      overrideModelId: pref.overrideModelId ?? null,
      smartbrainEnabled: pref.smartbrainEnabled ?? false,
      subagentEnabled: pref.subagentEnabled ?? false,
    });
  },
  startNewThreadWithMessage: (threadId, message) => {
    // 新会话首条消息：若用户在“空白对话”里已选了供应商/模型/知识库，
    // 需要把当前 UI 覆盖迁移到新 threadId，避免被空 pref 清掉。
    const state = get();
    const nextPrefs = { ...state.threadPreferences };
    const existingPref = readThreadPreference(nextPrefs, threadId);
    const draftPref = buildThreadPreferencePatch(existingPref, {
      overrideProviderId: state.overrideProviderId,
      overrideModelId: state.overrideModelId,
      smartbrainEnabled: state.smartbrainEnabled,
      subagentEnabled: state.subagentEnabled,
    });
    if (draftPref) {
      nextPrefs[threadId] = draftPref;
      saveThreadPreferences(nextPrefs);
    } else if (nextPrefs[threadId]) {
      delete nextPrefs[threadId];
      saveThreadPreferences(nextPrefs);
    }
    const pref = readThreadPreference(nextPrefs, threadId);
    set({
      currentThreadId: threadId,
      messages: [message],
      streamingText: "",
      streamingLabel: "",
      isStreaming: false,
      liveTurnUsage: null,
      currentGoal: null,
      latestPlanContent: null,
      activePlan: null,
      pendingComposerInsert: null,
      pendingMessageQueue: [],
      pendingFileReviews: {},
      browserPanelUrl: null,
      browserPanelTitle: null,
      browserPanelStatus: "idle",
      browserCanGoBack: false,
      browserCanGoForward: false,
      browserActive: false,
      browserDetached: false,
      overrideProviderId: pref.overrideProviderId ?? null,
      overrideModelId: pref.overrideModelId ?? null,
      smartbrainEnabled: pref.smartbrainEnabled ?? false,
      subagentEnabled: pref.subagentEnabled ?? false,
      threadPreferences: nextPrefs,
    });
  },
  setCurrentTurnId: (id) => set({ currentTurnId: id }),
  setCurrentModel: (model) => set({ currentModel: model }),
  setReasoningEffort: (effort) => {
    const normalized = normalizeReasoningEffort(effort);
    set({ reasoningEffort: normalized });
    standaloneConfigWrite([
      { keyPath: "model_reasoning_effort", value: normalized, mergeStrategy: "replace" },
    ]).catch((err) => console.error("Failed to sync reasoning effort to config:", err));
  },
  setConfiguredModels: (models) => {
    saveConfiguredModels(models);
    set({ configuredModels: models });
  },
  setActiveModelId: (id) => {
    saveActiveModelId(id);
    const state = get();
    const models = state.configuredModels;
    const entry = models.find((m) => m.id === id);
    const modelName = entry?.model ?? id;
    set({ activeModelId: id, currentModel: modelName });
    if (modelName) {
      const { model, provider: matchedProvider } = resolveProviderModelByEntry(state.providers, entry, modelName);

      const modelEndpoints = buildLocalPoolModelEndpoints(matchedProvider, model);
      const visionFallback = model
        ? resolveVisionFallbackConfig(model, state.providers)
        : {
          modelSupportsVision: Boolean(entry?.supportsVision),
          fallbackKind: null,
          fallbackProviderKey: null,
          fallbackModelId: null,
        };

      set({ activeEndpointIndex: modelEndpoints.length > 0 ? 0 : null });

      const edits: { keyPath: string; value: unknown; mergeStrategy: string }[] = [
        { keyPath: "model", value: modelName, mergeStrategy: "replace" },
        {
          keyPath: "model_context_window",
          value: modelContextLengthOrDefault(model),
          mergeStrategy: "replace",
        },
        {
          keyPath: "max_output_tokens",
          value: modelMaxOutputTokensOrDefault(model),
          mergeStrategy: "replace",
        },
        ...buildVisionFallbackConfigEdits(visionFallback),
        { keyPath: "model_endpoints", value: modelEndpoints, mergeStrategy: "replace" },
        { keyPath: "active_endpoint_index", value: modelEndpoints.length > 0 ? 0 : null, mergeStrategy: "replace" },
      ];
      standaloneConfigWrite(edits).catch((err) => console.error("Failed to sync model to config:", err));
    }
  },
  getActiveModel: () => {
    const { configuredModels, activeModelId } = get();
    return configuredModels.find((m) => m.id === activeModelId) ?? null;
  },
  setServerRuntime: (runtime) =>
    set((s) => {
      const isGeneral = s.currentProjectId === GENERAL_PROJECT_ID;
      return {
        projectRoot: runtime.cwd,
        workspaceCwd: !s.currentProjectId || isGeneral ? runtime.cwd : s.workspaceCwd,
        configDir: runtime.configDir,
        configPath: runtime.configPath,
      };
    }),
  setWorkspaceCwd: (cwd) => set({ workspaceCwd: cwd }),

  setUserHomeDir: (dir) => set({ userHomeDir: dir }),

  addProject: (cwd: string) => {
    const id = crypto.randomUUID();
    const name = getLeafName(cwd);
    const project: Project = { id, name, cwd };
    const next = [...get().projects, project];
    saveProjects(next);
    saveActiveProject(id, cwd);
    set({
      projects: next,
      currentProjectId: id,
      workspaceCwd: cwd,
      currentThreadId: null,
      messages: [],
      streamingText: "",
      liveTurnUsage: null,
      latestPlanContent: null,
      activePlan: null,
      currentGoal: null,
      browserPanelUrl: null,
      browserPanelTitle: null,
      browserPanelStatus: "idle",
      browserCanGoBack: false,
      browserCanGoForward: false,
      browserActive: false,
      browserDetached: false,
      overrideProviderId: null,
      overrideModelId: null,
      smartbrainEnabled: false,
      subagentEnabled: false,
    });
    return id;
  },

  selectProject: (projectId: string) => {
    const project = get().projects.find((p) => p.id === projectId);
    if (!project) return;
    // 切换项目前保存当前对话的运行时状态。
    get().saveCurrentThreadRuntimeState();
    saveActiveProject(projectId, project.cwd);
    set({
      currentProjectId: projectId,
      workspaceCwd: project.cwd,
      currentThreadId: null,
      messages: [],
      streamingText: "",
      streamingLabel: "",
      isStreaming: false,
      liveTurnUsage: null,
      currentTurnId: null,
      currentGoal: null,
      latestPlanContent: null,
      activePlan: null,
      selectedRobotId: null,
      robotCreateMode: false,
      pendingMessageQueue: [],
      pendingFileReviews: {},
      browserPanelUrl: null,
      browserPanelTitle: null,
      browserPanelStatus: "idle",
      browserCanGoBack: false,
      browserCanGoForward: false,
      browserActive: false,
      browserDetached: false,
      overrideProviderId: null,
      overrideModelId: null,
      smartbrainEnabled: false,
      subagentEnabled: false,
    });
  },

  selectGeneralMode: () => {
    const cwd = get().projectRoot ?? get().userHomeDir;
    // 切换到通用模式前保存当前对话的运行时状态。
    get().saveCurrentThreadRuntimeState();
    saveActiveProject(GENERAL_PROJECT_ID, cwd);
    set({
      currentProjectId: GENERAL_PROJECT_ID,
      workspaceCwd: cwd,
      currentThreadId: null,
      messages: [],
      streamingText: "",
      streamingLabel: "",
      isStreaming: false,
      liveTurnUsage: null,
      currentTurnId: null,
      currentGoal: null,
      latestPlanContent: null,
      activePlan: null,
      selectedRobotId: null,
      robotCreateMode: false,
      pendingMessageQueue: [],
      pendingFileReviews: {},
      browserPanelUrl: null,
      browserPanelTitle: null,
      browserPanelStatus: "idle",
      browserCanGoBack: false,
      browserCanGoForward: false,
      browserActive: false,
      browserDetached: false,
      overrideProviderId: null,
      overrideModelId: null,
      smartbrainEnabled: false,
      subagentEnabled: false,
    });
  },

  removeProject: (projectId: string) => {
    const next = get().projects.filter((p) => p.id !== projectId);
    saveProjects(next);
    const isCurrent = get().currentProjectId === projectId;
    if (isCurrent) {
      saveActiveProject(null, null);
    }
    // 同时清理该项目下所有对话映射
    const tpMap = { ...get().threadProjectMap };
    const threadsToRemove = new Set<string>();
    for (const [tid, pid] of Object.entries(tpMap)) {
      if (pid === projectId) {
        threadsToRemove.add(tid);
        delete tpMap[tid];
      }
    }
    saveThreadProjectMap(tpMap);
    const filteredThreads = get().threads.filter((t) => !threadsToRemove.has(t.id));
    // 清理被删除对话的运行时状态快照
    for (const tid of threadsToRemove) {
      get().deleteThreadRuntimeState(tid);
    }
    set({
      projects: next,
      threads: filteredThreads,
      threadProjectMap: tpMap,
      ...(isCurrent
        ? {
            currentProjectId: null,
            workspaceCwd: null,
            currentThreadId: null,
            messages: [],
            streamingText: "",
            streamingLabel: "",
            isStreaming: false,
            liveTurnUsage: null,
            currentGoal: null,
            latestPlanContent: null,
            activePlan: null,
            pendingMessageQueue: [],
            pendingFileReviews: {},
            overrideProviderId: null,
            overrideModelId: null,
            smartbrainEnabled: false,
            subagentEnabled: false,
          }
        : {}),
    });
  },

  // ─── 供应商管理 ─────────────────────────────────────────
  setProviders: (providers) => {
    saveProviders(providers);
    set({ providers });
  },

  setImageGenerationSettings: (settings) => {
    const normalized = normalizeImageGenerationSettings({
      ...get().imageGenerationSettings,
      ...settings,
    });
    saveImageGenerationSettings(normalized);
    set({ imageGenerationSettings: normalized });
  },

  activateProvider: (providerId: string) => {
    saveActiveProviderId(providerId);
    const state = get();
    const provider = state.providers.find((p) => p.id === providerId);
    if (!provider) {
      set({ activeProviderId: providerId });
      return;
    }

    const providerKey = provider.type || "custom";
    const providerOverride: Record<string, unknown> = {};
    if (provider.baseUrl) providerOverride.base_url = provider.baseUrl;
    if (provider.wireApi) providerOverride.wire_api = provider.wireApi;
    if (provider.apiKey) providerOverride.experimental_bearer_token = provider.apiKey;
    providerOverride.requires_openai_auth = provider.requiresOpenAIAuth;
    const activeEntry = state.activeModelId
      ? state.configuredModels.find((m) => m.id === state.activeModelId)
      : null;
    const preferredModelId = activeEntry
      && (activeEntry.provider === provider.id || activeEntry.provider === provider.type)
      ? activeEntry.model
      : undefined;
    // 启用供应商时同步全局默认模型，避免新对话继续落在旧模型/列表第一项。
    const selectedModel = resolveGlobalDefaultModel(provider, {
      preferredModelId,
      currentModel: state.currentModel,
    });
    const selectedModelId = selectedModel?.id ?? "";
    const modelEndpoints = buildLocalPoolModelEndpoints(provider, selectedModel);
    const visionFallback = resolveVisionFallbackConfig(selectedModel, state.providers);

    set({
      activeProviderId: providerId,
      currentModel: selectedModelId || null,
      activeEndpointIndex: modelEndpoints.length > 0 ? 0 : null,
    });

    const edits: { keyPath: string; value: unknown; mergeStrategy: string }[] = [
      { keyPath: "model_provider", value: providerKey, mergeStrategy: "replace" },
      { keyPath: "model", value: selectedModelId, mergeStrategy: "replace" },
      { keyPath: `model_providers.${providerKey}`, value: providerOverride, mergeStrategy: "replace" },
      {
        keyPath: "model_context_window",
        value: modelContextLengthOrDefault(selectedModel),
        mergeStrategy: "replace",
      },
      {
        keyPath: "max_output_tokens",
        value: modelMaxOutputTokensOrDefault(selectedModel),
        mergeStrategy: "replace",
      },
      ...buildVisionFallbackConfigEdits(visionFallback),
      { keyPath: "model_endpoints", value: modelEndpoints, mergeStrategy: "replace" },
      { keyPath: "active_endpoint_index", value: modelEndpoints.length > 0 ? 0 : null, mergeStrategy: "replace" },
    ];
    standaloneConfigWrite(edits).catch((err) => console.error("Failed to sync provider config:", err));
  },

  updateProvider: (providerId: string, updates: Partial<ProviderConfig>) => {
    const providers = get().providers.map((p) =>
      p.id === providerId ? { ...p, ...updates } : p,
    );
    saveProviders(providers);
    set({ providers });
  },

  addCustomProvider: (provider: ProviderConfig) => {
    const providers = [...get().providers, provider];
    saveProviders(providers);
    set({ providers });
    // 如果是第一个实例，自动激活
    if (providers.length === 1) {
      saveActiveProviderId(provider.id);
      set({ activeProviderId: provider.id });
    }
  },

  removeCustomProvider: (providerId: string) => {
    const providers = get().providers.filter((p) => p.id !== providerId);
    saveProviders(providers);
    // 若删除的是当前激活的，切换到第一个
    if (get().activeProviderId === providerId) {
      const nextId = providers[0]?.id ?? null;
      saveActiveProviderId(nextId);
      set({ providers, activeProviderId: nextId });
    } else {
      set({ providers });
    }
  },

  addProviderModel: (providerId: string, model: ProviderModel) => {
    const normalizedModel = normalizeProviderModel(model);
    const providers = get().providers.map((p) =>
      p.id === providerId ? { ...p, models: [...p.models, normalizedModel] } : p,
    );
    saveProviders(providers);
    set({ providers });
  },

  updateProviderModel: (providerId: string, modelId: string, updates: Partial<ProviderModel>) => {
    const normalizedUpdates: Partial<ProviderModel> = { ...updates };
    if (normalizedUpdates.contextLength !== undefined) {
      normalizedUpdates.contextLength = normalizeContextLength(normalizedUpdates.contextLength);
    }
    if (normalizedUpdates.maxOutputTokens !== undefined) {
      normalizedUpdates.maxOutputTokens = normalizeModelMaxOutputTokens(normalizedUpdates.maxOutputTokens);
    }

    const providers = get().providers.map((p) =>
      p.id === providerId
        ? {
            ...p,
            models: p.models.map((m) =>
              m.id === modelId ? normalizeProviderModel({ ...m, ...normalizedUpdates }) : m,
            ),
          }
        : p,
    );
    saveProviders(providers);
    set({ providers });

    const state = get();
    const activeProvider = providers.find((provider) => provider.id === providerId);
    if (!activeProvider || state.activeProviderId !== providerId) {
      return;
    }
    const activeEntry = state.activeModelId
      ? state.configuredModels.find((entry) => entry.id === state.activeModelId)
      : null;
    const selectedProviderMatches = activeEntry
      ? activeEntry.provider === providerId || activeEntry.provider === activeProvider.type
      : true;
    const isActiveModel = selectedProviderMatches
      && (
        activeEntry
          ? activeEntry.model === modelId
          : state.currentModel === modelId
      );
    if (!isActiveModel) {
      return;
    }

    const activeModel = activeProvider.models.find((model) => model.id === modelId);
    if (!activeModel) {
      return;
    }
    const modelEndpoints = buildLocalPoolModelEndpoints(activeProvider, activeModel);
    const nextActiveEndpointIndex = modelEndpoints.length > 0 ? 0 : null;
    const visionFallback = resolveVisionFallbackConfig(activeModel, providers);
    if (activeProvider.type === "local-pool") {
      set({ activeEndpointIndex: nextActiveEndpointIndex });
    }
    const edits: { keyPath: string; value: unknown; mergeStrategy: string }[] = [
      {
        keyPath: "model_context_window",
        value: modelContextLengthOrDefault(activeModel),
        mergeStrategy: "replace",
      },
      {
        keyPath: "max_output_tokens",
        value: modelMaxOutputTokensOrDefault(activeModel),
        mergeStrategy: "replace",
      },
      ...buildVisionFallbackConfigEdits(visionFallback),
    ];
    if (activeProvider.type === "local-pool") {
      edits.push(
        { keyPath: "model_endpoints", value: modelEndpoints, mergeStrategy: "replace" },
        { keyPath: "active_endpoint_index", value: nextActiveEndpointIndex, mergeStrategy: "replace" },
      );
    }
    standaloneConfigWrite(edits).catch((err) => {
      console.error("Failed to sync active model config:", err);
    });
  },

  removeProviderModel: (providerId: string, modelId: string) => {
    const providers = get().providers.map((p) =>
      p.id === providerId
        ? { ...p, models: p.models.filter((m) => m.id !== modelId) }
        : p,
    );
    saveProviders(providers);
    set({ providers });
  },

  getActiveProvider: () => {
    const { providers, activeProviderId } = get();
    return providers.find((p) => p.id === activeProviderId) ?? null;
  },

  // ─── 附件管理 ─────────────────────────────────────────
  setAttachedFiles: (files) => set({ attachedFiles: files }),
  addAttachedFile: (file) => set((s) => ({ attachedFiles: [...s.attachedFiles, file] })),
  removeAttachedFile: (index) =>
    set((s) => ({ attachedFiles: s.attachedFiles.filter((_, i) => i !== index) })),
  clearAttachedFiles: () => set({ attachedFiles: [] }),
  queueComposerInsert: (text) =>
    set((state) => {
      const payload = text.trim();
      if (!payload) {
        return {};
      }
      // 支持连续注入：若输入框尚未消费上一段内容，则按空行拼接，避免覆盖。
      return {
        pendingComposerInsert: state.pendingComposerInsert
          ? `${state.pendingComposerInsert}\n\n${payload}`
          : payload,
      };
    }),
  consumeComposerInsert: () => {
    const current = get().pendingComposerInsert;
    if (current) {
      set({ pendingComposerInsert: null });
    }
    return current;
  },

  enqueueMessage: (msg) =>
    set((s) => ({ pendingMessageQueue: [...s.pendingMessageQueue, msg] })),
  dequeueMessage: () => {
    const queue = get().pendingMessageQueue;
    if (queue.length === 0) return null;
    const [first, ...rest] = queue;
    set({ pendingMessageQueue: rest });
    return first;
  },
  removeQueuedMessage: (id) =>
    set((s) => ({
      pendingMessageQueue: s.pendingMessageQueue.filter((m) => m.id !== id),
    })),
  clearMessageQueue: () => set({ pendingMessageQueue: [] }),

  setThreads: (threads) => set({ threads }),
  addThread: (thread) => {
    const projectId = get().currentProjectId;
    if (projectId && !thread.projectId) {
      thread.projectId = projectId;
    }
    const effectivePid = thread.projectId ?? projectId;
    if (effectivePid) {
      const map = { ...get().threadProjectMap, [thread.id]: effectivePid };
      saveThreadProjectMap(map);
      set((s) => ({ threads: [thread, ...s.threads], threadProjectMap: map }));
    } else {
      set((s) => ({ threads: [thread, ...s.threads] }));
    }
  },
  updateThreadSummary: (threadId, patch) =>
    set((state) => {
      const index = state.threads.findIndex((thread) => thread.id === threadId);
      if (index < 0) {
        return {};
      }

      const current = state.threads[index];
      const nextName = patch.name !== undefined ? patch.name : current.name;
      const nextPreview = patch.preview !== undefined ? patch.preview : current.preview;
      const nextUpdatedAt = patch.updatedAt ?? current.updatedAt;

      // 避免无意义 setState：字段都没变化时直接跳过，减少渲染抖动。
      if (
        nextName === current.name &&
        nextPreview === current.preview &&
        nextUpdatedAt === current.updatedAt
      ) {
        return {};
      }

      const nextThread: ThreadSummary = {
        ...current,
        name: nextName,
        preview: nextPreview,
        updatedAt: nextUpdatedAt,
      };
      const threads = [...state.threads];
      threads[index] = nextThread;
      return { threads };
    }),

  deleteThread: (threadId: string) => {
    const isCurrent = get().currentThreadId === threadId;
    // 从线程列表移除
    const threads = get().threads.filter((t) => t.id !== threadId);
    // 从映射中移除
    const tpMap = { ...get().threadProjectMap };
    delete tpMap[threadId];
    saveThreadProjectMap(tpMap);
    // 清理该线程的运行时状态快照
    get().deleteThreadRuntimeState(threadId);
    // 同步清理对话级偏好（含小程序绑定），避免线程删除后残留孤儿配置
    const nextPrefs = { ...get().threadPreferences };
    let prefsChanged = false;
    if (nextPrefs[threadId]) {
      delete nextPrefs[threadId];
      prefsChanged = true;
      saveThreadPreferences(nextPrefs);
    }
    set({
      threads,
      threadProjectMap: tpMap,
      ...(prefsChanged ? { threadPreferences: nextPrefs } : {}),
      ...(isCurrent
        ? {
          currentThreadId: null,
          messages: [],
          streamingText: "",
          streamingLabel: "",
          isStreaming: false,
          liveTurnUsage: null,
          currentTurnId: null,
          latestPlanContent: null,
          activePlan: null,
          currentGoal: null,
          pendingMessageQueue: [],
          pendingFileReviews: {},
        }
        : {}),
    });
    threadArchive(threadId).catch(() => {});
  },

  setMessages: (messages) => set({ messages }),
  addMessage: (message) => {
    set((s) => ({ messages: [...s.messages, message] }));
  },
  truncateMessagesFrom: (messageId) => {
    set((s) => {
      const idx = s.messages.findIndex((m) => m.id === messageId);
      if (idx < 0) return {};
      return {
        messages: s.messages.slice(0, idx),
        streamingText: "",
        streamingLabel: "",
      };
    });
  },
  setLiveTurnUsage: (usage) => set({ liveTurnUsage: usage }),
  updateToolCallStatus: (toolId, status, output?) =>
    set((s) => {
      const msgs = [...s.messages];
      for (let i = msgs.length - 1; i >= 0; i--) {
        const tc = msgs[i].toolCalls;
        if (!tc) continue;
        const idx = tc.findIndex((c) => c.id === toolId);
        if (idx >= 0) {
          const updatedCalls = [...tc];
          updatedCalls[idx] = { ...updatedCalls[idx], status, ...(output !== undefined ? { output } : {}) };
          msgs[i] = { ...msgs[i], toolCalls: updatedCalls };
          return { messages: msgs };
        }
      }
      return {};
    }),
  updateToolCallPatchProgress: (toolId, changes) =>
    set((s) => {
      const normalized = changes.filter((change) => change.path.trim());
      if (!normalized.length) {
        return {};
      }

      const msgs = [...s.messages];
      for (let i = msgs.length - 1; i >= 0; i--) {
        const tc = msgs[i].toolCalls;
        if (!tc) continue;
        const idx = tc.findIndex((c) => c.id === toolId);
        if (idx >= 0) {
          const updatedCalls = [...tc];
          updatedCalls[idx] = { ...updatedCalls[idx], patchProgress: normalized };
          msgs[i] = { ...msgs[i], toolCalls: updatedCalls };
          return { messages: msgs };
        }
      }
      return {};
    }),
  upsertPendingFileReview: (review) =>
    set((state) => {
      const normalized = normalizePendingFileReview(review);
      return {
        pendingFileReviews: {
          ...state.pendingFileReviews,
          [normalized.callId]: normalized,
        },
      };
    }),
  removePendingFileReview: (callId) =>
    set((state) => {
      if (!state.pendingFileReviews[callId]) {
        return {};
      }
      const next = { ...state.pendingFileReviews };
      delete next[callId];
      return { pendingFileReviews: next };
    }),
  setPendingFileReviewSelectedPath: (callId, path) =>
    set((state) => {
      const review = state.pendingFileReviews[callId];
      if (!review) return {};
      if (!review.files.some((file) => file.path === path)) return {};
      if (review.selectedPath === path) return {};
      return {
        pendingFileReviews: {
          ...state.pendingFileReviews,
          [callId]: { ...review, selectedPath: path },
        },
      };
    }),
  setPendingFileReviewFileKeep: (callId, path, keep) =>
    set((state) => {
      const review = state.pendingFileReviews[callId];
      if (!review) return {};
      let changed = false;
      const files = review.files.map((file) => {
        if (file.path !== path) {
          return file;
        }
        if (file.keep === keep) {
          return file;
        }
        changed = true;
        return { ...file, keep };
      });
      if (!changed) {
        return {};
      }
      return {
        pendingFileReviews: {
          ...state.pendingFileReviews,
          [callId]: {
            ...review,
            files,
            keepAll: files.length > 0 && files.every((file) => file.keep),
          },
        },
      };
    }),
  setPendingFileReviewKeepAll: (callId, keepAll) =>
    set((state) => {
      const review = state.pendingFileReviews[callId];
      if (!review) return {};
      const files = review.files.map((file) => ({ ...file, keep: keepAll }));
      return {
        pendingFileReviews: {
          ...state.pendingFileReviews,
          [callId]: { ...review, files, keepAll },
        },
      };
    }),
  setPendingFileReviewEditedContent: (callId, path, content) =>
    set((state) => {
      const review = state.pendingFileReviews[callId];
      if (!review) return {};
      let changed = false;
      const files = review.files.map((file) => {
        if (file.path !== path || file.action === "deleted") {
          return file;
        }
        if ((file.editedContent ?? file.candidateContent ?? "") === content) {
          return file;
        }
        changed = true;
        const editedContent = file.candidateContent === content ? undefined : content;
        return {
          ...file,
          ...(editedContent !== undefined ? { editedContent } : { editedContent: undefined }),
        };
      });
      if (!changed) {
        return {};
      }
      return {
        pendingFileReviews: {
          ...state.pendingFileReviews,
          [callId]: { ...review, files, status: "pending", error: undefined },
        },
      };
    }),
  setPendingFileReviewStatus: (callId, status, error) =>
    set((state) => {
      const review = state.pendingFileReviews[callId];
      if (!review) return {};
      if (review.status === status && review.error === error) return {};
      return {
        pendingFileReviews: {
          ...state.pendingFileReviews,
          [callId]: { ...review, status, error },
        },
      };
    }),
  markRunningToolCallsInterrupted: (reason = "Tool interrupted by user.") =>
    set((s) => {
      let changed = false;
      const messages = s.messages.map((message) => {
        if (!message.toolCalls?.length) {
          return message;
        }
        let messageChanged = false;
        const nextCalls = message.toolCalls.map((toolCall) => {
          if (toolCall.status !== "running") {
            return toolCall;
          }
          changed = true;
          messageChanged = true;
          return {
            ...toolCall,
            status: "failed" as const,
            output: toolCall.output ?? reason,
          };
        });
        return messageChanged ? { ...message, toolCalls: nextCalls } : message;
      });

      if (!changed && s.browserPanelStatus !== "running") {
        return {};
      }

      return {
        messages: changed ? messages : s.messages,
        ...(s.browserPanelStatus === "running"
          ? { browserPanelStatus: "failed" as const }
          : {}),
      };
    }),
  appendStreamingText: (delta) =>
    set((s) => ({ streamingText: s.streamingText + delta })),
  clearStreamingText: () => set({ streamingText: "", streamingLabel: "" }),
  setStreaming: (v) => set(v ? { isStreaming: true } : { isStreaming: false, streamingLabel: "" }),
  flushAndStopStreaming: (options) =>
    set((state) => {
      const commitStreamingText = options?.commitStreamingText === true;
      const text = state.streamingText;
      const shouldCommit = commitStreamingText && text.length > 0;
      if (
        !shouldCommit &&
        !state.isStreaming &&
        text.length === 0 &&
        state.streamingLabel.length === 0 &&
        !state.currentTurnId
      ) {
        return {};
      }
      return {
        ...(shouldCommit
          ? {
            messages: [
              ...state.messages,
              {
                id: crypto.randomUUID(),
                role: "assistant" as const,
                content: text,
                timestamp: Date.now(),
              },
            ],
          }
          : {}),
        streamingText: "",
        streamingLabel: "",
        isStreaming: false,
        currentTurnId: null,
        liveTurnUsage: null,
      };
    }),
  setStreamingLabel: (label) => set({ streamingLabel: label }),
  setChatMode: (mode) => set({ chatMode: mode }),
  setLatestPlanContent: (content) => set({ latestPlanContent: content }),
  setActivePlan: (plan) => set({ activePlan: plan }),
  setCurrentGoal: (goal) => set({ currentGoal: normalizeThreadGoal(goal) }),
  setShowSettings: (v, options) =>
    set((state) =>
      v
        ? {
          showSettings: true,
          settingsInitialTab: options?.tab?.trim() || null,
          // 设置弹层打开时强制隐藏右侧区域，避免内置浏览器覆盖在最上层。
          rightPanelVisible: false,
        }
        : {
          showSettings: false,
          settingsInitialTab: null,
          // 关闭设置时保持当前右侧状态（不自动恢复）。
          rightPanelVisible: state.rightPanelVisible,
        }),
  setAutoApprove: (v) => {
    void appStateSet(AUTO_APPROVE_KEY, String(v));
    set({ autoApprove: v });
  },
  setSidebarTab: (tab) => {
    void appStateSet("sidebar-tab", tab);
    set({ sidebarTab: tab });
  },
  setSelectedRobotId: (id) => {
    set({ selectedRobotId: id, robotCreateMode: false });
    if (id) {
      set({ chatMode: "goal" });
    }
  },
  setRobotCreateMode: (v) => {
    set({ robotCreateMode: v, selectedRobotId: null });
  },
  setWorkflowExtractThreadId: (id) => {
    set({ workflowExtractThreadId: id });
  },
  setThreadModelOverride: (providerId, modelId) => {
    const state = get();
    const threadId = state.currentThreadId;
    const nextPrefs = { ...state.threadPreferences };
    if (threadId) {
      const nextPref = buildThreadPreferencePatch(nextPrefs[threadId], {
        overrideProviderId: providerId,
        overrideModelId: modelId,
      });
      if (nextPref) {
        nextPrefs[threadId] = nextPref;
      } else {
        delete nextPrefs[threadId];
      }
      saveThreadPreferences(nextPrefs);
    }
    set({
      overrideProviderId: providerId,
      overrideModelId: modelId,
      threadPreferences: nextPrefs,
    });
    if (threadId) {
      get().applyToThread(threadId, () => ({
        overrideProviderId: providerId,
        overrideModelId: modelId,
      }));
    }
  },
  setThreadSmartbrainEnabled: (enabled) => {
    const state = get();
    const threadId = state.currentThreadId;
    const nextPrefs = { ...state.threadPreferences };
    if (threadId) {
      const nextPref = buildThreadPreferencePatch(nextPrefs[threadId], {
        smartbrainEnabled: enabled,
      });
      if (nextPref) {
        nextPrefs[threadId] = nextPref;
      } else {
        delete nextPrefs[threadId];
      }
      saveThreadPreferences(nextPrefs);
    }
    set({
      smartbrainEnabled: enabled,
      threadPreferences: nextPrefs,
    });
    if (threadId) {
      get().applyToThread(threadId, () => ({
        smartbrainEnabled: enabled,
      }));
    }
  },
  setThreadSubagentEnabled: (enabled) => {
    const state = get();
    const threadId = state.currentThreadId;
    const nextPrefs = { ...state.threadPreferences };
    if (threadId) {
      const nextPref = buildThreadPreferencePatch(nextPrefs[threadId], {
        subagentEnabled: enabled,
      });
      if (nextPref) {
        nextPrefs[threadId] = nextPref;
      } else {
        delete nextPrefs[threadId];
      }
      saveThreadPreferences(nextPrefs);
    }
    set({
      subagentEnabled: enabled,
      threadPreferences: nextPrefs,
    });
    if (threadId) {
      get().applyToThread(threadId, () => ({
        subagentEnabled: enabled,
      }));
    }
  },
  bindThreadMiniapp: (threadId, binding) => {
    const state = get();
    const nextPrefs = { ...state.threadPreferences };
    const nextPref = buildThreadPreferencePatch(nextPrefs[threadId], {
      miniappSlug: binding?.slug ?? null,
      miniappName: binding?.name ?? null,
      miniappRootPath: binding?.rootPath ?? null,
    });
    if (nextPref) {
      nextPrefs[threadId] = nextPref;
    } else {
      delete nextPrefs[threadId];
    }
    saveThreadPreferences(nextPrefs);
    set({ threadPreferences: nextPrefs });
  },
  buildThreadChatProviderOverride: (providerId, modelId) => {
    const state = get();
    return buildThreadChatProviderOverrideSnapshot(
      state.providers,
      // 仅在参数省略时回退到当前 override；显式 null 表示“不使用 override”。
      providerId === undefined ? state.overrideProviderId : providerId,
      modelId === undefined ? state.overrideModelId : modelId,
    );
  },
  setSmartbrainExtractionStatus: (state) =>
    set({
      ...(state.running !== undefined
        ? { smartbrainExtractionRunning: state.running }
        : {}),
      ...(state.label !== undefined
        ? { smartbrainExtractionLabel: state.label }
        : {}),
      ...(state.progress !== undefined
        ? { smartbrainExtractionProgress: state.progress }
        : {}),
    }),
  setRightPanelVisible: (v) => set({ rightPanelVisible: v }),
  toggleRightPanel: () => set((s) => ({ rightPanelVisible: !s.rightPanelVisible })),
  setRightPanelTab: (tab) => set({ rightPanelTab: tab, rightPanelVisible: true }),
  setSidebarWidth: (width) => {
    const clamped = clampPanelWidth(width, SIDEBAR_WIDTH_MIN, SIDEBAR_WIDTH_MAX);
    saveSidebarWidth(clamped);
    set({ sidebarWidth: clamped });
  },
  setRightPanelWidth: (width) => {
    const clamped = clampPanelWidth(width, RIGHT_PANEL_WIDTH_MIN, RIGHT_PANEL_WIDTH_MAX);
    saveRightPanelWidth(clamped);
    set({ rightPanelWidth: clamped });
  },
  setBrowserPanelState: (state) =>
    set({
      ...(state.url !== undefined ? { browserPanelUrl: state.url } : {}),
      ...(state.title !== undefined ? { browserPanelTitle: state.title } : {}),
      ...(state.status !== undefined ? { browserPanelStatus: state.status } : {}),
      ...(state.canGoBack !== undefined ? { browserCanGoBack: state.canGoBack } : {}),
      ...(state.canGoForward !== undefined ? { browserCanGoForward: state.canGoForward } : {}),
    }),
  triggerBrowserSync: () =>
    set((s) => ({ browserSyncTrigger: s.browserSyncTrigger + 1 })),
  setBrowserActive: (v) => set({ browserActive: v }),
  setBrowserDetached: (v) => set({ browserDetached: v }),

  resolveThreadCwd: (threadId) => {
    const state = get();
    if (threadId) {
      const miniappCwd = state.threadPreferences[threadId]?.miniappRootPath?.trim();
      if (miniappCwd) return miniappCwd;
      const projectId = state.threadProjectMap[threadId];
      if (projectId && projectId !== GENERAL_PROJECT_ID) {
        const projectCwd = state.projects.find((p) => p.id === projectId)?.cwd?.trim();
        if (projectCwd) return projectCwd;
      }
    }
    return state.workspaceCwd || state.projectRoot || state.userHomeDir;
  },

  syncWorkspaceForThread: (threadId) => {
    const state = get();
    const projectId =
      state.threadProjectMap[threadId] ??
      state.threads.find((t) => t.id === threadId)?.projectId;
    if (!projectId) return;
    if (projectId === GENERAL_PROJECT_ID) {
      const cwd = state.projectRoot ?? state.userHomeDir;
      if (state.currentProjectId === GENERAL_PROJECT_ID && state.workspaceCwd === cwd) return;
      saveActiveProject(GENERAL_PROJECT_ID, cwd);
      set({
        currentProjectId: GENERAL_PROJECT_ID,
        workspaceCwd: cwd,
      });
      return;
    }
    const project = state.projects.find((p) => p.id === projectId);
    if (!project) return;
    if (state.currentProjectId === projectId && state.workspaceCwd === project.cwd) return;
    saveActiveProject(projectId, project.cwd);
    set({
      currentProjectId: projectId,
      workspaceCwd: project.cwd,
    });
  },

  createThread: async () => {
    try {
      // 创建新线程前保存当前对话的运行时状态。
      get().saveCurrentThreadRuntimeState();
      // await 前固化项目绑定，避免创建过程中切项目导致 thread 挂错目录。
      const projectId = get().currentProjectId;
      const resp = await standaloneThreadCreate();
      const threadId = resp?.thread?.id ?? null;
      if (threadId) {
        set({
          currentThreadId: threadId,
          messages: [],
          streamingText: "",
          streamingLabel: "",
          isStreaming: false,
          liveTurnUsage: null,
          currentTurnId: null,
          currentGoal: null,
          latestPlanContent: null,
          activePlan: null,
          selectedRobotId: null,
          robotCreateMode: false,
          pendingMessageQueue: [],
          pendingFileReviews: {},
          liveSubagents: {},
          // 新对话默认继承全局供应商/模型，本地知识库默认关闭。
          overrideProviderId: null,
          overrideModelId: null,
          smartbrainEnabled: false,
          subagentEnabled: false,
        });
        const newThread: ThreadSummary = {
          id: threadId,
          preview: "",
          updatedAt: Date.now(),
          projectId: projectId ?? undefined,
        };
        if (projectId) {
          const map = { ...get().threadProjectMap, [threadId]: projectId };
          saveThreadProjectMap(map);
          set((s) => ({
            threads: [newThread, ...s.threads.filter((thread) => thread.id !== threadId)],
            threadProjectMap: map,
          }));
        } else {
          set((s) => ({
            threads: [newThread, ...s.threads.filter((thread) => thread.id !== threadId)],
          }));
        }
      }
      return threadId;
    } catch (err) {
      console.error("Failed to create thread:", err);
      return null;
    }
  },

  loadThreads: async () => {
    try {
      const resp = await standaloneThreadList();
      const tpMap = get().threadProjectMap;
      const knownIds = new Set(Object.keys(tpMap));
      const backendThreads = resp?.data ?? [];

      const toDelete = backendThreads.filter((t) => !knownIds.has(t.id));
      for (const t of toDelete) {
        threadArchive(t.id).catch(() => {});
      }

      const threads: ThreadSummary[] = backendThreads
        .filter((t) => knownIds.has(t.id))
        .map((t) => ({
          id: t.id,
          name: t.name,
          preview: t.preview ?? t.name ?? "",
          updatedAt: t.updatedAt ?? Date.now(),
          projectId: tpMap[t.id],
        }));
      set({ threads });
    } catch (err) {
      console.error("Failed to load threads:", err);
    }
  },

  loadThread: async (threadId: string) => {
    // 切换前保存当前线程的运行时状态。
    get().saveCurrentThreadRuntimeState();
    // 先按 thread→project 同步 UI 工作区，避免消息是 A、文件树仍是 B。
    get().syncWorkspaceForThread(threadId);

    // 若该线程已有运行时状态快照（含后台事件更新），直接恢复，跳过后端加载。
    // 这保留了队列、流式文本、工具调用等在切换期间累积的上下文。
    if (get().restoreThreadRuntimeState(threadId)) {
      set({ currentThreadId: threadId });
      return;
    }

    try {
      const resp = await standaloneThreadRead(threadId);
      const rawThread = resp?.thread as RawThread | undefined;
      const turns = rawThread?.turns ?? [];
      const activePlan = normalizePlanFile(rawThread?.activePlan);
      const { messages, activePlan: hydratedActivePlan } = mapTurnsToMessages(
        turns as RawTurn[],
        activePlan,
      );

      // 加载历史消息后，将所有残留的 running 工具标记为 success，
      // 防止恢复老对话时出现永远转圈的工具卡片。
      const cleanedMessages = messages.map((msg) => {
        if (!msg.toolCalls?.length) return msg;
        const hasRunning = msg.toolCalls.some((tc) => tc.status === "running");
        if (!hasRunning) return msg;
        return {
          ...msg,
          toolCalls: msg.toolCalls.map((tc) =>
            tc.status === "running" ? { ...tc, status: "success" as const } : tc,
          ),
        };
      });

      const pref = readThreadPreference(get().threadPreferences, threadId);
      set({
        currentThreadId: threadId,
        currentTurnId: null,
        messages: cleanedMessages,
        streamingText: "",
        streamingLabel: "",
        isStreaming: false,
        liveTurnUsage: null,
        latestPlanContent: hydratedActivePlan?.content ?? null,
        activePlan: hydratedActivePlan,
        currentGoal: normalizeThreadGoal(
          rawThread?.goal
            ? { ...rawThread.goal, workflowProgress: rawThread.robotState ?? undefined }
            : null,
        ),
        selectedRobotId: null,
        robotCreateMode: false,
        pendingMessageQueue: [],
        pendingFileReviews: {},
        liveSubagents: {},
        browserPanelUrl: null,
        browserPanelTitle: null,
        browserPanelStatus: "idle",
        browserCanGoBack: false,
        browserCanGoForward: false,
        browserActive: false,
        browserDetached: false,
        overrideProviderId: pref.overrideProviderId ?? null,
        overrideModelId: pref.overrideModelId ?? null,
        smartbrainEnabled: pref.smartbrainEnabled ?? false,
        subagentEnabled: pref.subagentEnabled ?? false,
      });
      if (rawThread?.id) {
        set((state) => {
          const existing = state.threads.find((thread) => thread.id === threadId);
          const nextThread: ThreadSummary = {
            id: threadId,
            name: rawThread.name ?? existing?.name,
            preview: existing?.preview ?? "",
            updatedAt: existing?.updatedAt ?? Date.now(),
            projectId: existing?.projectId,
          };
          const threads = existing
            ? state.threads.map((thread) => (thread.id === threadId ? nextThread : thread))
            : [nextThread, ...state.threads];
          return { threads };
        });
      }
    } catch (err) {
      console.error("Failed to load thread:", err);
      set({
        currentThreadId: threadId,
        currentTurnId: null,
        messages: [],
        streamingText: "",
        streamingLabel: "",
        isStreaming: false,
        liveTurnUsage: null,
        currentGoal: null,
        latestPlanContent: null,
        activePlan: null,
        pendingMessageQueue: [],
        pendingFileReviews: {},
        liveSubagents: {},
        browserPanelUrl: null,
        browserPanelTitle: null,
        browserPanelStatus: "idle",
        browserCanGoBack: false,
        browserCanGoForward: false,
        browserActive: false,
        browserDetached: false,
      });
    }
  },

  // ─── Per-thread 运行时状态管理实现 ──────────────────────────
  saveCurrentThreadRuntimeState: () => {
    const state = get();
    const threadId = state.currentThreadId;
    if (!threadId) return;
    const snapshot = assembleRuntimeStateFromStore(state);
    set((s) => ({
      threadRuntimeStates: pruneThreadRuntimeStates({
        ...s.threadRuntimeStates,
        [threadId]: snapshot,
      }),
    }));
  },

  restoreThreadRuntimeState: (threadId) => {
    const saved = get().threadRuntimeStates[threadId];
    if (!saved) return false;
    const pref = readThreadPreference(get().threadPreferences, threadId);
    set({
      messages: saved.messages,
      streamingText: saved.streamingText,
      streamingLabel: saved.streamingLabel,
      isStreaming: saved.isStreaming,
      liveTurnUsage: saved.liveTurnUsage,
      currentTurnId: saved.currentTurnId,
      pendingMessageQueue: saved.pendingMessageQueue,
      pendingFileReviews: saved.pendingFileReviews,
      currentGoal: saved.currentGoal,
      activePlan: saved.activePlan,
      latestPlanContent: saved.latestPlanContent,
      chatMode: saved.chatMode,
      selectedRobotId: saved.selectedRobotId,
      robotCreateMode: saved.robotCreateMode,
      robotWaitCountdown: saved.robotWaitCountdown,
      liveSubagents: saved.liveSubagents ?? {},
      browserPanelUrl: saved.browserPanelUrl,
      browserPanelTitle: saved.browserPanelTitle,
      browserPanelStatus: saved.browserPanelStatus,
      browserCanGoBack: saved.browserCanGoBack,
      browserCanGoForward: saved.browserCanGoForward,
      browserActive: saved.browserActive,
      browserDetached: saved.browserDetached,
      overrideProviderId: saved.overrideProviderId ?? pref.overrideProviderId ?? null,
      overrideModelId: saved.overrideModelId ?? pref.overrideModelId ?? null,
      smartbrainEnabled: saved.smartbrainEnabled ?? pref.smartbrainEnabled ?? false,
      subagentEnabled: saved.subagentEnabled ?? pref.subagentEnabled ?? false,
    });
    return true;
  },

  applyToThread: (threadId, updater) => {
    const state = get();
    if (threadId === state.currentThreadId) {
      const draft = assembleRuntimeStateFromStore(state);
      const updates = updater(draft);
      set(updates as Partial<AppState>);
      return updates;
    }
    const existing = state.threadRuntimeStates[threadId] ?? createDefaultThreadRuntimeState();
    const updates = updater({ ...existing });
    const nextState: ThreadRuntimeState = {
      ...existing,
      ...updates,
      updatedAt: Date.now(),
    };
    set((s) => ({
      threadRuntimeStates: pruneThreadRuntimeStates({
        ...s.threadRuntimeStates,
        [threadId]: nextState,
      }),
    }));
    return updates;
  },

  getThreadRuntimeState: (threadId) => {
    const state = get();
    if (threadId === state.currentThreadId) {
      return assembleRuntimeStateFromStore(state);
    }
    return state.threadRuntimeStates[threadId] ?? null;
  },

  deleteThreadRuntimeState: (threadId) => {
    set((s) => {
      const next = { ...s.threadRuntimeStates };
      delete next[threadId];
      return { threadRuntimeStates: next };
    });
  },

  isThreadStreaming: (threadId) => {
    const state = get();
    if (threadId === state.currentThreadId) {
      return state.isStreaming;
    }
    return state.threadRuntimeStates[threadId]?.isStreaming ?? false;
  },

  // ─── Per-thread 方法族实现 ──────────────────────────────────
  addMessageToThread: (threadId, message) => {
    get().applyToThread(threadId, (draft) => ({
      messages: [...draft.messages, message],
    }));
  },

  appendToolCallsToThread: (threadId, items) => {
    if (!items.length) return null;
    let messageId: string | null = null;
    get().applyToThread(threadId, (draft) => {
      const msgs = [...draft.messages];
      const openIdx = findOpenToolCallGroupIndex(msgs);
      if (openIdx >= 0) {
        const existing = msgs[openIdx];
        messageId = existing.id;
        msgs[openIdx] = {
          ...existing,
          toolCalls: mergeToolCallItems(existing.toolCalls ?? [], items),
        };
        return { messages: msgs };
      }

      messageId = `tcg-${crypto.randomUUID()}`;
      msgs.push({
        id: messageId,
        role: "system",
        content: "",
        timestamp: Date.now(),
        toolCalls: items,
      });
      return { messages: msgs };
    });
    return messageId;
  },

  setStreamingForThread: (threadId, v) => {
    get().applyToThread(threadId, () => ({
      isStreaming: v,
      ...(v ? {} : { streamingLabel: "" }),
    }));
  },

  appendStreamingTextForThread: (threadId, delta) => {
    get().applyToThread(threadId, (draft) => ({
      streamingText: draft.streamingText + delta,
    }));
  },

  clearStreamingTextForThread: (threadId) => {
    get().applyToThread(threadId, () => ({
      streamingText: "",
      streamingLabel: "",
    }));
  },

  setStreamingLabelForThread: (threadId, label) => {
    get().applyToThread(threadId, () => ({
      streamingLabel: label,
    }));
  },

  flushAndStopStreamingForThread: (threadId, options) => {
    get().applyToThread(threadId, (draft) => {
      const commitStreamingText = options?.commitStreamingText === true;
      const text = draft.streamingText;
      const shouldCommit = commitStreamingText && text.length > 0;
      if (
        !shouldCommit &&
        !draft.isStreaming &&
        text.length === 0 &&
        draft.streamingLabel.length === 0 &&
        !draft.currentTurnId
      ) {
        return {};
      }
      return {
        ...(shouldCommit
          ? {
              messages: [
                ...draft.messages,
                {
                  id: crypto.randomUUID(),
                  role: "assistant" as const,
                  content: text,
                  timestamp: Date.now(),
                },
              ],
            }
          : {}),
        streamingText: "",
        streamingLabel: "",
        isStreaming: false,
        currentTurnId: null,
        liveTurnUsage: null,
      };
    });
  },

  setCurrentTurnIdForThread: (threadId, id) => {
    get().applyToThread(threadId, () => ({
      currentTurnId: id,
    }));
  },

  setLiveTurnUsageForThread: (threadId, usage) => {
    get().applyToThread(threadId, () => ({
      liveTurnUsage: usage,
    }));
  },

  setCurrentGoalForThread: (threadId, goal) => {
    get().applyToThread(threadId, (runtime) => ({
      currentGoal: goal
        ? normalizeThreadGoal(goal, runtime.currentGoal?.workflowProgress)
        : null,
    }));
  },

  updateToolCallStatusForThread: (threadId, toolId, status, output?) => {
    get().applyToThread(threadId, (draft) => {
      const msgs = [...draft.messages];
      for (let i = msgs.length - 1; i >= 0; i--) {
        const tc = msgs[i].toolCalls;
        if (!tc) continue;
        const idx = tc.findIndex((c) => c.id === toolId);
        if (idx >= 0) {
          const updatedCalls = [...tc];
          updatedCalls[idx] = {
            ...updatedCalls[idx],
            status,
            ...(output !== undefined ? { output } : {}),
          };
          msgs[i] = { ...msgs[i], toolCalls: updatedCalls };
          return { messages: msgs };
        }
      }
      return {};
    });
  },

  updateToolCallPatchProgressForThread: (threadId, toolId, changes) => {
    const normalized = changes.filter((change) => change.path.trim());
    if (!normalized.length) return;
    get().applyToThread(threadId, (draft) => {
      const msgs = [...draft.messages];
      for (let i = msgs.length - 1; i >= 0; i--) {
        const tc = msgs[i].toolCalls;
        if (!tc) continue;
        const idx = tc.findIndex((c) => c.id === toolId);
        if (idx >= 0) {
          const updatedCalls = [...tc];
          updatedCalls[idx] = { ...updatedCalls[idx], patchProgress: normalized };
          msgs[i] = { ...msgs[i], toolCalls: updatedCalls };
          return { messages: msgs };
        }
      }
      return {};
    });
  },

  markRunningToolCallsInterruptedForThread: (threadId, reason = "Tool interrupted by user.") => {
    get().applyToThread(threadId, (draft) => {
      let changed = false;
      const messages = draft.messages.map((message) => {
        if (!message.toolCalls?.length) {
          return message;
        }
        let messageChanged = false;
        const nextCalls = message.toolCalls.map((toolCall) => {
          if (toolCall.status !== "running") {
            return toolCall;
          }
          changed = true;
          messageChanged = true;
          return {
            ...toolCall,
            status: "failed" as const,
            output: toolCall.output ?? reason,
          };
        });
        return messageChanged ? { ...message, toolCalls: nextCalls } : message;
      });
      if (!changed) return {};
      return { messages };
    });
  },

  upsertPendingFileReviewForThread: (threadId, review) => {
    const normalized = normalizePendingFileReview(review);
    get().applyToThread(threadId, (draft) => ({
      pendingFileReviews: {
        ...draft.pendingFileReviews,
        [normalized.callId]: normalized,
      },
    }));
  },

  setPendingFileReviewStatusForThread: (threadId, callId, status, error?) => {
    get().applyToThread(threadId, (draft) => {
      const review = draft.pendingFileReviews[callId];
      if (!review) return {};
      if (review.status === status && review.error === error) return {};
      return {
        pendingFileReviews: {
          ...draft.pendingFileReviews,
          [callId]: { ...review, status, error },
        },
      };
    });
  },

  removePendingFileReviewForThread: (threadId, callId) => {
    get().applyToThread(threadId, (draft) => {
      if (!draft.pendingFileReviews[callId]) return {};
      const next = { ...draft.pendingFileReviews };
      delete next[callId];
      return { pendingFileReviews: next };
    });
  },

  dequeueMessageForThread: (threadId) => {
    let dequeued: QueuedMessage | null = null;
    get().applyToThread(threadId, (draft) => {
      if (draft.pendingMessageQueue.length === 0) return {};
      const [first, ...rest] = draft.pendingMessageQueue;
      dequeued = first;
      return { pendingMessageQueue: rest };
    });
    return dequeued;
  },

  enqueueMessageToThread: (threadId, msg) => {
    get().applyToThread(threadId, (draft) => ({
      pendingMessageQueue: [...draft.pendingMessageQueue, msg],
    }));
  },

  setRobotWaitCountdownForThread: (threadId, v) => {
    get().applyToThread(threadId, () => ({
      robotWaitCountdown: v,
    }));
  },

  upsertLiveSubagentForThread: (threadId, agent) => {
    if (!threadId || !agent?.id) return;
    get().applyToThread(threadId, (draft) => {
      const existing = draft.liveSubagents?.[agent.id];
      if (existing && agent.updatedAt < existing.updatedAt) {
        return {};
      }
      return {
        liveSubagents: {
          ...(draft.liveSubagents ?? {}),
          [agent.id]: {
            ...existing,
            ...agent,
            role: agent.role || existing?.role || "agent",
            prompt: agent.prompt || existing?.prompt,
            durationMs: agent.durationMs ?? existing?.durationMs ?? null,
            output: agent.output ?? existing?.output ?? null,
            error: agent.error ?? existing?.error ?? null,
            source: "live",
          },
        },
      };
    });
  },

  clearLiveSubagentsForThread: (threadId) => {
    get().applyToThread(threadId, () => ({
      liveSubagents: {},
    }));
  },

  removeLiveSubagentForThread: (threadId, agentId) => {
    if (!threadId || !agentId) return;
    get().applyToThread(threadId, (draft) => {
      const current = draft.liveSubagents ?? {};
      if (!current[agentId]) return {};
      const next = { ...current };
      delete next[agentId];
      return { liveSubagents: next };
    });
  },

  setActivePlanForThread: (threadId, plan) => {
    get().applyToThread(threadId, () => ({
      activePlan: plan,
    }));
  },

  setLatestPlanContentForThread: (threadId, content) => {
    get().applyToThread(threadId, () => ({
      latestPlanContent: content,
    }));
  },
}));

/**
 * 从 SQLite 加载所有持久化状态到 store。
 * 应在 App 挂载时调用一次。
 */
export async function initStoreFromDb(): Promise<void> {
  const { appStateGetAll } = await import("../api/app_state");
  const all = await appStateGetAll();

  let projects: Project[] = [];
  let threadProjectMap: Record<string, string> = {};
  let providers: ProviderConfig[] = [];
  let activeProviderId: string | null = null;
  let configuredModels: ModelEntry[] = [];
  let activeModelId: string | null = null;
  let autoApprove = false;
  let sidebarTab: SidebarTab = "chats";
  let sidebarWidth = DEFAULT_SIDEBAR_WIDTH;
  let rightPanelWidth = DEFAULT_RIGHT_PANEL_WIDTH;
  let imageGenerationSettings = normalizeImageGenerationSettings();
  let currentProjectId: string | null = null;
  let workspaceCwd: string | null = null;
  let threadPreferences: Record<string, ThreadPreference> = {};

  try {
    if (all[PROJECTS_KEY]) projects = JSON.parse(all[PROJECTS_KEY]) as Project[];
  } catch { /* ignore */ }

  try {
    if (all[THREAD_PROJECT_KEY]) threadProjectMap = JSON.parse(all[THREAD_PROJECT_KEY]) as Record<string, string>;
  } catch { /* ignore */ }

  try {
    if (all[PROVIDERS_KEY]) {
      const parsed = JSON.parse(all[PROVIDERS_KEY]) as PersistedProviderRecord[];
      providers = parsed.map((provider) => {
        const fallbackModelMaxTokens = normalizeModelMaxOutputTokens(provider.maxOutputTokens);
        return {
          id: provider.id ?? crypto.randomUUID(),
          type: provider.type ?? provider.id ?? "custom",
          name: provider.name ?? provider.type ?? provider.id ?? "Custom",
          category: provider.category ?? "other",
          baseUrl: provider.baseUrl ?? "",
          apiKey: provider.apiKey ?? "",
          wireApi: provider.wireApi ?? "chat",
          requiresOpenAIAuth: provider.requiresOpenAIAuth ?? false,
          models: (provider.models ?? []).map((model) =>
            normalizeProviderModel(model, fallbackModelMaxTokens),
          ),
          isCustom: provider.isCustom ?? provider.type === "custom",
          createdAt: provider.createdAt ?? Date.now(),
        };
      });
    }
  } catch { /* ignore */ }

  activeProviderId = all[ACTIVE_PROVIDER_KEY] ?? providers[0]?.id ?? null;

  try {
    if (all[CONFIGURED_MODELS_KEY]) configuredModels = JSON.parse(all[CONFIGURED_MODELS_KEY]) as ModelEntry[];
  } catch { /* ignore */ }

  activeModelId = all[ACTIVE_MODEL_KEY] ?? null;
  autoApprove = all[AUTO_APPROVE_KEY] === "true";
  if (all["sidebar-tab"] === "projects") sidebarTab = "projects";
  if (all[SIDEBAR_WIDTH_KEY]) {
    const parsed = Number(all[SIDEBAR_WIDTH_KEY]);
    if (Number.isFinite(parsed)) {
      sidebarWidth = clampPanelWidth(parsed, SIDEBAR_WIDTH_MIN, SIDEBAR_WIDTH_MAX);
    }
  }
  if (all[RIGHT_PANEL_WIDTH_KEY]) {
    const parsed = Number(all[RIGHT_PANEL_WIDTH_KEY]);
    if (Number.isFinite(parsed)) {
      rightPanelWidth = clampPanelWidth(parsed, RIGHT_PANEL_WIDTH_MIN, RIGHT_PANEL_WIDTH_MAX);
    }
  }
  try {
    if (all[IMAGE_GENERATION_SETTINGS_KEY]) {
      imageGenerationSettings = normalizeImageGenerationSettings(
        JSON.parse(all[IMAGE_GENERATION_SETTINGS_KEY]) as Partial<ImageGenerationSettings>,
      );
    }
  } catch { /* ignore */ }

  try {
    if (all[ACTIVE_PROJECT_KEY]) {
      const saved = JSON.parse(all[ACTIVE_PROJECT_KEY]) as { id: string; cwd: string };
      if (saved.id === GENERAL_PROJECT_ID) {
        currentProjectId = saved.id;
        workspaceCwd = null;
      } else if (projects.some((p) => p.id === saved.id)) {
        currentProjectId = saved.id;
        workspaceCwd = saved.cwd;
      }
    }
  } catch { /* ignore */ }

  try {
    if (all[THREAD_PREFERENCES_KEY]) {
      threadPreferences = JSON.parse(all[THREAD_PREFERENCES_KEY]) as Record<string, ThreadPreference>;
    }
  } catch { /* ignore */ }

  // 启动时把全局模型对齐到“启用供应商”的默认模型。
  // 若 config.toml 中的 model 仍属于该供应商，则优先沿用；否则回退到供应商模型列表第一项。
  const activeProvider = providers.find((provider) => provider.id === activeProviderId) ?? null;
  const activeModelEntry = activeModelId
    ? configuredModels.find((entry) => entry.id === activeModelId) ?? null
    : null;
  const preferredModelId = activeModelEntry
    && activeProvider
    && (activeModelEntry.provider === activeProvider.id || activeModelEntry.provider === activeProvider.type)
    ? activeModelEntry.model
    : null;
  const hydratedDefaultModel = resolveGlobalDefaultModel(activeProvider, {
    preferredModelId,
  });

  useAppStore.setState({
    projects,
    threadProjectMap,
    providers,
    activeProviderId,
    configuredModels,
    activeModelId,
    currentModel: hydratedDefaultModel?.id ?? null,
    autoApprove,
    imageGenerationSettings,
    sidebarTab,
    sidebarWidth,
    rightPanelWidth,
    currentProjectId,
    workspaceCwd,
    threadPreferences,
  });

  writeLayoutSnapshotToStorage({ sidebarWidth, rightPanelWidth });
}
