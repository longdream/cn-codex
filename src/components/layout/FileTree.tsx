import {
  IconBrowser,
  IconChevronDown,
  IconChevronRight,
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
  IconTrash,
} from "@tabler/icons-react";
import { useCallback, useEffect, useState, type DragEvent } from "react";
import { useIntl } from "react-intl";
import {
  deletePath,
  readTextFilePreview,
  readDirectory,
  revealInExplorer,
  type FileEntry,
  windowNavigateBrowser,
  windowOpenDocumentDetail,
} from "../../api/window";
import { useAppStore } from "../../stores/appStore";
import { ContextMenu, type ContextMenuEntry } from "../common/ContextMenu";
import { PATH_REF_MIME, supportsPathRefRangeByName } from "../../utils/pathRefSnippet";

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

function isWebPreviewFile(name: string): boolean {
  const ext = name.split(".").pop()?.toLowerCase() ?? "";
  return ext === "html" || ext === "htm";
}

function buildPathRefAttachment(
  path: string,
  name?: string,
  lineStart?: number,
  lineEnd?: number,
) {
  const sourcePath = path.trim();
  const fallbackName = sourcePath.split(/[\\/]/).filter(Boolean).pop() ?? "file";
  const normalizedName = (name ?? "").trim();
  const safeLineStart = typeof lineStart === "number" && Number.isFinite(lineStart)
    ? Math.max(1, Math.floor(lineStart))
    : undefined;
  const safeLineEnd = typeof lineEnd === "number" && Number.isFinite(lineEnd)
    ? Math.max(safeLineStart ?? 1, Math.floor(lineEnd))
    : undefined;
  return {
    kind: "pathRef" as const,
    name: normalizedName || fallbackName,
    type: PATH_REF_MIME,
    size: 0,
    sourcePath,
    ...(safeLineStart && safeLineEnd ? { lineStart: safeLineStart, lineEnd: safeLineEnd } : {}),
  };
}

function resolveLineCount(content: string): number {
  // 统一 \r\n/\r/\n 计数规则，确保标签中的行号范围与编辑器表现一致。
  const normalized = content.replace(/\r\n?/g, "\n");
  if (!normalized) {
    return 1;
  }
  return normalized.split("\n").length;
}

interface FileTreeProps {
  rootPath: string | null;
  refreshKey?: number;
}

export function FileTree({ rootPath, refreshKey }: FileTreeProps) {
  const intl = useIntl();
  const setRightPanelTab = useAppStore((state) => state.setRightPanelTab);
  const setBrowserPanelState = useAppStore((state) => state.setBrowserPanelState);
  const addAttachedFile = useAppStore((state) => state.addAttachedFile);
  const [nodes, setNodes] = useState<TreeNode[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [contextMenu, setContextMenu] = useState<{
    x: number;
    y: number;
    node: TreeNode;
  } | null>(null);
  const [detailOpenError, setDetailOpenError] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);

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
    // 切换项目目录时清空“打开详情窗失败”提示，避免旧错误误导用户。
    setDetailOpenError(null);
    setActionError(null);

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
          // 子目录读失败仅影响该节点，不阻断整个文件树。
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

  const removeNode = useCallback((nodePath: string) => {
    const prune = (items: TreeNode[]): TreeNode[] =>
      items
        .filter((node) => node.path !== nodePath)
        .map((node) => ({
          ...node,
          children: node.children ? prune(node.children) : node.children,
        }));
    setNodes((prev) => prune(prev));
  }, []);

  const handleDeleteNode = useCallback(async (node: TreeNode) => {
    const confirmed = node.isDir
      ? window.confirm(
        intl.formatMessage(
          { id: "fileTree.confirmDeleteFolder" },
          { name: node.name },
        ),
      )
      : window.confirm(
        intl.formatMessage(
          { id: "fileTree.confirmDeleteFile" },
          { name: node.name },
        ),
      );

    if (!confirmed) {
      return;
    }

    setActionError(null);
    try {
      await deletePath(node.path, node.isDir);
      removeNode(node.path);
    } catch (err) {
      setActionError(String(err));
    }
  }, [intl, removeNode]);

  const contextMenuItems: ContextMenuEntry[] = contextMenu
    ? [
        ...(!contextMenu.node.isDir
          ? [
              {
                id: "add-to-chat",
                label: intl.formatMessage({ id: "fileTree.addToChat" }),
                icon: <IconMessagePlus size={14} stroke={1.8} />,
                onClick: () => {
                  const targetNode = contextMenu.node;
                  if (!supportsPathRefRangeByName(targetNode.name)) {
                    addAttachedFile(buildPathRefAttachment(targetNode.path, targetNode.name));
                    return;
                  }
                  // txt/md 走“路径 + 行号范围”标签，不把正文内容塞进输入框。
                  void readTextFilePreview(targetNode.path)
                    .then((preview) => {
                      if (preview.truncated) {
                        addAttachedFile(buildPathRefAttachment(targetNode.path, targetNode.name));
                        return;
                      }
                      const lineCount = resolveLineCount(preview.content);
                      addAttachedFile(
                        buildPathRefAttachment(targetNode.path, targetNode.name, 1, lineCount),
                      );
                    })
                    .catch(() => {
                      addAttachedFile(buildPathRefAttachment(targetNode.path, targetNode.name));
                    });
                },
              } satisfies ContextMenuEntry,
              ...(isWebPreviewFile(contextMenu.node.name)
                ? [
                    {
                      id: "open-in-browser",
                      label: intl.formatMessage({ id: "fileTree.openInBrowser" }),
                      icon: <IconBrowser size={14} stroke={1.8} />,
                      onClick: () => {
                        const filePath = contextMenu.node.path;
                        setRightPanelTab("browser");
                        setBrowserPanelState({
                          url: filePath,
                          status: "success",
                        });
                        void windowNavigateBrowser(filePath, rootPath ?? undefined);
                      },
                    } satisfies ContextMenuEntry,
                  ]
                : []),
            ]
          : []),
        {
          id: "reveal",
          label: intl.formatMessage({ id: "contextMenu.openInExplorer" }),
          icon: <IconFolderOpen size={14} stroke={1.8} />,
          onClick: () => void revealInExplorer(contextMenu.node.path),
        },
        {
          id: "delete",
          label: contextMenu.node.isDir
            ? intl.formatMessage({ id: "fileTree.deleteFolder" })
            : intl.formatMessage({ id: "fileTree.deleteFile" }),
          icon: <IconTrash size={14} stroke={1.8} />,
          danger: true,
          onClick: () => {
            void handleDeleteNode(contextMenu.node);
          },
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

  const handleOpenFile = useCallback(async (node: TreeNode) => {
    if (node.isDir) return;
    setDetailOpenError(null);
    try {
      // 文档详情窗由后端做“单实例复用”，这里仅传文件路径作为打开入口。
      await windowOpenDocumentDetail(node.path, rootPath ?? undefined);
    } catch (err) {
      setDetailOpenError(String(err));
    }
  }, []);

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
    <div className="relative flex-1 overflow-y-auto overflow-x-hidden">
      {detailOpenError && (
        <div className="mx-2 mt-2 rounded-[var(--radius-sm)] border border-[var(--danger)]/35 bg-[var(--danger-soft)] px-2 py-1 text-[11px] text-[var(--danger)]">
          打开文档详情窗失败：{detailOpenError}
        </div>
      )}
      {actionError && (
        <div className="mx-2 mt-2 rounded-[var(--radius-sm)] border border-[var(--danger)]/35 bg-[var(--danger-soft)] px-2 py-1 text-[11px] text-[var(--danger)]">
          {intl.formatMessage({ id: "fileTree.deleteFailed" }, { error: actionError })}
        </div>
      )}
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
              onOpenFile={handleOpenFile}
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

function FileTreeNode({
  node,
  depth,
  onToggle,
  onOpenFile,
  onContextMenu,
  onDragStart,
}: FileTreeNodeProps) {
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
