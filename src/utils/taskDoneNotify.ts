import { invoke } from "@tauri-apps/api/core";
import { formatDuration } from "./formatDuration";

/**
 * 任务完成系统通知。
 *
 * 桌面端：调用 Rust 命令 `notify_task_done`，
 * 弹出 Windows 右下角原生系统通知，并通过 MessageBeep 播放系统提示音；
 * Web/降级环境：使用浏览器 Notification API + WebAudio 提示音兜底。
 */

export interface TaskDoneNotifyPayload {
  status?: "completed" | "failed" | "cancelled" | string;
  durationMs?: number;
  /** 覆盖默认标题（由调用方传入本地化文案）。 */
  title?: string;
  /** 覆盖默认正文（由调用方传入本地化文案）。 */
  body?: string;
}

/** 判断是否运行在 Tauri WebView 内。 */
export function isTauriRuntime(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

function titleForStatus(status: TaskDoneNotifyPayload["status"]): string {
  if (status === "failed") return "任务失败";
  if (status === "cancelled") return "任务已取消";
  return "任务已完成";
}

function bodyWithDuration(durationMs?: number): string {
  const duration = formatDuration(durationMs);
  return duration && duration !== "n/a" ? `耗时 ${duration}` : "";
}

export async function notifyTaskDone(payload: TaskDoneNotifyPayload = {}): Promise<void> {
  const { status = "completed", durationMs, title, body } = payload;
  const resolvedTitle = title ?? titleForStatus(status);
  const resolvedBody = body ?? bodyWithDuration(durationMs);
  if (isTauriRuntime()) {
    try {
      await invoke("notify_task_done", {
        request: {
          title: resolvedTitle,
          body: resolvedBody,
          sound: true,
          failed: status === "failed",
        },
      });
    } catch {
      // 通知是尽力而为的能力，失败时不应影响主流程。
    }
    return;
  }

  // 浏览器降级路径
  playWebAudioChime(status !== "failed");
  if (typeof Notification !== "undefined") {
    try {
      if (Notification.permission === "granted") {
        new Notification(resolvedTitle, { body: resolvedBody });
      } else if (Notification.permission !== "denied") {
        void Notification.requestPermission().then((permission) => {
          if (permission === "granted") {
            new Notification(resolvedTitle, { body: resolvedBody });
          }
        });
      }
    } catch {
      // ignore
    }
  }
}

function playWebAudioChime(successful: boolean): void {
  try {
    const AudioCtx =
      window.AudioContext ??
      (window as unknown as { webkitAudioContext?: typeof AudioContext }).webkitAudioContext;
    if (!AudioCtx) return;
    const ctx = new AudioCtx();
    const now = ctx.currentTime;
    const notes = successful ? [660, 880] : [440, 330];
    notes.forEach((freq, index) => {
      const osc = ctx.createOscillator();
      const gain = ctx.createGain();
      osc.type = "sine";
      osc.frequency.value = freq;
      const startAt = now + index * 0.18;
      gain.gain.setValueAtTime(0.0001, startAt);
      gain.gain.exponentialRampToValueAtTime(0.25, startAt + 0.02);
      gain.gain.exponentialRampToValueAtTime(0.0001, startAt + 0.22);
      osc.connect(gain).connect(ctx.destination);
      osc.start(startAt);
      osc.stop(startAt + 0.24);
    });
    setTimeout(() => void ctx.close().catch(() => {}), 800);
  } catch {
    // 音频播放失败静默忽略
  }
}
