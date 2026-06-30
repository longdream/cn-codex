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

export const GENERAL_PROJECT_ID = "__general__";

export type ChatMode = "chat" | "plan" | "goal";
export type GoalStatus = ThreadGoalStatus;
export type ThreadGoal = ApiThreadGoal;

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
}

export interface TokenUsage {
  promptTokens: number;
  completionTokens: number;
  totalTokens: number;
  callCount?: number;
  lastSinglePromptTokens?: number;
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

export type RightPanelTab = "browser" | "project" | "terminal" | "git";
export type SidebarTab = "chats" | "projects";
export interface SmartbrainExtractionProgress {
  current: number;
  total: number;
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
export const VISION_FALLBACK_KIND_MULTIMODAL: VisionFallbackKind = "multimodal";
export const VISION_FALLBACK_KIND_LOCAL_OCR: VisionFallbackKind = "local_ocr";

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
    changedFiles: turn.changedFiles ?? [],
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
      path: String(snapshot.path ?? "").trim(),
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
  if (
    promptTokens <= 0 &&
    completionTokens <= 0 &&
    totalTokens <= 0 &&
    callCount <= 0 &&
    lastSinglePromptTokens <= 0
  ) {
    return undefined;
  }

  return {
    promptTokens: Math.max(0, promptTokens),
    completionTokens: Math.max(0, completionTokens),
    totalTokens: Math.max(0, totalTokens),
    ...(callCount > 0 ? { callCount: Math.max(0, Math.round(callCount)) } : {}),
    ...(lastSinglePromptTokens > 0
      ? { lastSinglePromptTokens: Math.max(0, Math.round(lastSinglePromptTokens)) }
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

function normalizeThreadGoal(goal?: ApiThreadGoal | null): ThreadGoal | null {
  if (!goal?.objective?.trim()) {
    return null;
  }

  return {
    objective: goal.objective,
    status: goal.status ?? "active",
    tokenBudget: normalizeTokenBudget(goal.tokenBudget) ?? null,
    tokensUsed: Math.max(0, Number(goal.tokensUsed ?? 0)),
    createdAt: goal.createdAt,
    updatedAt: goal.updatedAt,
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
    messages,
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
const SIDEBAR_WIDTH_KEY = "sidebar-width";
const RIGHT_PANEL_WIDTH_KEY = "right-panel-width";

type PersistedProviderRecord = Omit<ProviderConfig, "models"> & {
  models?: Array<Partial<ProviderModel>>;
  maxOutputTokens?: unknown;
};

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
  void appStateSet(SIDEBAR_WIDTH_KEY, String(clampPanelWidth(width, SIDEBAR_WIDTH_MIN, SIDEBAR_WIDTH_MAX)));
}

function saveRightPanelWidth(width: number) {
  void appStateSet(
    RIGHT_PANEL_WIDTH_KEY,
    String(clampPanelWidth(width, RIGHT_PANEL_WIDTH_MIN, RIGHT_PANEL_WIDTH_MAX)),
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

  /** 输入框附件列表 */
  attachedFiles: AttachedFile[];
  /** 由外部面板注入到输入框的待插入文本（例如代码片段） */
  pendingComposerInsert: string | null;
  /** AI 回复期间排队等待发送的消息队列 */
  pendingMessageQueue: QueuedMessage[];
  /** apply_patch 的“写盘前审阅”会话，按 callId 建索引。 */
  pendingFileReviews: Record<string, PendingFileReview>;

  projects: Project[];
  currentProjectId: string | null;
  threadProjectMap: Record<string, string>;

  threads: ThreadSummary[];
  messages: ChatMessage[];
  streamingText: string;
  streamingLabel: string;
  isStreaming: boolean;
  chatMode: ChatMode;
  latestPlanContent: string | null;
  activePlan: PlanFile | null;
  currentGoal: ThreadGoal | null;
  showSettings: boolean;
  rightPanelVisible: boolean;
  rightPanelTab: RightPanelTab;
  sidebarWidth: number;
  rightPanelWidth: number;
  browserPanelUrl: string | null;
  browserPanelTitle: string | null;
  browserPanelStatus: "idle" | "running" | "success" | "failed";
  smartbrainExtractionRunning: boolean;
  smartbrainExtractionLabel: string | null;
  smartbrainExtractionProgress: SmartbrainExtractionProgress | null;
  browserSyncTrigger: number;
  browserActive: boolean;
  browserDetached: boolean;
  autoApprove: boolean;
  sidebarTab: SidebarTab;

  selectedRobotId: string | null;
  robotCreateMode: boolean;

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
  setShowSettings: (v: boolean) => void;
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
  setSmartbrainExtractionStatus: (state: Partial<{
    running: boolean;
    label: string | null;
    progress: SmartbrainExtractionProgress | null;
  }>) => void;
  setBrowserPanelState: (state: Partial<{
    url: string | null;
    title: string | null;
    status: "idle" | "running" | "success" | "failed";
  }>) => void;
  triggerBrowserSync: () => void;
  setBrowserActive: (v: boolean) => void;
  setBrowserDetached: (v: boolean) => void;
  createThread: () => Promise<string | null>;
  loadThreads: () => Promise<void>;
  loadThread: (threadId: string) => Promise<void>;
}

export const useAppStore = create<AppState>((set, get) => ({
  initialized: false,
  initError: null,
  currentThreadId: null,
  currentTurnId: null,
  currentModel: null,
  configuredModels: [],
  activeModelId: null,
  providers: [],
  activeProviderId: null,
  activeEndpointIndex: null,
  attachedFiles: [],
  pendingComposerInsert: null,
  pendingMessageQueue: [],
  pendingFileReviews: {},
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
  chatMode: "chat",
  latestPlanContent: null,
  activePlan: null,
  currentGoal: null,
  showSettings: false,
  autoApprove: false,
  sidebarTab: "chats",
  selectedRobotId: null,
  robotCreateMode: false,
  workflowExtractThreadId: null,
  rightPanelVisible: false,
  rightPanelTab: "browser",
  sidebarWidth: DEFAULT_SIDEBAR_WIDTH,
  rightPanelWidth: DEFAULT_RIGHT_PANEL_WIDTH,
  browserPanelUrl: null,
  browserPanelTitle: null,
  browserPanelStatus: "idle",
  smartbrainExtractionRunning: false,
  smartbrainExtractionLabel: null,
  smartbrainExtractionProgress: null,
  browserSyncTrigger: 0,
  browserActive: false,
  browserDetached: false,

  setInitialized: (v) => set({ initialized: v }),
  setInitError: (err) => set({ initError: err }),
  retryInit: null,
  setRetryInit: (fn) => set({ retryInit: fn }),
  setCurrentThread: (id) => {
    set({
      currentThreadId: id,
      messages: [],
      streamingText: "",
      streamingLabel: "",
      currentGoal: null,
      latestPlanContent: null,
      activePlan: null,
      selectedRobotId: null,
      robotCreateMode: false,
      pendingComposerInsert: null,
      pendingMessageQueue: [],
      pendingFileReviews: {},
    });
  },
  startNewThreadWithMessage: (threadId, message) => {
    set({
      currentThreadId: threadId,
      messages: [message],
      streamingText: "",
      streamingLabel: "",
      isStreaming: false,
      currentGoal: null,
      latestPlanContent: null,
      activePlan: null,
      pendingComposerInsert: null,
      pendingMessageQueue: [],
      pendingFileReviews: {},
    });
  },
  setCurrentTurnId: (id) => set({ currentTurnId: id }),
  setCurrentModel: (model) => set({ currentModel: model }),
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
      latestPlanContent: null,
      activePlan: null,
      currentGoal: null,
    });
    return id;
  },

  selectProject: (projectId: string) => {
    const project = get().projects.find((p) => p.id === projectId);
    if (!project) return;
    saveActiveProject(projectId, project.cwd);
    set({
      currentProjectId: projectId,
      workspaceCwd: project.cwd,
      currentThreadId: null,
      messages: [],
      streamingText: "",
      isStreaming: false,
      currentTurnId: null,
      currentGoal: null,
      latestPlanContent: null,
      activePlan: null,
      pendingMessageQueue: [],
    });
  },

  selectGeneralMode: () => {
    const cwd = get().projectRoot ?? get().userHomeDir;
    saveActiveProject(GENERAL_PROJECT_ID, cwd);
    set({
      currentProjectId: GENERAL_PROJECT_ID,
      workspaceCwd: cwd,
      currentThreadId: null,
      messages: [],
      streamingText: "",
      isStreaming: false,
      currentTurnId: null,
      currentGoal: null,
      latestPlanContent: null,
      activePlan: null,
      pendingMessageQueue: [],
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
            currentGoal: null,
            latestPlanContent: null,
            activePlan: null,
          }
        : {}),
    });
  },

  // ─── 供应商管理 ─────────────────────────────────────────
  setProviders: (providers) => {
    saveProviders(providers);
    set({ providers });
  },

  activateProvider: (providerId: string) => {
    saveActiveProviderId(providerId);
    set({ activeProviderId: providerId });
    const state = get();
    const provider = state.providers.find((p) => p.id === providerId);
    if (provider) {
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
      const selectedModel = provider.models.find((model) => model.id === preferredModelId)
        ?? provider.models[0];
      const modelEndpoints = buildLocalPoolModelEndpoints(provider, selectedModel);
      const visionFallback = resolveVisionFallbackConfig(selectedModel, state.providers);

      set({ activeEndpointIndex: modelEndpoints.length > 0 ? 0 : null });

      const edits: { keyPath: string; value: unknown; mergeStrategy: string }[] = [
        { keyPath: "model_provider", value: providerKey, mergeStrategy: "replace" },
        { keyPath: "model", value: selectedModel?.id ?? "", mergeStrategy: "replace" },
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
    }
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
    set({
      threads,
      threadProjectMap: tpMap,
      ...(isCurrent
        ? {
          currentThreadId: null,
          messages: [],
          streamingText: "",
          currentTurnId: null,
          latestPlanContent: null,
          activePlan: null,
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
      };
    }),
  setStreamingLabel: (label) => set({ streamingLabel: label }),
  setChatMode: (mode) => set({ chatMode: mode }),
  setLatestPlanContent: (content) => set({ latestPlanContent: content }),
  setActivePlan: (plan) => set({ activePlan: plan }),
  setCurrentGoal: (goal) => set({ currentGoal: normalizeThreadGoal(goal) }),
  setShowSettings: (v) =>
    set((state) =>
      v
        ? {
          showSettings: true,
          // 设置弹层打开时强制隐藏右侧区域，避免内置浏览器覆盖在最上层。
          rightPanelVisible: false,
        }
        : {
          showSettings: false,
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
    }),
  triggerBrowserSync: () =>
    set((s) => ({ browserSyncTrigger: s.browserSyncTrigger + 1 })),
  setBrowserActive: (v) => set({ browserActive: v }),
  setBrowserDetached: (v) => set({ browserDetached: v }),

  createThread: async () => {
    try {
      const resp = await standaloneThreadCreate();
      const threadId = resp?.thread?.id ?? null;
      if (threadId) {
        const projectId = get().currentProjectId;
        set({
          currentThreadId: threadId,
          messages: [],
          streamingText: "",
          isStreaming: false,
          currentTurnId: null,
          currentGoal: null,
          latestPlanContent: null,
          activePlan: null,
          pendingMessageQueue: [],
          pendingFileReviews: {},
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
    try {
      const resp = await standaloneThreadRead(threadId);
      const rawThread = resp?.thread as RawThread | undefined;
      const turns = rawThread?.turns ?? [];
      const activePlan = normalizePlanFile(rawThread?.activePlan);
      const { messages, activePlan: hydratedActivePlan } = mapTurnsToMessages(
        turns as RawTurn[],
        activePlan,
      );

      set({
        currentThreadId: threadId,
        currentTurnId: null,
        messages,
        streamingText: "",
        isStreaming: false,
        latestPlanContent: hydratedActivePlan?.content ?? null,
        activePlan: hydratedActivePlan,
        currentGoal: normalizeThreadGoal(rawThread?.goal),
        pendingMessageQueue: [],
        pendingFileReviews: {},
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
        isStreaming: false,
        currentGoal: null,
        latestPlanContent: null,
        activePlan: null,
        pendingMessageQueue: [],
        pendingFileReviews: {},
      });
    }
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
  let currentProjectId: string | null = null;
  let workspaceCwd: string | null = null;

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

  useAppStore.setState({
    projects,
    threadProjectMap,
    providers,
    activeProviderId,
    configuredModels,
    activeModelId,
    autoApprove,
    sidebarTab,
    sidebarWidth,
    rightPanelWidth,
    currentProjectId,
    workspaceCwd,
  });
}
