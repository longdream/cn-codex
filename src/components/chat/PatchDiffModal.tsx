import { IconLoader2, IconX } from "@tabler/icons-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useIntl, type IntlShape } from "react-intl";
import { readTextFilePreview, writeTextFilePreview } from "../../api/window";
import {
  buildPatchLineDiff,
  type PatchDiffLine,
} from "../../utils/lineDiff";
import { CodeBlock } from "./CodeBlock";

interface PatchDiffModalProps {
  open: boolean;
  titlePath: string;
  beforeContent: string;
  afterContent: string;
  fileAction: string;
  diffSource: "snapshot" | "patch" | "empty";
  canPersist: boolean;
  persistHint?: string;
  emptyHint?: string;
  onClose: () => void;
}

type PreviewStatus = "loading" | "ready" | "failed";
type PersistStatus = "idle" | "keeping" | "restoring";
type PersistNotice = {
  type: "success" | "error";
  message: string;
};

function linePrefix(type: PatchDiffLine["type"]): string {
  if (type === "add") return "+";
  if (type === "remove") return "-";
  return " ";
}

function fileLanguageFromPath(path: string): string {
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

function fallbackPreviewContent(beforeContent: string, afterContent: string): string {
  if (afterContent.trim().length > 0) {
    return afterContent;
  }
  if (beforeContent.trim().length > 0) {
    return beforeContent;
  }
  return "";
}

function diffSourceLabel(source: PatchDiffModalProps["diffSource"], intl: IntlShape): string {
  switch (source) {
    case "snapshot":
      return intl.formatMessage({ id: "diff.sourceSnapshot" });
    case "patch":
      return intl.formatMessage({ id: "diff.sourcePatch" });
    default:
      return intl.formatMessage({ id: "diff.sourceEmpty" });
  }
}

export function PatchDiffModal({
  open,
  titlePath,
  beforeContent,
  afterContent,
  fileAction,
  diffSource,
  canPersist,
  persistHint,
  emptyHint,
  onClose,
}: PatchDiffModalProps) {
  const intl = useIntl();
  const lines = useMemo(
    () => buildPatchLineDiff(beforeContent, afterContent),
    [beforeContent, afterContent],
  );
  const [previewStatus, setPreviewStatus] = useState<PreviewStatus>("loading");
  const [previewCode, setPreviewCode] = useState("");
  const [previewLanguage, setPreviewLanguage] = useState("text");
  const [previewNote, setPreviewNote] = useState<string | null>(null);
  const [persistStatus, setPersistStatus] = useState<PersistStatus>("idle");
  const [persistNotice, setPersistNotice] = useState<PersistNotice | null>(null);

  const loadPreview = useCallback(async () => {
    setPreviewStatus("loading");
    setPreviewCode("");
    setPreviewLanguage(fileLanguageFromPath(titlePath));
    setPreviewNote(null);

    if (!titlePath.trim()) {
      const fallback = fallbackPreviewContent(beforeContent, afterContent);
      setPreviewStatus(fallback ? "ready" : "failed");
      setPreviewCode(fallback);
      setPreviewNote(fallback
        ? intl.formatMessage({ id: "patchDiff.pathEmptySnapshot" })
        : intl.formatMessage({ id: "patchDiff.pathEmptyNoPreview" }));
      return;
    }

    // 先读取文件当前内容用于“详情预览”。
    // 如果文件已不存在（例如删除场景），回退到快照内容，保证弹窗仍可审阅。
    try {
      const preview = await readTextFilePreview(titlePath);
      setPreviewStatus("ready");
      setPreviewCode(preview.content);
      setPreviewLanguage(fileLanguageFromPath(preview.name || preview.path || titlePath));
      setPreviewNote(preview.truncated
        ? intl.formatMessage({ id: "patchDiff.fileTruncated" })
        : null);
    } catch {
      const fallback = fallbackPreviewContent(beforeContent, afterContent);
      if (fallback) {
        setPreviewStatus("ready");
        setPreviewCode(fallback);
        setPreviewNote(intl.formatMessage({ id: "patchDiff.fileUnreadableSnapshot" }));
      } else {
        setPreviewStatus("failed");
        setPreviewCode("");
        setPreviewNote(intl.formatMessage({ id: "patchDiff.fileUnreadableNoFallback" }));
      }
    }
  }, [afterContent, beforeContent, intl, titlePath]);

  useEffect(() => {
    if (!open) {
      return;
    }
    // 弹窗打开时支持 Esc 关闭，交互和主窗口其他弹层保持一致。
    const handler = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        onClose();
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [onClose, open]);

  useEffect(() => {
    if (!open) {
      return;
    }
    setPersistStatus("idle");
    setPersistNotice(null);
    void loadPreview();
  }, [loadPreview, open]);

  const normalizedAction = (fileAction || "modified").toLowerCase();
  const canWriteDisk = useMemo(() => (
    canPersist
      && diffSource === "snapshot"
      && normalizedAction === "modified"
      && titlePath.trim().length > 0
  ), [canPersist, diffSource, normalizedAction, titlePath]);

  const persistBlockedReason = useMemo(() => {
    if (canWriteDisk) {
      return null;
    }
    if (persistHint) {
      return persistHint;
    }
    if (normalizedAction !== "modified") {
      return intl.formatMessage({ id: "diff.onlyModifiedSupported" });
    }
    if (diffSource !== "snapshot") {
      return intl.formatMessage({ id: "diff.snapshotOnlyPersist" });
    }
    if (!titlePath.trim()) {
      return intl.formatMessage({ id: "patchDiff.pathEmptyPersist" });
    }
    return intl.formatMessage({ id: "patchDiff.snapshotCannotPersist" });
  }, [canWriteDisk, diffSource, intl, normalizedAction, persistHint, titlePath]);

  const handleKeep = useCallback(async () => {
    if (!canWriteDisk || persistStatus !== "idle") {
      return;
    }
    setPersistStatus("keeping");
    setPersistNotice(null);
    try {
      await writeTextFilePreview(titlePath, afterContent);
      setPersistNotice({
        type: "success",
        message: intl.formatMessage({ id: "patchDiff.keepSuccess" }),
      });
      await loadPreview();
    } catch (error) {
      setPersistNotice({
        type: "error",
        message: error instanceof Error ? error.message : String(error),
      });
    } finally {
      setPersistStatus("idle");
    }
  }, [afterContent, canWriteDisk, intl, loadPreview, persistStatus, titlePath]);

  const handleRestore = useCallback(async () => {
    if (!canWriteDisk || persistStatus !== "idle") {
      return;
    }
    setPersistStatus("restoring");
    setPersistNotice(null);
    try {
      await writeTextFilePreview(titlePath, beforeContent);
      setPersistNotice({
        type: "success",
        message: intl.formatMessage({ id: "patchDiff.restoreSuccess" }),
      });
      await loadPreview();
    } catch (error) {
      setPersistNotice({
        type: "error",
        message: error instanceof Error ? error.message : String(error),
      });
    } finally {
      setPersistStatus("idle");
    }
  }, [beforeContent, canWriteDisk, intl, loadPreview, persistStatus, titlePath]);

  if (!open) {
    return null;
  }

  return (
    <div
      className="patch-diff-backdrop"
      onClick={() => onClose()}
      role="presentation"
    >
      <div
        className="patch-diff-dialog"
        onClick={(event) => event.stopPropagation()}
      >
        <div className="patch-diff-header">
          <div className="min-w-0">
            <div className="text-[12px] font-semibold text-[var(--chat-prose)]">
              {intl.formatMessage({ id: "patchDiff.title" })}
            </div>
            <div
              className="mt-0.5 truncate font-mono text-[11px] text-[var(--chat-faint)]"
              title={titlePath}
            >
              {titlePath}
            </div>
          </div>
          <button
            type="button"
            onClick={() => onClose()}
            className="patch-diff-close"
            title={intl.formatMessage({ id: "patchDiff.close" })}
          >
            <IconX size={14} stroke={1.8} />
          </button>
        </div>

        <div className="patch-diff-toolbar">
          <div className="patch-diff-toolbar-meta">
            <span>
              {intl.formatMessage({ id: "patchDiff.source" })}
              {diffSourceLabel(diffSource, intl)}
            </span>
            <span>
              {intl.formatMessage({ id: "patchDiff.action" })}
              {normalizedAction}
            </span>
          </div>
          <div className="patch-diff-toolbar-actions">
            <button
              type="button"
              className="secondary-button rounded-[var(--radius-sm)] px-2 py-1 text-[11px]"
              disabled={!canWriteDisk || persistStatus !== "idle"}
              onClick={() => void handleRestore()}
              title={persistBlockedReason ?? intl.formatMessage({ id: "patchDiff.restoreTitle" })}
            >
              {intl.formatMessage({ id: persistStatus === "restoring" ? "diff.restoring" : "diff.restore" })}
            </button>
            <button
              type="button"
              className="primary-button rounded-[var(--radius-sm)] px-2 py-1 text-[11px]"
              disabled={!canWriteDisk || persistStatus !== "idle"}
              onClick={() => void handleKeep()}
              title={persistBlockedReason ?? intl.formatMessage({ id: "patchDiff.keepTitle" })}
            >
              {intl.formatMessage({ id: persistStatus === "keeping" ? "diff.keeping" : "diff.keep" })}
            </button>
          </div>
        </div>

        {(persistBlockedReason || persistNotice) && (
          <div className="patch-diff-feedback">
            {persistNotice ? (
              <p
                className={`patch-diff-feedback-text ${
                  persistNotice.type === "error"
                    ? "patch-diff-feedback-text--error"
                    : "patch-diff-feedback-text--success"
                }`}
              >
                {persistNotice.message}
              </p>
            ) : (
              <p className="patch-diff-feedback-text patch-diff-feedback-text--hint">
                {persistBlockedReason}
              </p>
            )}
          </div>
        )}

        <div className="patch-diff-body">
          <section className="patch-diff-panel patch-diff-preview-panel">
            <div className="patch-diff-panel-title">
              {intl.formatMessage({ id: "patchDiff.previewLabel" })}
            </div>
            <div className="patch-diff-preview-scroll thin-scrollbar">
              {previewStatus === "loading" && (
                <div className="flex items-center gap-2 px-3 py-2 text-[11px] text-[var(--chat-muted)]">
                  <IconLoader2 size={12} stroke={1.8} className="animate-spin" />
                  {intl.formatMessage({ id: "patchDiff.loadingPreview" })}
                </div>
              )}
              {previewStatus === "failed" && (
                <p className="px-3 py-2 text-[11px] text-[var(--danger)]">
                  {previewNote ?? intl.formatMessage({ id: "patchDiff.loadFailed" })}
                </p>
              )}
              {previewStatus === "ready" && (
                <div className="px-3 py-2">
                  {previewNote && (
                    <p className="mb-2 text-[11px] text-[var(--chat-muted)]">
                      {previewNote}
                    </p>
                  )}
                  <CodeBlock
                    code={previewCode.length > 0
                      ? previewCode
                      : intl.formatMessage({ id: "patchDiff.emptyFilePlaceholder" })}
                    language={previewLanguage}
                  />
                </div>
              )}
            </div>
          </section>

          <section className="patch-diff-panel patch-diff-result-panel">
            <div className="patch-diff-panel-title">
              {intl.formatMessage({ id: "patchDiff.diffLabel" })}
            </div>
            <div className="patch-diff-lines-scroll thin-scrollbar">
              {lines.length === 0 ? (
                <div className="px-3 py-3 text-[11px] text-[var(--chat-muted)]">
                  {emptyHint ?? intl.formatMessage({ id: "diff.noLineDiff" })}
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
                    <span className="patch-diff-line-text">{line.text}</span>
                  </div>
                ))
              )}
            </div>
          </section>
        </div>
      </div>
    </div>
  );
}
