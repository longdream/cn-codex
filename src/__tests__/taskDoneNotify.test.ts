import { beforeEach, describe, expect, it, vi } from "vitest";

const invoke = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invoke(...args),
}));

import { notifyTaskDone } from "../utils/taskDoneNotify";

function setTauriRuntime(active: boolean) {
  if (active) {
    (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
  } else {
    delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
  }
}

describe("notifyTaskDone", () => {
  beforeEach(() => {
    invoke.mockReset();
    invoke.mockResolvedValue(undefined);
    setTauriRuntime(true);
  });

  it("invokes notify_task_done with completed defaults in Tauri runtime", async () => {
    await notifyTaskDone({ status: "completed", durationMs: 65_000 });

    expect(invoke).toHaveBeenCalledTimes(1);
    const [command, args] = invoke.mock.calls[0];
    expect(command).toBe("notify_task_done");
    expect(args.request).toMatchObject({
      title: "任务已完成",
      body: "耗时 1m 5s",
      sound: true,
      failed: false,
    });
  });

  it("marks failed status with warning sound flag and localized override", async () => {
    await notifyTaskDone({ status: "failed", title: "自定义失败", body: "" });

    const [, args] = invoke.mock.calls[0];
    expect(args.request).toMatchObject({
      title: "自定义失败",
      sound: true,
      failed: true,
    });
  });

  it("does not invoke the command outside the Tauri runtime", async () => {
    setTauriRuntime(false);
    const originalNotification = (window as unknown as Record<string, unknown>).Notification;
    const notificationCtor = vi.fn();
    (window as unknown as Record<string, unknown>).Notification = notificationCtor;

    const notificationSpy = vi
      .spyOn(window, "Notification")
      .mockImplementation(((...args: unknown[]) => notificationCtor(...args)) as unknown as typeof Notification);

    try {
      await notifyTaskDone({ status: "completed" });
      expect(invoke).not.toHaveBeenCalled();
    } finally {
      notificationSpy.mockRestore();
      if (originalNotification === undefined) {
        delete (window as unknown as Record<string, unknown>).Notification;
      } else {
        (window as unknown as Record<string, unknown>).Notification = originalNotification;
      }
      setTauriRuntime(false);
    }
  });

  it("swallows backend errors so turn flow is never blocked", async () => {
    invoke.mockRejectedValueOnce(new Error("boom"));
    await expect(notifyTaskDone({ status: "completed" })).resolves.toBeUndefined();
  });
});
