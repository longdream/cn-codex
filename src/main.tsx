import React from "react";
import ReactDOM from "react-dom/client";
import { invoke } from "@tauri-apps/api/core";
import App from "./App";
import "./index.css";

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

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
