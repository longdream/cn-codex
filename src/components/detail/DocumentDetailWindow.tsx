import {
  IconArrowBackUp,
  IconCheck,
  IconDeviceFloppy,
  IconFileCode,
  IconMessagePlus,
  IconPhoto,
  IconX,
} from "@tabler/icons-react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useIntl } from "react-intl";
import ReactMarkdown, { type Components } from "react-markdown";
import rehypeHighlight from "rehype-highlight";
import remarkGfm from "remark-gfm";
import {
  documentDetailInsertSnippet,
  readTextFilePreview,
  windowCloseDocumentDetail,
  windowGetDocumentDetailPath,
  windowMinimize,
  writeTextFilePreview,
  type TextFilePreviewResult,
} from "../../api/window";
import { formatCodeSnippet } from "../../utils/formatCodeSnippet";

interface SelectionMeta {
  text: string;
  startLine: number;
  endLine: number;
  lineCount: number;
}

interface FloatingPosition {
  left: number;
  top: number;
}

type NoticeKind = "success" | "error" | "info";
type MarkdownViewMode = "preview" | "source";

const markdownPreviewComponents: Components = {
  h1: ({ children }) => (
    <h1 className="mt-5 mb-2 text-lg font-semibold text-[var(--chat-prose)]">{children}</h1>
  ),
  h2: ({ children }) => (
    <h2 className="mt-4 mb-2 text-[16px] font-semibold text-[var(--chat-prose)]">{children}</h2>
  ),
  h3: ({ children }) => (
    <h3 className="mt-3 mb-1.5 text-[15px] font-semibold text-[var(--chat-prose)]">{children}</h3>
  ),
  p: ({ children }) => <p className="whitespace-pre-wrap break-words">{children}</p>,
  ul: ({ children }) => (
    <ul className="my-3 ml-6 list-disc space-y-1.5 text-[var(--chat-prose)]">{children}</ul>
  ),
  ol: ({ children }) => (
    <ol className="my-3 ml-6 list-decimal space-y-1.5 text-[var(--chat-prose)]">{children}</ol>
  ),
  li: ({ children }) => <li className="leading-relaxed">{children}</li>,
  a: ({ href, children }) => (
    <a
      href={href}
      target="_blank"
      rel="noreferrer"
      className="text-[var(--accent)] underline decoration-[0.08em] underline-offset-2 hover:opacity-80"
    >
      {children}
    </a>
  ),
  blockquote: ({ children }) => (
    <blockquote className="my-3 border-l-2 border-[var(--chat-line)] pl-3 text-[var(--chat-muted)]">
      {children}
    </blockquote>
  ),
  pre: ({ children }) => (
    <pre className="thin-scrollbar my-3 overflow-auto rounded-[var(--radius-sm)] border border-[var(--chat-line)] bg-[var(--surface-main)] p-3 text-[12px] leading-6">
      {children}
    </pre>
  ),
  code: ({ className, children }) => {
    const code = String(children ?? "").replace(/\n$/, "");
    const isBlockCode = Boolean(className) || code.includes("\n");
    if (isBlockCode) {
      return <code className={`hljs ${className ?? ""}`.trim()}>{code}</code>;
    }
    return <code className="chat-inline-code">{code}</code>;
  },
  table: ({ children }) => (
    <div className="thin-scrollbar my-3 overflow-x-auto">
      <table className="chat-md-table">{children}</table>
    </div>
  ),
  thead: ({ children }) => <thead className="bg-[var(--chat-card-solid)]">{children}</thead>,
  tbody: ({ children }) => <tbody>{children}</tbody>,
  tr: ({ children }) => <tr className="border-b border-[var(--chat-line)] last:border-b-0">{children}</tr>,
  th: ({ children }) => (
    <th className="border-r border-[var(--chat-line)] px-3 py-2 text-left font-semibold last:border-r-0">
      {children}
    </th>
  ),
  td: ({ children }) => (
    <td className="border-r border-[var(--chat-line)] px-3 py-2 align-top last:border-r-0">{children}</td>
  ),
};

function lineOfOffset(source: string, offset: number): number {
  let line = 1;
  for (let i = 0; i < offset && i < source.length; i += 1) {
    if (source[i] === "\n") {
      line += 1;
    }
  }
  return line;
}

function fileLanguage(name: string): string {
  const ext = name.split(".").pop()?.toLowerCase() ?? "";
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

const IMAGE_EXTENSIONS = new Set([
  "png", "jpg", "jpeg", "gif", "svg", "webp", "ico", "bmp",
]);

function isImageFile(name: string): boolean {
  const ext = name.split(".").pop()?.toLowerCase() ?? "";
  return IMAGE_EXTENSIONS.has(ext);
}

function resolveSelectionMeta(source: string, start: number, end: number): SelectionMeta | null {
  if (end <= start) {
    return null;
  }
  const safeStart = Math.max(0, Math.min(start, source.length));
  const safeEnd = Math.max(safeStart, Math.min(end, source.length));
  const selectedText = source.slice(safeStart, safeEnd);
  if (!selectedText.trim()) {
    return null;
  }
  const startLine = lineOfOffset(source, safeStart);
  // 结束行按“最后一个选中文本字符”定位，避免选区末尾是换行时多算一行。
  const endLine = lineOfOffset(source, Math.max(safeStart, safeEnd - 1));
  return {
    text: selectedText,
    startLine,
    endLine,
    lineCount: endLine - startLine + 1,
  };
}

function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value));
}

function measureTextareaCaretPosition(
  textarea: HTMLTextAreaElement,
  position: number,
): FloatingPosition | null {
  const style = window.getComputedStyle(textarea);
  const mirror = document.createElement("div");
  // 复制关键样式构建“镜像排版容器”，用于计算文本光标像素位置。
  const mirroredProps = [
    "boxSizing",
    "fontFamily",
    "fontSize",
    "fontWeight",
    "fontStyle",
    "letterSpacing",
    "lineHeight",
    "paddingTop",
    "paddingRight",
    "paddingBottom",
    "paddingLeft",
    "borderTopWidth",
    "borderRightWidth",
    "borderBottomWidth",
    "borderLeftWidth",
    "textTransform",
    "textIndent",
    "textDecoration",
    "tabSize",
    "whiteSpace",
    "wordBreak",
    "overflowWrap",
  ] as const;
  mirroredProps.forEach((prop) => {
    mirror.style[prop] = style[prop];
  });
  mirror.style.position = "fixed";
  mirror.style.visibility = "hidden";
  mirror.style.left = "-99999px";
  mirror.style.top = "0";
  mirror.style.width = `${textarea.clientWidth}px`;
  const noWrap = textarea.wrap === "off";
  mirror.style.whiteSpace = noWrap ? "pre" : "pre-wrap";
  mirror.style.wordBreak = noWrap ? "normal" : "break-word";
  mirror.style.overflow = "hidden";

  const contentBeforeCaret = textarea.value.slice(0, position);
  const contentAfterCaret = textarea.value.slice(position);
  mirror.textContent = contentBeforeCaret;
  const marker = document.createElement("span");
  marker.textContent = contentAfterCaret[0] ?? " ";
  mirror.appendChild(marker);
  document.body.appendChild(mirror);

  const markerRect = marker.getBoundingClientRect();
  const mirrorRect = mirror.getBoundingClientRect();
  const textareaRect = textarea.getBoundingClientRect();
  const caretLeft =
    textareaRect.left + (markerRect.left - mirrorRect.left) - textarea.scrollLeft;
  const caretTop = textareaRect.top + (markerRect.top - mirrorRect.top) - textarea.scrollTop;

  document.body.removeChild(mirror);
  return { left: caretLeft, top: caretTop };
}

function runWindowAction(action: () => Promise<void>, label: string): void {
  const onError = (err: unknown) => {
    console.error(`Document detail window ${label} failed:`, err);
  };
  try {
    void action().catch(onError);
  } catch (err) {
    onError(err);
  }
}

export function DocumentDetailWindow() {
  const intl = useIntl();
  const [activePath, setActivePath] = useState<string | null>(null);
  const [preview, setPreview] = useState<TextFilePreviewResult | null>(null);
  const [isImage, setIsImage] = useState(false);
  const [draftContent, setDraftContent] = useState("");
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [selectionMeta, setSelectionMeta] = useState<SelectionMeta | null>(null);
  const [floatingPos, setFloatingPos] = useState<FloatingPosition | null>(null);
  const [notice, setNotice] = useState<{ kind: NoticeKind; text: string } | null>(null);
  const [markdownViewMode, setMarkdownViewMode] = useState<MarkdownViewMode>("preview");

  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const highlightContentRef = useRef<HTMLDivElement>(null);
  const isDirty = useMemo(
    () => Boolean(preview && draftContent !== preview.content),
    [preview, draftContent],
  );
  const languageLabel = useMemo(
    () => (preview ? fileLanguage(preview.name) : "text"),
    [preview],
  );
  const isMarkdownFile = languageLabel === "markdown";
  const showMarkdownPreview = isMarkdownFile && markdownViewMode === "preview";
  const highlightedCodeMarkdown = useMemo(() => {
    if (!preview) {
      return "";
    }
    return `\`\`\`${languageLabel}\n${draftContent}\n\`\`\``;
  }, [preview, draftContent, languageLabel]);

  const syncHighlightScroll = useCallback(() => {
    const textarea = textareaRef.current;
    const highlightContent = highlightContentRef.current;
    if (!textarea || !highlightContent) {
      return;
    }
    // 将高亮层按 textarea 当前滚动量做反向位移，
    // 这样只保留一个可编辑区，同时维持“语法样式跟随编辑”。
    highlightContent.style.transform = `translate(${-textarea.scrollLeft}px, ${-textarea.scrollTop}px)`;
  }, []);

  const syncSelection = useCallback(() => {
    const textarea = textareaRef.current;
    if (!textarea) {
      setSelectionMeta(null);
      setFloatingPos(null);
      return;
    }

    const start = textarea.selectionStart ?? 0;
    const end = textarea.selectionEnd ?? 0;
    const meta = resolveSelectionMeta(draftContent, start, end);
    setSelectionMeta(meta);
    if (!meta) {
      setFloatingPos(null);
      return;
    }

    const caretPos = measureTextareaCaretPosition(textarea, end);
    if (!caretPos) {
      setFloatingPos(null);
      return;
    }
    // 按“选区右下角附近”放置浮动按钮，并做边界收敛，避免被窗口裁切。
    setFloatingPos({
      left: clamp(caretPos.left + 10, 12, window.innerWidth - 170),
      top: clamp(caretPos.top + 24, 12, window.innerHeight - 48),
    });
  }, [draftContent]);

  const loadPreview = useCallback(async (path: string) => {
    setLoading(true);
    setLoadError(null);
    setNotice(null);
    setSelectionMeta(null);
    setFloatingPos(null);

    const fileName = path.replace(/\\/g, "/").split("/").pop() ?? "";
    if (isImageFile(fileName)) {
      setIsImage(true);
      setPreview(null);
      setDraftContent("");
      setLoading(false);
      return;
    }

    setIsImage(false);
    try {
      const result = await readTextFilePreview(path);
      setPreview(result);
      setDraftContent(result.content);
    } catch (err) {
      setPreview(null);
      setDraftContent("");
      setLoadError(String(err));
    } finally {
      setLoading(false);
    }
  }, []);

  const handleSave = useCallback(async () => {
    if (!preview) return;
    if (preview.truncated) {
      setNotice({
        kind: "error",
        text: intl.formatMessage({ id: "docDetail.truncatedSaveBlocked" }),
      });
      return;
    }

    setSaving(true);
    setNotice(null);
    try {
      const saved = await writeTextFilePreview(preview.path, draftContent);
      setPreview((prev) =>
        prev
          ? {
              ...prev,
              content: draftContent,
              size: saved.size,
              truncated: false,
            }
          : prev,
      );
      setNotice({
        kind: "success",
        text: intl.formatMessage({ id: "docDetail.savedSuccess" }, { size: saved.size }),
      });
    } catch (err) {
      setNotice({
        kind: "error",
        text: intl.formatMessage({ id: "docDetail.saveFailed" }, { error: String(err) }),
      });
    } finally {
      setSaving(false);
    }
  }, [preview, draftContent, intl]);

  const handleInsertSelection = useCallback(async () => {
    if (!preview || !selectionMeta) return;
    const snippet = formatCodeSnippet({
      path: preview.path,
      startLine: selectionMeta.startLine,
      endLine: selectionMeta.endLine,
      language: fileLanguage(preview.name),
      content: selectionMeta.text,
    });
    try {
      await documentDetailInsertSnippet(snippet.text);
      setNotice({
        kind: "success",
        text: snippet.truncated
          ? intl.formatMessage({ id: "docDetail.insertTruncated" }, { lines: selectionMeta.lineCount })
          : intl.formatMessage({ id: "docDetail.insertSuccess" }, { lines: selectionMeta.lineCount }),
      });
    } catch (err) {
      setNotice({
        kind: "error",
        text: intl.formatMessage({ id: "docDetail.insertFailed" }, { error: String(err) }),
      });
    }
  }, [preview, selectionMeta, intl]);

  useEffect(() => {
    let cancelled = false;
    void windowGetDocumentDetailPath().then((path) => {
      if (cancelled) return;
      setActivePath(path ?? null);
    });
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    if (!activePath) {
      setPreview(null);
      setDraftContent("");
      setSelectionMeta(null);
      setFloatingPos(null);
      return;
    }
    void loadPreview(activePath);
  }, [activePath, loadPreview]);

  useEffect(() => {
    setMarkdownViewMode(languageLabel === "markdown" ? "preview" : "source");
  }, [preview?.path, languageLabel]);

  useEffect(() => {
    if (!showMarkdownPreview) return;
    setSelectionMeta(null);
    setFloatingPos(null);
  }, [showMarkdownPreview]);

  useEffect(() => {
    let unlisten: UnlistenFn | null = null;
    void listen<{ path?: string }>(`document-detail-open`, (event) => {
      const nextPath = event.payload?.path?.trim();
      if (!nextPath) return;
      setActivePath(nextPath);
    }).then((fn) => {
      unlisten = fn;
    });
    return () => {
      if (unlisten) {
        void unlisten();
      }
    };
  }, []);

  useEffect(() => {
    if (!selectionMeta) {
      return;
    }
    const handleWindowResize = () => {
      syncSelection();
    };
    window.addEventListener("resize", handleWindowResize);
    const textarea = textareaRef.current;
    const handleTextareaScroll = () => {
      syncSelection();
      syncHighlightScroll();
    };
    textarea?.addEventListener("scroll", handleTextareaScroll);
    return () => {
      window.removeEventListener("resize", handleWindowResize);
      textarea?.removeEventListener("scroll", handleTextareaScroll);
    };
  }, [selectionMeta, syncHighlightScroll, syncSelection]);

  useEffect(() => {
    // 切换文件后重置滚动位移，避免新文件沿用旧文件滚动位置导致“样式错位”。
    const textarea = textareaRef.current;
    if (!textarea) {
      return;
    }
    textarea.scrollTop = 0;
    textarea.scrollLeft = 0;
    syncHighlightScroll();
  }, [preview?.path, syncHighlightScroll]);

  const imageFileName = useMemo(
    () => (activePath ? activePath.replace(/\\/g, "/").split("/").pop() ?? "" : ""),
    [activePath],
  );
  const imageSrc = useMemo(
    () => (isImage && activePath ? convertFileSrc(activePath) : null),
    [isImage, activePath],
  );

  const canSave = Boolean(preview) && !saving && !loading && isDirty && !preview?.truncated;

  return (
    <div className="flex h-dvh w-screen flex-col bg-[var(--surface-panel)] text-[var(--text-base)]">
      <div className="flex h-9 items-center border-b border-[var(--border-subtle)] bg-[var(--surface-sidebar)]">
        <div
          data-tauri-drag-region
          className="flex min-w-0 flex-1 items-center gap-2 px-3"
        >
          {isImage ? (
            <IconPhoto size={14} stroke={1.8} className="text-pink-400" />
          ) : (
            <IconFileCode size={14} stroke={1.8} className="text-[var(--accent)]" />
          )}
          <span className="truncate text-[12px] text-[var(--text-strong)]">
            {isImage ? imageFileName : (preview?.name ?? intl.formatMessage({ id: "docDetail.title" }))}
          </span>
          {isDirty && (
            <span className="rounded-full border border-[var(--warning)]/40 bg-[var(--warning)]/12 px-2 py-0.5 text-[10px] text-[var(--warning)]">
              {intl.formatMessage({ id: "docDetail.unsaved" })}
            </span>
          )}
        </div>
        <div className="flex h-full items-center">
          {!isImage && (
            <button
              type="button"
              disabled={!canSave}
              onClick={() => void handleSave()}
              className="flex h-full items-center gap-1 px-3 text-[11px] text-[var(--text-muted)] transition-colors hover:bg-[var(--surface-elevated)] hover:text-[var(--text-strong)] disabled:opacity-45"
              title={
                preview?.truncated
                  ? intl.formatMessage({ id: "docDetail.saveTitleDisabled" })
                  : intl.formatMessage({ id: "docDetail.saveTitleEnabled" })
              }
            >
              <IconDeviceFloppy size={13} stroke={1.8} />
              {intl.formatMessage({ id: "docDetail.save" })}
            </button>
          )}
          <button
            type="button"
            onClick={() => runWindowAction(windowMinimize, "minimize")}
            className="flex h-full w-11 items-center justify-center text-[var(--text-muted)] transition-colors hover:bg-[var(--surface-elevated)]"
            title={intl.formatMessage({ id: "docDetail.minimize" })}
          >
            <svg width="10" height="1" viewBox="0 0 10 1" fill="currentColor">
              <rect width="10" height="1" />
            </svg>
          </button>
          <button
            type="button"
            onClick={() =>
              runWindowAction(windowCloseDocumentDetail, "close document detail")
            }
            className="flex h-full w-11 items-center justify-center text-[var(--text-muted)] transition-colors hover:bg-[#e81123] hover:text-white"
            title={intl.formatMessage({ id: "docDetail.close" })}
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
          {preview?.path ?? activePath ?? intl.formatMessage({ id: "docDetail.noFileSelected" })}
        </div>
      </div>

      {notice && (
        <div
          className={`mx-3 mt-3 flex items-center gap-2 rounded-[var(--radius-sm)] border px-2 py-1.5 text-[11px] ${
            notice.kind === "success"
              ? "border-[var(--accent-border)] bg-[var(--accent-soft)] text-[var(--accent-strong)]"
              : notice.kind === "error"
                ? "border-[var(--danger)]/35 bg-[var(--danger-soft)] text-[var(--danger)]"
                : "border-[var(--border-subtle)] bg-[var(--surface-soft)] text-[var(--text-muted)]"
          }`}
        >
          {notice.kind === "success" ? (
            <IconCheck size={13} stroke={1.8} />
          ) : notice.kind === "error" ? (
            <IconX size={13} stroke={1.8} />
          ) : (
            <IconArrowBackUp size={13} stroke={1.8} />
          )}
          <span className="truncate">{notice.text}</span>
        </div>
      )}

      <div className="relative min-h-0 flex-1 p-3">
        {loading ? (
          <div className="flex h-full items-center justify-center text-sm text-[var(--text-faint)]">
            {intl.formatMessage({ id: "docDetail.loading" })}
          </div>
        ) : loadError ? (
          <div className="thin-scrollbar h-full overflow-auto rounded-[var(--radius-sm)] border border-[var(--danger)]/35 bg-[var(--danger-soft)] p-3 text-sm text-[var(--danger)]">
            {loadError}
          </div>
        ) : isImage && imageSrc ? (
          <div className="flex h-full flex-col overflow-hidden rounded-[var(--radius-sm)] border border-[var(--chat-line)] bg-[var(--surface-main)]">
            <div className="flex items-center justify-between border-b border-[var(--chat-line)] px-3 py-1.5">
              <span className="text-[11px] text-[var(--chat-faint)]">
                {intl.formatMessage({ id: "docDetail.imagePreviewLabel" })}
              </span>
              <span className="font-mono text-[11px] text-[var(--chat-faint)]">
                {imageFileName.split(".").pop()?.toUpperCase()}
              </span>
            </div>
            <div className="flex min-h-0 flex-1 items-center justify-center overflow-auto p-4">
              <img
                src={imageSrc}
                alt={imageFileName}
                className="max-h-full max-w-full object-contain"
                draggable={false}
              />
            </div>
          </div>
        ) : preview ? (
          <>
            {preview.truncated && (
              <div className="mb-2 rounded-[var(--radius-sm)] border border-[var(--warning)]/35 bg-[var(--warning)]/12 px-3 py-2 text-[11px] text-[var(--warning)]">
                {intl.formatMessage({ id: "docDetail.truncatedWarning" })}
              </div>
            )}
            {showMarkdownPreview ? (
              <div className="h-full min-h-0 overflow-hidden rounded-[var(--radius-sm)] border border-[var(--chat-line)] bg-[var(--chat-paper)]">
                <div className="flex items-center justify-between border-b border-[var(--chat-line)] px-3 py-1.5">
                  <span className="text-[11px] text-[var(--chat-faint)]">
                    {intl.formatMessage({ id: "docDetail.markdownPreviewLabel" })}
                  </span>
                  <div className="flex items-center gap-2">
                    <div className="inline-flex items-center rounded-[var(--radius-sm)] border border-[var(--chat-line)] bg-[var(--surface-main)] p-0.5">
                      <button
                        type="button"
                        onClick={() => setMarkdownViewMode("preview")}
                        className="rounded-[var(--radius-sm)] px-2 py-0.5 text-[10px] text-[var(--chat-prose)] bg-[var(--accent-soft)]"
                      >
                        {intl.formatMessage({ id: "docDetail.preview" })}
                      </button>
                      <button
                        type="button"
                        onClick={() => setMarkdownViewMode("source")}
                        className="rounded-[var(--radius-sm)] px-2 py-0.5 text-[10px] text-[var(--chat-faint)] transition-colors hover:text-[var(--chat-prose)]"
                      >
                        {intl.formatMessage({ id: "docDetail.source" })}
                      </button>
                    </div>
                    <span className="font-mono text-[11px] text-[var(--chat-faint)]">
                      {languageLabel}
                    </span>
                  </div>
                </div>
                <div className="thin-scrollbar h-[calc(100%-31px)] overflow-auto px-4 py-3">
                  <article className="chat-prose">
                    <ReactMarkdown
                      remarkPlugins={[remarkGfm]}
                      rehypePlugins={[rehypeHighlight]}
                      components={markdownPreviewComponents}
                    >
                      {draftContent}
                    </ReactMarkdown>
                  </article>
                </div>
              </div>
            ) : (
              <div className="detail-code-shell">
                <div className="flex items-center justify-between border-b border-[var(--chat-line)] px-3 py-1.5">
                  <span className="text-[11px] text-[var(--chat-faint)]">
                    {isMarkdownFile
                      ? intl.formatMessage({ id: "docDetail.markdownSourceLabel" })
                      : intl.formatMessage({ id: "docDetail.codePreviewLabel" })}
                  </span>
                  <div className="flex items-center gap-2">
                    {isMarkdownFile && (
                      <div className="inline-flex items-center rounded-[var(--radius-sm)] border border-[var(--chat-line)] bg-[var(--surface-main)] p-0.5">
                        <button
                          type="button"
                          onClick={() => setMarkdownViewMode("preview")}
                          className="rounded-[var(--radius-sm)] px-2 py-0.5 text-[10px] text-[var(--chat-faint)] transition-colors hover:text-[var(--chat-prose)]"
                        >
                          {intl.formatMessage({ id: "docDetail.preview" })}
                        </button>
                        <button
                          type="button"
                          onClick={() => setMarkdownViewMode("source")}
                          className="rounded-[var(--radius-sm)] px-2 py-0.5 text-[10px] text-[var(--chat-prose)] bg-[var(--accent-soft)]"
                        >
                          {intl.formatMessage({ id: "docDetail.source" })}
                        </button>
                      </div>
                    )}
                    <span className="font-mono text-[11px] text-[var(--chat-faint)]">
                      {languageLabel}
                    </span>
                  </div>
                </div>
                <div className="detail-code-editor-layer">
                  <div className="detail-code-highlight-layer">
                    <div ref={highlightContentRef} className="detail-code-highlight-content">
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
                        {highlightedCodeMarkdown}
                      </ReactMarkdown>
                    </div>
                  </div>
                  <textarea
                    ref={textareaRef}
                    value={draftContent}
                    onChange={(event) => {
                      setDraftContent(event.target.value);
                    }}
                    onMouseUp={syncSelection}
                    onKeyUp={syncSelection}
                    onSelect={syncSelection}
                    onScroll={syncHighlightScroll}
                    spellCheck={false}
                    wrap="off"
                    className="detail-code-textarea thin-scrollbar"
                  />
                </div>
              </div>
            )}
          </>
        ) : (
          <div className="flex h-full items-center justify-center text-sm text-[var(--text-faint)]">
            {intl.formatMessage({ id: "docDetail.waitingForFile" })}
          </div>
        )}
      </div>

      {selectionMeta && floatingPos && !showMarkdownPreview && (
        <button
          type="button"
          onClick={() => void handleInsertSelection()}
          className="fixed z-[999] inline-flex items-center gap-1 rounded-[var(--radius-sm)] border border-[var(--accent-border)] bg-[var(--accent-soft)] px-2 py-1 text-[11px] text-[var(--accent-strong)] shadow-[var(--shadow-soft)] transition-colors hover:bg-[var(--surface-elevated)]"
          style={{ left: `${floatingPos.left}px`, top: `${floatingPos.top}px` }}
        >
          <IconMessagePlus size={13} stroke={1.8} />
          {intl.formatMessage({ id: "docDetail.insertToChat" })}
        </button>
      )}
    </div>
  );
}
