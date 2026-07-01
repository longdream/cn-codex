import React from "react";
import ReactDOM from "react-dom/client";
import { invoke } from "@tauri-apps/api/core";
import App from "./App";
import { windowShowMain } from "./api/window";
import "./index.css";

const startupScriptStart = performance.now();
const STARTUP_STABLE_EVENT = "cn-codex:startup-stable";
// 启动可见性等待上限：超过该时间仍未收到稳定信号也先显示主窗，
// 避免发布版在慢机器上长时间“黑屏但进程已启动”。
const STARTUP_SHOW_WINDOW_TIMEOUT_MS = 1500;

function logStartupPhase(phase: string): void {
  const elapsedMs = (performance.now() - startupScriptStart).toFixed(1);
  console.info(`[startup][web] ${phase} (+${elapsedMs} ms)`);
}

async function waitForStartupStable(
  timeoutMs = STARTUP_SHOW_WINDOW_TIMEOUT_MS,
): Promise<void> {
  if (document.documentElement.dataset.startupStable === "1") {
    return;
  }
  await new Promise<void>((resolve) => {
    let resolved = false;
    const cleanup = () => {
      window.removeEventListener(STARTUP_STABLE_EVENT, onStable);
      window.clearTimeout(timeoutHandle);
    };
    const finish = () => {
      if (resolved) {
        return;
      }
      resolved = true;
      cleanup();
      resolve();
    };
    const onStable = () => {
      logStartupPhase("startup_stable_event");
      finish();
    };
    const timeoutHandle = window.setTimeout(() => {
      logStartupPhase(`startup_stable_timeout_${timeoutMs}ms`);
      finish();
    }, timeoutMs);
    window.addEventListener(STARTUP_STABLE_EVENT, onStable, { once: true });
  });
}

// F12+D 组合键打开 DevTools（任何环境均可用）
let f12Pressed = false;
document.addEventListener("keydown", (e) => {
  if (e.key === "F12") {
    e.preventDefault();
    f12Pressed = true;
  }
  if (f12Pressed && (e.key === "d" || e.key === "D")) {
    e.preventDefault();
    void invoke("window_toggle_devtools");
  }
});
document.addEventListener("keyup", (e) => {
  if (e.key === "F12") f12Pressed = false;
});

// 全局未捕获 Promise rejection 处理，防止 WebView2 崩溃白屏
window.addEventListener("unhandledrejection", (event) => {
  console.error("Unhandled promise rejection:", event.reason);
  event.preventDefault();
});

document.addEventListener("DOMContentLoaded", () => {
  logStartupPhase("dom_content_loaded");
});

requestAnimationFrame(() => {
  logStartupPhase("first_animation_frame");
});

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);

logStartupPhase("react_root_render_called");

void waitForStartupStable()
  .then(() => windowShowMain())
  .then(() => {
    logStartupPhase("main_window_shown");
  })
  .catch((err) => {
    console.warn("windowShowMain failed:", err);
  });
