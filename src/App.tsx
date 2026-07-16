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
import { AppBackground } from "./components/common/AppBackground";
import { RightPanel } from "./components/layout/RightPanel";
import { Sidebar } from "./components/layout/Sidebar";
import { StatusBar } from "./components/layout/StatusBar";
import { TitleBar } from "./components/layout/TitleBar";
import { ApprovalModal } from "./components/approval/ApprovalModal";
import { FortuneBubble } from "./components/common/FortuneBubble";
import { RecordingToggle } from "./components/common/RecordingToggle";
import { UpdateModal } from "./components/common/UpdateModal";
import { ComputerUseOverlay } from "./components/common/ComputerUseOverlay";
import { SettingsPanel } from "./components/settings/SettingsPanel";
import { WorkflowExtractModal } from "./components/workflow/WorkflowExtractModal";
import { useTauriEvents } from "./hooks/useTauriEvents";
import enUS from "./i18n/en-US/common.json";
import zhCN from "./i18n/zh-CN/common.json";
import {
  useAppStore,
  initStoreFromDb,
  normalizeImageGenerationSettings,
  SIDEBAR_WIDTH_MIN,
  SIDEBAR_WIDTH_MAX,
  RIGHT_PANEL_WIDTH_MIN,
  RIGHT_PANEL_WIDTH_MAX,
} from "./stores/appStore";
import { useSettingsStore, initSettingsFromDb } from "./stores/settingsStore";

const messages: Record<string, Record<string, string>> = {
  "zh-CN": zhCN,
  "en-US": enUS,
};
const STARTUP_STABLE_EVENT = "cn-codex:startup-stable";

// 聊天主区域最小可用宽度：拖拽时始终保留该空间，避免输入区/消息区被压坏。
const MAIN_PANEL_MIN_WIDTH = 560;

function extractErrorMessage(err: unknown): string {
  if (typeof err === "string") return err;
  if (err instanceof Error) return err.message;
  return String(err);
}

function AppContent() {
  useTauriEvents();
  return null;
}

function App() {
  const locale = useSettingsStore((s) => s.locale);
  const theme = useSettingsStore((s) => s.theme);
  const initialized = useAppStore((s) => s.initialized);
  const initError = useAppStore((s) => s.initError);
  const initStarted = useRef(false);
  const initRunSeq = useRef(0);
  const startupStableNotified = useRef(false);

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
      // 仅当 config.toml 中的 model 仍属于当前启用供应商时才覆盖，
      // 避免启动后把全局模型拉回列表第一项/旧供应商模型。
      void (async () => {
        try {
          const cfg = await standaloneConfigRead();
          const model = typeof cfg?.config?.model === "string" ? cfg.config.model.trim() : "";
          if (model && isLatestRun()) {
            const store = useAppStore.getState();
            const activeProvider = store.providers.find((provider) => provider.id === store.activeProviderId) ?? null;
            const modelBelongsToActiveProvider = Boolean(
              activeProvider?.models.some((item) => item.id === model),
            );
            if (modelBelongsToActiveProvider) {
              store.setCurrentModel(model);
            } else if (!store.currentModel && activeProvider?.models[0]?.id) {
              // 没有可用全局模型时，回退到启用供应商的默认模型。
              store.setCurrentModel(activeProvider.models[0].id);
            }
          }
          const activeEpIdx = cfg?.config?.active_endpoint_index;
          if (typeof activeEpIdx === "number" && isLatestRun()) {
            useAppStore.setState({ activeEndpointIndex: activeEpIdx });
          }
          if (isLatestRun()) {
            const rawImageGeneration = cfg?.config?.image_generation;
            const imageConfig =
              rawImageGeneration && typeof rawImageGeneration === "object"
                ? (rawImageGeneration as Record<string, unknown>)
                : null;
            useAppStore.getState().setImageGenerationSettings(
              normalizeImageGenerationSettings({
                enabled:
                  typeof imageConfig?.enabled === "boolean"
                    ? imageConfig.enabled
                    : undefined,
                model: typeof imageConfig?.model === "string" ? imageConfig.model : undefined,
                baseUrl: typeof imageConfig?.base_url === "string" ? imageConfig.base_url : undefined,
                apiKey: typeof imageConfig?.api_key === "string" ? imageConfig.api_key : undefined,
              }),
            );
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

  const emitStartupStable = useCallback(() => {
    if (startupStableNotified.current) {
      return;
    }
    startupStableNotified.current = true;
    document.documentElement.dataset.startupStable = "1";
    window.dispatchEvent(new Event(STARTUP_STABLE_EVENT));
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

  useEffect(() => {
    if (!initialized && !initError) {
      return;
    }
    // 启动稳定信号：关键初始化结束后再通知主窗显示，减少可见中间态。
    requestAnimationFrame(() => {
      requestAnimationFrame(() => {
        emitStartupStable();
      });
    });
  }, [emitStartupStable, initError, initialized]);

  const showSettings = useAppStore((s) => s.showSettings);
  const setShowSettings = useAppStore((s) => s.setShowSettings);
  const rightPanelVisible = useAppStore((s) => s.rightPanelVisible);
  const sidebarWidth = useAppStore((s) => s.sidebarWidth);
  const rightPanelWidth = useAppStore((s) => s.rightPanelWidth);
  const setSidebarWidth = useAppStore((s) => s.setSidebarWidth);
  const setRightPanelWidth = useAppStore((s) => s.setRightPanelWidth);
  const resizeStateRef = useRef<{
    kind: "left" | "right";
    pointerId: number;
    startX: number;
    startSidebarWidth: number;
    startRightPanelWidth: number;
    rightPanelVisibleAtStart: boolean;
  } | null>(null);

  const clamp = useCallback((value: number, min: number, max: number): number => {
    return Math.min(max, Math.max(min, value));
  }, []);

  const clearResizeState = useCallback(() => {
    document.body.classList.remove("app-resizing");
    resizeStateRef.current = null;
  }, []);

  const applyResizeFromPointer = useCallback(
    (clientX: number) => {
      const state = resizeStateRef.current;
      if (!state) {
        return;
      }

      const viewportWidth = window.innerWidth;
      if (state.kind === "left") {
        // 左侧拖拽：基于起点偏移调整左栏宽度，并按“中间最小宽度”动态收敛上限。
        const nextWidth = state.startSidebarWidth + (clientX - state.startX);
        const maxByLayout =
          viewportWidth -
          (state.rightPanelVisibleAtStart ? state.startRightPanelWidth : 0) -
          MAIN_PANEL_MIN_WIDTH;
        const dynamicMax = Math.max(SIDEBAR_WIDTH_MIN, Math.min(SIDEBAR_WIDTH_MAX, maxByLayout));
        setSidebarWidth(clamp(nextWidth, SIDEBAR_WIDTH_MIN, dynamicMax));
        return;
      }

      // 右侧拖拽：用“起始右栏宽度 - 鼠标位移”得到目标值，保证主区宽度不被侵占。
      const nextWidth = state.startRightPanelWidth - (clientX - state.startX);
      const maxByLayout = viewportWidth - state.startSidebarWidth - MAIN_PANEL_MIN_WIDTH;
      const dynamicMax = Math.max(
        RIGHT_PANEL_WIDTH_MIN,
        Math.min(RIGHT_PANEL_WIDTH_MAX, maxByLayout),
      );
      setRightPanelWidth(clamp(nextWidth, RIGHT_PANEL_WIDTH_MIN, dynamicMax));
    },
    [clamp, setRightPanelWidth, setSidebarWidth],
  );

  const startResize = useCallback(
    (kind: "left" | "right") => (event: React.PointerEvent<HTMLDivElement>) => {
      if (event.button !== 0) {
        return;
      }
      event.preventDefault();
      event.currentTarget.setPointerCapture(event.pointerId);
      resizeStateRef.current = {
        kind,
        pointerId: event.pointerId,
        startX: event.clientX,
        startSidebarWidth: sidebarWidth,
        startRightPanelWidth: rightPanelWidth,
        rightPanelVisibleAtStart: rightPanelVisible,
      };
      document.body.classList.add("app-resizing");
    },
    [rightPanelVisible, rightPanelWidth, sidebarWidth],
  );

  const handleResizerPointerMove = useCallback(
    (event: React.PointerEvent<HTMLDivElement>) => {
      const state = resizeStateRef.current;
      if (!state || state.pointerId !== event.pointerId) {
        return;
      }
      applyResizeFromPointer(event.clientX);
    },
    [applyResizeFromPointer],
  );

  const handleResizerPointerUp = useCallback(
    (event: React.PointerEvent<HTMLDivElement>) => {
      const state = resizeStateRef.current;
      if (!state || state.pointerId !== event.pointerId) {
        return;
      }
      if (event.currentTarget.hasPointerCapture(event.pointerId)) {
        event.currentTarget.releasePointerCapture(event.pointerId);
      }
      clearResizeState();
    },
    [clearResizeState],
  );

  const handleResizerPointerCancel = useCallback(
    (event: React.PointerEvent<HTMLDivElement>) => {
      const state = resizeStateRef.current;
      if (!state || state.pointerId !== event.pointerId) {
        return;
      }
      if (event.currentTarget.hasPointerCapture(event.pointerId)) {
        event.currentTarget.releasePointerCapture(event.pointerId);
      }
      clearResizeState();
    },
    [clearResizeState],
  );

  const enforceLayoutBounds = useCallback(() => {
    // 当窗口尺寸变化时重新校验左右宽度，避免历史持久化值在小窗口下挤占中间区域。
    // 使用 store 最新状态，避免在拖拽过程中因宽度变化触发 effect 重建并中断拖拽。
    const viewportWidth = window.innerWidth;
    const {
      sidebarWidth: currentSidebarWidth,
      rightPanelWidth: currentRightPanelWidth,
      rightPanelVisible: currentRightPanelVisible,
    } = useAppStore.getState();

    const rightWidth = currentRightPanelVisible ? currentRightPanelWidth : 0;
    const maxSidebarByLayout = viewportWidth - rightWidth - MAIN_PANEL_MIN_WIDTH;
    const sidebarMax = Math.max(
      SIDEBAR_WIDTH_MIN,
      Math.min(SIDEBAR_WIDTH_MAX, maxSidebarByLayout),
    );
    if (currentSidebarWidth > sidebarMax) {
      setSidebarWidth(sidebarMax);
    }

    if (!currentRightPanelVisible) {
      return;
    }

    const maxRightByLayout = viewportWidth - currentSidebarWidth - MAIN_PANEL_MIN_WIDTH;
    const rightMax = Math.max(
      RIGHT_PANEL_WIDTH_MIN,
      Math.min(RIGHT_PANEL_WIDTH_MAX, maxRightByLayout),
    );
    if (currentRightPanelWidth > rightMax) {
      setRightPanelWidth(rightMax);
    }
  }, [setRightPanelWidth, setSidebarWidth]);

  useEffect(() => {
    enforceLayoutBounds();
  }, [enforceLayoutBounds, rightPanelVisible]);

  useEffect(() => {
    window.addEventListener("resize", enforceLayoutBounds);
    return () => {
      window.removeEventListener("resize", enforceLayoutBounds);
    };
  }, [enforceLayoutBounds]);

  useEffect(() => {
    return () => {
      clearResizeState();
    };
  }, [clearResizeState]);

  return (
    <ErrorBoundary>
      <IntlProvider
        locale={locale}
        messages={messages[locale] ?? zhCN}
        defaultLocale="zh-CN"
      >
        <AppContent />
        <div className="app-frame">
          <AppBackground />
          <div className="app-shell-content">
            <TitleBar />
            <div className="app-workbench">
              <Sidebar />
              <div
                role="separator"
                aria-label="Resize sidebar"
                aria-orientation="vertical"
                className="app-resizer app-resizer-left"
                onPointerDown={startResize("left")}
                onPointerMove={handleResizerPointerMove}
                onPointerUp={handleResizerPointerUp}
                onPointerCancel={handleResizerPointerCancel}
              />
              <div className="app-main">
                <ChatPage />
              </div>
              {/* 右侧面板始终挂载：关闭时只隐藏，避免卸载 TerminalPanel 导致终端进程被关闭 */}
              <div
                className="flex h-full flex-shrink-0"
                style={rightPanelVisible ? undefined : { display: "none" }}
                aria-hidden={!rightPanelVisible}
              >
                <div
                  role="separator"
                  aria-label="Resize right panel"
                  aria-orientation="vertical"
                  className="app-resizer app-resizer-right"
                  onPointerDown={startResize("right")}
                  onPointerMove={handleResizerPointerMove}
                  onPointerUp={handleResizerPointerUp}
                  onPointerCancel={handleResizerPointerCancel}
                />
                <RightPanel />
              </div>
            </div>
            <StatusBar />
          </div>
        </div>
        {showSettings && <SettingsPanel onClose={() => setShowSettings(false)} />}
        <ApprovalModal />
        <WorkflowExtractModal />
        <FortuneBubble />
        <RecordingToggle />
        <UpdateModal />
        <ComputerUseOverlay />
      </IntlProvider>
    </ErrorBoundary>
  );
}

export default App;
