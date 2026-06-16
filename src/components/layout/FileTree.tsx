import {
  IconCode,
  IconChevronDown,
  IconChevronRight,
  IconEye,
  IconFile,
  IconFileCode,
  IconFileText,
  IconFileTypeCss,
  IconFileTypeHtml,
  IconFileTypeJsx,
  IconFileTypeTs,
  IconFolder,
  IconFolderOpen,
  IconJson,
  IconMarkdown,
  IconMessagePlus,
  IconPhoto,
  IconX,
} from "@tabler/icons-react";
import { useCallback, useEffect, useMemo, useRef, useState, type DragEvent } from "react";
import { useIntl } from "react-intl";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { readDirectory, readFileForAttach, readTextFilePreview, type FileEntry, type TextFilePreviewResult } from "../../api/window";
import { revealInExplorer } from "../../api/window";
import { useAppStore } from "../../stores/appStore";
import { ContextMenu, type ContextMenuEntry } from "../common/ContextMenu";

interface TreeNode extends FileEntry {
  children?: TreeNode[];
  loaded: boolean;
  expanded: boolean;
}

function fileIcon(name: string, size: number) {
  const ext = name.split(".").pop()?.toLowerCase() ?? "";
  const iconProps = { size, stroke: 1.5 };
  switch (ext) {
    case "ts":
    case "tsx":
      return <IconFileTypeTs {...iconProps} className="text-blue-400" />;
    case "js":
    case "jsx":
    case "mjs":
    case "cjs":
      return <IconFileTypeJsx {...iconProps} className="text-yellow-400" />;
    case "css":
    case "scss":
    case "less":
      return <IconFileTypeCss {...iconProps} className="text-purple-400" />;
    case "html":
    case "htm":
      return <IconFileTypeHtml {...iconProps} className="text-orange-400" />;
    case "json":
    case "jsonc":
      return <IconJson {...iconProps} className="text-yellow-300" />;
    case "md":
    case "mdx":
      return <IconMarkdown {...iconProps} className="text-gray-400" />;
    case "rs":
      return <IconFileCode {...iconProps} className="text-orange-300" />;
    case "py":
      return <IconFileCode {...iconProps} className="text-green-400" />;
    case "go":
      return <IconFileCode {...iconProps} className="text-cyan-400" />;
    case "toml":
    case "yaml":
    case "yml":
      return <IconFileText {...iconProps} className="text-gray-400" />;
    case "png":
    case "jpg":
    case "jpeg":
    case "gif":
    case "svg":
    case "webp":
    case "ico":
      return <IconPhoto {...iconProps} className="text-pink-400" />;
    case "txt":
    case "log":
      return <IconFileText {...iconProps} className="text-gray-400" />;
    default:
      return <IconFile {...iconProps} className="text-[var(--text-faint)]" />;
  }
}

interface FileTreeProps {
  rootPath: string | null;
  refreshKey?: number;
}

type PreviewMode = "preview" | "code";

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

function isMarkdownFile(name: string, mimeType: string): boolean {
  const lower = name.toLowerCase();
  return mimeType === "text/markdown" || lower.endsWith(".md") || lower.endsWith(".mdx");
}

export function FileTree({ rootPath, refreshKey }: FileTreeProps) {
  const intl = useIntl();
  const [nodes, setNodes] = useState<TreeNode[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [contextMenu, setContextMenu] = useState<{
    x: number;
    y: number;
    node: TreeNode;
  } | null>(null);
  const [preview, setPreview] = useState<TextFilePreviewResult | null>(null);
  const [previewLoading, setPreviewLoading] = useState(false);
  const [previewError, setPreviewError] = useState<string | null>(null);
  const [previewMode, setPreviewMode] = useState<PreviewMode>("code");
  const containerRef = useRef<HTMLDivElement>(null);

  const loadChildren = useCallback(async (path: string): Promise<TreeNode[]> => {
    const entries = await readDirectory(path);
    return entries.map((entry) => ({
      ...entry,
      children: entry.isDir ? [] : undefined,
      loaded: false,
      expanded: false,
    }));
  }, []);

  useEffect(() => {
    if (!rootPath) {
      setNodes([]);
      return;
    }
    let cancelled = false;
    setLoading(true);
    setError(null);
    loadChildren(rootPath)
      .then((children) => {
        if (!cancelled) {
          setNodes(children);
          setLoading(false);
        }
      })
      .catch((err) => {
        if (!cancelled) {
          setError(String(err));
          setLoading(false);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [rootPath, refreshKey, loadChildren]);

  const toggleExpand = useCallback(
    async (nodePath: string) => {
      const updateNodes = (items: TreeNode[]): TreeNode[] =>
        items.map((node) => {
          if (node.path === nodePath) {
            return { ...node, expanded: !node.expanded };
          }
          if (node.children) {
            return { ...node, children: updateNodes(node.children) };
          }
          return node;
        });

      const findNode = (items: TreeNode[]): TreeNode | null => {
        for (const node of items) {
          if (node.path === nodePath) return node;
          if (node.children) {
            const found = findNode(node.children);
            if (found) return found;
          }
        }
        return null;
      };

      const target = findNode(nodes);
      if (!target || !target.isDir) return;

      if (!target.loaded) {
        try {
          const children = await loadChildren(nodePath);
          setNodes((prev) => {
            const update = (items: TreeNode[]): TreeNode[] =>
              items.map((n) => {
                if (n.path === nodePath) {
                  return { ...n, children, loaded: true, expanded: true };
                }
                if (n.children) {
                  return { ...n, children: update(n.children) };
                }
                return n;
              });
            return update(prev);
          });
        } catch {
          // silently ignore load errors for subdirectories
        }
      } else {
        setNodes((prev) => updateNodes(prev));
      }
    },
    [nodes, loadChildren],
  );

  const handleContextMenu = useCallback(
    (e: React.MouseEvent, node: TreeNode) => {
      e.preventDefault();
      e.stopPropagation();
      setContextMenu({ x: e.clientX, y: e.clientY, node });
    },
    [],
  );

  const contextMenuItems: ContextMenuEntry[] = contextMenu
    ? [
        ...(!contextMenu.node.isDir
          ? [
              {
                id: "add-to-chat",
                label: intl.formatMessage({ id: "fileTree.addToChat" }),
                icon: <IconMessagePlus size={14} stroke={1.8} />,
                onClick: () => {
                  void readFileForAttach(contextMenu.node.path).then((result) => {
                    useAppStore.getState().addAttachedFile({
                      name: result.name,
                      type: result.mimeType,
                      dataUrl: result.dataUrl,
                      size: result.size,
                      sourcePath: result.sourcePath,
                    });
                  });
                },
              } satisfies ContextMenuEntry,
            ]
          : []),
        {
          id: "reveal",
          label: intl.formatMessage({ id: "contextMenu.openInExplorer" }),
          icon: <IconFolderOpen size={14} stroke={1.8} />,
          onClick: () => void revealInExplorer(contextMenu.node.path),
        },
      ]
    : [];

  const handleDragStart = useCallback((e: DragEvent, node: TreeNode) => {
    if (node.isDir) {
      e.preventDefault();
      return;
    }
    e.dataTransfer.setData(
      "application/x-cn-codex-file",
      JSON.stringify({ path: node.path, name: node.name }),
    );
    e.dataTransfer.effectAllowed = "copy";
  }, []);

  const handlePreviewFile = useCallback(async (node: TreeNode) => {
    if (node.isDir) return;
    setPreviewLoading(true);
    setPreviewError(null);
    try {
      const result = await readTextFilePreview(node.path);
      setPreview(result);
      setPreviewMode(isMarkdownFile(result.name, result.mimeType) ? "preview" : "code");
    } catch (err) {
      setPreview(null);
      setPreviewError(String(err));
    } finally {
      setPreviewLoading(false);
    }
  }, []);

  const markdownPreview = useMemo(() => {
    if (!preview || !isMarkdownFile(preview.name, preview.mimeType)) return null;
    return (
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
        components={{
          code(props) {
            const { children, className } = props;
            return (
              <code className={`chat-inline-code ${className ?? ""}`.trim()}>
                {children}
              </code>
            );
          },
          pre(props) {
            return (
              <pre className="thin-scrollbar overflow-x-auto rounded-[var(--radius-sm)] border border-[var(--chat-line)] bg-[var(--chat-paper)] p-3 text-[12px]">
                {props.children}
              </pre>
            );
          },
          table(props) {
            return (
              <div className="thin-scrollbar overflow-x-auto">
                <table className="chat-md-table">{props.children}</table>
              </div>
            );
          },
        }}
      >
        {preview.content}
      </ReactMarkdown>
    );
  }, [preview]);

  if (!rootPath) {
    return (
      <div className="flex flex-1 items-center justify-center p-4 text-xs text-[var(--text-faint)]">
        {intl.formatMessage({ id: "fileTree.noProject" })}
      </div>
    );
  }

  if (loading) {
    return (
      <div className="flex flex-1 items-center justify-center p-4 text-xs text-[var(--text-faint)]">
        {intl.formatMessage({ id: "fileTree.loading" })}
      </div>
    );
  }

  if (error) {
    return (
      <div className="flex flex-1 items-center justify-center p-4 text-xs text-[var(--danger)]">
        {error}
      </div>
    );
  }

  return (
    <div ref={containerRef} className="flex-1 overflow-y-auto overflow-x-hidden">
      {nodes.length === 0 ? (
        <div className="p-4 text-center text-xs text-[var(--text-faint)]">
          {intl.formatMessage({ id: "fileTree.emptyDir" })}
        </div>
      ) : (
        <div className="py-1">
          {nodes.map((node) => (
            <FileTreeNode
              key={node.path}
              node={node}
              depth={0}
              onToggle={toggleExpand}
              onOpenFile={handlePreviewFile}
              onContextMenu={handleContextMenu}
              onDragStart={handleDragStart}
            />
          ))}
        </div>
      )}
      {contextMenu && (
        <ContextMenu
          position={{ x: contextMenu.x, y: contextMenu.y }}
          items={contextMenuItems}
          onClose={() => setContextMenu(null)}
        />
      )}
      {(preview || previewLoading || previewError) && (
        <div className="absolute inset-0 z-20 flex items-center justify-center bg-black/45 p-4">
          <div className="flex h-full max-h-[90vh] w-full max-w-5xl flex-col overflow-hidden rounded-[var(--radius-lg)] border border-[var(--border-strong)] bg-[var(--surface-panel)] shadow-[var(--shadow-strong)]">
            <div className="flex items-center gap-3 border-b border-[var(--border-subtle)] px-4 py-3">
              <div className="min-w-0 flex-1">
                <div className="truncate text-sm font-semibold text-[var(--text-strong)]">
                  {preview?.name ?? intl.formatMessage({ id: "common.loading" })}
                </div>
                <div className="truncate font-mono text-[11px] text-[var(--text-faint)]">
                  {preview?.path ?? ""}
                </div>
              </div>
              {preview && isMarkdownFile(preview.name, preview.mimeType) && (
                <div className="flex items-center gap-1 rounded-[var(--radius-sm)] border border-[var(--border-subtle)] bg-[var(--surface-soft)] p-1">
                  <button
                    type="button"
                    onClick={() => setPreviewMode("preview")}
                    className={`flex items-center gap-1 rounded-[var(--radius-sm)] px-2 py-1 text-xs ${
                      previewMode === "preview"
                        ? "bg-[var(--accent-soft)] text-[var(--accent-strong)]"
                        : "text-[var(--text-muted)] hover:bg-[var(--surface-elevated)] hover:text-[var(--text-strong)]"
                    }`}
                  >
                    <IconEye size={13} stroke={1.8} />
                    {intl.formatMessage({ id: "fileTree.preview" })}
                  </button>
                  <button
                    type="button"
                    onClick={() => setPreviewMode("code")}
                    className={`flex items-center gap-1 rounded-[var(--radius-sm)] px-2 py-1 text-xs ${
                      previewMode === "code"
                        ? "bg-[var(--accent-soft)] text-[var(--accent-strong)]"
                        : "text-[var(--text-muted)] hover:bg-[var(--surface-elevated)] hover:text-[var(--text-strong)]"
                    }`}
                  >
                    <IconCode size={13} stroke={1.8} />
                    {intl.formatMessage({ id: "fileTree.code" })}
                  </button>
                </div>
              )}
              <button
                type="button"
                onClick={() => {
                  setPreview(null);
                  setPreviewError(null);
                  setPreviewLoading(false);
                }}
                className="flex h-8 w-8 items-center justify-center rounded-[var(--radius-sm)] text-[var(--text-faint)] transition-colors hover:bg-[var(--surface-elevated)] hover:text-[var(--text-strong)]"
                title={intl.formatMessage({ id: "common.close" })}
              >
                <IconX size={16} stroke={1.8} />
              </button>
            </div>
            <div className="min-h-0 flex-1 overflow-hidden">
              {previewLoading ? (
                <div className="flex h-full items-center justify-center p-6 text-sm text-[var(--text-faint)]">
                  {intl.formatMessage({ id: "common.loading" })}
                </div>
              ) : previewError ? (
                <div className="thin-scrollbar h-full overflow-auto p-6 text-sm text-[var(--danger)]">
                  {previewError}
                </div>
              ) : preview ? (
                <div className="thin-scrollbar h-full overflow-auto p-5">
                  {preview.truncated && (
                      <div className="mb-4 rounded-[var(--radius-sm)] border border-[var(--accent-border)] bg-[var(--accent-soft)] px-3 py-2 text-xs text-[var(--text-muted)]">
                      {intl.formatMessage({ id: "fileTree.previewTruncated" })}
                    </div>
                  )}
                  {previewMode === "preview" && markdownPreview ? (
                    <article className="chat-prose max-w-none">
                      {markdownPreview}
                    </article>
                  ) : (
                    <div className="chat-code-block overflow-hidden">
                      <div className="flex items-center justify-between border-b border-[var(--chat-line)] px-3 py-1.5">
                        <span className="text-[11px] text-[var(--chat-faint)]">
                          {fileLanguage(preview.name)}
                        </span>
                      </div>
                      <pre className="thin-scrollbar max-h-[70vh] overflow-auto px-3 py-3 text-[13px] leading-relaxed">
                        <code className="text-[var(--chat-prose)]">{preview.content}</code>
                      </pre>
                    </div>
                  )}
                </div>
              ) : null}
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

interface FileTreeNodeProps {
  node: TreeNode;
  depth: number;
  onToggle: (path: string) => void;
  onOpenFile: (node: TreeNode) => void;
  onContextMenu: (e: React.MouseEvent, node: TreeNode) => void;
  onDragStart: (e: DragEvent, node: TreeNode) => void;
}

function FileTreeNode({ node, depth, onToggle, onOpenFile, onContextMenu, onDragStart }: FileTreeNodeProps) {
  const paddingLeft = 8 + depth * 16;

  return (
    <>
      <div
        className="group flex cursor-pointer items-center gap-1 py-[3px] pr-2 text-[12px] transition-colors hover:bg-[var(--surface-elevated)]"
        style={{ paddingLeft }}
        onClick={() => {
          if (node.isDir) {
            onToggle(node.path);
          } else {
            onOpenFile(node);
          }
        }}
        onContextMenu={(e) => onContextMenu(e, node)}
        draggable={!node.isDir}
        onDragStart={(e) => onDragStart(e, node)}
        title={node.path}
      >
        {node.isDir ? (
          <>
            <span className="flex h-4 w-4 flex-shrink-0 items-center justify-center text-[var(--text-faint)]">
              {node.expanded ? (
                <IconChevronDown size={12} stroke={2} />
              ) : (
                <IconChevronRight size={12} stroke={2} />
              )}
            </span>
            <span className="flex-shrink-0">
              {node.expanded ? (
                <IconFolderOpen size={15} stroke={1.5} className="text-[var(--accent)]" />
              ) : (
                <IconFolder size={15} stroke={1.5} className="text-[var(--accent)]" />
              )}
            </span>
          </>
        ) : (
          <>
            <span className="h-4 w-4 flex-shrink-0" />
            <span className="flex-shrink-0">{fileIcon(node.name, 15)}</span>
          </>
        )}
        <span className="min-w-0 truncate text-[var(--text-base)]">{node.name}</span>
      </div>
      {node.isDir && node.expanded && node.children && (
        <>
          {node.children.map((child) => (
            <FileTreeNode
              key={child.path}
              node={child}
              depth={depth + 1}
              onToggle={onToggle}
              onOpenFile={onOpenFile}
              onContextMenu={onContextMenu}
              onDragStart={onDragStart}
            />
          ))}
        </>
      )}
    </>
  );
}
