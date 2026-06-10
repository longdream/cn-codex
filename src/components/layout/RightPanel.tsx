import { IconBrowser, IconExternalLink, IconFolderOpen, IconX } from "@tabler/icons-react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useAppStore } from "../../stores/appStore";
import { revealInExplorer, windowCloseBrowser, windowOpenBrowser, windowResizeBrowser } from "../../api/window";

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

export function RightPanel() {
  const rightPanelTab = useAppStore((s) => s.rightPanelTab);
  const browserPanelUrl = useAppStore((s) => s.browserPanelUrl);
  const browserPanelStatus = useAppStore((s) => s.browserPanelStatus);
  const workspaceCwd = useAppStore((s) => s.workspaceCwd);
  const messages = useAppStore((s) => s.messages);
  const setRightPanelTab = useAppStore((s) => s.setRightPanelTab);

  const [browserActive, setBrowserActive] = useState(false);
  const browserContainerRef = useRef<HTMLDivElement>(null);
  const resizeTimerRef = useRef<number | null>(null);

  const browserCall = useMemo(() => latestBrowserToolCall(messages), [messages]);
  const browserOutput = useMemo(
    () => parseBrowserRunOutput(browserCall?.output),
    [browserCall?.output],
  );

  const screenshots = browserOutput?.screenshots ?? [];

  const syncBrowserPosition = useCallback(() => {
    const el = browserContainerRef.current;
    if (!el || !browserActive) return;
    const rect = el.getBoundingClientRect();
    if (rect.width > 0 && rect.height > 0) {
      void windowResizeBrowser(
        Math.round(rect.x),
        Math.round(rect.y),
        Math.round(rect.width),
        Math.round(rect.height),
      );
    }
  }, [browserActive]);

  useEffect(() => {
    if (!browserActive || rightPanelTab !== "browser") return;
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
  }, [browserActive, rightPanelTab, syncBrowserPosition]);

  const handleOpenBrowser = useCallback((url?: string) => {
    const el = browserContainerRef.current;
    if (!el) return;
    const rect = el.getBoundingClientRect();
    void windowOpenBrowser(url, {
      x: Math.round(rect.x),
      y: Math.round(rect.y),
      width: Math.max(Math.round(rect.width), 200),
      height: Math.max(Math.round(rect.height), 200),
    }).then(() => {
      setBrowserActive(true);
      setTimeout(syncBrowserPosition, 100);
      requestAnimationFrame(syncBrowserPosition);
    });
  }, [syncBrowserPosition]);

  const handleCloseBrowser = useCallback(() => {
    void windowCloseBrowser();
    setBrowserActive(false);
  }, []);

  // 切换 tab 时隐藏/恢复 webview 位置（不关闭）
  useEffect(() => {
    if (browserActive && rightPanelTab !== "browser") {
      void windowResizeBrowser(-9999, -9999, 0, 0);
    } else if (browserActive && rightPanelTab === "browser") {
      syncBrowserPosition();
    }
  }, [rightPanelTab, browserActive, syncBrowserPosition]);

  // 当切到 browser tab 时自动激活 webview
  useEffect(() => {
    if (rightPanelTab === "browser" && !browserActive) {
      handleOpenBrowser();
    }
  }, [rightPanelTab, browserActive, handleOpenBrowser]);

  return (
    <aside className="flex h-full w-[24rem] flex-shrink-0 flex-col border-l border-[var(--border-subtle)] bg-[var(--surface-panel)]">
      <div className="flex items-center gap-1 border-b border-[var(--border-subtle)] px-2 py-2">
        <button
          onClick={() => setRightPanelTab("browser")}
          className={`flex items-center gap-1.5 rounded-[var(--radius-sm)] px-2 py-1 text-xs transition-colors ${
            rightPanelTab === "browser"
              ? "bg-[var(--accent-soft)] text-[var(--accent-strong)]"
              : "text-[var(--text-muted)] hover:bg-[var(--surface-elevated)] hover:text-[var(--text-strong)]"
          }`}
        >
          <IconBrowser size={14} stroke={1.8} />
          Browser
        </button>
        <button
          onClick={() => setRightPanelTab("project")}
          className={`flex items-center gap-1.5 rounded-[var(--radius-sm)] px-2 py-1 text-xs transition-colors ${
            rightPanelTab === "project"
              ? "bg-[var(--accent-soft)] text-[var(--accent-strong)]"
              : "text-[var(--text-muted)] hover:bg-[var(--surface-elevated)] hover:text-[var(--text-strong)]"
          }`}
        >
          <IconFolderOpen size={14} stroke={1.8} />
          Project
        </button>
      </div>

      {rightPanelTab === "browser" ? (
        <div className="flex flex-1 flex-col overflow-hidden">
          {/* Browser info bar */}
          <div className="flex items-center gap-2 border-b border-[var(--border-subtle)] px-3 py-2">
            <div className="flex min-w-0 flex-1 items-center gap-1.5">
              <span
                className={`h-2 w-2 flex-shrink-0 rounded-full ${
                  browserActive
                    ? browserPanelStatus === "running"
                      ? "bg-[var(--warning)] animate-pulse"
                      : "bg-[var(--accent)]"
                    : "bg-[var(--text-faint)]"
                }`}
              />
              <span className="truncate font-mono text-[11px] text-[var(--text-muted)]">
                {browserPanelUrl ?? browserOutput?.finalUrl ?? "No active page"}
              </span>
            </div>
            {browserActive && (
              <button
                type="button"
                onClick={handleCloseBrowser}
                className="flex h-5 w-5 flex-shrink-0 items-center justify-center rounded-sm text-[var(--text-faint)] transition-colors hover:bg-[var(--surface-elevated)] hover:text-[var(--danger)]"
                title="Close browser"
              >
                <IconX size={12} stroke={2} />
              </button>
            )}
          </div>

          {/* Browser webview container - this div's bounds control the embedded webview position */}
          <div
            ref={browserContainerRef}
            className="relative flex-1"
          >
            {!browserActive && (
              <div className="absolute inset-0 flex flex-col items-center justify-center gap-3 p-4">
                <IconBrowser size={32} stroke={1.2} className="text-[var(--text-faint)]" />
                <p className="text-center text-xs text-[var(--text-muted)]">
                  Loading browser...
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
      ) : (
        <div className="flex flex-1 flex-col p-3">
          <div className="rounded-[var(--radius-md)] border border-[var(--border-subtle)] bg-[var(--surface-main)] p-3">
            <p className="text-xs font-semibold text-[var(--text-strong)]">Current project</p>
            <p className="mt-2 break-all font-mono text-[11px] text-[var(--text-muted)]">
              {workspaceCwd ?? "No project selected"}
            </p>
            <button
              onClick={() => {
                if (workspaceCwd) {
                  void revealInExplorer(workspaceCwd);
                }
              }}
              disabled={!workspaceCwd}
              className="mt-3 flex items-center gap-1.5 rounded-[var(--radius-sm)] border border-[var(--border-subtle)] bg-[var(--surface-elevated)] px-3 py-1.5 text-xs text-[var(--text-muted)] transition-colors hover:text-[var(--text-strong)] disabled:opacity-40"
            >
              <IconFolderOpen size={14} stroke={1.8} />
              Open in Explorer
            </button>
          </div>
        </div>
      )}
    </aside>
  );
}
