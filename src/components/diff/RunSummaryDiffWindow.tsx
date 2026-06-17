import {
  IconArrowBackUp,
  IconCheck,
  IconDeviceFloppy,
  IconFileDiff,
  IconX,
} from "@tabler/icons-react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { useCallback, useEffect, useMemo, useState } from "react";
import ReactMarkdown from "react-markdown";
import rehypeHighlight from "rehype-highlight";
import remarkGfm from "remark-gfm";
import {
  windowCloseRunSummaryDiff,
  windowGetRunSummaryDiffPayload,
  windowMinimize,
  writeTextFilePreview,
  type RunSummaryDiffPayload,
} from "../../api/window";
import { buildPatchLineDiff, type PatchDiffLine } from "../../utils/lineDiff";

type NoticeKind = "success" | "error" | "info";
type PersistStatus = "idle" | "keeping" | "restoring";

function runWindowAction(action: () => Promise<void>, label: string): void {
  const onError = (err: unknown) => {
    console.error(`RunSummary diff window ${label} failed:`, err);
  };
  try {
    void action().catch(onError);
  } catch (err) {
    onError(err);
  }
}

function linePrefix(type: PatchDiffLine["type"]): string {
  if (type === "add") return "+";
  if (type === "remove") return "-";
  return " ";
}

function fileLanguage(path: string): string {
  const ext = path.split(".").pop()?.toLowerCase() ?? "";
  switch (ext) {
    case "ts":
    case "tsx":
      return "typescript";
    case "js":
    case "jsx":
    case "mjs":
    case "cjs":
      return "javascript";
    case "css":
    case "scss":
    case "less":
      return "css";
    case "html":
    case "htm":
      return "html";
    case "json":
    case "jsonc":
      return "json";
    case "md":
    case "mdx":
      return "markdown";
    case "rs":
      return "rust";
    case "py":
      return "python";
    case "go":
      return "go";
    case "toml":
      return "toml";
    case "yaml":
    case "yml":
      return "yaml";
    case "sh":
    case "bash":
    case "zsh":
      return "bash";
    default:
      return ext || "text";
  }
}

function diffSourceLabel(source: RunSummaryDiffPayload["diffSource"]): string {
  switch (source) {
    case "snapshot":
      return "本轮快照";
    case "patch":
      return "补丁回退";
    default:
      return "无数据";
  }
}

function pathBasename(path: string): string {
  return path.split(/[\\/]/).filter(Boolean).pop() ?? path;
}

function lineCodeMarkdown(text: string, language: string): string {
  // 使用 ~~~ 代码围栏渲染单行高亮，避免行内容中 ``` 破坏 markdown 结构。
  const safeLine = (text || " ").replace(/~~~/g, "\\~\\~\\~");
  return `~~~${language}\n${safeLine}\n~~~`;
}

function DiffLineCode({ text, language }: { text: string; language: string }) {
  const markdown = useMemo(
    () => lineCodeMarkdown(text, language),
    [text, language],
  );
  return (
    <ReactMarkdown
      remarkPlugins={[remarkGfm]}
      rehypePlugins={[rehypeHighlight]}
      components={{
        pre(props) {
          return (
            <pre className="m-0 overflow-visible bg-transparent p-0">
              {props.children}
            </pre>
          );
        },
        code(props) {
          const { className, children } = props;
          return (
            <code className={`hljs ${className ?? ""}`.trim()}>
              {children}
            </code>
          );
        },
      }}
    >
      {markdown}
    </ReactMarkdown>
  );
}

export function RunSummaryDiffWindow() {
  const [payload, setPayload] = useState<RunSummaryDiffPayload | null>(null);
  const [persistStatus, setPersistStatus] = useState<PersistStatus>("idle");
  const [notice, setNotice] = useState<{ kind: NoticeKind; text: string } | null>(null);

  useEffect(() => {
    let cancelled = false;
    void windowGetRunSummaryDiffPayload()
      .then((result) => {
        if (cancelled) {
          return;
        }
        setPayload(result);
      })
      .catch((err) => {
        if (cancelled) {
          return;
        }
        setNotice({
          kind: "error",
          text: `读取 Diff 初始数据失败：${String(err)}`,
        });
      });
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    let unlisten: UnlistenFn | null = null;
    void listen<RunSummaryDiffPayload>("runsummary-diff-open", (event) => {
      setPayload(event.payload ?? null);
      setPersistStatus("idle");
      setNotice(null);
    }).then((fn) => {
      unlisten = fn;
    });
    return () => {
      if (unlisten) {
        void unlisten();
      }
    };
  }, []);

  const lines = useMemo(() => (
    payload
      ? buildPatchLineDiff(payload.beforeContent, payload.afterContent)
      : []
  ), [payload]);

  const languageLabel = useMemo(
    () => fileLanguage(payload?.path ?? ""),
    [payload?.path],
  );
  const action = (payload?.fileAction ?? "modified").toLowerCase();
  const source = payload?.diffSource ?? "empty";
  const canWriteDisk = Boolean(
    payload
    && payload.canPersist
    && source === "snapshot"
    && action === "modified"
    && payload.path.trim().length > 0,
  );

  const persistBlockedReason = useMemo(() => {
    if (canWriteDisk) {
      return null;
    }
    if (!payload) {
      return "等待主窗口传入 Diff 数据。";
    }
    if (payload.persistHint?.trim()) {
      return payload.persistHint.trim();
    }
    if (action !== "modified") {
      return "当前仅 modified 文件支持 Keep/Restore。";
    }
    if (source !== "snapshot") {
      return "仅快照模式可写盘。";
    }
    return "当前数据不可写盘。";
  }, [action, canWriteDisk, payload, source]);

  const handleKeep = useCallback(async () => {
    if (!payload || !canWriteDisk || persistStatus !== "idle") {
      return;
    }
    setPersistStatus("keeping");
    setNotice(null);
    try {
      const result = await writeTextFilePreview(payload.path, payload.afterContent);
      setNotice({ kind: "success", text: `Keep 成功，已写入 ${result.path} (${result.size} bytes)` });
    } catch (err) {
      setNotice({ kind: "error", text: `Keep 失败：${String(err)}` });
    } finally {
      setPersistStatus("idle");
    }
  }, [canWriteDisk, payload, persistStatus]);

  const handleRestore = useCallback(async () => {
    if (!payload || !canWriteDisk || persistStatus !== "idle") {
      return;
    }
    setPersistStatus("restoring");
    setNotice(null);
    try {
      const result = await writeTextFilePreview(payload.path, payload.beforeContent);
      setNotice({ kind: "success", text: `Restore 成功，已还原 ${result.path} (${result.size} bytes)` });
    } catch (err) {
      setNotice({ kind: "error", text: `Restore 失败：${String(err)}` });
    } finally {
      setPersistStatus("idle");
    }
  }, [canWriteDisk, payload, persistStatus]);

  return (
    <div className="flex h-dvh w-screen flex-col bg-[var(--surface-panel)] text-[var(--text-base)]">
      <div className="flex h-9 items-center border-b border-[var(--border-subtle)] bg-[var(--surface-sidebar)]">
        <div
          data-tauri-drag-region
          className="flex min-w-0 flex-1 items-center gap-2 px-3"
        >
          <IconFileDiff size={14} stroke={1.8} className="text-[var(--accent)]" />
          <span className="truncate text-[12px] text-[var(--text-strong)]">
            {payload ? pathBasename(payload.path) : "RunSummary Diff"}
          </span>
          <span className="rounded-full border border-[var(--accent-border)] bg-[var(--accent-soft)] px-2 py-0.5 text-[10px] text-[var(--accent-strong)]">
            {diffSourceLabel(source)}
          </span>
        </div>
        <div className="flex h-full items-center">
          <button
            type="button"
            onClick={() => void handleRestore()}
            disabled={!canWriteDisk || persistStatus !== "idle"}
            className="flex h-full items-center gap-1 px-3 text-[11px] text-[var(--text-muted)] transition-colors hover:bg-[var(--surface-elevated)] hover:text-[var(--text-strong)] disabled:opacity-45"
            title={persistBlockedReason ?? "恢复为修改前版本"}
          >
            <IconArrowBackUp size={13} stroke={1.8} />
            {persistStatus === "restoring" ? "Restoring..." : "Restore"}
          </button>
          <button
            type="button"
            onClick={() => void handleKeep()}
            disabled={!canWriteDisk || persistStatus !== "idle"}
            className="flex h-full items-center gap-1 px-3 text-[11px] text-[var(--text-muted)] transition-colors hover:bg-[var(--surface-elevated)] hover:text-[var(--text-strong)] disabled:opacity-45"
            title={persistBlockedReason ?? "保留修改并写入硬盘"}
          >
            <IconDeviceFloppy size={13} stroke={1.8} />
            {persistStatus === "keeping" ? "Keeping..." : "Keep"}
          </button>
          <button
            type="button"
            onClick={() => runWindowAction(windowMinimize, "minimize")}
            className="flex h-full w-11 items-center justify-center text-[var(--text-muted)] transition-colors hover:bg-[var(--surface-elevated)]"
            title="最小化"
          >
            <svg width="10" height="1" viewBox="0 0 10 1" fill="currentColor">
              <rect width="10" height="1" />
            </svg>
          </button>
          <button
            type="button"
            onClick={() => runWindowAction(windowCloseRunSummaryDiff, "close runsummary diff")}
            className="flex h-full w-11 items-center justify-center text-[var(--text-muted)] transition-colors hover:bg-[#e81123] hover:text-white"
            title="关闭"
          >
            <svg
              width="10"
              height="10"
              viewBox="0 0 10 10"
              fill="none"
              stroke="currentColor"
              strokeWidth="1.2"
            >
              <line x1="0" y1="0" x2="10" y2="10" />
              <line x1="10" y1="0" x2="0" y2="10" />
            </svg>
          </button>
        </div>
      </div>

      <div className="border-b border-[var(--border-subtle)] bg-[var(--surface-main)] px-3 py-2">
        <div className="truncate font-mono text-[11px] text-[var(--text-faint)]">
          {payload?.path ?? "等待主窗口传入 Diff 内容"}
        </div>
        <div className="mt-1 text-[11px] text-[var(--text-muted)]">
          action: {action}
        </div>
      </div>

      {(persistBlockedReason || notice) && (
        <div className="mx-3 mt-3 rounded-[var(--radius-sm)] border border-[var(--border-subtle)] bg-[var(--surface-soft)] px-2 py-1.5 text-[11px] text-[var(--text-muted)]">
          {notice ? (
            <span className="inline-flex items-center gap-1.5">
              {notice.kind === "success" ? (
                <IconCheck size={13} stroke={1.8} />
              ) : notice.kind === "error" ? (
                <IconX size={13} stroke={1.8} />
              ) : (
                <IconFileDiff size={13} stroke={1.8} />
              )}
              {notice.text}
            </span>
          ) : (
            persistBlockedReason
          )}
        </div>
      )}

      <div className="min-h-0 flex-1 overflow-hidden p-3 pt-2">
        {!payload ? (
          <div className="flex h-full items-center justify-center rounded-[var(--radius-sm)] border border-[var(--chat-line)] bg-[var(--chat-card)] text-[12px] text-[var(--chat-muted)]">
            等待主窗口打开 Diff...
          </div>
        ) : (
          <div className="h-full overflow-auto rounded-[var(--radius-sm)] border border-[var(--chat-line)] bg-[var(--chat-card-solid)]">
            {lines.length === 0 ? (
              <div className="px-3 py-3 text-[11px] text-[var(--chat-muted)]">
                {payload.emptyHint ?? "当前文件缺少可对比的行级差异。"}
              </div>
            ) : (
              lines.map((line, index) => (
                <div
                  key={`${index}:${line.type}:${line.oldLineNumber ?? 0}:${line.newLineNumber ?? 0}`}
                  className={`patch-diff-line patch-diff-line--${line.type}`}
                >
                  <span className="patch-diff-line-number">
                    {line.oldLineNumber ?? ""}
                  </span>
                  <span className="patch-diff-line-number">
                    {line.newLineNumber ?? ""}
                  </span>
                  <span className="patch-diff-line-prefix">{linePrefix(line.type)}</span>
                  <div className="patch-diff-line-text patch-diff-line-text--code">
                    <DiffLineCode text={line.text} language={languageLabel} />
                  </div>
                </div>
              ))
            )}
          </div>
        )}
      </div>
    </div>
  );
}
