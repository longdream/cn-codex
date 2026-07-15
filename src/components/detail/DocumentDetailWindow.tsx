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
import { save } from "@tauri-apps/plugin-dialog";
import { writeFile } from "@tauri-apps/plugin-fs";
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
  windowGetDocumentDetailLine,
  windowMinimize,
  writeTextFilePreview,
  type TextFilePreviewResult,
} from "../../api/window";
import { formatCodeSnippet } from "../../utils/formatCodeSnippet";
import { highlightCodeHtml } from "../../utils/highlightCode";
import { exportMarkdownAsDocxBytes, exportMarkdownAsPdfBytes } from "../../utils/markdownExport";
import { encodePathRefRangeSnippet, supportsPathRefRangeByName } from "../../utils/pathRefSnippet";

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
type LineEnding = "\n" | "\r\n";

function normalizeEditorText(text: string): string {
  // textarea 的 selectionStart/End 按逻辑字符计数；Windows 的 CRLF 会让光标/删除整体偏一位。
  return text.replace(/\r\n/g, "\n").replace(/\r/g, "\n");
}

function detectLineEnding(text: string): LineEnding {
  return text.includes("\r\n") ? "\r\n" : "\n";
}

function serializeEditorText(text: string, lineEnding: LineEnding): string {
  if (lineEnding === "\n") {
    return text;
  }
  return text.replace(/\n/g, "\r\n");
}

function isPrimaryModifier(event: KeyboardEvent): boolean {
  return event.ctrlKey || event.metaKey;
}

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

function replaceExtension(path: string, extension: "docx" | "pdf"): string {
  const normalizedExt = extension.toLowerCase();
  // 仅替换最后一段扩展名，路径中的目录点号不参与替换。
  if (/\.[^\\/]+$/.test(path)) {
    return path.replace(/\.[^\\/]+$/, `.${normalizedExt}`);
  }
  return `${path}.${normalizedExt}`;
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
  const [pendingLine, setPendingLine] = useState<number | null>(null);
  const [preview, setPreview] = useState<TextFilePreviewResult | null>(null);
  const [isImage, setIsImage] = useState(false);
  const [draftContent, setDraftContent] = useState("");
  const [lineEnding, setLineEnding] = useState<LineEnding>("\n");
  // 初始即为加载中，避免窗口打开瞬间先渲染空白/等待态，再闪一下“加载中”。
  const [loading, setLoading] = useState(true);
  const [bootstrapped, setBootstrapped] = useState(false);
  const [saving, setSaving] = useState(false);
  const [exportingFormat, setExportingFormat] = useState<"docx" | "pdf" | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [selectionMeta, setSelectionMeta] = useState<SelectionMeta | null>(null);
  const [floatingPos, setFloatingPos] = useState<FloatingPosition | null>(null);
  const [notice, setNotice] = useState<{ kind: NoticeKind; text: string } | null>(null);
  const [markdownViewMode, setMarkdownViewMode] = useState<MarkdownViewMode>("preview");

  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const editorScrollRef = useRef<HTMLDivElement>(null);
  const isDirty = useMemo(
    () => Boolean(preview && draftContent !== normalizeEditorText(preview.content)),
    [preview, draftContent],
  );
  const languageLabel = useMemo(
    () => (preview ? fileLanguage(preview.name) : "text"),
    [preview],
  );
  const isMarkdownFile = languageLabel === "markdown";
  const showMarkdownPreview = isMarkdownFile && markdownViewMode === "preview";
  const highlightedCodeHtml = useMemo(
    () => highlightCodeHtml(draftContent, languageLabel),
    [draftContent, languageLabel],
  );

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
      setLineEnding("\n");
      setLoading(false);
      return;
    }

    setIsImage(false);
    try {
      const result = await readTextFilePreview(path);
      const normalizedContent = normalizeEditorText(result.content);
      setPreview(result);
      setDraftContent(normalizedContent);
      setLineEnding(detectLineEnding(result.content));
    } catch (err) {
      setPreview(null);
      setDraftContent("");
      setLineEnding("\n");
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
      const contentToWrite = serializeEditorText(draftContent, lineEnding);
      const saved = await writeTextFilePreview(preview.path, contentToWrite);
      setPreview((prev) =>
        prev
          ? {
              ...prev,
              content: contentToWrite,
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
  }, [preview, draftContent, lineEnding, intl]);

  const handleExportMarkdown = useCallback(
    async (format: "docx" | "pdf") => {
      if (!preview || !isMarkdownFile) {
        return;
      }
      if (preview.truncated) {
        setNotice({
          kind: "error",
          text: intl.formatMessage({ id: "docDetail.exportTruncatedBlocked" }),
        });
        return;
      }

      setExportingFormat(format);
      setNotice(null);
      try {
        const targetPath = await save({
          title: intl.formatMessage({
            id: format === "docx" ? "docDetail.exportDocxTitle" : "docDetail.exportPdfTitle",
          }),
          defaultPath: replaceExtension(preview.path, format),
          filters: [
            {
              name: format.toUpperCase(),
              extensions: [format],
            },
          ],
        });
        if (!targetPath) {
          return;
        }
        const bytes = format === "docx"
          ? await exportMarkdownAsDocxBytes(draftContent)
          : await exportMarkdownAsPdfBytes(draftContent);
        await writeFile(targetPath, bytes);
        setNotice({
          kind: "success",
          text: intl.formatMessage(
            { id: "docDetail.exportSuccess" },
            { format: format.toUpperCase(), path: targetPath },
          ),
        });
      } catch (err) {
        setNotice({
          kind: "error",
          text: intl.formatMessage(
            { id: "docDetail.exportFailed" },
            { format: format.toUpperCase(), error: String(err) },
          ),
        });
      } finally {
        setExportingFormat(null);
      }
    },
    [preview, isMarkdownFile, draftContent, intl],
  );

  const handleInsertSelection = useCallback(async () => {
    if (!preview || !selectionMeta) return;
    if (supportsPathRefRangeByName(preview.name)) {
      // txt/md 只插入“路径 + 行号范围”标签，避免把整段正文塞到输入框中。
      const snippet = encodePathRefRangeSnippet({
        kind: "pathRefRange",
        sourcePath: preview.path,
        name: preview.name,
        lineStart: selectionMeta.startLine,
        lineEnd: selectionMeta.endLine,
      });
      try {
        await documentDetailInsertSnippet(snippet);
        setNotice({
          kind: "success",
          text: intl.formatMessage(
            { id: "docDetail.insertTagSuccess" },
            { start: selectionMeta.startLine, end: selectionMeta.endLine },
          ),
        });
      } catch (err) {
        setNotice({
          kind: "error",
          text: intl.formatMessage({ id: "docDetail.insertFailed" }, { error: String(err) }),
        });
      }
      return;
    }
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
    void Promise.all([windowGetDocumentDetailPath(), windowGetDocumentDetailLine()]).then(
      ([path, line]) => {
        if (cancelled) return;
        setActivePath(path ?? null);
        setPendingLine(typeof line === "number" && line > 0 ? Math.floor(line) : null);
        setBootstrapped(true);
        if (!path) {
          setLoading(false);
        }
      },
    );
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    if (!activePath) {
      setPreview(null);
      setDraftContent("");
      setLineEnding("\n");
      setSelectionMeta(null);
      setFloatingPos(null);
      if (bootstrapped) {
        setLoading(false);
      }
      return;
    }
    void loadPreview(activePath);
  }, [activePath, loadPreview, bootstrapped]);

  useEffect(() => {
    if (!pendingLine || loading || !preview || isImage || showMarkdownPreview) {
      return;
    }
    const textarea = textareaRef.current;
    if (!textarea) {
      return;
    }

    const targetLine = Math.max(1, pendingLine);
    const lines = draftContent.replace(/\r\n?/g, "\n").split("\n");
    const safeLine = Math.min(targetLine, Math.max(1, lines.length));
    let offset = 0;
    for (let i = 0; i < safeLine - 1; i += 1) {
      offset += lines[i].length + 1;
    }
    const lineText = lines[safeLine - 1] ?? "";
    const end = offset + lineText.length;

    // 等布局稳定后再定位，避免刚写入内容时 scrollHeight 还不准。
    const timer = window.setTimeout(() => {
      try {
        textarea.focus();
        textarea.setSelectionRange(offset, end);
        const style = window.getComputedStyle(textarea);
        const lineHeight = Number.parseFloat(style.lineHeight) || 20;
        const targetTop = Math.max(0, (safeLine - 1) * lineHeight - textarea.clientHeight / 3);
        const scroller = editorScrollRef.current;
        if (scroller) {
          scroller.scrollTop = targetTop;
          scroller.scrollLeft = 0;
        }
      } finally {
        setPendingLine(null);
      }
    }, 40);

    return () => {
      window.clearTimeout(timer);
    };
  }, [
    pendingLine,
    loading,
    preview,
    isImage,
    showMarkdownPreview,
    draftContent,
  ]);

  useEffect(() => {
    if (pendingLine) {
      // 内容搜索跳转时优先进入源码视图，确保行定位可用。
      setMarkdownViewMode("source");
      return;
    }
    setMarkdownViewMode(languageLabel === "markdown" ? "preview" : "source");
  }, [preview?.path, languageLabel, pendingLine]);

  useEffect(() => {
    if (!showMarkdownPreview) return;
    setSelectionMeta(null);
    setFloatingPos(null);
  }, [showMarkdownPreview]);

  useEffect(() => {
    let unlisten: UnlistenFn | null = null;
    void listen<{ path?: string; line?: number | null }>(`document-detail-open`, (event) => {
      const nextPath = event.payload?.path?.trim();
      if (!nextPath) return;
      setActivePath(nextPath);
      const nextLine = event.payload?.line;
      setPendingLine(
        typeof nextLine === "number" && Number.isFinite(nextLine) && nextLine > 0
          ? Math.floor(nextLine)
          : null,
      );
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
    const scroller = editorScrollRef.current;
    const handleEditorScroll = () => {
      syncSelection();
    };
    scroller?.addEventListener("scroll", handleEditorScroll);
    return () => {
      window.removeEventListener("resize", handleWindowResize);
      scroller?.removeEventListener("scroll", handleEditorScroll);
    };
  }, [selectionMeta, syncSelection]);

  useEffect(() => {
    // 切换文件后重置滚动位置，避免沿用旧文件滚动状态。
    const scroller = editorScrollRef.current;
    if (!scroller) {
      return;
    }
    scroller.scrollTop = 0;
    scroller.scrollLeft = 0;
  }, [preview?.path]);

  const imageFileName = useMemo(
    () => (activePath ? activePath.replace(/\\/g, "/").split("/").pop() ?? "" : ""),
    [activePath],
  );
  const imageSrc = useMemo(
    () => (isImage && activePath ? convertFileSrc(activePath) : null),
    [isImage, activePath],
  );

  const canSave = Boolean(preview) && !saving && !loading && isDirty && !preview?.truncated;
  const canExportMarkdown = Boolean(preview) && isMarkdownFile && !loading && !preview?.truncated;

  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.defaultPrevented || event.isComposing) {
        return;
      }

      const key = event.key.toLowerCase();
      const primary = isPrimaryModifier(event);

      // Ctrl/Cmd + S：保存
      if (primary && !event.altKey && key === "s") {
        event.preventDefault();
        if (canSave) {
          void handleSave();
        }
        return;
      }

      // Esc：关闭详情窗口
      if (!primary && !event.altKey && !event.shiftKey && key === "escape") {
        event.preventDefault();
        runWindowAction(windowCloseDocumentDetail, "close document detail");
        return;
      }

      // Markdown 文件：Ctrl/Cmd + Shift + P/E 切换预览/源码
      if (primary && event.shiftKey && !event.altKey && isMarkdownFile && !isImage) {
        if (key === "p") {
          event.preventDefault();
          setMarkdownViewMode("preview");
          return;
        }
        if (key === "e") {
          event.preventDefault();
          setMarkdownViewMode("source");
          return;
        }
      }
    };

    window.addEventListener("keydown", handleKeyDown);
    return () => {
      window.removeEventListener("keydown", handleKeyDown);
    };
  }, [canSave, handleSave, isMarkdownFile, isImage]);

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
          {!isImage && isMarkdownFile && (
            <>
              <button
                type="button"
                disabled={!canExportMarkdown || exportingFormat !== null}
                onClick={() => void handleExportMarkdown("docx")}
                className="flex h-full items-center gap-1 px-3 text-[11px] text-[var(--text-muted)] transition-colors hover:bg-[var(--surface-elevated)] hover:text-[var(--text-strong)] disabled:opacity-45"
                title={
                  preview?.truncated
                    ? intl.formatMessage({ id: "docDetail.exportTitleDisabled" })
                    : intl.formatMessage({ id: "docDetail.exportDocxTitle" })
                }
              >
                {intl.formatMessage({ id: "docDetail.exportDocx" })}
              </button>
              <button
                type="button"
                disabled={!canExportMarkdown || exportingFormat !== null}
                onClick={() => void handleExportMarkdown("pdf")}
                className="flex h-full items-center gap-1 px-3 text-[11px] text-[var(--text-muted)] transition-colors hover:bg-[var(--surface-elevated)] hover:text-[var(--text-strong)] disabled:opacity-45"
                title={
                  preview?.truncated
                    ? intl.formatMessage({ id: "docDetail.exportTitleDisabled" })
                    : intl.formatMessage({ id: "docDetail.exportPdfTitle" })
                }
              >
                {intl.formatMessage({ id: "docDetail.exportPdf" })}
              </button>
            </>
          )}
          {!isImage && (
            <button
              type="button"
              disabled={!canSave}
              onClick={() => void handleSave()}
              className="flex h-full items-center gap-1 px-3 text-[11px] text-[var(--text-muted)] transition-colors hover:bg-[var(--surface-elevated)] hover:text-[var(--text-strong)] disabled:opacity-45"
              title={
                preview?.truncated
                  ? intl.formatMessage({ id: "docDetail.saveTitleDisabled" })
                  : intl.formatMessage({ id: "docDetail.saveTitleEnabledShortcut" })
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
            title={intl.formatMessage({ id: "docDetail.closeShortcut" })}
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
        {loading || !bootstrapped ? (
          <div className="flex h-full items-center justify-center rounded-[var(--radius-sm)] border border-[var(--chat-line)] bg-[var(--surface-main)] text-sm text-[var(--text-muted)]">
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
                        title={intl.formatMessage({ id: "docDetail.previewShortcut" })}
                      >
                        {intl.formatMessage({ id: "docDetail.preview" })}
                      </button>
                      <button
                        type="button"
                        onClick={() => setMarkdownViewMode("source")}
                        className="rounded-[var(--radius-sm)] px-2 py-0.5 text-[10px] text-[var(--chat-faint)] transition-colors hover:text-[var(--chat-prose)]"
                        title={intl.formatMessage({ id: "docDetail.sourceShortcut" })}
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
                          title={intl.formatMessage({ id: "docDetail.previewShortcut" })}
                        >
                          {intl.formatMessage({ id: "docDetail.preview" })}
                        </button>
                        <button
                          type="button"
                          onClick={() => setMarkdownViewMode("source")}
                          className="rounded-[var(--radius-sm)] px-2 py-0.5 text-[10px] text-[var(--chat-prose)] bg-[var(--accent-soft)]"
                          title={intl.formatMessage({ id: "docDetail.sourceShortcut" })}
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
                <div ref={editorScrollRef} className="detail-code-editor-layer thin-scrollbar">
                  <div className="detail-code-editor-surface">
                    <div className="detail-code-highlight-layer" aria-hidden="true">
                      <pre className="detail-code-highlight-pre">
                        <code
                          className={`hljs language-${languageLabel}`}
                          // highlight.js 输出与原文字符一一对应，仅插入颜色 span。
                          dangerouslySetInnerHTML={{
                            __html: highlightedCodeHtml || "\n",
                          }}
                        />
                      </pre>
                    </div>
                    <textarea
                      ref={textareaRef}
                      value={draftContent}
                      onChange={(event) => {
                        setDraftContent(normalizeEditorText(event.target.value));
                      }}
                      onMouseUp={syncSelection}
                      onKeyUp={syncSelection}
                      onSelect={syncSelection}
                      spellCheck={false}
                      wrap="off"
                      className="detail-code-textarea"
                    />
                  </div>
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
