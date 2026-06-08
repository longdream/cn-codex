import { create } from "zustand";
import {
  standaloneConfigWrite,
  standaloneThreadCreate,
  standaloneThreadList,
  standaloneThreadRead,
} from "../api";
import type { ProviderConfig, ProviderPreset, ProviderModel, AttachedFile } from "../types/provider";

export interface ToolCallItem {
  id: string;
  name: string;
  arguments: string;
  status: "running" | "success" | "failed";
  displayLabel: string;
  output?: string;
}

export interface ChatMessage {
  id: string;
  role: "user" | "assistant" | "system";
  content: string;
  timestamp: number;
  toolCalls?: ToolCallItem[];
  commandStatus?: string;
  fileChanges?: { path: string; action: string }[];
}

export interface ThreadSummary {
  id: string;
  name?: string;
  preview: string;
  updatedAt: number;
  archived: boolean;
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
  startedAt?: number | null;
  completedAt?: number | null;
}

function toMillis(value?: number | null): number {
  if (!value) {
    return Date.now();
  }
  return value > 10_000_000_000 ? value : value * 1000;
}

function toolDisplayLabelFromArgs(name: string, args: string): string {
  try {
    const parsed = JSON.parse(args);
    switch (name) {
      case "shell":
        return Array.isArray(parsed.command) ? parsed.command.join(" ") : String(parsed.command ?? "shell");
      case "read_file":
        return parsed.path ?? "read_file";
      case "write_file":
        return parsed.path ?? "write_file";
      case "list_directory":
        return parsed.path ?? ".";
      default:
        return name;
    }
  } catch {
    return name;
  }
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
  }

  console.log("[mapTurnsToMessages] produced", messages.length, "messages from", turns.length, "turns");
  return messages;
}

const PROJECTS_KEY = "cn-codex-projects";
const THREAD_PROJECT_KEY = "cn-codex-thread-projects";
const ACTIVE_PROJECT_KEY = "cn-codex-active-project";
const CONFIGURED_MODELS_KEY = "cn-codex-configured-models";
const ACTIVE_MODEL_KEY = "cn-codex-active-model";
const PROVIDERS_KEY = "cn-codex-providers";
const ACTIVE_PROVIDER_KEY = "cn-codex-active-provider";

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
  };
}

function loadProviders(): ProviderConfig[] {
  try {
    const raw = localStorage.getItem(PROVIDERS_KEY);
    if (raw) {
      const parsed = JSON.parse(raw) as ProviderConfig[];
      // 兼容旧数据：如果实例缺少 type/createdAt 字段，进行修补
      return parsed.map((p) => ({
        ...p,
        type: p.type ?? p.id ?? "custom",
        createdAt: p.createdAt ?? Date.now(),
      }));
    }
  } catch { /* 忽略解析错误 */ }
  return [];
}

function saveProviders(providers: ProviderConfig[]) {
  localStorage.setItem(PROVIDERS_KEY, JSON.stringify(providers));
}

function loadActiveProviderId(): string | null {
  return localStorage.getItem(ACTIVE_PROVIDER_KEY);
}

function saveActiveProviderId(id: string | null) {
  if (id) {
    localStorage.setItem(ACTIVE_PROVIDER_KEY, id);
  } else {
    localStorage.removeItem(ACTIVE_PROVIDER_KEY);
  }
}

function loadConfiguredModels(): ModelEntry[] {
  try {
    const raw = localStorage.getItem(CONFIGURED_MODELS_KEY);
    return raw ? JSON.parse(raw) : [];
  } catch {
    return [];
  }
}

function saveConfiguredModels(models: ModelEntry[]) {
  localStorage.setItem(CONFIGURED_MODELS_KEY, JSON.stringify(models));
}

function loadActiveModelId(): string | null {
  return localStorage.getItem(ACTIVE_MODEL_KEY);
}

function saveActiveModelId(id: string | null) {
  if (id) {
    localStorage.setItem(ACTIVE_MODEL_KEY, id);
  } else {
    localStorage.removeItem(ACTIVE_MODEL_KEY);
  }
}

function loadProjects(): Project[] {
  try {
    const raw = localStorage.getItem(PROJECTS_KEY);
    return raw ? (JSON.parse(raw) as Project[]) : [];
  } catch {
    return [];
  }
}

function saveProjects(projects: Project[]) {
  localStorage.setItem(PROJECTS_KEY, JSON.stringify(projects));
}

function loadThreadProjectMap(): Record<string, string> {
  try {
    const raw = localStorage.getItem(THREAD_PROJECT_KEY);
    return raw ? (JSON.parse(raw) as Record<string, string>) : {};
  } catch {
    return {};
  }
}

function saveThreadProjectMap(map: Record<string, string>) {
  localStorage.setItem(THREAD_PROJECT_KEY, JSON.stringify(map));
}

function loadActiveProject(projects: Project[]): { id: string; cwd: string } | null {
  try {
    const raw = localStorage.getItem(ACTIVE_PROJECT_KEY);
    if (!raw) return null;
    const saved = JSON.parse(raw) as { id: string; cwd: string };
    if (projects.some((p) => p.id === saved.id)) return saved;
    return null;
  } catch {
    return null;
  }
}

function saveActiveProject(id: string | null, cwd: string | null) {
  if (id && cwd) {
    localStorage.setItem(ACTIVE_PROJECT_KEY, JSON.stringify({ id, cwd }));
  } else {
    localStorage.removeItem(ACTIVE_PROJECT_KEY);
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
  isStreaming: boolean;
  showSettings: boolean;

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

  addProject: (cwd: string) => string;
  selectProject: (projectId: string) => void;
  removeProject: (projectId: string) => void;

  setThreads: (threads: ThreadSummary[]) => void;
  addThread: (thread: ThreadSummary) => void;
  /** 删除指定对话 */
  deleteThread: (threadId: string) => void;
  setMessages: (messages: ChatMessage[]) => void;
  addMessage: (message: ChatMessage) => void;
  updateToolCallStatus: (toolId: string, status: "success" | "failed", output?: string) => void;
  appendStreamingText: (delta: string) => void;
  clearStreamingText: () => void;
  setStreaming: (v: boolean) => void;
  setShowSettings: (v: boolean) => void;
  createThread: () => Promise<string | null>;
  loadThreads: () => Promise<void>;
  loadThread: (threadId: string) => Promise<void>;
}

const _initialProjects = loadProjects();
const _restoredActive = loadActiveProject(_initialProjects);
const _initialProviders = loadProviders();
const _initialActiveProviderId = loadActiveProviderId() ?? _initialProviders[0]?.id ?? null;

export const useAppStore = create<AppState>((set, get) => ({
  initialized: false,
  initError: null,
  currentThreadId: null,
  currentTurnId: null,
  currentModel: null,
  configuredModels: loadConfiguredModels(),
  activeModelId: loadActiveModelId(),
  providers: _initialProviders,
  activeProviderId: _initialActiveProviderId,
  attachedFiles: [],
  workspaceCwd: _restoredActive?.cwd ?? null,
  configDir: null,
  configPath: null,

  projects: _initialProjects,
  currentProjectId: _restoredActive?.id ?? null,
  threadProjectMap: loadThreadProjectMap(),

  threads: [],
  messages: [],
  streamingText: "",
  isStreaming: false,
  showSettings: false,

  setInitialized: (v) => set({ initialized: v }),
  setInitError: (err) => set({ initError: err }),
  retryInit: null,
  setRetryInit: (fn) => set({ retryInit: fn }),
  setCurrentThread: (id) => {
    console.warn("[store] setCurrentThread:", id, "clearing messages. Stack:", new Error().stack);
    set({ currentThreadId: id, messages: [], streamingText: "" });
  },
  startNewThreadWithMessage: (threadId, message) => {
    console.debug("[store] startNewThreadWithMessage:", threadId);
    set({ currentThreadId: threadId, messages: [message], streamingText: "", isStreaming: false });
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
    set({ activeModelId: id, currentModel: entry?.model ?? id });
  },
  getActiveModel: () => {
    const { configuredModels, activeModelId } = get();
    return configuredModels.find((m) => m.id === activeModelId) ?? null;
  },
  setServerRuntime: (runtime) =>
    set((s) => ({
      workspaceCwd: s.currentProjectId ? s.workspaceCwd : runtime.cwd,
      configDir: runtime.configDir,
      configPath: runtime.configPath,
    })),
  setWorkspaceCwd: (cwd) => set({ workspaceCwd: cwd }),

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
    if (projectId && thread.projectId === projectId) {
      const map = { ...get().threadProjectMap, [thread.id]: projectId };
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
  },

  setMessages: (messages) => {
    console.debug("[store] setMessages:", messages.length, "msgs");
    set({ messages });
  },
  addMessage: (message) => {
    console.debug("[store] addMessage:", message.role, message.content?.slice(0, 40) || "(toolCalls)");
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
  appendStreamingText: (delta) =>
    set((s) => ({ streamingText: s.streamingText + delta })),
  clearStreamingText: () => set({ streamingText: "" }),
  setStreaming: (v) => {
    const prev = get().isStreaming;
    if (prev !== v) console.debug("[store] setStreaming:", prev, "->", v);
    set({ isStreaming: v });
  },
  setShowSettings: (v) => set({ showSettings: v }),

  createThread: async () => {
    console.log("[store] createThread called. Stack:", new Error().stack?.split('\n').slice(0, 5).join('\n'));
    try {
      const resp = await standaloneThreadCreate();
      const threadId = resp?.thread?.id ?? null;
      console.log("[store] createThread got threadId:", threadId);
      if (threadId) {
        const projectId = get().currentProjectId;
        set({
          currentThreadId: threadId,
          messages: [],
          streamingText: "",
          isStreaming: false,
          currentTurnId: null,
        });
        const newThread: ThreadSummary = {
          id: threadId,
          preview: "",
          updatedAt: Date.now(),
          archived: false,
          projectId: projectId ?? undefined,
        };
        if (projectId) {
          const map = { ...get().threadProjectMap, [threadId]: projectId };
          saveThreadProjectMap(map);
          set((s) => ({
            threads: [newThread, ...s.threads],
            threadProjectMap: map,
          }));
        } else {
          set((s) => ({ threads: [newThread, ...s.threads] }));
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
      const threads: ThreadSummary[] = (resp?.data ?? []).map((t) => ({
        id: t.id,
        name: t.name,
        preview: t.preview ?? t.name ?? "",
        updatedAt: t.updatedAt ?? Date.now(),
        archived: t.archived ?? false,
        projectId: tpMap[t.id],
      }));
      set({ threads: threads.filter((t) => !t.archived) });
    } catch (err) {
      console.error("Failed to load threads:", err);
    }
  },

  loadThread: async (threadId: string) => {
    console.log("[store] loadThread:", threadId);
    try {
      const resp = await standaloneThreadRead(threadId);
      const rawThread = resp?.thread;
      const turns = rawThread?.turns ?? [];
      console.log("[store] loadThread raw turns:", turns.length, "items per turn:", turns.map((t: RawTurn) => (t.items ?? []).length));
      const messages = mapTurnsToMessages(turns as RawTurn[]);
      console.log("[store] loadThread mapped messages:", messages.length, messages.map((m) => `${m.role}:${m.toolCalls ? "toolCalls(" + m.toolCalls.length + ")" : m.content?.slice(0, 30)}`));

      set({
        currentThreadId: threadId,
        currentTurnId: null,
        messages,
        streamingText: "",
        isStreaming: false,
      });
      if (rawThread?.id) {
        set((state) => {
          const existing = state.threads.find((thread) => thread.id === threadId);
          const nextThread: ThreadSummary = {
            id: threadId,
            name: rawThread.name ?? existing?.name,
            preview: existing?.preview ?? "",
            updatedAt: existing?.updatedAt ?? Date.now(),
            archived: existing?.archived ?? false,
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
      });
    }
  },
}));
