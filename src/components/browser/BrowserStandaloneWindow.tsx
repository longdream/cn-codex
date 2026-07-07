import {
  IconBrowser,
  IconChevronLeft,
  IconChevronRight,
  IconHome,
  IconRefresh,
} from "@tabler/icons-react";
import { listen } from "@tauri-apps/api/event";
import { useCallback, useEffect, useRef, useState } from "react";
import { useIntl } from "react-intl";
import {
  browserGetNavigationState,
  browserGoBack,
  browserGoForward,
  browserNavigateHome,
  browserRefreshPreview,
  windowCloseBrowser,
  windowMinimize,
  windowNavigateBrowser,
  windowResizeBrowser,
  type BrowserNavigationState,
} from "../../api/window";

interface BrowserNavigationPayload {
  url?: string;
  title?: string;
  canGoBack?: boolean;
  canGoForward?: boolean;
}

function normalizeNavigationState(payload: BrowserNavigationPayload): BrowserNavigationState {
  const rawUrl = typeof payload.url === "string" ? payload.url.trim() : "";
  return {
    url: rawUrl || "about:blank",
    title: typeof payload.title === "string" ? payload.title : "",
    canGoBack: Boolean(payload.canGoBack),
    canGoForward: Boolean(payload.canGoForward),
  };
}

function runWindowAction(action: () => Promise<void>, label: string): void {
  const onError = (err: unknown) => {
    console.error(`Browser standalone window ${label} failed:`, err);
  };
  try {
    void action().catch(onError);
  } catch (err) {
    onError(err);
  }
}

export function BrowserStandaloneWindow() {
  const intl = useIntl();
  const browserContainerRef = useRef<HTMLDivElement>(null);
  const resizeTimerRef = useRef<number | null>(null);
  const addressFocusedRef = useRef(false);
  const [ready, setReady] = useState(false);
  const [errorText, setErrorText] = useState<string | null>(null);
  const [addressInput, setAddressInput] = useState("");
  const [navigation, setNavigation] = useState<BrowserNavigationState>({
    url: "about:blank",
    title: "",
    canGoBack: false,
    canGoForward: false,
  });

  const applyNavigation = useCallback((next: BrowserNavigationState) => {
    setNavigation(next);
    setReady(true);
    setErrorText(null);
    if (!addressFocusedRef.current) {
      setAddressInput(next.url);
    }
  }, []);

  const syncNavigationState = useCallback(() => {
    void browserGetNavigationState()
      .then((next) => {
        applyNavigation(next);
      })
      .catch((err) => {
        setErrorText(String(err));
      });
  }, [applyNavigation]);

  const syncBrowserPosition = useCallback(() => {
    const el = browserContainerRef.current;
    if (!el) {
      return;
    }
    const rect = el.getBoundingClientRect();
    if (rect.width <= 0 || rect.height <= 0) {
      return;
    }
    void windowResizeBrowser(
      Math.round(rect.x),
      Math.round(rect.y),
      Math.round(rect.width),
      Math.round(rect.height),
    ).catch(() => undefined);
  }, []);

  const handleNavigateSubmit = useCallback((rawUrl: string) => {
    const url = rawUrl.trim();
    if (!url) {
      return;
    }
    setAddressInput(url);
    void windowNavigateBrowser(url)
      .then(() => {
        syncNavigationState();
      })
      .catch((err) => {
        setErrorText(String(err));
      });
  }, [syncNavigationState]);

  const handleBack = useCallback(() => {
    void browserGoBack()
      .then((next) => {
        applyNavigation(next);
      })
      .catch((err) => {
        setErrorText(String(err));
      });
  }, [applyNavigation]);

  const handleForward = useCallback(() => {
    void browserGoForward()
      .then((next) => {
        applyNavigation(next);
      })
      .catch((err) => {
        setErrorText(String(err));
      });
  }, [applyNavigation]);

  const handleRefresh = useCallback(() => {
    void browserRefreshPreview()
      .then(() => {
        syncNavigationState();
      })
      .catch((err) => {
        setErrorText(String(err));
      });
  }, [syncNavigationState]);

  const handleHome = useCallback(() => {
    void browserNavigateHome()
      .then((next) => {
        applyNavigation(next);
      })
      .catch((err) => {
        setErrorText(String(err));
      });
  }, [applyNavigation]);

  useEffect(() => {
    let cancelled = false;
    const boot = async () => {
      for (let i = 0; i < 16; i += 1) {
        try {
          const state = await browserGetNavigationState();
          if (cancelled) {
            return;
          }
          applyNavigation(state);
          syncBrowserPosition();
          return;
        } catch {
          await new Promise((resolve) => {
            window.setTimeout(resolve, i < 6 ? 120 : 280);
          });
        }
      }
      if (!cancelled) {
        setErrorText(intl.formatMessage({ id: "rightPanel.noActivePage" }));
      }
    };
    void boot();
    return () => {
      cancelled = true;
    };
  }, [applyNavigation, intl, syncBrowserPosition]);

  useEffect(() => {
    const timer = window.setInterval(() => {
      syncNavigationState();
    }, 1200);
    return () => {
      window.clearInterval(timer);
    };
  }, [syncNavigationState]);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | null = null;
    void listen<BrowserNavigationPayload>("browser-navigation-changed", (event) => {
      applyNavigation(normalizeNavigationState(event.payload));
    }).then((fn) => {
      if (disposed) {
        fn();
        return;
      }
      unlisten = fn;
    }).catch(() => undefined);
    return () => {
      disposed = true;
      if (unlisten) {
        unlisten();
      }
    };
  }, [applyNavigation]);

  useEffect(() => {
    const el = browserContainerRef.current;
    if (!el) return;
    const observer = new ResizeObserver(() => {
      if (resizeTimerRef.current) {
        cancelAnimationFrame(resizeTimerRef.current);
      }
      resizeTimerRef.current = requestAnimationFrame(syncBrowserPosition);
    });
    observer.observe(el);
    syncBrowserPosition();
    const onWindowResize = () => {
      if (resizeTimerRef.current) {
        cancelAnimationFrame(resizeTimerRef.current);
      }
      resizeTimerRef.current = requestAnimationFrame(syncBrowserPosition);
    };
    window.addEventListener("resize", onWindowResize);
    return () => {
      observer.disconnect();
      window.removeEventListener("resize", onWindowResize);
      if (resizeTimerRef.current) {
        cancelAnimationFrame(resizeTimerRef.current);
      }
    };
  }, [syncBrowserPosition]);

  return (
    <div className="browser-standalone-window flex h-screen flex-col bg-[var(--surface-panel)]">
      <div className="browser-standalone-toolbar flex h-[38px] items-center gap-1 border-b border-[var(--border-subtle)] px-2">
        <div
          data-tauri-drag-region
          className="browser-standalone-window-drag flex h-6 min-w-[120px] max-w-[180px] items-center gap-1 rounded-[var(--radius-sm)] px-1.5"
          title={navigation.title || intl.formatMessage({ id: "rightPanel.browser" })}
        >
          <IconBrowser size={13} stroke={1.9} className="text-[var(--accent)]" />
          <span className="truncate text-[11px] text-[var(--text-muted)]">
            {navigation.title || intl.formatMessage({ id: "rightPanel.browserPopupTitle" })}
          </span>
        </div>
        <button
          type="button"
          onClick={handleBack}
          disabled={!navigation.canGoBack}
          className="flex h-6 w-6 items-center justify-center rounded-[var(--radius-sm)] text-[var(--text-faint)] transition-colors hover:bg-[var(--surface-elevated)] hover:text-[var(--text-strong)] disabled:cursor-not-allowed disabled:opacity-35"
          title={intl.formatMessage({ id: "rightPanel.browserBack" })}
        >
          <IconChevronLeft size={13} stroke={1.9} />
        </button>
        <button
          type="button"
          onClick={handleForward}
          disabled={!navigation.canGoForward}
          className="flex h-6 w-6 items-center justify-center rounded-[var(--radius-sm)] text-[var(--text-faint)] transition-colors hover:bg-[var(--surface-elevated)] hover:text-[var(--text-strong)] disabled:cursor-not-allowed disabled:opacity-35"
          title={intl.formatMessage({ id: "rightPanel.browserForward" })}
        >
          <IconChevronRight size={13} stroke={1.9} />
        </button>
        <button
          type="button"
          onClick={handleRefresh}
          className="flex h-6 w-6 items-center justify-center rounded-[var(--radius-sm)] text-[var(--text-faint)] transition-colors hover:bg-[var(--surface-elevated)] hover:text-[var(--text-strong)]"
          title={intl.formatMessage({ id: "rightPanel.browserRefresh" })}
        >
          <IconRefresh size={12} stroke={1.9} />
        </button>
        <button
          type="button"
          onClick={handleHome}
          className="flex h-6 w-6 items-center justify-center rounded-[var(--radius-sm)] text-[var(--text-faint)] transition-colors hover:bg-[var(--surface-elevated)] hover:text-[var(--text-strong)]"
          title={intl.formatMessage({ id: "rightPanel.browserHome" })}
        >
          <IconHome size={12} stroke={1.9} />
        </button>
        <input
          type="text"
          value={addressInput}
          placeholder={intl.formatMessage({ id: "rightPanel.noActivePage" })}
          className="min-w-0 flex-1 rounded-[var(--radius-sm)] border border-[var(--border-subtle)] bg-[var(--surface-main)] px-2 py-0.5 font-mono text-[11px] text-[var(--text-muted)] outline-none transition-colors focus:border-[var(--accent)] focus:text-[var(--text-strong)]"
          onFocus={() => {
            addressFocusedRef.current = true;
          }}
          onBlur={() => {
            addressFocusedRef.current = false;
            setAddressInput(navigation.url);
          }}
          onChange={(event) => setAddressInput(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter") {
              handleNavigateSubmit(addressInput);
            }
          }}
        />
        <button
          type="button"
          onClick={() => runWindowAction(windowMinimize, "minimize")}
          className="flex h-6 w-6 items-center justify-center rounded-[var(--radius-sm)] text-[var(--text-muted)] transition-colors hover:bg-[var(--surface-elevated)]"
          title={intl.formatMessage({ id: "titleBar.minimize" })}
        >
          <svg width="9" height="1" viewBox="0 0 9 1" fill="currentColor">
            <rect width="9" height="1" />
          </svg>
        </button>
        <button
          type="button"
          onClick={() => runWindowAction(windowCloseBrowser, "close browser popup")}
          className="flex h-6 w-6 items-center justify-center rounded-[var(--radius-sm)] text-[var(--text-muted)] transition-colors hover:bg-[#e81123] hover:text-white"
          title={intl.formatMessage({ id: "titleBar.close" })}
        >
          <svg width="9" height="9" viewBox="0 0 9 9" fill="none" stroke="currentColor" strokeWidth="1.2">
            <line x1="0" y1="0" x2="9" y2="9" />
            <line x1="9" y1="0" x2="0" y2="9" />
          </svg>
        </button>
      </div>

      {errorText && (
        <div className="border-b border-[var(--border-subtle)] bg-[var(--surface-main)] px-3 py-1.5 text-[11px] text-[var(--danger)]">
          {errorText}
        </div>
      )}

      <div ref={browserContainerRef} className="relative flex-1">
        {!ready && (
          <div className="absolute inset-0 flex items-center justify-center text-[11px] text-[var(--text-muted)]">
            {intl.formatMessage({ id: "rightPanel.loadingBrowser" })}
          </div>
        )}
      </div>
    </div>
  );
}
