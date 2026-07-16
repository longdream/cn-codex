import React, { useEffect, useState } from "react";
import ReactDOM from "react-dom/client";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { IntlProvider } from "react-intl";
import { ComputerUseOverlayFrame } from "./components/common/ComputerUseOverlayFrame";
import { useSettingsStore, initSettingsFromDb } from "./stores/settingsStore";
import enUS from "./i18n/en-US/common.json";
import zhCN from "./i18n/zh-CN/common.json";
import "./index.css";

const messages: Record<string, Record<string, string>> = {
  "zh-CN": zhCN,
  "en-US": enUS,
};

const OVERLAY_STATE_EVENT = "computer-use-overlay-state";

function ComputerUseOverlayApp() {
  const locale = useSettingsStore((s) => s.locale);
  // 默认关闭；只有主窗口明确广播 active=true 时才显示，避免对话结束后残留外框。
  const [active, setActive] = useState(false);

  useEffect(() => {
    void initSettingsFromDb();
  }, []);

  useEffect(() => {
    // 整屏覆盖窗必须保持透明，否则会盖住桌面内容。
    document.documentElement.classList.add("computer-use-overlay-root");
    document.body.classList.add("computer-use-overlay-root");
    document.documentElement.style.background = "transparent";
    document.body.style.background = "transparent";
    return () => {
      document.documentElement.classList.remove("computer-use-overlay-root");
      document.body.classList.remove("computer-use-overlay-root");
      document.documentElement.style.removeProperty("background");
      document.body.style.removeProperty("background");
    };
  }, []);

  useEffect(() => {
    let disposed = false;
    let unlisten: UnlistenFn | null = null;

    void listen<{ active?: boolean }>(OVERLAY_STATE_EVENT, (event) => {
      if (disposed) {
        return;
      }
      setActive(Boolean(event.payload?.active));
    }).then((fn) => {
      if (disposed) {
        fn();
        return;
      }
      unlisten = fn;
    });

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  return (
    <IntlProvider
      locale={locale}
      messages={messages[locale] ?? zhCN}
      defaultLocale="zh-CN"
    >
      <ComputerUseOverlayFrame active={active} variant="screen" />
    </IntlProvider>
  );
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <ComputerUseOverlayApp />
  </React.StrictMode>,
);
