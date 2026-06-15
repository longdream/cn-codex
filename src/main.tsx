import React from "react";
import ReactDOM from "react-dom/client";
import { invoke } from "@tauri-apps/api/core";
import App from "./App";
import { windowShowMain } from "./api/window";
import "./index.css";

const startupScriptStart = performance.now();

function logStartupPhase(phase: string): void {
  const elapsedMs = (performance.now() - startupScriptStart).toFixed(1);
  console.info(`[startup][web] ${phase} (+${elapsedMs} ms)`);
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

// 延迟两帧再显示主窗口：
// 1) 让 React 首次挂载先完成，避免用户看到中间态白窗；
// 2) 若前端资源较慢，至少会看到 index.html 的深色启动占位而不是纯白。
requestAnimationFrame(() => {
  requestAnimationFrame(() => {
    void windowShowMain()
      .then(() => {
        logStartupPhase("main_window_shown");
      })
      .catch((err) => {
        console.warn("windowShowMain failed:", err);
      });
  });
});
