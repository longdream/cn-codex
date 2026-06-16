import { create } from "zustand";

export interface MobileMessage {
  id: string;
  role: "user" | "assistant" | "system" | "tool";
  content: string;
  timestamp: number;
}

export interface ThreadSummary {
  id: string;
  name: string | null;
  preview: string;
  updatedAt: number;
  messageCount: number;
}

interface MobileState {
  view: "threads" | "chat";
  threads: ThreadSummary[];
  currentThreadId: string | null;
  activeThreadId: string | null;
  messages: MobileMessage[];
  streamingText: string;
  isStreaming: boolean;
  connected: boolean;

  setView: (v: "threads" | "chat") => void;
  setThreads: (threads: ThreadSummary[]) => void;
  setCurrentThread: (id: string | null) => void;
  setActiveThreadId: (id: string | null) => void;
  setMessages: (messages: MobileMessage[]) => void;
  appendStreamingText: (delta: string) => void;
  clearStreamingText: () => void;
  setStreaming: (v: boolean) => void;
  setConnected: (v: boolean) => void;
  addMessage: (msg: MobileMessage) => void;
}

export const useMobileStore = create<MobileState>((set) => ({
  view: "threads",
  threads: [],
  currentThreadId: null,
  activeThreadId: null,
  messages: [],
  streamingText: "",
  isStreaming: false,
  connected: false,

  setView: (v) => set({ view: v }),
  setThreads: (threads) => set({ threads }),
  setCurrentThread: (id) => set({ currentThreadId: id, view: "chat" }),
  setActiveThreadId: (id) => set({ activeThreadId: id }),
  setMessages: (messages) => set({ messages }),
  appendStreamingText: (delta) =>
    set((s) => ({ streamingText: s.streamingText + delta })),
  clearStreamingText: () => set({ streamingText: "" }),
  setStreaming: (v) => set({ isStreaming: v }),
  setConnected: (v) => set({ connected: v }),
  addMessage: (msg) => set((s) => ({ messages: [...s.messages, msg] })),
}));
