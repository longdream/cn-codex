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
import type { ProviderConfig, ProviderPreset, ProviderModel, AttachedFile } from "../types/provider";

export const GENERAL_PROJECT_ID = "__general__";

export type ChatMode = "chat" | "goal";
export type GoalStatus = ThreadGoalStatus;
export type ThreadGoal = ApiThreadGoal;

export interface ChatSendOptions {
  goalBudgetTokens?: number;
}

export interface FileChange {
  path: string;
  action: string;
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
}

export interface RunSummary {
  turnId: string;
  mode: ChatMode;
  cwd?: string;
  startedAt?: number;
  completedAt?: number;
  durationMs?: number;
  changedFiles: FileChange[];
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

export interface ChatMessage {
  id: string;
  role: "user" | "assistant" | "system";
  content: string;
  timestamp: number;
  toolCalls?: ToolCallItem[];
  commandStatus?: string;
  fileChanges?: { path: string; action: string }[];
  runSummary?: RunSummary;
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

export type RightPanelTab = "browser" | "project";

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
  usage?: TokenUsage | null;
  goalBudgetTokens?: number | null;
  budgetLimited?: boolean | null;
}

interface RawThread {
  id: string;
  name?: string;
  goal?: ApiThreadGoal | null;
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
      case "image_generate":
        return parsed.output_path ?? promptPreview(parsed.prompt) ?? "image_generate";
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
    mode: turn.mode === "goal" ? "goal" : "chat",
    cwd: turn.cwd ?? undefined,
    startedAt: turn.startedAt ? toMillis(turn.startedAt) : undefined,
    completedAt: turn.completedAt ? toMillis(turn.completedAt) : undefined,
    durationMs: turn.durationMs ?? undefined,
    changedFiles: turn.changedFiles ?? [],
    usage: normalizeTokenUsage(turn.usage),
    goalBudgetTokens: normalizeTokenBudget(turn.goalBudgetTokens),
    budgetLimited: Boolean(turn.budgetLimited),
  };
}

function normalizeTokenUsage(usage?: TokenUsage | null): TokenUsage | undefined {
  if (!usage) {
    return undefined;
  }

  const promptTokens = Number(usage.promptTokens ?? 0);
  const completionTokens = Number(usage.completionTokens ?? 0);
  const totalTokens = Number(usage.totalTokens ?? promptTokens + completionTokens);
  if (promptTokens <= 0 && completionTokens <= 0 && totalTokens <= 0) {
    return undefined;
  }

  return {
    promptTokens: Math.max(0, promptTokens),
    completionTokens: Math.max(0, completionTokens),
    totalTokens: Math.max(0, totalTokens),
  };
}

function normalizeTokenBudget(value?: number | null): number | undefined {
  const budget = Number(value ?? 0);
  if (!Number.isFinite(budget) || budget <= 0) {
    return undefined;
  }
  return Math.floor(budget);
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

function mapTurnsToMessages(turns: RawTurn[]): ChatMessage[] {
  const messages: ChatMessage[] = [];
  const toolResultMap = new Map<string, string>();

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
        const text = (item.content ?? [])
          .filter((content) => content.type === "text" && content.text)
          .map((content) => content.text?.trim() ?? "")
          .filter(Boolean)
          .join("\n\n");

        if (text) {
          messages.push({
            id: item.id ?? crypto.randomUUID(),
            role: "user",
            content: text,
            timestamp: toMillis(turn.startedAt),
          });
        }
      }

      if (item.type === "agentMessage" && item.text?.trim()) {
        messages.push({
          id: item.id ?? crypto.randomUUID(),
          role: "assistant",
          content: item.text.trim(),
          timestamp: toMillis(turn.completedAt ?? turn.startedAt),
        });
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

  return messages;
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
      { id: "gpt-4o", label: "GPT-4o", supportsVision: true },
      { id: "gpt-4o-mini", label: "GPT-4o Mini", supportsVision: true },
      { id: "o3-mini", label: "o3-mini", supportsVision: false },
    ],
  },
  {
    type: "anthropic", name: "Anthropic", category: "global",
    defaultBaseUrl: "https://api.anthropic.com/v1", defaultWireApi: "anthropic", requiresOpenAIAuth: false,
    signupUrl: "https://console.anthropic.com/settings/keys",
    defaultModels: [
      { id: "claude-sonnet-4-20250514", label: "Claude Sonnet 4", supportsVision: true },
      { id: "claude-opus-4-20250514", label: "Claude Opus 4", supportsVision: true },
    ],
  },
  {
    type: "google", name: "Google Gemini", category: "global",
    defaultBaseUrl: "https://generativelanguage.googleapis.com/v1beta/openai", defaultWireApi: "chat", requiresOpenAIAuth: false,
    signupUrl: "https://aistudio.google.com/apikey",
    defaultModels: [{ id: "gemini-2.5-pro", label: "Gemini 2.5 Pro", supportsVision: true }],
  },
  {
    type: "deepseek", name: "DeepSeek", category: "china",
    defaultBaseUrl: "https://api.deepseek.com/v1", defaultWireApi: "chat", requiresOpenAIAuth: false,
    signupUrl: "https://platform.deepseek.com/api_keys",
    defaultModels: [
      { id: "deepseek-chat", label: "DeepSeek Chat", supportsVision: false },
      { id: "deepseek-reasoner", label: "DeepSeek Reasoner", supportsVision: false },
    ],
  },
  {
    type: "volcengine", name: "火山引擎 Ark", category: "china",
    defaultBaseUrl: "https://ark.cn-beijing.volces.com/api/v3", defaultWireApi: "chat", requiresOpenAIAuth: false,
    signupUrl: "https://console.volcengine.com/ark/region:ark+cn-beijing/apiKey",
    defaultModels: [{ id: "deepseek-v4-pro-260425", label: "DeepSeek V4 Pro", supportsVision: false }],
  },
  {
    type: "qwen", name: "通义千问", category: "china",
    defaultBaseUrl: "https://dashscope.aliyuncs.com/compatible-mode/v1", defaultWireApi: "chat", requiresOpenAIAuth: false,
    signupUrl: "https://dashscope.console.aliyun.com/apiKey",
    defaultModels: [{ id: "qwen-max", label: "Qwen Max", supportsVision: true }],
  },
  {
    type: "zhipu", name: "智谱 AI", category: "china",
    defaultBaseUrl: "https://open.bigmodel.cn/api/paas/v4", defaultWireApi: "chat", requiresOpenAIAuth: false,
    signupUrl: "https://open.bigmodel.cn/usercenter/apikeys",
    defaultModels: [{ id: "glm-4-plus", label: "GLM-4 Plus", supportsVision: true }],
  },
  {
    type: "moonshot", name: "Moonshot AI", category: "china",
    defaultBaseUrl: "https://api.moonshot.cn/v1", defaultWireApi: "chat", requiresOpenAIAuth: false,
    signupUrl: "https://platform.moonshot.cn/console/api-keys",
    defaultModels: [{ id: "moonshot-v1-128k", label: "Moonshot V1 128K", supportsVision: false }],
  },
  {
    type: "siliconflow", name: "SiliconFlow", category: "china",
    defaultBaseUrl: "https://api.siliconflow.cn/v1", defaultWireApi: "chat", requiresOpenAIAuth: false,
    signupUrl: "https://cloud.siliconflow.cn/account/ak",
    defaultModels: [{ id: "deepseek-ai/DeepSeek-V3", label: "DeepSeek V3", supportsVision: false }],
  },
  {
    type: "baichuan", name: "百川智能", category: "china",
    defaultBaseUrl: "https://api.baichuan-ai.com/v1", defaultWireApi: "chat", requiresOpenAIAuth: false,
    signupUrl: "https://platform.baichuan-ai.com/console/apikey",
    defaultModels: [{ id: "Baichuan4", label: "Baichuan 4", supportsVision: false }],
  },
  {
    type: "ollama", name: "Ollama", category: "local",
    defaultBaseUrl: "http://localhost:11434/v1", defaultWireApi: "chat", requiresOpenAIAuth: false,
    signupUrl: "https://ollama.com/download",
    defaultModels: [{ id: "qwen2.5-coder:7b", label: "Qwen 2.5 Coder 7B", supportsVision: false }],
  },
  {
    type: "lmstudio", name: "LM Studio", category: "local",
    defaultBaseUrl: "http://localhost:1234/v1", defaultWireApi: "chat", requiresOpenAIAuth: false,
    signupUrl: "https://lmstudio.ai/",
    defaultModels: [{ id: "local-model", label: "Local Model", supportsVision: false }],
  },
  {
    type: "custom", name: "自定义", category: "other",
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
    models: [...preset.defaultModels],
    isCustom: preset.type === "custom",
    createdAt: Date.now(),
    maxOutputTokens: 131072,
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

  /** 输入框附件列表 */
  attachedFiles: AttachedFile[];

  projects: Project[];
  currentProjectId: string | null;
  threadProjectMap: Record<string, string>;

  threads: ThreadSummary[];
  messages: ChatMessage[];
  streamingText: string;
  streamingLabel: string;
  isStreaming: boolean;
  chatMode: ChatMode;
  currentGoal: ThreadGoal | null;
  showSettings: boolean;
  rightPanelVisible: boolean;
  rightPanelTab: RightPanelTab;
  browserPanelUrl: string | null;
  browserPanelTitle: string | null;
  browserPanelStatus: "idle" | "running" | "success" | "failed";
  autoApprove: boolean;

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
  removeProviderModel: (providerId: string, modelId: string) => void;
  getActiveProvider: () => ProviderConfig | null;

  /** 附件管理 */
  setAttachedFiles: (files: AttachedFile[]) => void;
  addAttachedFile: (file: AttachedFile) => void;
  removeAttachedFile: (index: number) => void;
  clearAttachedFiles: () => void;

  userHomeDir: string | null;
  setUserHomeDir: (dir: string) => void;

  projectRoot: string | null;

  addProject: (cwd: string) => string;
  selectProject: (projectId: string) => void;
  selectGeneralMode: () => void;
  removeProject: (projectId: string) => void;

  setThreads: (threads: ThreadSummary[]) => void;
  addThread: (thread: ThreadSummary) => void;
  /** 删除指定对话 */
  deleteThread: (threadId: string) => void;
  setMessages: (messages: ChatMessage[]) => void;
  addMessage: (message: ChatMessage) => void;
  updateToolCallStatus: (toolId: string, status: "success" | "failed", output?: string) => void;
  updateToolCallPatchProgress: (toolId: string, changes: PatchProgressChange[]) => void;
  markRunningToolCallsInterrupted: (reason?: string) => void;
  appendStreamingText: (delta: string) => void;
  clearStreamingText: () => void;
  setStreaming: (v: boolean) => void;
  setStreamingLabel: (label: string) => void;
  setChatMode: (mode: ChatMode) => void;
  setCurrentGoal: (goal: ThreadGoal | null) => void;
  setShowSettings: (v: boolean) => void;
  setRightPanelVisible: (v: boolean) => void;
  toggleRightPanel: () => void;
  setRightPanelTab: (tab: RightPanelTab) => void;
  setAutoApprove: (v: boolean) => void;
  setBrowserPanelState: (state: Partial<{
    url: string | null;
    title: string | null;
    status: "idle" | "running" | "success" | "failed";
  }>) => void;
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
  attachedFiles: [],
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
  currentGoal: null,
  showSettings: false,
  autoApprove: false,
  rightPanelVisible: false,
  rightPanelTab: "browser",
  browserPanelUrl: null,
  browserPanelTitle: null,
  browserPanelStatus: "idle",

  setInitialized: (v) => set({ initialized: v }),
  setInitError: (err) => set({ initError: err }),
  retryInit: null,
  setRetryInit: (fn) => set({ retryInit: fn }),
  setCurrentThread: (id) => {
    set({ currentThreadId: id, messages: [], streamingText: "", streamingLabel: "", currentGoal: null });
  },
  startNewThreadWithMessage: (threadId, message) => {
    set({
      currentThreadId: threadId,
      messages: [message],
      streamingText: "",
      streamingLabel: "",
      isStreaming: false,
      currentGoal: null,
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
    const models = get().configuredModels;
    const entry = models.find((m) => m.id === id);
    const modelName = entry?.model ?? id;
    set({ activeModelId: id, currentModel: modelName });
    // 同步写入 config.toml，确保后端立即使用新模型
    if (modelName) {
      standaloneConfigWrite([
        { keyPath: "model", value: modelName, mergeStrategy: "replace" },
      ]).catch((err) => console.error("Failed to sync model to config:", err));
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
      chatMode: "chat",
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
    // 同步写入 config.toml，使用 provider.type 作为 model_provider key
    const provider = get().providers.find((p) => p.id === providerId);
    if (provider) {
      const providerKey = provider.type || "custom";
      const providerOverride: Record<string, unknown> = {};
      if (provider.baseUrl) providerOverride.base_url = provider.baseUrl;
      if (provider.wireApi) providerOverride.wire_api = provider.wireApi;
      if (provider.apiKey) providerOverride.experimental_bearer_token = provider.apiKey;
      providerOverride.requires_openai_auth = provider.requiresOpenAIAuth;
      const defaultModel = provider.models[0]?.id ?? "";
      standaloneConfigWrite([
        { keyPath: "model_provider", value: providerKey, mergeStrategy: "replace" },
        { keyPath: "model", value: defaultModel, mergeStrategy: "replace" },
        { keyPath: `model_providers.${providerKey}`, value: providerOverride, mergeStrategy: "replace" },
        { keyPath: "max_output_tokens", value: provider.maxOutputTokens ?? 131072, mergeStrategy: "replace" },
      ]).catch((err) => console.error("Failed to sync provider config:", err));
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
    const providers = get().providers.map((p) =>
      p.id === providerId ? { ...p, models: [...p.models, model] } : p,
    );
    saveProviders(providers);
    set({ providers });
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
        ? { currentThreadId: null, messages: [], streamingText: "", currentTurnId: null }
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
  setStreamingLabel: (label) => set({ streamingLabel: label }),
  setChatMode: (mode) => set({ chatMode: mode }),
  setCurrentGoal: (goal) => set({ currentGoal: normalizeThreadGoal(goal) }),
  setShowSettings: (v) => set({ showSettings: v }),
  setAutoApprove: (v) => {
    void appStateSet(AUTO_APPROVE_KEY, String(v));
    set({ autoApprove: v });
  },
  setRightPanelVisible: (v) => set({ rightPanelVisible: v }),
  toggleRightPanel: () => set((s) => ({ rightPanelVisible: !s.rightPanelVisible })),
  setRightPanelTab: (tab) => set({ rightPanelTab: tab, rightPanelVisible: true }),
  setBrowserPanelState: (state) =>
    set({
      ...(state.url !== undefined ? { browserPanelUrl: state.url } : {}),
      ...(state.title !== undefined ? { browserPanelTitle: state.title } : {}),
      ...(state.status !== undefined ? { browserPanelStatus: state.status } : {}),
    }),

  createThread: async () => {
    try {
      const resp = await standaloneThreadCreate();
      const threadId = resp?.thread?.id ?? null;
      if (threadId) {
        const projectId = get().currentProjectId;
        const isGeneral = projectId === GENERAL_PROJECT_ID;
        set({
          currentThreadId: threadId,
          messages: [],
          streamingText: "",
          isStreaming: false,
          currentTurnId: null,
          currentGoal: null,
          ...(isGeneral ? { chatMode: "chat" as ChatMode } : {}),
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
      const messages = mapTurnsToMessages(turns as RawTurn[]);

      set({
        currentThreadId: threadId,
        currentTurnId: null,
        messages,
        streamingText: "",
        isStreaming: false,
        currentGoal: normalizeThreadGoal(rawThread?.goal),
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
      const parsed = JSON.parse(all[PROVIDERS_KEY]) as ProviderConfig[];
      providers = parsed.map((p) => ({
        ...p,
        type: p.type ?? p.id ?? "custom",
        createdAt: p.createdAt ?? Date.now(),
        maxOutputTokens: p.maxOutputTokens ?? 131072,
      }));
    }
  } catch { /* ignore */ }

  activeProviderId = all[ACTIVE_PROVIDER_KEY] ?? providers[0]?.id ?? null;

  try {
    if (all[CONFIGURED_MODELS_KEY]) configuredModels = JSON.parse(all[CONFIGURED_MODELS_KEY]) as ModelEntry[];
  } catch { /* ignore */ }

  activeModelId = all[ACTIVE_MODEL_KEY] ?? null;
  autoApprove = all[AUTO_APPROVE_KEY] === "true";

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
    currentProjectId,
    workspaceCwd,
  });
}
