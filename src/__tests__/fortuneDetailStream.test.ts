import { beforeEach, describe, expect, it, vi } from "vitest";

const { listenMock } = vi.hoisted(() => ({
  listenMock: vi.fn(),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: listenMock,
}));

import {
  INITIAL_FORTUNE_DETAIL_STREAM_STATE,
  reduceFortuneDetailStreamState,
  registerFortuneDetailStreamListeners,
} from "../hooks/useFortuneDetailStream";

describe("reduceFortuneDetailStreamState", () => {
  it("appends delta only for matching requestId", () => {
    const started = reduceFortuneDetailStreamState(INITIAL_FORTUNE_DETAIL_STREAM_STATE, {
      type: "request:start",
      requestId: "req-1",
    });
    const ignored = reduceFortuneDetailStreamState(started, {
      type: "event:delta",
      payload: { requestId: "req-2", delta: "ignore" },
    });
    const updated = reduceFortuneDetailStreamState(ignored, {
      type: "event:delta",
      payload: { requestId: "req-1", delta: "hello" },
    });

    expect(ignored.streamedText).toBe("");
    expect(updated.streamedText).toBe("hello");
    expect(updated.status).toBe("streaming");
  });

  it("parses detail on completed event for matching requestId", () => {
    const started = reduceFortuneDetailStreamState(INITIAL_FORTUNE_DETAIL_STREAM_STATE, {
      type: "request:start",
      requestId: "req-1",
    });
    const completed = reduceFortuneDetailStreamState(started, {
      type: "event:completed",
      payload: {
        requestId: "req-1",
        text: JSON.stringify({
          qimenDetail: "奇门详解",
          advice: "今日宜稳住节奏",
        }),
      },
    });

    expect(completed.status).toBe("completed");
    expect(completed.detail?.qimenDetail).toContain("奇门详解");
    expect(completed.detail?.advice).toContain("稳住节奏");
  });

  it("enters error state when completed text cannot be parsed", () => {
    const started = reduceFortuneDetailStreamState(INITIAL_FORTUNE_DETAIL_STREAM_STATE, {
      type: "request:start",
      requestId: "req-1",
    });
    const errored = reduceFortuneDetailStreamState(started, {
      type: "event:completed",
      payload: {
        requestId: "req-1",
        text: "不是 JSON",
      },
    });

    expect(errored.status).toBe("error");
    expect(errored.error).toContain("parse");
  });

  it("handles error branch and ignores other requestId", () => {
    const started = reduceFortuneDetailStreamState(INITIAL_FORTUNE_DETAIL_STREAM_STATE, {
      type: "request:start",
      requestId: "req-1",
    });
    const ignored = reduceFortuneDetailStreamState(started, {
      type: "event:error",
      payload: { requestId: "req-2", message: "should-ignore" },
    });
    const errored = reduceFortuneDetailStreamState(ignored, {
      type: "event:error",
      payload: { requestId: "req-1", message: "stream failed" },
    });

    expect(ignored.status).toBe("loading");
    expect(errored.status).toBe("error");
    expect(errored.error).toBe("stream failed");
  });
});

describe("registerFortuneDetailStreamListeners", () => {
  beforeEach(() => {
    listenMock.mockReset();
  });

  it("registers all listeners and unsubscribes on cleanup", async () => {
    const handlers = new Map<string, (event: { payload: unknown }) => void>();
    const unlistenFns = [vi.fn(), vi.fn(), vi.fn(), vi.fn()];
    let callIndex = 0;
    listenMock.mockImplementation((eventName: string, handler: (event: { payload: unknown }) => void) => {
      handlers.set(eventName, handler);
      const unlisten = unlistenFns[callIndex] ?? vi.fn();
      callIndex += 1;
      return Promise.resolve(unlisten);
    });

    const onStarted = vi.fn();
    const onDelta = vi.fn();
    const onCompleted = vi.fn();
    const onError = vi.fn();

    const cleanup = await registerFortuneDetailStreamListeners({
      onStarted,
      onDelta,
      onCompleted,
      onError,
    });

    expect(listenMock).toHaveBeenCalledTimes(4);

    handlers.get("fortune-detail-started")?.({ payload: { requestId: "req-1" } });
    handlers.get("fortune-detail-delta")?.({ payload: { requestId: "req-1", delta: "abc" } });
    handlers.get("fortune-detail-completed")?.({
      payload: { requestId: "req-1", text: "{\"qimenDetail\":\"x\",\"advice\":\"y\"}" },
    });
    handlers.get("fortune-detail-error")?.({ payload: { requestId: "req-1", message: "boom" } });

    expect(onStarted).toHaveBeenCalledWith({ requestId: "req-1" });
    expect(onDelta).toHaveBeenCalledWith({ requestId: "req-1", delta: "abc" });
    expect(onCompleted).toHaveBeenCalledWith({
      requestId: "req-1",
      text: "{\"qimenDetail\":\"x\",\"advice\":\"y\"}",
    });
    expect(onError).toHaveBeenCalledWith({ requestId: "req-1", message: "boom" });

    cleanup();
    for (const unlisten of unlistenFns) {
      expect(unlisten).toHaveBeenCalledTimes(1);
    }
  });
});
