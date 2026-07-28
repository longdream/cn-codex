import {
  IconBrowser,
  IconClipboard,
  IconCopy,
  IconCut,
  IconChevronDown,
  IconChevronRight,
  IconFile,
  IconFilePlus,
  IconFileCode,
  IconFileText,
  IconFileTypeCss,
  IconFileTypeHtml,
  IconFileTypeJsx,
  IconFileTypeTs,
  IconFolder,
  IconFolderPlus,
  IconFolderOpen,
  IconJson,
  IconMarkdown,
  IconMessagePlus,
  IconPencil,
  IconPhoto,
  IconSearch,
  IconTrash,
  IconX,
} from "@tabler/icons-react";
import { useCallback, useEffect, useMemo, useState, type DragEvent } from "react";
import { useIntl } from "react-intl";
import {
  copyPathEntry,
  createPathEntry,
  deletePath,
  readTextFilePreview,
  readDirectory,
  renamePathEntry,
  revealInExplorer,
  searchWorkspaceFiles,
  type FileEntry,
  type WorkspaceSearchMatch,
  windowNavigateBrowser,
  windowOpenDocumentDetail,
} from "../../api/window";
import { useAppStore } from "../../stores/appStore";
import { ContextMenu, type ContextMenuEntry } from "../common/ContextMenu";
import { PATH_REF_MIME, supportsPathRefRangeByName } from "../../utils/pathRefSnippet";
import { getParentPath, joinPath, pathsEqual, uniqueNameInDirectory } from "../../utils/fileTreeSelection";

interface TreeNode extends FileEntry {
  children?: TreeNode[];
  loaded: boolean;
  expanded: boolean;
}

interface FileTreeClipboard {
  node: Pick<TreeNode, "path" | "name" | "isDir">;
  operation: "copy" | "cut";
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
    node?: TreeNode;
    targetDir: string;
  } | null>(null);
  const [clipboard, setClipboard] = useState<FileTreeClipboard | null>(null);
  const [detailOpenError, setDetailOpenError] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const [searchQuery, setSearchQuery] = useState("");
  const [debouncedQuery, setDebouncedQuery] = useState("");
  const [includeQuery, setIncludeQuery] = useState("");
  const [debouncedInclude, setDebouncedInclude] = useState("");
  const [caseSensitive, setCaseSensitive] = useState(false);
  const [collapsedContentGroups, setCollapsedContentGroups] = useState<Record<string, boolean>>({});
  const [searching, setSearching] = useState(false);
  const [searchError, setSearchError] = useState<string | null>(null);
  const [searchMatches, setSearchMatches] = useState<WorkspaceSearchMatch[]>([]);
  const [searchTruncated, setSearchTruncated] = useState(false);

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

  useEffect(() => {
    const timer = window.setTimeout(() => {
      setDebouncedQuery(searchQuery.trim());
    }, 280);
    return () => {
      window.clearTimeout(timer);
    };
  }, [searchQuery]);

  useEffect(() => {
    const timer = window.setTimeout(() => {
      setDebouncedInclude(includeQuery.trim());
    }, 280);
    return () => {
      window.clearTimeout(timer);
    };
  }, [includeQuery]);

  useEffect(() => {
    if (!rootPath) {
      setSearchMatches([]);
      setSearchError(null);
      setSearching(false);
      setSearchTruncated(false);
      return;
    }
    if (!debouncedQuery) {
      setSearchMatches([]);
      setSearchError(null);
      setSearching(false);
      setSearchTruncated(false);
      return;
    }

    let cancelled = false;
    setSearching(true);
    setSearchError(null);

    void searchWorkspaceFiles(rootPath, debouncedQuery, 120, {
      caseSensitive,
      include: debouncedInclude,
    })
      .then((result) => {
        if (cancelled) return;
        setSearchMatches(result.matches);
        setSearchTruncated(result.truncated);
        setSearching(false);
      })
      .catch((err) => {
        if (cancelled) return;
        setSearchMatches([]);
        setSearchTruncated(false);
        setSearchError(String(err));
        setSearching(false);
      });

    return () => {
      cancelled = true;
    };
  }, [rootPath, debouncedQuery, debouncedInclude, caseSensitive, refreshKey]);

  const nameMatches = useMemo(
    () => searchMatches.filter((item) => item.kind === "name"),
    [searchMatches],
  );
  const contentMatches = useMemo(
    () => searchMatches.filter((item) => item.kind === "content"),
    [searchMatches],
  );
  const contentGroups = useMemo(() => {
    const groups = new Map<
      string,
      {
        path: string;
        name: string;
        relativePath: string;
        matches: WorkspaceSearchMatch[];
      }
    >();
    for (const item of contentMatches) {
      const existing = groups.get(item.path);
      if (existing) {
        existing.matches.push(item);
      } else {
        groups.set(item.path, {
          path: item.path,
          name: item.name,
          relativePath: item.relativePath,
          matches: [item],
        });
      }
    }
    return Array.from(groups.values());
  }, [contentMatches]);
  const isSearchMode = debouncedQuery.length > 0;

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
      setContextMenu({
        x: e.clientX,
        y: e.clientY,
        node,
        targetDir: node.isDir ? node.path : getParentPath(node.path),
      });
    },
    [],
  );

  const handleWorkspaceContextMenu = useCallback((e: React.MouseEvent) => {
    e.preventDefault();
    setContextMenu({ x: e.clientX, y: e.clientY, targetDir: rootPath ?? "" });
  }, [rootPath]);

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

  const refreshTree = useCallback(async () => {
    if (!rootPath) return;
    setActionError(null);
    try {
      setNodes(await loadChildren(rootPath));
    } catch (err) {
      setActionError(String(err));
    }
  }, [loadChildren, rootPath]);

  const handleCreateEntry = useCallback(async (parentDir: string, isDir: boolean) => {
    const name = window.prompt(
      intl.formatMessage({ id: isDir ? "fileTree.newFolderPrompt" : "fileTree.newFilePrompt" }),
      isDir ? "new-folder" : "untitled",
    )?.trim();
    if (!name) return;

    setActionError(null);
    try {
      await createPathEntry(parentDir, name, isDir);
      await refreshTree();
    } catch (err) {
      setActionError(String(err));
    }
  }, [intl, refreshTree]);

  const handleRenameNode = useCallback(async (node: TreeNode) => {
    const name = window.prompt(
      intl.formatMessage({ id: "fileTree.renamePrompt" }, { name: node.name }),
      node.name,
    )?.trim();
    if (!name || name === node.name) return;

    setActionError(null);
    try {
      await renamePathEntry(node.path, joinPath(getParentPath(node.path), name));
      await refreshTree();
    } catch (err) {
      setActionError(String(err));
    }
  }, [intl, refreshTree]);

  const handlePaste = useCallback(async (targetDir: string) => {
    if (!clipboard) return;
    if (clipboard.operation === "cut" && pathsEqual(getParentPath(clipboard.node.path), targetDir)) {
      return;
    }

    setActionError(null);
    try {
      const entries = await readDirectory(targetDir);
      const targetName = clipboard.operation === "copy"
        ? uniqueNameInDirectory(entries.map((entry) => entry.name), clipboard.node.name)
        : clipboard.node.name;
      const targetPath = joinPath(targetDir, targetName);
      if (clipboard.operation === "copy") {
        await copyPathEntry(clipboard.node.path, targetPath);
      } else {
        await renamePathEntry(clipboard.node.path, targetPath);
        setClipboard(null);
      }
      await refreshTree();
    } catch (err) {
      setActionError(String(err));
    }
  }, [clipboard, refreshTree]);

  const contextMenuNode = contextMenu?.node;
  const contextMenuItems: ContextMenuEntry[] = contextMenu
    ? [
        ...(contextMenuNode && !contextMenuNode.isDir
          ? [
              {
                id: "add-to-chat",
                label: intl.formatMessage({ id: "fileTree.addToChat" }),
                icon: <IconMessagePlus size={14} stroke={1.8} />,
                onClick: () => {
                  const targetNode = contextMenuNode;
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
              ...(isWebPreviewFile(contextMenuNode.name)
                ? [
                    {
                      id: "open-in-browser",
                      label: intl.formatMessage({ id: "fileTree.openInBrowser" }),
                      icon: <IconBrowser size={14} stroke={1.8} />,
                      onClick: () => {
                        const filePath = contextMenuNode.path;
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
          id: "new-file",
          label: intl.formatMessage({ id: "fileTree.newFile" }),
          icon: <IconFilePlus size={14} stroke={1.8} />,
          onClick: () => void handleCreateEntry(contextMenu.targetDir, false),
        },
        {
          id: "new-folder",
          label: intl.formatMessage({ id: "fileTree.newFolder" }),
          icon: <IconFolderPlus size={14} stroke={1.8} />,
          onClick: () => void handleCreateEntry(contextMenu.targetDir, true),
        },
        { id: "create-divider", divider: true },
        ...(contextMenuNode
          ? [
              {
                id: "rename",
                label: intl.formatMessage({ id: "fileTree.rename" }),
                icon: <IconPencil size={14} stroke={1.8} />,
                onClick: () => void handleRenameNode(contextMenuNode),
              } satisfies ContextMenuEntry,
              {
                id: "copy",
                label: intl.formatMessage({ id: "fileTree.copy" }),
                icon: <IconCopy size={14} stroke={1.8} />,
                onClick: () => setClipboard({ node: contextMenuNode, operation: "copy" }),
              } satisfies ContextMenuEntry,
              {
                id: "cut",
                label: intl.formatMessage({ id: "fileTree.cut" }),
                icon: <IconCut size={14} stroke={1.8} />,
                onClick: () => setClipboard({ node: contextMenuNode, operation: "cut" }),
              } satisfies ContextMenuEntry,
            ]
          : []),
        {
          id: "paste",
          label: intl.formatMessage({ id: "fileTree.paste" }),
          icon: <IconClipboard size={14} stroke={1.8} />,
          disabled: !clipboard || (clipboard.operation === "cut" && pathsEqual(getParentPath(clipboard.node.path), contextMenu.targetDir)),
          onClick: () => void handlePaste(contextMenu.targetDir),
        },
        ...(contextMenuNode
          ? [
              { id: "file-divider", divider: true } satisfies ContextMenuEntry,
              {
                id: "reveal",
                label: intl.formatMessage({ id: "contextMenu.openInExplorer" }),
                icon: <IconFolderOpen size={14} stroke={1.8} />,
                onClick: () => void revealInExplorer(contextMenuNode.path),
              } satisfies ContextMenuEntry,
              {
                id: "delete",
                label: contextMenuNode.isDir
                  ? intl.formatMessage({ id: "fileTree.deleteFolder" })
                  : intl.formatMessage({ id: "fileTree.deleteFile" }),
                icon: <IconTrash size={14} stroke={1.8} />,
                danger: true,
                onClick: () => {
                  void handleDeleteNode(contextMenuNode);
                },
              } satisfies ContextMenuEntry,
            ]
          : []),
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

  const handleOpenFile = useCallback(
    async (node: TreeNode, line?: number | null) => {
      if (node.isDir) return;
      setDetailOpenError(null);
      try {
        // 文档详情窗由后端做“单实例复用”；内容搜索可额外带上目标行号。
        await windowOpenDocumentDetail(node.path, rootPath ?? undefined, line);
      } catch (err) {
        setDetailOpenError(String(err));
      }
    },
    [rootPath],
  );

  if (!rootPath) {
    return (
      <div className="flex flex-1 items-center justify-center p-4 text-xs text-[var(--text-faint)]">
        {intl.formatMessage({ id: "fileTree.noProject" })}
      </div>
    );
  }

  if (loading && !isSearchMode) {
    return (
      <div className="flex flex-1 items-center justify-center p-4 text-xs text-[var(--text-faint)]">
        {intl.formatMessage({ id: "fileTree.loading" })}
      </div>
    );
  }

  if (error && !isSearchMode) {
    return (
      <div className="flex flex-1 items-center justify-center p-4 text-xs text-[var(--danger)]">
        {error}
      </div>
    );
  }

  return (
    <div className="relative flex flex-1 flex-col overflow-hidden">
      <div className="border-b border-[var(--border-subtle)] px-2 py-2">
        <div className="relative">
          <IconSearch
            size={13}
            stroke={1.8}
            className="pointer-events-none absolute left-2 top-1/2 -translate-y-1/2 text-[var(--text-faint)]"
          />
          <input
            type="text"
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            placeholder={intl.formatMessage({ id: "fileTree.searchPlaceholder" })}
            className="h-7 w-full rounded-[var(--radius-sm)] border border-[var(--border-subtle)] bg-[var(--surface-panel)] py-1 pl-7 pr-7 text-[11px] text-[var(--text-base)] outline-none transition-colors placeholder:text-[var(--text-faint)] focus:border-[var(--accent)]"
          />
          {searchQuery && (
            <button
              type="button"
              onClick={() => setSearchQuery("")}
              className="absolute right-1.5 top-1/2 flex h-5 w-5 -translate-y-1/2 items-center justify-center rounded-[var(--radius-sm)] text-[var(--text-faint)] transition-colors hover:bg-[var(--surface-elevated)] hover:text-[var(--text-strong)]"
              title={intl.formatMessage({ id: "fileTree.searchClear" })}
            >
              <IconX size={12} stroke={1.8} />
            </button>
          )}
        </div>
        <div className="mt-1.5 flex items-center gap-1.5">
          <input
            type="text"
            value={includeQuery}
            onChange={(e) => setIncludeQuery(e.target.value)}
            placeholder={intl.formatMessage({ id: "fileTree.searchIncludePlaceholder" })}
            className="h-6 min-w-0 flex-1 rounded-[var(--radius-sm)] border border-[var(--border-subtle)] bg-[var(--surface-panel)] px-2 text-[10px] text-[var(--text-base)] outline-none transition-colors placeholder:text-[var(--text-faint)] focus:border-[var(--accent)]"
          />
          <button
            type="button"
            onClick={() => setCaseSensitive((value) => !value)}
            className={`flex h-6 min-w-6 items-center justify-center rounded-[var(--radius-sm)] border px-1.5 text-[10px] font-semibold transition-colors ${
              caseSensitive
                ? "border-[var(--accent-border)] bg-[var(--accent-soft)] text-[var(--accent-strong)]"
                : "border-[var(--border-subtle)] text-[var(--text-faint)] hover:bg-[var(--surface-elevated)] hover:text-[var(--text-strong)]"
            }`}
            title={intl.formatMessage({ id: "fileTree.searchCaseSensitive" })}
          >
            Aa
          </button>
        </div>
      </div>

      <div
        className="relative flex-1 overflow-y-auto overflow-x-hidden"
        onContextMenu={handleWorkspaceContextMenu}
      >
        {detailOpenError && (
          <div className="mx-2 mt-2 rounded-[var(--radius-sm)] border border-[var(--danger)]/35 bg-[var(--danger-soft)] px-2 py-1 text-[11px] text-[var(--danger)]">
            {intl.formatMessage({ id: "fileTree.openDetailFailed" }, { error: detailOpenError })}
          </div>
        )}
        {actionError && (
          <div className="mx-2 mt-2 rounded-[var(--radius-sm)] border border-[var(--danger)]/35 bg-[var(--danger-soft)] px-2 py-1 text-[11px] text-[var(--danger)]">
            {intl.formatMessage({ id: "fileTree.actionFailed" }, { error: actionError })}
          </div>
        )}
        {searchError && (
          <div className="mx-2 mt-2 rounded-[var(--radius-sm)] border border-[var(--danger)]/35 bg-[var(--danger-soft)] px-2 py-1 text-[11px] text-[var(--danger)]">
            {intl.formatMessage({ id: "fileTree.searchFailed" }, { error: searchError })}
          </div>
        )}

        {isSearchMode ? (
          searching ? (
            <div className="p-4 text-center text-xs text-[var(--text-faint)]">
              {intl.formatMessage({ id: "fileTree.searching" })}
            </div>
          ) : searchMatches.length === 0 ? (
            <div className="p-4 text-center text-xs text-[var(--text-faint)]">
              {intl.formatMessage({ id: "fileTree.searchEmpty" })}
            </div>
          ) : (
            <div className="py-1">
              {nameMatches.length > 0 && (
                <div className="mb-1">
                  <div className="px-3 py-1 text-[10px] font-semibold uppercase tracking-wide text-[var(--text-faint)]">
                    {intl.formatMessage(
                      { id: "fileTree.searchNameResults" },
                      { count: nameMatches.length },
                    )}
                  </div>
                  {nameMatches.map((item) => (
                    <SearchResultItem
                      key={`name:${item.path}`}
                      item={item}
                      query={debouncedQuery}
                      caseSensitive={caseSensitive}
                      onOpen={() => {
                        if (item.isDir) return;
                        void handleOpenFile({
                          name: item.name,
                          path: item.path,
                          isDir: false,
                          size: 0,
                          loaded: true,
                          expanded: false,
                        });
                      }}
                    />
                  ))}
                </div>
              )}
              {contentGroups.length > 0 && (
                <div>
                  <div className="px-3 py-1 text-[10px] font-semibold uppercase tracking-wide text-[var(--text-faint)]">
                    {intl.formatMessage(
                      { id: "fileTree.searchContentResults" },
                      { count: contentMatches.length, files: contentGroups.length },
                    )}
                  </div>
                  {contentGroups.map((group) => {
                    const collapsed = Boolean(collapsedContentGroups[group.path]);
                    return (
                      <div key={`group:${group.path}`} className="mb-0.5">
                        <button
                          type="button"
                          onClick={() =>
                            setCollapsedContentGroups((prev) => ({
                              ...prev,
                              [group.path]: !prev[group.path],
                            }))
                          }
                          className="flex w-full items-center gap-1 px-3 py-1 text-left transition-colors hover:bg-[var(--surface-elevated)]"
                          title={group.path}
                        >
                          <span className="flex h-4 w-4 flex-shrink-0 items-center justify-center text-[var(--text-faint)]">
                            {collapsed ? (
                              <IconChevronRight size={12} stroke={2} />
                            ) : (
                              <IconChevronDown size={12} stroke={2} />
                            )}
                          </span>
                          <span className="flex-shrink-0">{fileIcon(group.name, 14)}</span>
                          <span className="min-w-0 flex-1 truncate text-[12px] text-[var(--text-base)]">
                            {group.name}
                          </span>
                          <span className="flex-shrink-0 text-[10px] text-[var(--text-faint)]">
                            {group.matches.length}
                          </span>
                        </button>
                        {!collapsed &&
                          group.matches.map((item, index) => (
                            <SearchResultItem
                              key={`content:${item.path}:${item.line ?? 0}:${index}`}
                              item={item}
                              query={debouncedQuery}
                              caseSensitive={caseSensitive}
                              compact
                              onOpen={() => {
                                void handleOpenFile(
                                  {
                                    name: item.name,
                                    path: item.path,
                                    isDir: false,
                                    size: 0,
                                    loaded: true,
                                    expanded: false,
                                  },
                                  item.line,
                                );
                              }}
                            />
                          ))}
                      </div>
                    );
                  })}
                </div>
              )}
              {searchTruncated && (
                <div className="px-3 py-2 text-[10px] text-[var(--text-faint)]">
                  {intl.formatMessage({ id: "fileTree.searchTruncated" })}
                </div>
              )}
            </div>
          )
        ) : nodes.length === 0 ? (
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

function highlightMatchText(
  text: string,
  query: string,
  caseSensitive: boolean,
): Array<{ text: string; matched: boolean }> {
  if (!query) {
    return [{ text, matched: false }];
  }
  const source = caseSensitive ? text : text.toLowerCase();
  const needle = caseSensitive ? query : query.toLowerCase();
  const parts: Array<{ text: string; matched: boolean }> = [];
  let cursor = 0;
  while (cursor < text.length) {
    const found = source.indexOf(needle, cursor);
    if (found < 0) {
      parts.push({ text: text.slice(cursor), matched: false });
      break;
    }
    if (found > cursor) {
      parts.push({ text: text.slice(cursor, found), matched: false });
    }
    parts.push({ text: text.slice(found, found + needle.length), matched: true });
    cursor = found + Math.max(needle.length, 1);
  }
  return parts.length > 0 ? parts : [{ text, matched: false }];
}

function SearchResultItem({
  item,
  onOpen,
  query = "",
  caseSensitive = false,
  compact = false,
}: {
  item: WorkspaceSearchMatch;
  onOpen: () => void;
  query?: string;
  caseSensitive?: boolean;
  compact?: boolean;
}) {
  const previewParts = item.preview
    ? highlightMatchText(item.preview, query, caseSensitive)
    : [];

  return (
    <button
      type="button"
      onClick={onOpen}
      className={`flex w-full flex-col gap-0.5 text-left transition-colors hover:bg-[var(--surface-elevated)] ${
        compact ? "px-3 py-1 pl-8" : "px-3 py-1.5"
      }`}
      title={item.path}
    >
      {!compact && (
        <div className="flex min-w-0 items-center gap-1.5">
          <span className="flex-shrink-0">
            {item.isDir ? (
              <IconFolder size={14} stroke={1.5} className="text-[var(--accent)]" />
            ) : (
              fileIcon(item.name, 14)
            )}
          </span>
          <span className="min-w-0 truncate text-[12px] text-[var(--text-base)]">
            {item.name}
            {typeof item.line === "number" ? (
              <span className="text-[var(--text-faint)]">:{item.line}</span>
            ) : null}
          </span>
        </div>
      )}
      {!compact && (
        <div className="min-w-0 truncate pl-5 text-[10px] text-[var(--text-faint)]">
          {item.relativePath}
        </div>
      )}
      {compact && typeof item.line === "number" ? (
        <div className="min-w-0 truncate text-[10px] text-[var(--text-faint)]">
          {item.line}
        </div>
      ) : null}
      {item.preview ? (
        <div
          className={`min-w-0 truncate font-mono text-[10px] text-[var(--text-muted)] ${
            compact ? "" : "pl-5"
          }`}
        >
          {previewParts.map((part, index) =>
            part.matched ? (
              <mark
                key={`m-${index}`}
                className="rounded-[2px] bg-[var(--accent-soft)] px-[1px] text-[var(--accent-strong)]"
              >
                {part.text}
              </mark>
            ) : (
              <span key={`t-${index}`}>{part.text}</span>
            ),
          )}
        </div>
      ) : null}
    </button>
  );
}
