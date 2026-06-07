import { describe, it, expect, beforeEach, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

import { invoke } from "@tauri-apps/api/core";
const mockInvoke = vi.mocked(invoke);

import { useAppStore } from "../stores/appStore";

describe("appStore", () => {
  beforeEach(() => {
    useAppStore.setState({
      initialized: false,
      currentThreadId: null,
      currentTurnId: null,
      currentModel: null,
      threads: [],
      messages: [],
      streamingText: "",
      isStreaming: false,
      showSettings: false,
    });
    vi.clearAllMocks();
  });

  describe("synchronous setters", () => {
    it("setInitialized updates initialized state", () => {
      useAppStore.getState().setInitialized(true);
      expect(useAppStore.getState().initialized).toBe(true);
    });

    it("setCurrentThread sets thread id and clears messages/streaming", () => {
      useAppStore.setState({
        messages: [{ id: "1", role: "user", content: "hello", timestamp: 1 }],
        streamingText: "partial",
      });
      useAppStore.getState().setCurrentThread("thread-123");
      const state = useAppStore.getState();
      expect(state.currentThreadId).toBe("thread-123");
      expect(state.messages).toEqual([]);
      expect(state.streamingText).toBe("");
    });

    it("setCurrentModel updates model", () => {
      useAppStore.getState().setCurrentModel("gpt-4.1");
      expect(useAppStore.getState().currentModel).toBe("gpt-4.1");
    });

    it("setStreaming updates isStreaming", () => {
      useAppStore.getState().setStreaming(true);
      expect(useAppStore.getState().isStreaming).toBe(true);
    });

    it("setShowSettings updates showSettings", () => {
      useAppStore.getState().setShowSettings(true);
      expect(useAppStore.getState().showSettings).toBe(true);
    });
  });

  describe("addMessage", () => {
    it("appends message to list", () => {
      useAppStore.getState().addMessage({ id: "m1", role: "user", content: "hi", timestamp: 1 });
      useAppStore.getState().addMessage({ id: "m2", role: "assistant", content: "hello", timestamp: 2 });
      expect(useAppStore.getState().messages).toHaveLength(2);
      expect(useAppStore.getState().messages[1].content).toBe("hello");
    });
  });

  describe("appendStreamingText", () => {
    it("concatenates delta text", () => {
      useAppStore.getState().appendStreamingText("Hello ");
      useAppStore.getState().appendStreamingText("world");
      expect(useAppStore.getState().streamingText).toBe("Hello world");
    });
  });

  describe("addThread", () => {
    it("prepends new thread to list", () => {
      useAppStore.setState({
        threads: [{ id: "old", preview: "", updatedAt: 1, archived: false }],
      });
      useAppStore.getState().addThread({ id: "new", preview: "", updatedAt: 2, archived: false });
      const threads = useAppStore.getState().threads;
      expect(threads[0].id).toBe("new");
      expect(threads[1].id).toBe("old");
    });
  });

  describe("createThread", () => {
    it("sets currentThreadId and adds thread on success", async () => {
      mockInvoke.mockResolvedValueOnce({ thread: { id: "t-abc" } });
      const id = await useAppStore.getState().createThread();
      expect(id).toBe("t-abc");
      expect(useAppStore.getState().currentThreadId).toBe("t-abc");
      expect(useAppStore.getState().threads.some((t) => t.id === "t-abc")).toBe(true);
    });

    it("resets state on failure", async () => {
      mockInvoke.mockRejectedValueOnce(new Error("fail"));
      const id = await useAppStore.getState().createThread();
      expect(id).toBeNull();
      expect(useAppStore.getState().currentThreadId).toBeNull();
    });

    it("does not duplicate existing thread", async () => {
      useAppStore.setState({
        threads: [{ id: "t-abc", preview: "", updatedAt: 1, archived: false }],
      });
      mockInvoke.mockResolvedValueOnce({ thread: { id: "t-abc" } });
      await useAppStore.getState().createThread();
      expect(useAppStore.getState().threads.filter((t) => t.id === "t-abc")).toHaveLength(1);
    });
  });

  describe("loadThreads", () => {
    it("filters out archived threads", async () => {
      mockInvoke.mockResolvedValueOnce({
        data: [
          { id: "t1", name: "Active", archived: false, updatedAt: 1 },
          { id: "t2", name: "Archived", archived: true, updatedAt: 2 },
          { id: "t3", name: "Also Active", archived: false, updatedAt: 3 },
        ],
      });
      await useAppStore.getState().loadThreads();
      const threads = useAppStore.getState().threads;
      expect(threads).toHaveLength(2);
      expect(threads.every((t) => !t.archived)).toBe(true);
    });
  });
});
