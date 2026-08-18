import { describe, expect, it } from "vitest";
import {
  StreamingTextBatcher,
  type FrameScheduler,
} from "../utils/streamingTextBatcher";

function controlledScheduler() {
  let nextHandle = 1;
  const callbacks = new Map<number, () => void>();
  const cancelled: number[] = [];
  const scheduler: FrameScheduler = {
    request(callback) {
      const handle = nextHandle++;
      callbacks.set(handle, callback);
      return handle;
    },
    cancel(handle) {
      cancelled.push(handle);
      callbacks.delete(handle);
    },
  };

  return {
    scheduler,
    cancelled,
    pendingFrames: () => callbacks.size,
    runFrame() {
      const scheduled = [...callbacks.values()];
      callbacks.clear();
      scheduled.forEach((callback) => callback());
    },
  };
}

describe("StreamingTextBatcher", () => {
  it("publishes cumulative chunks once per thread per animation frame", () => {
    const frames = controlledScheduler();
    const published: Array<[string, string]> = [];
    const batcher = new StreamingTextBatcher(
      (threadId, text) => published.push([threadId, text]),
      frames.scheduler,
    );

    for (let index = 0; index < 100_000; index += 1) {
      batcher.append("thread-a", "x");
    }
    batcher.append("thread-b", "other");

    expect(frames.pendingFrames()).toBe(1);
    expect(batcher.pendingLength("thread-a")).toBe(100_000);
    expect(published).toEqual([]);

    frames.runFrame();

    expect(published).toEqual([
      ["thread-a", "x".repeat(100_000)],
      ["thread-b", "other"],
    ]);
    expect(batcher.pendingLength("thread-a")).toBe(0);
  });

  it("flushes one thread synchronously without losing another thread", () => {
    const frames = controlledScheduler();
    const published: Array<[string, string]> = [];
    const batcher = new StreamingTextBatcher(
      (threadId, text) => published.push([threadId, text]),
      frames.scheduler,
    );

    batcher.append("thread-a", "tail-a");
    batcher.append("thread-b", "tail-b");
    batcher.flush("thread-a");

    expect(published).toEqual([["thread-a", "tail-a"]]);
    expect(frames.pendingFrames()).toBe(1);

    frames.runFrame();
    expect(published).toEqual([
      ["thread-a", "tail-a"],
      ["thread-b", "tail-b"],
    ]);
  });

  it("discards stale text and cancels empty scheduled frames", () => {
    const frames = controlledScheduler();
    const published: Array<[string, string]> = [];
    const batcher = new StreamingTextBatcher(
      (threadId, text) => published.push([threadId, text]),
      frames.scheduler,
    );

    batcher.append("thread-a", "partial");
    batcher.discard("thread-a");

    expect(frames.pendingFrames()).toBe(0);
    expect(frames.cancelled).toEqual([1]);
    frames.runFrame();
    expect(published).toEqual([]);

    batcher.append("thread-a", "new");
    batcher.dispose();
    expect(frames.pendingFrames()).toBe(0);
    expect(published).toEqual([]);
  });
});
