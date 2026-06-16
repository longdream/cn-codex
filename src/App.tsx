import { useCallback, useEffect, useRef } from "react";
import { IntlProvider } from "react-intl";
import {
  getServerStatus,
  standaloneInit,
  standaloneConfigRead,
} from "./api";
import { getUserHomeDir } from "./api/window";
import { ChatPage } from "./components/chat/ChatPage";
import { ErrorBoundary } from "./components/common/ErrorBoundary";
import { RightPanel } from "./components/layout/RightPanel";
import { Sidebar } from "./components/layout/Sidebar";
import { TitleBar } from "./components/layout/TitleBar";
import { ApprovalModal } from "./components/approval/ApprovalModal";
import { SettingsPanel } from "./components/settings/SettingsPanel";
import { useTauriEvents } from "./hooks/useTauriEvents";
import enUS from "./i18n/en-US/common.json";
import zhCN from "./i18n/zh-CN/common.json";
import { useAppStore, initStoreFromDb } from "./stores/appStore";
import { useSettingsStore, initSettingsFromDb } from "./stores/settingsStore";

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
  const initRunSeq = useRef(0);

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
    const runId = ++initRunSeq.current;
    const initStartedAt = performance.now();
    const isLatestRun = () => initRunSeq.current === runId;
    const logInitPhase = (phase: string) => {
      const elapsedMs = (performance.now() - initStartedAt).toFixed(1);
      console.info(`[startup][web] init#${runId} ${phase} (+${elapsedMs} ms)`);
    };

    const store = useAppStore.getState();
    store.setInitError(null);
    store.setInitialized(false);
    logInitPhase("begin");

    try {
      // 第 1 阶段（关键路径）：仅执行“必须完成后才能正常渲染壳”的初始化。
      // 这部分越短，发布版首屏可见时间就越快。
      await standaloneInit();
      logInitPhase("standalone_init_done");

      // 从 SQLite 恢复项目/会话映射与主题设置，这两项会直接影响首屏布局与文案。
      await Promise.all([initStoreFromDb(), initSettingsFromDb()]);
      logInitPhase("sqlite_state_restored");

      // 如果这次初始化已经被更新的一轮覆盖，则立即停止后续回写，避免状态被旧任务污染。
      if (!isLatestRun()) {
        logInitPhase("aborted_before_interactive_due_to_newer_run");
        return;
      }

      // 第 2 阶段（可交互门槛）：关键数据准备完成后立即放开 UI，
      // 把历史会话恢复等重操作放到后台，避免用户看到长时间白屏或“卡启动”。
      useAppStore.getState().setInitialized(true);
      useAppStore.getState().setInitError(null);
      logInitPhase("interactive_ready");

      // 第 3 阶段（后台恢复）：这些任务互相独立，不应阻塞首屏。
      // 3.1 恢复用户目录，仅用于部分路径推断，失败不影响主流程。
      void (async () => {
        try {
          const homeDir = await getUserHomeDir();
          if (!isLatestRun()) {
            return;
          }
          useAppStore.getState().setUserHomeDir(homeDir);
          logInitPhase("home_dir_restored");
        } catch {
          // Home dir is best-effort
        }
      })();

      // 3.2 恢复运行时状态 + 当前会话，并在最后刷新侧栏会话列表。
      void (async () => {
        try {
          const status = await getServerStatus();
          if (!isLatestRun()) {
            return;
          }
          const latestStore = useAppStore.getState();
          latestStore.setServerRuntime({
            cwd: status.cwd,
            configDir: status.configDir,
            configPath: status.configPath,
          });
          logInitPhase("server_status_restored");
          if (status.currentThreadId) {
            try {
              latestStore.setCurrentThread(status.currentThreadId);
              await latestStore.loadThread(status.currentThreadId);
              if (isLatestRun()) {
                logInitPhase("current_thread_restored");
              }
            } catch (restoreErr) {
              console.warn("Failed to restore current thread:", restoreErr);
            }
          }
        } catch {
          // Runtime paths are best-effort
        } finally {
          if (!isLatestRun()) {
            return;
          }
          // 会话列表刷新是纯增强能力，不应影响应用可用性。
          // 即便失败也只会导致侧栏数据延后，不会阻断聊天功能。
          await useAppStore.getState().loadThreads().catch((loadErr) => {
            console.warn("Failed to refresh thread list in background:", loadErr);
          });
          if (isLatestRun()) {
            logInitPhase("thread_list_refreshed");
          }
        }
      })();

      // 3.3 恢复当前模型配置，属于体验增强项，不应挡首屏。
      void (async () => {
        try {
          const cfg = await standaloneConfigRead();
          const model = (cfg?.config?.model as string) ?? null;
          if (model) {
            if (!isLatestRun()) {
              return;
            }
            useAppStore.getState().setCurrentModel(model);
          }
          if (isLatestRun()) {
            logInitPhase("model_config_restored");
          }
        } catch {
          // Config read is best-effort
        }
      })();
    } catch (err) {
      console.error("Engine init failed:", err);
      if (isLatestRun()) {
        useAppStore.getState().setInitError(
          `Engine initialization failed: ${extractErrorMessage(err)}`,
        );
        logInitPhase("failed");
      }
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
  const rightPanelVisible = useAppStore((s) => s.rightPanelVisible);

  return (
    <ErrorBoundary>
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
            {rightPanelVisible && <RightPanel />}
          </div>
        </div>
        {showSettings && <SettingsPanel onClose={() => setShowSettings(false)} />}
        <ApprovalModal />
      </IntlProvider>
    </ErrorBoundary>
  );
}

export default App;
