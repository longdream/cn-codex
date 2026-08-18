export interface FrameScheduler {
  request(callback: () => void): number;
  cancel(handle: number): void;
}

const browserFrameScheduler: FrameScheduler = {
  request(callback) {
    if (typeof globalThis.requestAnimationFrame === "function") {
      return globalThis.requestAnimationFrame(callback);
    }
    return window.setTimeout(callback, 0);
  },
  cancel(handle) {
    if (typeof globalThis.cancelAnimationFrame === "function") {
      globalThis.cancelAnimationFrame(handle);
    } else {
      window.clearTimeout(handle);
    }
  },
};

interface PendingText {
  chunks: string[];
  length: number;
}

/** Coalesces high-frequency text chunks into at most one store write per thread per frame. */
export class StreamingTextBatcher {
  private pendingByThread = new Map<string, PendingText>();
  private frameHandle: number | null = null;
  private frameGeneration = 0;
  private disposed = false;

  constructor(
    private readonly publish: (threadId: string, text: string) => void,
    private readonly scheduler: FrameScheduler = browserFrameScheduler,
  ) {}

  append(threadId: string, delta: string): void {
    if (this.disposed || !delta) return;
    const pending = this.pendingByThread.get(threadId);
    if (pending) {
      pending.chunks.push(delta);
      pending.length += delta.length;
    } else {
      this.pendingByThread.set(threadId, { chunks: [delta], length: delta.length });
    }
    if (this.frameHandle != null) return;
    const generation = ++this.frameGeneration;
    this.frameHandle = this.scheduler.request(() => {
      if (generation !== this.frameGeneration) return;
      this.publishFrame();
    });
  }

  pendingLength(threadId: string): number {
    return this.pendingByThread.get(threadId)?.length ?? 0;
  }

  flush(threadId: string): void {
    if (this.disposed) return;
    const pending = this.pendingByThread.get(threadId);
    if (!pending) return;
    this.pendingByThread.delete(threadId);
    this.cancelFrameIfIdle();
    this.publish(threadId, pending.chunks.join(""));
  }

  discard(threadId: string): void {
    if (this.disposed) return;
    this.pendingByThread.delete(threadId);
    this.cancelFrameIfIdle();
  }

  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    this.pendingByThread.clear();
    if (this.frameHandle != null) {
      this.scheduler.cancel(this.frameHandle);
      this.frameGeneration += 1;
      this.frameHandle = null;
    }
  }

  private cancelFrameIfIdle(): void {
    if (this.pendingByThread.size > 0 || this.frameHandle == null) return;
    this.scheduler.cancel(this.frameHandle);
    this.frameGeneration += 1;
    this.frameHandle = null;
  }

  private publishFrame(): void {
    this.frameHandle = null;
    if (this.disposed || this.pendingByThread.size === 0) return;
    const pending = this.pendingByThread;
    this.pendingByThread = new Map();
    for (const [threadId, text] of pending) {
      this.publish(threadId, text.chunks.join(""));
    }
  }
}
