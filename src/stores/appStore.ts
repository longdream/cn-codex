import { create } from "zustand";
import {
  standaloneThreadCreate,
  standaloneThreadList,
  standaloneThreadRead,
} from "../api";

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

  addProject: (cwd: string) => string;
  selectProject: (projectId: string) => void;
  removeProject: (projectId: string) => void;

  setThreads: (threads: ThreadSummary[]) => void;
  addThread: (thread: ThreadSummary) => void;
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

export const useAppStore = create<AppState>((set, get) => ({
  initialized: false,
  initError: null,
  currentThreadId: null,
  currentTurnId: null,
  currentModel: null,
  configuredModels: loadConfiguredModels(),
  activeModelId: loadActiveModelId(),
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
    set({
      projects: next,
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
