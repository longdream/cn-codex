import { IconBrowser, IconExternalLink, IconFolderOpen } from "@tabler/icons-react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { openPath } from "@tauri-apps/plugin-opener";
import { useMemo } from "react";
import { useAppStore } from "../../stores/appStore";

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
  // 兼容 Windows 扩展路径前缀（\\?\），否则 convertFileSrc 会生成不可访问 URL。
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
  const browserPanelTitle = useAppStore((s) => s.browserPanelTitle);
  const browserPanelStatus = useAppStore((s) => s.browserPanelStatus);
  const workspaceCwd = useAppStore((s) => s.workspaceCwd);
  const messages = useAppStore((s) => s.messages);
  const setRightPanelTab = useAppStore((s) => s.setRightPanelTab);

  const browserCall = useMemo(() => latestBrowserToolCall(messages), [messages]);
  const browserOutput = useMemo(
    () => parseBrowserRunOutput(browserCall?.output),
    [browserCall?.output],
  );

  const screenshots = browserOutput?.screenshots ?? [];

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
        <div className="thin-scrollbar flex-1 overflow-y-auto p-3">
          <div className="rounded-[var(--radius-md)] border border-[var(--border-subtle)] bg-[var(--surface-main)] p-3">
            <div className="flex items-center justify-between gap-2">
              <p className="text-xs font-semibold text-[var(--text-strong)]">WebView JS Injection</p>
              <span
                className={`rounded-[var(--radius-sm)] px-1.5 py-0.5 text-[11px] ${
                  browserPanelStatus === "running"
                    ? "bg-[rgba(245,158,11,0.12)] text-[var(--warning)]"
                    : browserPanelStatus === "success"
                      ? "bg-[var(--accent-soft)] text-[var(--accent-strong)]"
                      : browserPanelStatus === "failed"
                        ? "bg-[var(--danger-soft)] text-[var(--danger)]"
                        : "bg-[var(--surface-elevated)] text-[var(--text-faint)]"
                }`}
              >
                {browserPanelStatus}
              </span>
            </div>
            <p className="mt-2 break-all font-mono text-[11px] text-[var(--text-muted)]">
              {browserPanelUrl ?? browserOutput?.finalUrl ?? "No active browser run"}
            </p>
            {browserPanelTitle || browserOutput?.title ? (
              <p className="mt-2 text-xs text-[var(--text-base)]">
                {browserPanelTitle ?? browserOutput?.title}
              </p>
            ) : null}
            {browserOutput?.browserMode ? (
              <p className="mt-2 text-[11px] text-[var(--text-faint)]">
                Mode: {browserOutput.browserMode}
              </p>
            ) : null}
            {browserPanelStatus === "failed" && screenshots.length === 0 ? (
              <div className="mt-3 rounded-[var(--radius-sm)] border border-[var(--danger-soft)] bg-[var(--danger-soft)] px-2.5 py-2">
                <p className="text-[11px] font-medium text-[var(--danger)]">
                  {browserOutput?.message ?? "Browser run 失败，未生成可预览截图。"}
                </p>
                {browserOutput?.hint ? (
                  <p className="mt-1 text-[11px] text-[var(--text-muted)]">{browserOutput.hint}</p>
                ) : (
                  <p className="mt-1 text-[11px] text-[var(--text-muted)]">
                    请检查工具输出中的错误详情，并确认内置浏览器窗口与 CDP 通道可用。
                  </p>
                )}
                {browserOutput?.errorCode ? (
                  <p className="mt-1 font-mono text-[10px] text-[var(--text-faint)]">
                    {browserOutput.errorCode}
                  </p>
                ) : null}
              </div>
            ) : null}
          </div>

          {screenshots.length > 0 ? (
            <div className="mt-3 space-y-3">
              {screenshots.map((shot) => {
                const src = localPreviewSrc(shot, workspaceCwd);
                return (
                  <div key={shot} className="rounded-[var(--radius-md)] border border-[var(--border-subtle)] bg-[var(--surface-main)] p-2">
                    <div className="mb-2 flex items-center gap-1.5 text-[11px] text-[var(--text-muted)]">
                      <IconExternalLink size={12} stroke={1.8} />
                      <span className="min-w-0 break-all font-mono">{shot}</span>
                    </div>
                    {src ? (
                      <img
                        src={src}
                        alt={shot}
                        className="max-h-[240px] w-full rounded-[var(--radius-sm)] border border-[var(--border-subtle)] object-contain"
                      />
                    ) : null}
                  </div>
                );
              })}
            </div>
          ) : null}
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
                  void openPath(workspaceCwd);
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
