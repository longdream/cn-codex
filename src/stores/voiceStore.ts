import { create } from "zustand";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import {
  voiceGetModelStatuses,
  voiceStartupCheck,
  voiceDownloadModel,
  type VoiceDownloadProgress,
  type VoiceModelStatus,
} from "../api/voice";

/** 右下角下载列表条目 */
export interface VoiceDownloadEntry {
  modelId: string;
  label: string;
  status: "downloading" | "extracting" | "ready" | "failed";
  downloaded: number;
  total: number;
  speed: number;
  error?: string | null;
}

/** LLM 意图识别结果 */
export type VoiceIntent = "chat" | "solve";

export interface VoiceIntentResult {
  intent: VoiceIntent;
  /** chat: 闲聊回复（TTS 播放，不入界面） */
  reply?: string;
  /** solve: 问题总结（用户确认后入主链路） */
  summary?: string;
  /** solve: LLM 生成的开始任务话术（TTS 播放） */
  ack?: string;
}

/** 待确认的 solve 卡片 */
export interface VoicePendingConfirm {
  id: string;
  summary: string;
  ack: string;
  rawText: string;
}

/** 个性设置（与主链路无关，仅作用于闲聊） */
export interface VoicePersonaSettings {
  /** 友好随和 / 专业严谨 / 幽默风趣 / 简洁直接 / 自定义 */
  personaPreset: string;
  /** 口语化 / 正式 / 活泼网络化 / 简短 */
  languageStyle: string;
  customPrompt: string;
  /** 闲聊上下文轮数 1~20 */
  chatContextTurns: number;
  /** TTS 音色 id */
  ttsSid: number;
  /** TTS 语速 0.8~1.5 */
  ttsSpeed: number;
  /** VAD 自动断句 */
  vadEnabled: boolean;
}

const PERSONA_STORAGE_KEY = "voice-persona-settings-v1";

function loadPersonaSettings(): VoicePersonaSettings {
  try {
    const raw = localStorage.getItem(PERSONA_STORAGE_KEY);
    if (raw) {
      const parsed = JSON.parse(raw) as Partial<VoicePersonaSettings>;
      return {
        personaPreset: parsed.personaPreset ?? "friendly",
        languageStyle: parsed.languageStyle ?? "casual",
        customPrompt: parsed.customPrompt ?? "",
        chatContextTurns: Math.min(20, Math.max(1, parsed.chatContextTurns ?? 10)),
        ttsSid: parsed.ttsSid ?? 0,
        ttsSpeed: Math.min(1.5, Math.max(0.8, parsed.ttsSpeed ?? 1.0)),
        vadEnabled: parsed.vadEnabled ?? true,
      };
    }
  } catch {
    // ignore
  }
  return {
    personaPreset: "friendly",
    languageStyle: "casual",
    customPrompt: "",
    chatContextTurns: 10,
    ttsSid: 0,
    ttsSpeed: 1.0,
    vadEnabled: true,
  };
}

function savePersonaSettings(settings: VoicePersonaSettings) {
  try {
    localStorage.setItem(PERSONA_STORAGE_KEY, JSON.stringify(settings));
  } catch {
    // ignore
  }
}

interface VoiceStore {
  /** 启动检查是否已执行 */
  startupChecked: boolean;
  /** 各模型状态 */
  modelStatuses: Record<string, string>;
  /** 右下角下载列表 */
  downloadEntries: VoiceDownloadEntry[];
  /** 下载列表是否折叠为小图标 */
  downloadsCollapsed: boolean;
  /** 未就绪提示（使用 ASR/TTS 时模型未就绪） */
  notReadyToast: { kind: "asr" | "tts"; visible: boolean } | null;
  /** 语音模式是否开启 */
  voiceModeOn: boolean;
  /** 聆听状态 */
  listenState: "idle" | "listening" | "processing";
  /** 闲聊中（TTS 播放期间可被打断） */
  chatReplyPending: boolean;
  /** solve 确认卡片 */
  pendingConfirm: VoicePendingConfirm | null;
  /** 个性设置 */
  persona: VoicePersonaSettings;
  /** 闲聊上下文（内存，不落盘） */
  chatHistory: Array<{ role: "user" | "assistant"; content: string }>;

  runStartupCheck: () => Promise<void>;
  refreshStatuses: () => Promise<void>;
  retryDownload: (modelId: string) => Promise<void>;
  retryAllFailed: () => Promise<void>;
  setDownloadsCollapsed: (collapsed: boolean) => Promise<void>;
  dismissNotReadyToast: () => void;
  showNotReadyToast: (kind: "asr" | "tts") => void;
  setVoiceMode: (on: boolean) => Promise<void>;
  setListenState: (s: "idle" | "listening" | "processing") => void;
  appendChatHistory: (role: "user" | "assistant", content: string) => void;
  clearChatHistory: () => void;
  setPendingConfirm: (confirm: VoicePendingConfirm | null) => void;
  updatePersona: (patch: Partial<VoicePersonaSettings>) => void;
  applyDownloadProgress: (p: VoiceDownloadProgress) => void;
}

let unlisteners: UnlistenFn[] = [];

export const useVoiceStore = create<VoiceStore>((set, get) => ({
  startupChecked: false,
  modelStatuses: {},
  downloadEntries: [],
  downloadsCollapsed: false,
  notReadyToast: null,
  voiceModeOn: false,
  listenState: "idle",
  chatReplyPending: false,
  pendingConfirm: null,
  persona: loadPersonaSettings(),
  chatHistory: [],

  runStartupCheck: async () => {
    if (get().startupChecked) return;
    set({ startupChecked: true });
    // 注册事件监听。
    const un1 = await listen<VoiceDownloadProgress>(
      "voice://download-progress",
      (event) => get().applyDownloadProgress(event.payload),
    );
    unlisteners.push(un1);
    try {
      await voiceStartupCheck();
    } catch (err) {
      console.error("[voice] startup check failed:", err);
    }
    await get().refreshStatuses();
  },

  refreshStatuses: async () => {
    try {
      const statuses: VoiceModelStatus[] = await voiceGetModelStatuses();
      const map: Record<string, string> = {};
      for (const s of statuses) map[s.id] = s.status;
      set({ modelStatuses: map });
    } catch (err) {
      console.error("[voice] refresh statuses failed:", err);
    }
  },

  retryDownload: async (modelId: string) => {
    set((s) => ({
      downloadEntries: s.downloadEntries.filter((e) => e.modelId !== modelId),
      notReadyToast: null,
    }));
    try {
      await voiceDownloadModel(modelId);
    } catch (err) {
      console.error("[voice] retry download failed:", err);
    }
    await get().refreshStatuses();
  },

  retryAllFailed: async () => {
    const failed = Object.entries(get().modelStatuses)
      .filter(([, status]) => status === "failed" || status === "not_installed")
      .map(([id]) => id);
    for (const id of failed) {
      await get().retryDownload(id);
    }
  },

  setDownloadsCollapsed: async (collapsed: boolean) => {
    set({ downloadsCollapsed: collapsed });
  },

  dismissNotReadyToast: () => set({ notReadyToast: null }),

  showNotReadyToast: (kind: "asr" | "tts") => {
    set({ notReadyToast: { kind, visible: true } });
    setTimeout(() => {
      const toast = get().notReadyToast;
      if (toast && toast.kind === kind && toast.visible) {
        set({ notReadyToast: null });
      }
    }, 6000);
  },

  setVoiceMode: async (on: boolean) => {
    set({ voiceModeOn: on });
  },

  setListenState: (s) => set({ listenState: s }),

  appendChatHistory: (role, content) =>
    set((s) => ({
      chatHistory: [
        ...s.chatHistory.slice(-(s.persona.chatContextTurns * 2)),
        { role, content },
      ],
    })),

  clearChatHistory: () => set({ chatHistory: [] }),

  setPendingConfirm: (confirm) => set({ pendingConfirm: confirm }),

  updatePersona: (patch) =>
    set((s) => {
      const next = { ...s.persona, ...patch };
      savePersonaSettings(next);
      return { persona: next };
    }),

  applyDownloadProgress: (p) => {
    set((s) => {
      const exists = s.downloadEntries.some((e) => e.modelId === p.modelId);
      let entries: VoiceDownloadEntry[];
      if (p.status === "ready") {
        entries = s.downloadEntries.filter((e) => e.modelId !== p.modelId);
      } else if (exists) {
        entries = s.downloadEntries.map((e) =>
          e.modelId === p.modelId ? { ...e, ...p } : e,
        );
      } else {
        entries = [...s.downloadEntries, { ...p }];
      }
      return {
        downloadEntries: entries,
        modelStatuses: { ...s.modelStatuses, [p.modelId]: p.status === "failed" ? "failed" : "downloading" },
      };
    });
    if (p.status === "ready") {
      void get().refreshStatuses();
    }
  },
}));

/** 模型是否全部就绪 */
export function areModelsReady(statuses: Record<string, string>): {
  asr: boolean;
  tts: boolean;
  vad: boolean;
} {
  return {
    asr: statuses["asr"] === "ready",
    tts: statuses["tts"] === "ready",
    vad: statuses["vad"] === "ready",
  };
}
