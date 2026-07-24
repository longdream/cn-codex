import { IconApps, IconBrowser, IconCheck, IconChevronLeft, IconChevronRight, IconExternalLink, IconFolderOpen, IconGitBranch, IconHome, IconMessagePlus, IconNetwork, IconPencil, IconRefresh, IconTerminal2, IconX } from "@tabler/icons-react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { memo, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useIntl } from "react-intl";
import { useAppStore } from "../../stores/appStore";
import { browserApplyDomEdit, browserGetEditContext, browserGetNavigationState, browserGoBack, browserGoForward, browserNavigateHome, browserPollPickedElement, browserRefreshPreview, browserStartPickMode, browserStopPickMode, revealInExplorer, windowAttachBrowser, windowCloseBrowser, windowDetachBrowser, windowNavigateBrowser, windowOpenBrowser, windowResizeBrowser, type BrowserDomEditRequest, type BrowserEditContext, type BrowserNavigationState, type BrowserPickedElement } from "../../api/window";
import { FileTree } from "./FileTree";
import { GitPanel } from "./GitPanel";
import { TerminalPanel } from "./TerminalPanel";
import { LanCollabPanel } from "../lan/LanCollabPanel";
import { MiniAppSidePanel } from "./MiniAppSidePanel";

function latestBrowserToolCall(messages: ReturnType<typeof useAppStore.getState>["messages"]) {
  for (let i = messages.length - 1; i >= 0; i -= 1) {
    const toolCalls = messages[i].toolCalls;
    if (!toolCalls) continue;
    const match = [...toolCalls].reverse().find((call) => call.name === "browser_run");
    if (match) {
      return match;
    }
  }
  return null;
}

type BrowserToolCallLite = {
  id: string;
  status: string;
  output?: string;
};

let cachedBrowserMessagesRef: ReturnType<typeof useAppStore.getState>["messages"] | null = null;
let cachedBrowserToolCallSignature: string | null = null;

function selectLatestBrowserToolCallSignature(
  messages: ReturnType<typeof useAppStore.getState>["messages"],
): string | null {
  // Critical: this selector is evaluated on every store update, including streaming tokens.
  // Cache by messages reference so chat output stays O(1) and does not freeze the right panel.
  if (messages === cachedBrowserMessagesRef) {
    return cachedBrowserToolCallSignature;
  }
  cachedBrowserMessagesRef = messages;
  const match = latestBrowserToolCall(messages);
  if (!match) {
    cachedBrowserToolCallSignature = null;
    return null;
  }
  // Primitive signature keeps zustand v5 selector stable without equalityFn.
  cachedBrowserToolCallSignature = JSON.stringify({
    id: match.id,
    status: match.status,
    output: match.output ?? null,
  });
  return cachedBrowserToolCallSignature;
}

function parseBrowserToolCallSignature(signature: string | null): BrowserToolCallLite | null {
  if (!signature) {
    return null;
  }
  try {
    const parsed = JSON.parse(signature) as {
      id?: unknown;
      status?: unknown;
      output?: unknown;
    };
    if (typeof parsed.id !== "string" || typeof parsed.status !== "string") {
      return null;
    }
    return {
      id: parsed.id,
      status: parsed.status,
      output: typeof parsed.output === "string" ? parsed.output : undefined,
    };
  } catch {
    return null;
  }
}

function parseBrowserRunOutput(output?: string): {
  ok?: boolean;
  errorCode?: string;
  message?: string;
  hint?: string;
  screenshots: string[];
  finalUrl?: string;
  title?: string;
  browserMode?: string;
} | null {
  if (!output) return null;
  const start = output.indexOf("{");
  const end = output.lastIndexOf("}");
  if (start < 0 || end <= start) return null;

  try {
    const parsed = JSON.parse(output.slice(start, end + 1)) as Record<string, unknown>;
    return {
      ok: typeof parsed.ok === "boolean" ? parsed.ok : undefined,
      errorCode: typeof parsed.errorCode === "string" ? parsed.errorCode : undefined,
      message: typeof parsed.message === "string" ? parsed.message : undefined,
      hint: typeof parsed.hint === "string" ? parsed.hint : undefined,
      screenshots: Array.isArray(parsed.screenshots)
        ? parsed.screenshots.filter((item): item is string => typeof item === "string" && item.trim().length > 0)
        : [],
      finalUrl: typeof parsed.finalUrl === "string" ? parsed.finalUrl : undefined,
      title: typeof parsed.title === "string" ? parsed.title : undefined,
      browserMode: typeof parsed.browserMode === "string" ? parsed.browserMode : undefined,
    };
  } catch {
    return null;
  }
}

function localPreviewSrc(path: string, workspaceCwd: string | null): string | null {
  if (!path) return null;
  const sanitized = normalizeLocalImagePath(path);
  if (!sanitized) return null;
  const normalized = sanitized.replace(/\\/g, "/");
  if (/^[a-zA-Z]:\//.test(normalized)) {
    return convertFileSrc(normalized);
  }
  if (normalized.startsWith("//")) {
    return convertFileSrc(normalized);
  }
  if (normalized.startsWith("/")) {
    return convertFileSrc(normalized);
  }
  if (!workspaceCwd) return null;
  const base = workspaceCwd.replace(/\\/g, "/").replace(/\/+$/, "");
  return convertFileSrc(`${base}/${normalized}`);
}

function normalizeLocalImagePath(raw: string): string {
  const trimmed = raw.trim();
  if (!trimmed) return "";
  if (trimmed.startsWith("\\\\?\\UNC\\")) {
    return `\\\\${trimmed.slice("\\\\?\\UNC\\".length)}`;
  }
  if (trimmed.startsWith("\\\\?\\")) {
    return trimmed.slice("\\\\?\\".length);
  }
  return trimmed;
}

const WEB_SNIPPET_MIME = "application/x-cn-codex-web-snippet";

function safeInvokeUnlisten(unlisten: (() => void) | null | undefined): void {
  if (!unlisten) {
    return;
  }
  try {
    const maybePromise = (unlisten as () => unknown)();
    void Promise.resolve(maybePromise).catch(() => undefined);
  } catch {
    // callback 已失效时直接吞掉，避免 unhandled rejection 打断 UI 生命周期。
  }
}

function buildWebSnippetName(picked: BrowserPickedElement): string {
  const tag = picked.tagName?.trim().toLowerCase();
  const normalizedTag = tag && tag.length > 0 ? tag : "element";
  const normalizedText = picked.text?.trim() ?? "";
  if (!normalizedText) {
    return normalizedTag;
  }
  const clipped = normalizedText.length > 24
    ? `${normalizedText.slice(0, 24)}...`
    : normalizedText;
  return `${normalizedTag}: ${clipped}`;
}

export function RightPanel() {
  const intl = useIntl();
  const rightPanelTab = useAppStore((s) => s.rightPanelTab);
  const rightPanelWidth = useAppStore((s) => s.rightPanelWidth);
  const rightPanelVisible = useAppStore((s) => s.rightPanelVisible);
  const browserPanelUrl = useAppStore((s) => s.browserPanelUrl);
  const browserPanelStatus = useAppStore((s) => s.browserPanelStatus);
  const browserCanGoBack = useAppStore((s) => s.browserCanGoBack);
  const browserCanGoForward = useAppStore((s) => s.browserCanGoForward);
  const browserSyncTrigger = useAppStore((s) => s.browserSyncTrigger);
  const browserActive = useAppStore((s) => s.browserActive);
  const browserDetached = useAppStore((s) => s.browserDetached);
  const setBrowserActive = useAppStore((s) => s.setBrowserActive);
  const setBrowserDetached = useAppStore((s) => s.setBrowserDetached);
  const workspaceCwd = useAppStore((s) => s.workspaceCwd);
  // Only track the latest browser_run tool call so chat streaming does not re-render the whole right panel.
  const browserCallSignature = useAppStore((s) => selectLatestBrowserToolCallSignature(s.messages));
  const browserCall = useMemo(
    () => parseBrowserToolCallSignature(browserCallSignature),
    [browserCallSignature],
  );
  const currentThreadId = useAppStore((s) => s.currentThreadId);
  const addAttachedFile = useAppStore((s) => s.addAttachedFile);
  const setRightPanelTab = useAppStore((s) => s.setRightPanelTab);
  const setBrowserPanelState = useAppStore((s) => s.setBrowserPanelState);
  const browserContainerRef = useRef<HTMLDivElement>(null);
  const resizeTimerRef = useRef<number | null>(null);
  const pickedAtRef = useRef(0);
  const attachingBackRef = useRef(false);
  const browserActiveRef = useRef(false);
  const browserDetachedRef = useRef(false);
  const addressFocusedRef = useRef(false);
  const rightPanelTabRef = useRef(rightPanelTab);
  const syncBrowserPositionRef = useRef<() => void>(() => undefined);
  const attachPopupBackToPanelRef = useRef<(nextUrl?: string) => Promise<void>>(() => Promise.resolve());

  const [browserEditContext, setBrowserEditContext] = useState<BrowserEditContext | null>(null);
  const [browserEditMode, setBrowserEditMode] = useState(false);
  const [browserEditError, setBrowserEditError] = useState<string | null>(null);
  const [pickedElement, setPickedElement] = useState<BrowserPickedElement | null>(null);
  const [editText, setEditText] = useState("");
  const [editColor, setEditColor] = useState("");
  const [editFontSize, setEditFontSize] = useState("");
  const [applyingEdit, setApplyingEdit] = useState(false);
  const [addressInput, setAddressInput] = useState("");

  const browserToolbarCompact = rightPanelWidth < 420;
  const browserToolbarExtraCompact = rightPanelWidth < 360;
  const browserToolbarUltraCompact = rightPanelWidth < 320;
  const browserEditorCompact = rightPanelWidth < 430;
  const browserEditorExtraCompact = rightPanelWidth < 360;

  const browserOutput = useMemo(
    () => parseBrowserRunOutput(browserCall?.output),
    [browserCall?.output],
  );

  const screenshots = browserOutput?.screenshots ?? [];
  const browserReady = browserActive || browserDetached;

  const applyNavigationState = useCallback((navigation: BrowserNavigationState) => {
    const nextUrl = navigation.url?.trim() || "about:blank";
    setBrowserPanelState({
      url: nextUrl,
      title: navigation.title ?? "",
      canGoBack: navigation.canGoBack,
      canGoForward: navigation.canGoForward,
      status: "success",
    });
  }, [setBrowserPanelState]);

  const syncNavigationState = useCallback(() => {
    if (!browserReady) {
      return;
    }
    void browserGetNavigationState()
      .then((navigation) => {
        applyNavigationState(navigation);
      })
      .catch(() => undefined);
  }, [applyNavigationState, browserReady]);

  useEffect(() => {
    if (addressFocusedRef.current) {
      return;
    }
    setAddressInput(browserPanelUrl ?? browserOutput?.finalUrl ?? "");
  }, [browserPanelUrl, browserOutput?.finalUrl]);

  const syncBrowserPosition = useCallback(() => {
    const el = browserContainerRef.current;
    // 独立窗口模式下 WebView 已挂到 popup，主窗口绝不能再写坐标，否则会把页面挤偏。
    if (!el || !browserActive || browserDetached) return;
    const rect = el.getBoundingClientRect();
    if (rect.width > 0 && rect.height > 0) {
      void windowResizeBrowser(
        Math.round(rect.x),
        Math.round(rect.y),
        Math.round(rect.width),
        Math.round(rect.height),
      );
    }
  }, [browserActive, browserDetached]);

  useEffect(() => {
    if (!browserActive || browserDetached || !rightPanelVisible || rightPanelTab !== "browser") return;
    const el = browserContainerRef.current;
    if (!el) return;

    const observer = new ResizeObserver(() => {
      if (resizeTimerRef.current) cancelAnimationFrame(resizeTimerRef.current);
      resizeTimerRef.current = requestAnimationFrame(syncBrowserPosition);
    });
    observer.observe(el);

    syncBrowserPosition();

    const onWindowResize = () => {
      if (resizeTimerRef.current) cancelAnimationFrame(resizeTimerRef.current);
      resizeTimerRef.current = requestAnimationFrame(syncBrowserPosition);
    };
    window.addEventListener("resize", onWindowResize);

    return () => {
      observer.disconnect();
      window.removeEventListener("resize", onWindowResize);
      if (resizeTimerRef.current) cancelAnimationFrame(resizeTimerRef.current);
    };
  }, [browserActive, browserDetached, rightPanelVisible, rightPanelTab, syncBrowserPosition]);

  const handleOpenBrowser = useCallback((url?: string) => {
    const el = browserContainerRef.current;
    if (!el) return;
    const rect = el.getBoundingClientRect();
    const normalizedUrl = url?.trim();
    setBrowserPanelState({
      ...(normalizedUrl ? { url: normalizedUrl } : {}),
      status: "running",
      canGoBack: false,
      canGoForward: false,
    });
    void windowOpenBrowser(url, {
      x: Math.round(rect.x),
      y: Math.round(rect.y),
      width: Math.max(Math.round(rect.width), 200),
      height: Math.max(Math.round(rect.height), 200),
    }, workspaceCwd ?? undefined).then((info) => {
      setBrowserActive(true);
      setBrowserDetached(false);
      setBrowserPanelState({
        url: info.url,
        status: "success",
      });
      setTimeout(syncBrowserPosition, 100);
      requestAnimationFrame(syncBrowserPosition);
      syncNavigationState();
    }).catch(() => {
      setBrowserActive(true);
      setBrowserDetached(false);
      syncBrowserPosition();
    });
  }, [workspaceCwd, setBrowserActive, setBrowserDetached, setBrowserPanelState, syncBrowserPosition, syncNavigationState]);

  const handleCloseBrowser = useCallback(() => {
    void browserStopPickMode().catch(() => undefined).finally(() => {
      setBrowserEditMode(false);
      setPickedElement(null);
      setBrowserEditError(null);
      setBrowserEditContext(null);
      void windowCloseBrowser().finally(() => {
        // 主动关闭时同步清理状态，避免后续仍显示旧页面状态。
        setBrowserPanelState({
          status: "idle",
          url: null,
          title: null,
          canGoBack: false,
          canGoForward: false,
        });
        setBrowserActive(false);
        setBrowserDetached(false);
      });
    });
  }, [setBrowserActive, setBrowserDetached, setBrowserPanelState]);

  const refreshEditContext = useCallback(() => {
    if (!browserActive || rightPanelTab !== "browser") {
      setBrowserEditContext(null);
      return;
    }
    void browserGetEditContext()
      .then((context) => {
        setBrowserEditContext(context);
        if (!context.editable && browserEditMode) {
          setBrowserEditMode(false);
          setPickedElement(null);
        }
      })
      .catch((err) => {
        setBrowserEditContext(null);
        setBrowserEditError(String(err));
      });
  }, [browserActive, rightPanelTab, browserEditMode]);

  const scheduleRefreshEditContext = useCallback(() => {
    // attach 后 WebView/CDP 初始化存在短暂竞态；延迟 + 单次重试降低瞬时失败。
    window.setTimeout(() => {
      refreshEditContext();
    }, 160);
    window.setTimeout(() => {
      refreshEditContext();
    }, 520);
  }, [refreshEditContext]);

  const attachPopupBackToPanel = useCallback((nextUrl?: string) => {
    if (attachingBackRef.current) {
      return Promise.resolve();
    }
    attachingBackRef.current = true;
    const el = browserContainerRef.current;
    const rect = el?.getBoundingClientRect();
    const safeRect = rect
      ? {
        x: Math.round(rect.x),
        y: Math.round(rect.y),
        width: Math.max(Math.round(rect.width), 200),
        height: Math.max(Math.round(rect.height), 200),
      }
      : undefined;
    return windowAttachBrowser(nextUrl, safeRect, workspaceCwd ?? undefined)
      .then((info) => {
        setBrowserDetached(false);
        setBrowserActive(true);
        setBrowserEditError(null);
        setBrowserPanelState({
          url: info.url,
          status: "success",
          canGoBack: false,
          canGoForward: false,
        });
        requestAnimationFrame(syncBrowserPosition);
        setTimeout(syncBrowserPosition, 80);
        scheduleRefreshEditContext();
        syncNavigationState();
      })
      .catch((err) => {
        setBrowserEditError(String(err));
      })
      .finally(() => {
        attachingBackRef.current = false;
      });
  }, [workspaceCwd, setBrowserDetached, setBrowserActive, setBrowserPanelState, syncBrowserPosition, scheduleRefreshEditContext, syncNavigationState]);

  useEffect(() => {
    browserActiveRef.current = browserActive;
    browserDetachedRef.current = browserDetached;
    rightPanelTabRef.current = rightPanelTab;
  }, [browserActive, browserDetached, rightPanelTab]);

  useEffect(() => {
    syncBrowserPositionRef.current = syncBrowserPosition;
  }, [syncBrowserPosition]);

  useEffect(() => {
    attachPopupBackToPanelRef.current = attachPopupBackToPanel;
  }, [attachPopupBackToPanel]);

  const handleToggleDetachMode = useCallback(() => {
    if (browserDetached) {
      void attachPopupBackToPanel();
      return;
    }

    void browserStopPickMode().catch(() => undefined).finally(() => {
      setBrowserEditMode(false);
      setPickedElement(null);
      setBrowserEditError(null);
      void windowDetachBrowser()
        .then((info) => {
          setBrowserDetached(true);
          setBrowserActive(false);
          setBrowserEditError(null);
          setBrowserPanelState({
            url: info.url,
            status: "success",
            canGoBack: false,
            canGoForward: false,
          });
          syncNavigationState();
        })
        .catch((err) => {
          setBrowserEditError(String(err));
        });
    });
  }, [browserDetached, attachPopupBackToPanel, setBrowserDetached, setBrowserActive, setBrowserPanelState, syncNavigationState]);

  const handleNavigateSubmit = useCallback((rawUrl: string) => {
    const url = rawUrl.trim();
    if (!url) {
      return;
    }
    setAddressInput(url);
    if (browserReady) {
      void windowNavigateBrowser(url, workspaceCwd ?? undefined)
        .then(() => {
          setBrowserEditMode(false);
          setPickedElement(null);
          if (!browserDetached) {
            refreshEditContext();
          }
          syncNavigationState();
        })
        .catch((err) => {
          setBrowserEditError(String(err));
        });
      return;
    }
    handleOpenBrowser(url);
  }, [browserReady, workspaceCwd, browserDetached, refreshEditContext, syncNavigationState, handleOpenBrowser]);

  const handleBrowserBack = useCallback(() => {
    if (!browserReady) {
      return;
    }
    void browserGoBack()
      .then((navigation) => {
        applyNavigationState(navigation);
      })
      .catch((err) => {
        setBrowserEditError(String(err));
      });
  }, [browserReady, applyNavigationState]);

  const handleBrowserForward = useCallback(() => {
    if (!browserReady) {
      return;
    }
    void browserGoForward()
      .then((navigation) => {
        applyNavigationState(navigation);
      })
      .catch((err) => {
        setBrowserEditError(String(err));
      });
  }, [browserReady, applyNavigationState]);

  const handleBrowserRefresh = useCallback(() => {
    if (!browserReady) {
      return;
    }
    void browserRefreshPreview()
      .then(() => {
        syncNavigationState();
      })
      .catch((err) => {
        setBrowserEditError(String(err));
      });
  }, [browserReady, syncNavigationState]);

  const handleBrowserHome = useCallback(() => {
    if (!browserReady) {
      return;
    }
    void browserNavigateHome()
      .then((navigation) => {
        applyNavigationState(navigation);
      })
      .catch((err) => {
        setBrowserEditError(String(err));
      });
  }, [browserReady, applyNavigationState]);

  const handleToggleEditMode = useCallback(() => {
    if (browserDetached) {
      setBrowserEditError(intl.formatMessage({ id: "rightPanel.webEditUnavailableDetached" }));
      return;
    }
    if (browserEditMode) {
      void browserStopPickMode()
        .catch((err) => {
          setBrowserEditError(String(err));
        })
        .finally(() => {
          setBrowserEditMode(false);
          setPickedElement(null);
        });
      return;
    }

    void browserStartPickMode()
      .then((context) => {
        setBrowserEditContext(context);
        setBrowserEditMode(true);
        setBrowserEditError(null);
      })
      .catch((err) => {
        setBrowserEditMode(false);
        setBrowserEditError(String(err));
      });
  }, [browserDetached, browserEditMode, intl]);

  const appendPickedSnippetToComposer = useCallback((picked: BrowserPickedElement) => {
    const sourcePath = picked.sourcePath?.trim() || browserEditContext?.sourcePath?.trim() || undefined;
    const selectorCandidates = Array.isArray(picked.selectorCandidates)
      ? picked.selectorCandidates.map((item) => item.trim()).filter((item) => item.length > 0)
      : [];
    addAttachedFile({
      kind: "webSnippet",
      name: buildWebSnippetName(picked),
      type: WEB_SNIPPET_MIME,
      size: (picked.text ?? "").trim().length,
      url: picked.url,
      selector: picked.selector,
      selectorCandidates,
      sourcePath,
      tagName: picked.tagName,
      text: picked.text,
      rect: {
        x: picked.x,
        y: picked.y,
        width: picked.width,
        height: picked.height,
      },
    });
  }, [addAttachedFile, browserEditContext?.sourcePath]);

  const handleApplyDomEdit = useCallback(() => {
    if (!pickedElement) return;
    const selector = pickedElement.selector || pickedElement.selectorCandidates[0] || "";
    if (!selector) {
      setBrowserEditError(intl.formatMessage({ id: "rightPanel.webEditSelectorMissing" }));
      return;
    }
    const request: BrowserDomEditRequest = {
      selector,
      text: editText,
      ...(editColor.trim() ? { color: editColor.trim() } : {}),
      ...(editFontSize.trim() ? { fontSize: editFontSize.trim() } : {}),
    };

    setApplyingEdit(true);
    setBrowserEditError(null);
    void browserApplyDomEdit(request)
      .then((result) => {
        setPickedElement((prev) => prev
          ? {
            ...prev,
            selector: result.selector,
            sourcePath: result.sourcePath ?? prev.sourcePath,
            text: editText,
          }
          : prev);
      })
      .catch((err) => {
        setBrowserEditError(String(err));
      })
      .finally(() => {
        setApplyingEdit(false);
      });
  }, [pickedElement, editText, editColor, editFontSize, intl]);

  const handleInsertPickedToChat = useCallback(() => {
    if (!pickedElement) return;
    appendPickedSnippetToComposer(pickedElement);
  }, [pickedElement, appendPickedSnippetToComposer]);

  // 切换 tab 时隐藏/恢复 webview 位置（不关闭）
  useEffect(() => {
    // 右侧面板关闭或切到非浏览器 tab 时，把原生 WebView 移出视野，但不关闭页面。
    if (browserDetached) {
      return;
    }
    if (browserActive && (!rightPanelVisible || rightPanelTab !== "browser")) {
      void windowResizeBrowser(-9999, -9999, 0, 0);
    } else if (browserActive && rightPanelVisible && rightPanelTab === "browser") {
      syncBrowserPosition();
    }
  }, [rightPanelTab, rightPanelVisible, browserActive, browserDetached, syncBrowserPosition]);

  // browser_run 开始时强制重新定位 WebView，避免黑屏
  useEffect(() => {
    if (
      browserSyncTrigger > 0 &&
      browserActive &&
      !browserDetached &&
      rightPanelVisible &&
      rightPanelTab === "browser"
    ) {
      syncBrowserPosition();
    }
  }, [browserSyncTrigger, browserActive, browserDetached, rightPanelVisible, rightPanelTab, syncBrowserPosition]);

  // 后端 CDP 就绪后重新定位 WebView（解决后端 -9999 覆盖前端定位的竞态）
  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | null = null;
    void listen("browser-webview-ready", () => {
      if (
        browserActiveRef.current &&
        !browserDetachedRef.current &&
        useAppStore.getState().rightPanelVisible &&
        rightPanelTabRef.current === "browser"
      ) {
        syncBrowserPositionRef.current();
        setTimeout(() => syncBrowserPositionRef.current(), 100);
      }
    }).then((fn) => {
      if (disposed) {
        safeInvokeUnlisten(fn);
        return;
      }
      unlisten = fn;
    }).catch(() => undefined);
    return () => {
      disposed = true;
      safeInvokeUnlisten(unlisten);
    };
  }, []);

  // browserActive 变为 true 时延迟同步位置（确保 DOM 已布局）
  useEffect(() => {
    if (browserActive && !browserDetached && rightPanelVisible && rightPanelTab === "browser") {
      const timer = setTimeout(syncBrowserPosition, 50);
      requestAnimationFrame(syncBrowserPosition);
      return () => clearTimeout(timer);
    }
  }, [browserActive, browserDetached, rightPanelVisible, rightPanelTab, syncBrowserPosition]);

  // 切换对话时：若恢复的浏览器状态为活跃且浏览器未打开，则自动打开并导航到保存的 URL。
  useEffect(() => {
    if (browserActive && browserPanelUrl && rightPanelTab === "browser") {
      handleOpenBrowser(browserPanelUrl);
    } else if (!browserActive && !browserDetached && rightPanelTab === "browser") {
      // 恢复的对话浏览器不活跃，但当前 WebView 还在，需要移出屏幕
      void windowResizeBrowser(-9999, -9999, 0, 0);
    }
    // 只在当前对话 ID 变更时触发
  }, [currentThreadId]); // eslint-disable-line react-hooks/exhaustive-deps

  // 当切到 browser tab 时自动激活 webview（仅在后端未主动创建时触发）
  useEffect(() => {
    if (
      rightPanelVisible &&
      rightPanelTab === "browser" &&
      !browserActive &&
      !browserDetached
    ) {
      handleOpenBrowser(browserPanelUrl ?? browserOutput?.finalUrl);
    }
  }, [
    rightPanelVisible,
    rightPanelTab,
    browserActive,
    browserDetached,
    handleOpenBrowser,
    browserPanelUrl,
    browserOutput?.finalUrl,
  ]);

  useEffect(() => {
    refreshEditContext();
  }, [refreshEditContext, browserPanelUrl, browserOutput?.finalUrl, browserSyncTrigger]);

  useEffect(() => {
    let disposed = false;
    let offDetached: (() => void) | null = null;
    let offClosed: (() => void) | null = null;
    void listen<{ detached?: boolean; url?: string | null }>("browser-detached-changed", (event) => {
      const detached = Boolean(event.payload?.detached);
      setBrowserDetached(detached);
      if (detached) {
        setBrowserActive(false);
        setBrowserEditError(null);
        setBrowserEditMode(false);
        setPickedElement(null);
      } else if (rightPanelTabRef.current === "browser") {
        setBrowserActive(true);
        setBrowserEditError(null);
        requestAnimationFrame(() => syncBrowserPositionRef.current());
        syncNavigationState();
      }
      if (typeof event.payload?.url === "string" && event.payload.url.trim()) {
        setBrowserPanelState({ url: event.payload.url.trim() });
      }
    }).then((fn) => {
      if (disposed) {
        safeInvokeUnlisten(fn);
        return;
      }
      offDetached = fn;
    }).catch(() => undefined);
    void listen<{ url?: string | null }>("browser-popup-closed", (event) => {
      if (!browserDetachedRef.current || attachingBackRef.current || rightPanelTabRef.current !== "browser") {
        return;
      }
      const nextUrl = typeof event.payload?.url === "string" && event.payload.url.trim()
        ? event.payload.url.trim()
        : undefined;
      void attachPopupBackToPanelRef.current(nextUrl);
    }).then((fn) => {
      if (disposed) {
        safeInvokeUnlisten(fn);
        return;
      }
      offClosed = fn;
    }).catch(() => undefined);
    return () => {
      disposed = true;
      safeInvokeUnlisten(offDetached);
      safeInvokeUnlisten(offClosed);
    };
  }, [setBrowserDetached, setBrowserActive, setBrowserPanelState, syncNavigationState]);

  useEffect(() => {
    if (!browserEditMode) {
      return;
    }

    const timer = window.setInterval(() => {
      void browserPollPickedElement()
        .then((picked) => {
          if (!picked || picked.pickedAt <= pickedAtRef.current) {
            return;
          }
          pickedAtRef.current = picked.pickedAt;
          setPickedElement(picked);
          setEditText(picked.text ?? "");
          appendPickedSnippetToComposer(picked);
        })
        .catch((err) => {
          setBrowserEditError(String(err));
        });
    }, 400);

    return () => {
      window.clearInterval(timer);
    };
  }, [browserEditMode, appendPickedSnippetToComposer]);

  useEffect(() => {
    if (!browserEditMode) return;
    if (browserDetached) {
      void browserStopPickMode().catch(() => undefined);
      setBrowserEditMode(false);
      return;
    }
    if (browserActive && rightPanelTab === "browser") return;
    void browserStopPickMode().catch(() => undefined);
    setBrowserEditMode(false);
  }, [browserEditMode, browserDetached, browserActive, rightPanelTab]);

  useEffect(() => {
    const onRefreshRequest = () => {
      if (!browserActive && !browserDetached) return;
      handleBrowserRefresh();
    };
    window.addEventListener("cn-codex:browser-refresh-requested", onRefreshRequest);
    return () => {
      window.removeEventListener("cn-codex:browser-refresh-requested", onRefreshRequest);
    };
  }, [browserActive, browserDetached, handleBrowserRefresh]);

  useEffect(() => {
    if (rightPanelTab !== "browser" || !browserReady) {
      return;
    }
    syncNavigationState();
    const timer = window.setInterval(() => {
      syncNavigationState();
    }, 1200);
    return () => {
      window.clearInterval(timer);
    };
  }, [rightPanelTab, browserReady, syncNavigationState]);

  useEffect(() => {
    return () => {
      // 应用级卸载时关闭 WebView。关闭右侧面板不再卸载本组件，因此不会误杀页面。
      void browserStopPickMode().catch(() => undefined).finally(() => {
        void windowCloseBrowser().finally(() => {
          setBrowserActive(false);
          setBrowserDetached(false);
          setBrowserPanelState({
            status: "idle",
            url: null,
            title: null,
            canGoBack: false,
            canGoForward: false,
          });
          setBrowserEditMode(false);
          setPickedElement(null);
        });
      });
    };
  }, [setBrowserActive, setBrowserDetached, setBrowserPanelState]);

  const browserTabLabel = intl.formatMessage({ id: "rightPanel.browser" });
  const projectTabLabel = intl.formatMessage({ id: "rightPanel.project" });
  const terminalTabLabel = intl.formatMessage({ id: "rightPanel.terminal" });
  const gitTabLabel = intl.formatMessage({ id: "rightPanel.git" });
  const lanTabLabel = intl.formatMessage({ id: "rightPanel.lan" });
  const miniappTabLabel = intl.formatMessage({ id: "rightPanel.miniapp" });
  const canToggleEditMode = !browserDetached && (browserEditMode || Boolean(browserEditContext?.editable));
  const editModeTitle = browserEditMode
    ? intl.formatMessage({ id: "rightPanel.webEditDisable" })
    : browserDetached
      ? intl.formatMessage({ id: "rightPanel.webEditUnavailableDetached" })
    : canToggleEditMode
      ? intl.formatMessage({ id: "rightPanel.webEditEnable" })
      : (browserEditContext?.reason
          ?? intl.formatMessage({ id: "rightPanel.webEditUnavailable" }));
  const detachModeTitle = browserDetached
    ? intl.formatMessage({ id: "rightPanel.attachBrowser" })
    : intl.formatMessage({ id: "rightPanel.detachBrowser" });

  return (
    <aside
      className="flex h-full flex-shrink-0 flex-col border-l border-[var(--border-subtle)] bg-[var(--surface-panel)]"
      style={{ width: `${rightPanelWidth}px` }}
    >
      <div className="flex items-center gap-1 border-b border-[var(--border-subtle)] px-2 py-2">
        <button
          onClick={() => setRightPanelTab("browser")}
          className={`flex h-7 w-7 items-center justify-center rounded-[var(--radius-sm)] transition-colors ${
            rightPanelTab === "browser"
              ? "bg-[var(--accent-soft)] text-[var(--accent-strong)]"
              : "text-[var(--text-muted)] hover:bg-[var(--surface-elevated)] hover:text-[var(--text-strong)]"
          }`}
          title={browserTabLabel}
          aria-label={browserTabLabel}
        >
          <IconBrowser size={14} stroke={1.8} />
        </button>
        <button
          onClick={() => setRightPanelTab("project")}
          className={`flex h-7 w-7 items-center justify-center rounded-[var(--radius-sm)] transition-colors ${
            rightPanelTab === "project"
              ? "bg-[var(--accent-soft)] text-[var(--accent-strong)]"
              : "text-[var(--text-muted)] hover:bg-[var(--surface-elevated)] hover:text-[var(--text-strong)]"
          }`}
          title={projectTabLabel}
          aria-label={projectTabLabel}
        >
          <IconFolderOpen size={14} stroke={1.8} />
        </button>
        <button
          onClick={() => setRightPanelTab("terminal")}
          className={`flex h-7 w-7 items-center justify-center rounded-[var(--radius-sm)] transition-colors ${
            rightPanelTab === "terminal"
              ? "bg-[var(--accent-soft)] text-[var(--accent-strong)]"
              : "text-[var(--text-muted)] hover:bg-[var(--surface-elevated)] hover:text-[var(--text-strong)]"
          }`}
          title={terminalTabLabel}
          aria-label={terminalTabLabel}
        >
          <IconTerminal2 size={14} stroke={1.8} />
        </button>
        <button
          onClick={() => setRightPanelTab("git")}
          className={`flex h-7 w-7 items-center justify-center rounded-[var(--radius-sm)] transition-colors ${
            rightPanelTab === "git"
              ? "bg-[var(--accent-soft)] text-[var(--accent-strong)]"
              : "text-[var(--text-muted)] hover:bg-[var(--surface-elevated)] hover:text-[var(--text-strong)]"
          }`}
          title={gitTabLabel}
          aria-label={gitTabLabel}
        >
          <IconGitBranch size={14} stroke={1.8} />
        </button>
        <button
          onClick={() => setRightPanelTab("lan")}
          className={`flex h-7 w-7 items-center justify-center rounded-[var(--radius-sm)] transition-colors ${
            rightPanelTab === "lan"
              ? "bg-[var(--accent-soft)] text-[var(--accent-strong)]"
              : "text-[var(--text-muted)] hover:bg-[var(--surface-elevated)] hover:text-[var(--text-strong)]"
          }`}
          title={lanTabLabel}
          aria-label={lanTabLabel}
        >
          <IconNetwork size={14} stroke={1.8} />
        </button>
        <button
          onClick={() => setRightPanelTab("miniapp")}
          className={`flex h-7 w-7 items-center justify-center rounded-[var(--radius-sm)] transition-colors ${
            rightPanelTab === "miniapp"
              ? "bg-[var(--accent-soft)] text-[var(--accent-strong)]"
              : "text-[var(--text-muted)] hover:bg-[var(--surface-elevated)] hover:text-[var(--text-strong)]"
          }`}
          title={miniappTabLabel}
          aria-label={miniappTabLabel}
        >
          <IconApps size={14} stroke={1.8} />
        </button>
      </div>

      {rightPanelTab === "browser" && (
        <div className="flex flex-1 flex-col overflow-hidden">
          {/* Browser address bar */}
          <div className="flex flex-wrap items-center gap-2 border-b border-[var(--border-subtle)] px-3 py-1.5">
            <div className="flex min-w-0 flex-1 items-center gap-1">
              <button
                type="button"
                onClick={handleBrowserBack}
                disabled={!browserReady || !browserCanGoBack}
                className="flex h-6 w-6 flex-shrink-0 items-center justify-center rounded-[var(--radius-sm)] text-[var(--text-faint)] transition-colors hover:bg-[var(--surface-elevated)] hover:text-[var(--text-strong)] disabled:cursor-not-allowed disabled:opacity-35"
                title={intl.formatMessage({ id: "rightPanel.browserBack" })}
                aria-label={intl.formatMessage({ id: "rightPanel.browserBack" })}
              >
                <IconChevronLeft size={13} stroke={1.9} />
              </button>
              {!browserToolbarUltraCompact && (
                <button
                  type="button"
                  onClick={handleBrowserForward}
                  disabled={!browserReady || !browserCanGoForward}
                  className="flex h-6 w-6 flex-shrink-0 items-center justify-center rounded-[var(--radius-sm)] text-[var(--text-faint)] transition-colors hover:bg-[var(--surface-elevated)] hover:text-[var(--text-strong)] disabled:cursor-not-allowed disabled:opacity-35"
                  title={intl.formatMessage({ id: "rightPanel.browserForward" })}
                  aria-label={intl.formatMessage({ id: "rightPanel.browserForward" })}
                >
                  <IconChevronRight size={13} stroke={1.9} />
                </button>
              )}
              {!browserToolbarCompact && (
                <button
                  type="button"
                  onClick={handleBrowserRefresh}
                  disabled={!browserReady}
                  className="flex h-6 w-6 flex-shrink-0 items-center justify-center rounded-[var(--radius-sm)] text-[var(--text-faint)] transition-colors hover:bg-[var(--surface-elevated)] hover:text-[var(--text-strong)] disabled:cursor-not-allowed disabled:opacity-35"
                  title={intl.formatMessage({ id: "rightPanel.browserRefresh" })}
                  aria-label={intl.formatMessage({ id: "rightPanel.browserRefresh" })}
                >
                  <IconRefresh size={12} stroke={1.9} />
                </button>
              )}
              {!browserToolbarExtraCompact && (
                <button
                  type="button"
                  onClick={handleBrowserHome}
                  disabled={!browserReady}
                  className="flex h-6 w-6 flex-shrink-0 items-center justify-center rounded-[var(--radius-sm)] text-[var(--text-faint)] transition-colors hover:bg-[var(--surface-elevated)] hover:text-[var(--text-strong)] disabled:cursor-not-allowed disabled:opacity-35"
                  title={intl.formatMessage({ id: "rightPanel.browserHome" })}
                  aria-label={intl.formatMessage({ id: "rightPanel.browserHome" })}
                >
                  <IconHome size={12} stroke={1.9} />
                </button>
              )}
              <span
                className={`h-2 w-2 flex-shrink-0 rounded-full ${
                  browserActive
                    ? browserPanelStatus === "running"
                      ? "bg-[var(--warning)] animate-pulse"
                      : "bg-[var(--accent)]"
                    : "bg-[var(--text-faint)]"
                }`}
              />
              <input
                type="text"
                className="min-w-0 flex-1 rounded-[var(--radius-sm)] border border-[var(--border-subtle)] bg-[var(--surface-main)] px-2 py-0.5 font-mono text-[11px] text-[var(--text-muted)] outline-none transition-colors focus:border-[var(--accent)] focus:text-[var(--text-strong)]"
                value={addressInput}
                placeholder={intl.formatMessage({ id: "rightPanel.noActivePage" })}
                onFocus={() => {
                  addressFocusedRef.current = true;
                }}
                onBlur={() => {
                  addressFocusedRef.current = false;
                  setAddressInput(browserPanelUrl ?? browserOutput?.finalUrl ?? "");
                }}
                onChange={(event) => setAddressInput(event.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") {
                    handleNavigateSubmit(addressInput);
                  }
                }}
              />
            </div>
            <div className="ml-auto flex flex-shrink-0 items-center gap-1">
              <button
                type="button"
                onClick={handleToggleEditMode}
                disabled={!canToggleEditMode}
                className="flex h-6 w-6 flex-shrink-0 items-center justify-center rounded-[var(--radius-sm)] text-[var(--text-faint)] transition-colors hover:bg-[var(--surface-elevated)] hover:text-[var(--text-strong)] disabled:cursor-not-allowed disabled:opacity-40"
                title={editModeTitle}
                aria-label={editModeTitle}
              >
                <IconPencil size={13} stroke={1.8} />
              </button>
              {!browserToolbarExtraCompact && (
                <button
                  type="button"
                  onClick={handleInsertPickedToChat}
                  disabled={!pickedElement}
                  className="flex h-6 w-6 flex-shrink-0 items-center justify-center rounded-[var(--radius-sm)] text-[var(--text-faint)] transition-colors hover:bg-[var(--surface-elevated)] hover:text-[var(--text-strong)] disabled:cursor-not-allowed disabled:opacity-40"
                  title={intl.formatMessage({ id: "rightPanel.webEditAddToChat" })}
                  aria-label={intl.formatMessage({ id: "rightPanel.webEditAddToChat" })}
                >
                  <IconMessagePlus size={13} stroke={1.8} />
                </button>
              )}
              <button
                type="button"
                onClick={handleToggleDetachMode}
                className={`flex h-6 w-6 flex-shrink-0 items-center justify-center rounded-[var(--radius-sm)] transition-colors ${
                  browserDetached
                    ? "bg-[var(--accent-soft)] text-[var(--accent-strong)]"
                    : "text-[var(--text-faint)] hover:bg-[var(--surface-elevated)] hover:text-[var(--text-strong)]"
                }`}
                title={detachModeTitle}
                aria-label={detachModeTitle}
              >
                <IconExternalLink size={12} stroke={1.9} />
              </button>
              {(browserActive || browserDetached) && (
                <button
                  type="button"
                  onClick={handleCloseBrowser}
                  className="flex h-5 w-5 flex-shrink-0 items-center justify-center rounded-sm text-[var(--text-faint)] transition-colors hover:bg-[var(--surface-elevated)] hover:text-[var(--danger)]"
                  title={intl.formatMessage({ id: "rightPanel.closeBrowser" })}
                >
                  <IconX size={12} stroke={2} />
                </button>
              )}
            </div>
          </div>

          {(browserEditMode || pickedElement || browserEditError) && (
            <div className="space-y-1 border-b border-[var(--border-subtle)] bg-[var(--surface-main)] px-3 py-2">
              <div className="flex items-center gap-2 text-[10px] text-[var(--text-faint)]">
                <span
                  className={`inline-flex items-center rounded-full px-2 py-0.5 ${
                    browserEditMode
                      ? "bg-[var(--accent-soft)] text-[var(--accent-strong)]"
                      : "bg-[var(--surface-elevated)]"
                  }`}
                >
                  {browserEditMode
                    ? intl.formatMessage({ id: "rightPanel.webEditModeOn" })
                    : intl.formatMessage({ id: "rightPanel.webEditModeOff" })}
                </span>
                {browserEditContext?.sourcePath && (
                  <span className="min-w-0 truncate font-mono">{browserEditContext.sourcePath}</span>
                )}
              </div>
              {pickedElement && (
                <>
                  <div className="truncate font-mono text-[10px] text-[var(--text-muted)]">
                    {pickedElement.selector || pickedElement.selectorCandidates[0]}
                  </div>
                  <div className={`grid gap-1 ${browserEditorExtraCompact ? "grid-cols-1" : browserEditorCompact ? "grid-cols-[1fr_88px]" : "grid-cols-[1fr_88px_72px]"}`}>
                    <input
                      type="text"
                      value={editText}
                      onChange={(event) => setEditText(event.target.value)}
                      className="min-w-0 rounded-[var(--radius-sm)] border border-[var(--border-subtle)] bg-[var(--surface-panel)] px-2 py-1 text-[11px] text-[var(--text-base)] outline-none transition-colors focus:border-[var(--accent)]"
                      placeholder={intl.formatMessage({ id: "rightPanel.webEditTextPlaceholder" })}
                    />
                    <input
                      type="text"
                      value={editColor}
                      onChange={(event) => setEditColor(event.target.value)}
                      className="rounded-[var(--radius-sm)] border border-[var(--border-subtle)] bg-[var(--surface-panel)] px-2 py-1 font-mono text-[11px] text-[var(--text-base)] outline-none transition-colors focus:border-[var(--accent)]"
                      placeholder="#22c55e"
                    />
                    <input
                      type="text"
                      value={editFontSize}
                      onChange={(event) => setEditFontSize(event.target.value)}
                      className="rounded-[var(--radius-sm)] border border-[var(--border-subtle)] bg-[var(--surface-panel)] px-2 py-1 font-mono text-[11px] text-[var(--text-base)] outline-none transition-colors focus:border-[var(--accent)]"
                      placeholder="16px"
                    />
                  </div>
                  <button
                    type="button"
                    onClick={handleApplyDomEdit}
                    disabled={applyingEdit}
                    className="inline-flex h-6 items-center gap-1 rounded-[var(--radius-sm)] border border-[var(--accent-border)] bg-[var(--accent-soft)] px-2 text-[11px] text-[var(--accent-strong)] transition-colors hover:bg-[var(--surface-elevated)] disabled:cursor-not-allowed disabled:opacity-60"
                    title={intl.formatMessage({ id: "rightPanel.webEditApplyPreview" })}
                  >
                    <IconCheck size={11} stroke={1.8} />
                    {intl.formatMessage({ id: "rightPanel.webEditApplyPreview" })}
                  </button>
                </>
              )}
              {browserEditError && (
                <p className="text-[11px] text-[var(--danger)]">
                  {browserEditError}
                </p>
              )}
            </div>
          )}

          {/* Browser webview container - this div's bounds control the embedded webview position */}
          <div
            ref={browserContainerRef}
            className="relative flex-1"
          >
            {browserDetached ? (
              <div className="absolute inset-0 flex flex-col items-center justify-center gap-3 p-4">
                <IconExternalLink size={30} stroke={1.5} className="text-[var(--text-faint)]" />
                <p className="max-w-[260px] text-center text-xs text-[var(--text-muted)]">
                  {intl.formatMessage({ id: "rightPanel.detachedHint" })}
                </p>
                <button
                  type="button"
                  onClick={() => void attachPopupBackToPanel()}
                  className="inline-flex h-7 items-center rounded-[var(--radius-sm)] border border-[var(--accent-border)] bg-[var(--accent-soft)] px-3 text-xs text-[var(--accent-strong)] transition-colors hover:bg-[var(--surface-elevated)]"
                >
                  {intl.formatMessage({ id: "rightPanel.returnToPanel" })}
                </button>
              </div>
            ) : !browserActive && (
              <div className="absolute inset-0 flex flex-col items-center justify-center gap-3 p-4">
                <IconBrowser size={32} stroke={1.2} className="text-[var(--text-faint)]" />
                <p className="text-center text-xs text-[var(--text-muted)]">
                  {intl.formatMessage({ id: "rightPanel.loadingBrowser" })}
                </p>

                {screenshots.length > 0 && (
                  <div className="mt-3 w-full space-y-2">
                    {screenshots.map((shot) => {
                      const src = localPreviewSrc(shot, workspaceCwd);
                      return (
                        <div key={shot} className="rounded-[var(--radius-sm)] border border-[var(--border-subtle)] bg-[var(--surface-main)] p-2">
                          <div className="mb-1 flex items-center gap-1 text-[10px] text-[var(--text-faint)]">
                            <IconExternalLink size={10} stroke={1.8} />
                            <span className="min-w-0 truncate font-mono">{shot.split(/[\\/]/).pop()}</span>
                          </div>
                          {src ? (
                            <img
                              src={src}
                              alt={shot}
                              className="max-h-[160px] w-full rounded-[var(--radius-sm)] border border-[var(--border-subtle)] object-contain"
                            />
                          ) : null}
                        </div>
                      );
                    })}
                  </div>
                )}
              </div>
            )}
          </div>
        </div>
      )}

      {/* 次级面板始终挂载：切换到浏览器 tab 或关闭右侧时只隐藏，终端进程继续运行 */}
      <div
        className={
          rightPanelTab === "browser"
            ? "hidden"
            : "flex min-h-0 flex-1 flex-col overflow-hidden"
        }
        aria-hidden={rightPanelTab === "browser"}
      >
        <RightPanelSecondaryTab tab={rightPanelTab} workspaceCwd={workspaceCwd} />
      </div>
    </aside>
  );
}

const RightPanelSecondaryTab = memo(function RightPanelSecondaryTab({
  tab,
  workspaceCwd,
}: {
  tab: "project" | "terminal" | "git" | "browser" | "lan" | "miniapp";
  workspaceCwd: string | null;
}) {
  // 终端打开后保持挂载：切换到项目/Git 时只隐藏，避免 PTY 被卸载关闭。
  const [terminalMounted, setTerminalMounted] = useState(tab === "terminal");

  useEffect(() => {
    if (tab === "terminal") {
      setTerminalMounted(true);
    }
  }, [tab]);

  return (
    <>
      {tab === "git" ? (
        <GitPanel workspaceCwd={workspaceCwd} />
      ) : tab === "lan" ? (
        <LanCollabPanel />
      ) : tab === "miniapp" ? (
        <MiniAppSidePanel />
      ) : tab === "project" || tab === "browser" ? (
        <ProjectTab workspaceCwd={workspaceCwd} />
      ) : null}

      {terminalMounted && (
        <div
          className={
            tab === "terminal"
              ? "flex min-h-0 flex-1 flex-col overflow-hidden"
              : "hidden"
          }
          aria-hidden={tab !== "terminal"}
        >
          <TerminalPanel />
        </div>
      )}
    </>
  );
});

const ProjectTab = memo(function ProjectTab({ workspaceCwd }: { workspaceCwd: string | null }) {
  const intl = useIntl();
  const [refreshKey, setRefreshKey] = useState(0);

  const projectName = workspaceCwd
    ? workspaceCwd.split(/[\\/]/).filter(Boolean).pop() ?? workspaceCwd
    : null;

  return (
    <div className="flex flex-1 flex-col overflow-hidden">
      <div className="flex items-center gap-2 border-b border-[var(--border-subtle)] px-3 py-2">
        <span className="min-w-0 flex-1 truncate text-xs font-semibold text-[var(--text-strong)]" title={workspaceCwd ?? undefined}>
          {projectName ?? intl.formatMessage({ id: "fileTree.noProject" })}
        </span>
        {workspaceCwd && (
          <div className="flex items-center gap-1">
            <button
              type="button"
              onClick={() => setRefreshKey((k) => k + 1)}
              className="flex h-6 w-6 items-center justify-center rounded-[var(--radius-sm)] text-[var(--text-faint)] transition-colors hover:bg-[var(--surface-elevated)] hover:text-[var(--text-strong)]"
              title={intl.formatMessage({ id: "fileTree.refresh" })}
            >
              <IconRefresh size={14} stroke={1.8} />
            </button>
            <button
              type="button"
              onClick={() => void revealInExplorer(workspaceCwd)}
              className="flex h-6 w-6 items-center justify-center rounded-[var(--radius-sm)] text-[var(--text-faint)] transition-colors hover:bg-[var(--surface-elevated)] hover:text-[var(--text-strong)]"
              title={intl.formatMessage({ id: "fileTree.openInExplorer" })}
            >
              <IconFolderOpen size={14} stroke={1.8} />
            </button>
          </div>
        )}
      </div>
      <FileTree rootPath={workspaceCwd} refreshKey={refreshKey} />
    </div>
  );
});
