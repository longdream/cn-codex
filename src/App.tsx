import { useCallback, useEffect, useRef } from "react";
import { IntlProvider } from "react-intl";
import {
  getServerStatus,
  standaloneInit,
  standaloneConfigRead,
} from "./api";
import { ChatPage } from "./components/chat/ChatPage";
import { Sidebar } from "./components/layout/Sidebar";
import { TitleBar } from "./components/layout/TitleBar";
import { SettingsPanel } from "./components/settings/SettingsPanel";
import { useTauriEvents } from "./hooks/useTauriEvents";
import enUS from "./i18n/en-US/common.json";
import zhCN from "./i18n/zh-CN/common.json";
import { useAppStore } from "./stores/appStore";
import { useSettingsStore } from "./stores/settingsStore";

const messages: Record<string, Record<string, string>> = {
  "zh-CN": zhCN,
  "en-US": enUS,
};

function extractErrorMessage(err: unknown): string {
  if (typeof err === "string") return err;
  if (err instanceof Error) return err.message;
  return String(err);
}

function App() {
  const locale = useSettingsStore((s) => s.locale);
  const theme = useSettingsStore((s) => s.theme);
  const initStarted = useRef(false);

  useTauriEvents();

  useEffect(() => {
    const media = window.matchMedia("(prefers-color-scheme: light)");

    const applyTheme = () => {
      const resolvedTheme =
        theme === "system" ? (media.matches ? "light" : "dark") : theme;
      document.documentElement.dataset.theme = resolvedTheme;
      document.documentElement.style.colorScheme = resolvedTheme;
    };

    applyTheme();

    if (theme !== "system") {
      return;
    }

    media.addEventListener("change", applyTheme);
    return () => media.removeEventListener("change", applyTheme);
  }, [theme]);

  const doInit = useCallback(async () => {
    const store = useAppStore.getState();
    store.setInitError(null);
    store.setInitialized(false);

    try {
      await standaloneInit();

      try {
        const status = await getServerStatus();
        useAppStore.getState().setServerRuntime({
          cwd: status.cwd,
          configDir: status.configDir,
          configPath: status.configPath,
        });
      } catch {
        // Runtime paths are best-effort
      }

      try {
        const cfg = await standaloneConfigRead();
        const model = (cfg?.config?.model as string) ?? null;
        if (model) {
          useAppStore.getState().setCurrentModel(model);
        }
      } catch {
        // Config read is best-effort
      }

      useAppStore.getState().setInitialized(true);
      useAppStore.getState().setInitError(null);

      await useAppStore.getState().loadThreads();
      console.info("cn-codex engine initialized");
    } catch (err) {
      console.error("Engine init failed:", err);
      useAppStore.getState().setInitError(
        `Engine initialization failed: ${extractErrorMessage(err)}`,
      );
    }
  }, []);

  useEffect(() => {
    useAppStore.getState().setRetryInit(() => {
      initStarted.current = false;
      void doInit();
    });
  }, [doInit]);

  useEffect(() => {
    if (initStarted.current) return;
    initStarted.current = true;
    void doInit();
  }, [doInit]);

  const showSettings = useAppStore((s) => s.showSettings);
  const setShowSettings = useAppStore((s) => s.setShowSettings);

  return (
    <IntlProvider
      locale={locale}
      messages={messages[locale] ?? zhCN}
      defaultLocale="zh-CN"
    >
      <div className="app-frame">
        <TitleBar />
        <div className="app-workbench">
          <Sidebar />
          <div className="app-main">
            <ChatPage />
          </div>
        </div>
      </div>
      {showSettings && <SettingsPanel onClose={() => setShowSettings(false)} />}
    </IntlProvider>
  );
}

export default App;
