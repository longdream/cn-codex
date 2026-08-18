import { beforeEach, describe, expect, it, vi } from "vitest";

const { invokeMock } = vi.hoisted(() => ({
  invokeMock: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: invokeMock,
}));

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

describe("standaloneConfigWrite", () => {
  beforeEach(() => {
    vi.resetModules();
    invokeMock.mockReset();
  });

  it("submits writes in call order", async () => {
    const firstResult = { status: "ok", filePath: "config.toml" };
    const secondResult = { status: "ok", filePath: "config.toml" };
    const firstInvoke = deferred<typeof firstResult>();
    invokeMock
      .mockImplementationOnce(() => firstInvoke.promise)
      .mockResolvedValueOnce(secondResult);
    const { standaloneConfigWrite } = await import("../api/standalone");

    const firstWrite = standaloneConfigWrite([
      { keyPath: "model", value: "model-a" },
    ]);
    const secondWrite = standaloneConfigWrite([
      { keyPath: "model", value: "model-b" },
    ]);
    await Promise.resolve();

    expect(invokeMock).toHaveBeenCalledTimes(1);
    firstInvoke.resolve(firstResult);
    await expect(firstWrite).resolves.toEqual(firstResult);
    await expect(secondWrite).resolves.toEqual(secondResult);
    expect(invokeMock).toHaveBeenCalledTimes(2);
    expect(invokeMock.mock.calls[1]).toEqual([
      "standalone_config_write",
      { edits: [{ keyPath: "model", value: "model-b" }] },
    ]);
  });

  it("continues the queue after a failed write", async () => {
    const firstInvoke = deferred<{ status: string; filePath: string }>();
    const secondResult = { status: "ok", filePath: "config.toml" };
    invokeMock
      .mockImplementationOnce(() => firstInvoke.promise)
      .mockResolvedValueOnce(secondResult);
    const { standaloneConfigWrite } = await import("../api/standalone");

    const failedWrite = standaloneConfigWrite([
      { keyPath: "model", value: "invalid" },
    ]);
    const nextWrite = standaloneConfigWrite([
      { keyPath: "model", value: "valid" },
    ]);
    await Promise.resolve();

    firstInvoke.reject(new Error("write failed"));
    await expect(failedWrite).rejects.toThrow("write failed");
    await expect(nextWrite).resolves.toEqual(secondResult);
    expect(invokeMock).toHaveBeenCalledTimes(2);
  });
});
