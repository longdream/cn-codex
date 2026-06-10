import {
  IconChevronDown,
  IconChevronRight,
  IconFolder,
  IconFolderOpen,
  IconFolderPlus,
  IconMessagePlus,
  IconRefresh,
  IconSearch,
  IconSettings,
  IconTrash,
} from "@tabler/icons-react";
import { useCallback, useMemo, useState } from "react";
import { useIntl } from "react-intl";
import { open } from "@tauri-apps/plugin-dialog";
import { revealInExplorer } from "../../api/window";
import { useAppStore, type Project } from "../../stores/appStore";
import { ContextMenu, type ContextMenuEntry, type ContextMenuPosition } from "../common/ContextMenu";

function formatThreadTime(timestamp: number, locale: string): string {
  try {
    const now = Date.now();
    const diff = now - timestamp;
    const hours = Math.floor(diff / 3_600_000);
    if (hours < 1) return new Intl.DateTimeFormat(locale, { minute: "numeric" }).format(timestamp) + "m";
    if (hours < 24) return `${hours}h`;
    return new Intl.DateTimeFormat(locale, { month: "short", day: "numeric" }).format(timestamp);
  } catch {
    return "";
  }
}

export function Sidebar() {
  const intl = useIntl();
  const projects = useAppStore((s) => s.projects);
  const currentProjectId = useAppStore((s) => s.currentProjectId);
  const threads = useAppStore((s) => s.threads);
  const currentThreadId = useAppStore((s) => s.currentThreadId);
  const initialized = useAppStore((s) => s.initialized);
  const initError = useAppStore((s) => s.initError);
  const retryInit = useAppStore((s) => s.retryInit);
  const threadProjectMap = useAppStore((s) => s.threadProjectMap);
  const createThread = useAppStore((s) => s.createThread);
  const loadThread = useAppStore((s) => s.loadThread);
  const [creating, setCreating] = useState(false);
  const [searchQuery, setSearchQuery] = useState("");

  const handleAddProject = useCallback(async () => {
    try {
      const selected = await open({ directory: true, multiple: false });
      if (selected && typeof selected === "string") {
        useAppStore.getState().addProject(selected);
      }
    } catch (err) {
      console.error("Failed to open folder dialog:", err);
    }
  }, []);

  const handleNewChat = useCallback(async () => {
    if (creating || !currentProjectId) return;
    setCreating(true);
    try {
      await createThread();
    } finally {
      setCreating(false);
    }
  }, [creating, currentProjectId, createThread]);

  const projectThreads = useMemo(() => {
    const map = new Map<string, typeof threads>();
    for (const p of projects) {
      map.set(p.id, []);
    }
    for (const t of threads) {
      const pid = t.projectId ?? threadProjectMap[t.id];
      if (pid && map.has(pid)) {
        map.get(pid)!.push(t);
      }
    }
    for (const [, list] of map) {
      list.sort((a, b) => b.updatedAt - a.updatedAt);
    }
    return map;
  }, [projects, threads, threadProjectMap]);

  const filteredProjectThreads = useMemo(() => {
    if (!searchQuery.trim()) return projectThreads;
    const q = searchQuery.toLowerCase();
    const filtered = new Map<string, typeof threads>();
    for (const [pid, list] of projectThreads) {
      const matching = list.filter(
        (t) =>
          (t.name?.toLowerCase().includes(q)) ||
          t.preview.toLowerCase().includes(q),
      );
      if (matching.length > 0) {
        filtered.set(pid, matching);
      }
    }
    return filtered;
  }, [projectThreads, searchQuery]);

  return (
    <aside className="thin-scrollbar flex h-full w-[16rem] flex-shrink-0 flex-col overflow-hidden bg-[var(--surface-sidebar)]">
      {/* Header */}
      <div className="flex flex-col gap-1.5 px-3 pt-3 pb-1">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-2">
            <span
              className={`h-2 w-2 flex-shrink-0 rounded-full ${
                initialized
                  ? "bg-[var(--accent)]"
                  : initError
                    ? "bg-[var(--danger)]"
                    : "bg-[var(--warning)] animate-pulse"
              }`}
            />
            <h1 className="text-[13px] font-semibold tracking-tight text-[var(--text-strong)]">
              CN-Codex
            </h1>
          </div>
          <button
            onClick={handleNewChat}
            disabled={creating || !currentProjectId}
            className="flex h-7 w-7 items-center justify-center rounded-[var(--radius-sm)] text-[var(--text-muted)] transition-colors hover:bg-[var(--surface-elevated)] hover:text-[var(--text-strong)] disabled:opacity-40"
            title={intl.formatMessage({ id: "sidebar.newChat" })}
          >
            <IconMessagePlus size={15} stroke={1.8} />
          </button>
        </div>
        {initError && retryInit && (
          <button
            onClick={retryInit}
            className="flex items-center gap-1.5 text-[11px] text-[var(--danger)] hover:underline"
          >
            <IconRefresh size={10} stroke={2} />
            {intl.formatMessage({ id: "status.retry" })}
          </button>
        )}
      </div>

      {/* Search */}
      <div className="px-3 pb-2">
        <div className="relative">
          <IconSearch
            size={13}
            stroke={1.8}
            className="pointer-events-none absolute left-2 top-1/2 -translate-y-1/2 text-[var(--text-faint)]"
          />
          <input
            type="text"
            placeholder={intl.formatMessage({ id: "sidebar.search" })}
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            className="w-full rounded-[var(--radius-sm)] border border-[var(--border-subtle)] bg-[var(--surface-soft)] py-1.5 pl-7 pr-2 text-[13px] text-[var(--text-base)] placeholder:text-[var(--text-faint)] outline-none focus:border-[var(--accent-border)]"
          />
        </div>
      </div>

      {/* Project groups */}
      <div className="flex-1 overflow-y-auto px-2 py-2">
        {projects.length === 0 ? (
          <div className="rounded-[var(--radius-md)] border border-dashed border-[var(--border-strong)] px-3 py-6 text-center">
            <IconFolder size={24} stroke={1.5} className="mx-auto mb-2 text-[var(--text-faint)]" />
            <p className="text-xs font-medium text-[var(--text-muted)]">
              {intl.formatMessage({ id: "project.empty" })}
            </p>
            <p className="mt-1 text-[11px] text-[var(--text-faint)]">
              {intl.formatMessage({ id: "project.emptyHint" })}
            </p>
          </div>
        ) : (
          projects.map((project) => (
            <ProjectGroup
              key={project.id}
              project={project}
              threads={filteredProjectThreads.get(project.id) ?? []}
              isActive={currentProjectId === project.id}
              currentThreadId={currentThreadId}
              onSelect={() => useAppStore.getState().selectProject(project.id)}
              onRemove={() => useAppStore.getState().removeProject(project.id)}
              onNewChat={() => {
                useAppStore.getState().selectProject(project.id);
                void createThread();
              }}
              onThreadClick={(threadId) => {
                if (currentProjectId !== project.id) {
                  useAppStore.getState().selectProject(project.id);
                }
                void loadThread(threadId);
              }}
              onThreadDelete={(threadId) => useAppStore.getState().deleteThread(threadId)}
              locale={intl.locale}
            />
          ))
        )}
      </div>

      {/* Bottom */}
      <div className="border-t border-[var(--border-subtle)] p-2 space-y-0.5">
        <button
          onClick={handleAddProject}
          className="flex w-full items-center gap-2 rounded-[var(--radius-sm)] px-2.5 py-1.5 text-xs text-[var(--text-muted)] transition-colors hover:bg-[var(--surface-elevated)] hover:text-[var(--text-strong)]"
        >
          <IconFolderPlus size={14} stroke={1.8} />
          {intl.formatMessage({ id: "project.add" })}
        </button>
        <button
          onClick={() => useAppStore.getState().setShowSettings(true)}
          className="flex w-full items-center gap-2 rounded-[var(--radius-sm)] px-2.5 py-1.5 text-xs text-[var(--text-muted)] transition-colors hover:bg-[var(--surface-elevated)] hover:text-[var(--text-strong)]"
        >
          <IconSettings size={14} stroke={1.8} />
          {intl.formatMessage({ id: "sidebar.settings" })}
        </button>
      </div>
    </aside>
  );
}

function ProjectGroup({
  project,
  threads,
  isActive,
  currentThreadId,
  onSelect,
  onRemove,
  onNewChat,
  onThreadClick,
  onThreadDelete,
  locale,
}: {
  project: Project;
  threads: Array<{ id: string; name?: string; preview: string; updatedAt: number }>;
  isActive: boolean;
  currentThreadId: string | null;
  onSelect: () => void;
  onRemove: () => void;
  onNewChat: () => void;
  onThreadClick: (threadId: string) => void;
  onThreadDelete: (threadId: string) => void;
  locale: string;
}) {
  const intl = useIntl();
  const [expanded, setExpanded] = useState(true);
  const [confirmRemove, setConfirmRemove] = useState(false);
  const [contextMenu, setContextMenu] = useState<ContextMenuPosition | null>(null);

  const contextMenuItems: ContextMenuEntry[] = useMemo(() => [
    {
      id: "open-folder",
      label: intl.formatMessage({ id: "contextMenu.openInExplorer" }),
      icon: <IconFolderOpen size={14} stroke={1.8} />,
      onClick: () => void revealInExplorer(project.cwd),
    },
    {
      id: "new-chat",
      label: intl.formatMessage({ id: "sidebar.newChat" }),
      icon: <IconMessagePlus size={14} stroke={1.8} />,
      onClick: onNewChat,
    },
    { id: "divider-1", divider: true },
    {
      id: "remove-project",
      label: intl.formatMessage({ id: "project.remove" }),
      icon: <IconTrash size={14} stroke={1.8} />,
      onClick: () => setConfirmRemove(true),
      danger: true,
    },
  ], [intl, project.cwd, onNewChat]);

  const handleContextMenu = useCallback((e: React.MouseEvent) => {
    e.preventDefault();
    setContextMenu({ x: e.clientX, y: e.clientY });
  }, []);

  return (
    <div className="mb-1">
      {contextMenu && (
        <ContextMenu
          items={contextMenuItems}
          position={contextMenu}
          onClose={() => setContextMenu(null)}
        />
      )}

      {/* Project header */}
      <div
        onClick={() => {
          onSelect();
          setExpanded(true);
        }}
        onContextMenu={handleContextMenu}
        className={`group flex w-full cursor-pointer items-center gap-1.5 rounded-[var(--radius-sm)] px-2 py-1.5 text-left text-xs transition-colors ${
          isActive
            ? "bg-[var(--accent-soft)] text-[var(--accent)]"
            : "text-[var(--text-muted)] hover:bg-[var(--surface-elevated)] hover:text-[var(--text-strong)]"
        }`}
      >
        <button
          onClick={(e) => {
            e.stopPropagation();
            setExpanded(!expanded);
          }}
          className="flex-shrink-0 opacity-60 hover:opacity-100"
        >
          {expanded ? (
            <IconChevronDown size={12} stroke={2} />
          ) : (
            <IconChevronRight size={12} stroke={2} />
          )}
        </button>
        <IconFolder size={13} stroke={1.8} className="flex-shrink-0" />
        <span className="min-w-0 flex-1 truncate font-medium" title={project.cwd}>
          {project.name}
        </span>
        <span className="flex-shrink-0 text-[11px] opacity-60">{threads.length}</span>
        <button
          onClick={(e) => {
            e.stopPropagation();
            setConfirmRemove(true);
          }}
          className="flex-shrink-0 opacity-0 transition-opacity group-hover:opacity-60 hover:!opacity-100 hover:text-[var(--danger)]"
          title={intl.formatMessage({ id: "project.remove" })}
        >
          <IconTrash size={12} stroke={1.8} />
        </button>
      </div>

      {/* 确认删除项目弹窗 */}
      {confirmRemove && (
        <div className="mx-2 mt-1 rounded-md border border-[rgba(220,92,92,0.35)] bg-[rgba(220,92,92,0.08)] p-2 text-xs">
          <p className="text-[var(--text-strong)]">
            {intl.formatMessage({ id: "project.confirmRemove" })}
          </p>
          <div className="mt-1.5 flex items-center gap-2">
            <button
              onClick={() => { onRemove(); setConfirmRemove(false); }}
              className="rounded-sm bg-[var(--danger)] px-2 py-0.5 text-[11px] text-white hover:opacity-90"
            >
              {intl.formatMessage({ id: "common.confirm" })}
            </button>
            <button
              onClick={() => setConfirmRemove(false)}
              className="rounded-sm px-2 py-0.5 text-[11px] text-[var(--text-muted)] hover:text-[var(--text-strong)]"
            >
              {intl.formatMessage({ id: "common.cancel" })}
            </button>
          </div>
        </div>
      )}

      {/* Thread list */}
      {expanded && (
        <div className="ml-3 mt-0.5 space-y-0.5 border-l border-[var(--border-subtle)] pl-2">
          {threads.length === 0 ? (
            <p className="px-2 py-1 text-[11px] text-[var(--text-faint)]">
              {intl.formatMessage({ id: "project.noThreads" })}
            </p>
          ) : (
            threads.map((thread) => {
              const title = thread.name || thread.preview || intl.formatMessage({ id: "chat.threadUntitled" });
              return (
                <ThreadItem
                  key={thread.id}
                  thread={thread}
                  title={title}
                  isCurrent={currentThreadId === thread.id}
                  locale={locale}
                  onClick={() => onThreadClick(thread.id)}
                  onDelete={() => onThreadDelete(thread.id)}
                />
              );
            })
          )}
        </div>
      )}
    </div>
  );
}

/** 单个对话项，支持 hover 删除和右键菜单 */
function ThreadItem({
  thread,
  title,
  isCurrent,
  locale,
  onClick,
  onDelete,
}: {
  thread: { id: string; updatedAt: number };
  title: string;
  isCurrent: boolean;
  locale: string;
  onClick: () => void;
  onDelete: () => void;
}) {
  const intl = useIntl();
  const [contextMenu, setContextMenu] = useState<ContextMenuPosition | null>(null);

  const contextMenuItems: ContextMenuEntry[] = useMemo(() => [
    {
      id: "delete-thread",
      label: intl.formatMessage({ id: "thread.delete" }),
      icon: <IconTrash size={14} stroke={1.8} />,
      onClick: onDelete,
      danger: true,
    },
  ], [intl, onDelete]);

  const handleContextMenu = useCallback((e: React.MouseEvent) => {
    e.preventDefault();
    setContextMenu({ x: e.clientX, y: e.clientY });
  }, []);

  return (
    <>
      {contextMenu && (
        <ContextMenu
          items={contextMenuItems}
          position={contextMenu}
          onClose={() => setContextMenu(null)}
        />
      )}
      <div
        className={`group/thread flex w-full items-center justify-between gap-1 rounded-[var(--radius-sm)] px-2 py-1.5 text-left transition-colors cursor-pointer ${
          isCurrent
            ? "bg-[var(--surface-elevated)] text-[var(--text-strong)]"
            : "text-[var(--text-muted)] hover:bg-[var(--surface-elevated)] hover:text-[var(--text-base)]"
        }`}
        onClick={onClick}
        onContextMenu={handleContextMenu}
      >
        <p className="min-w-0 flex-1 truncate text-xs">{title}</p>
        <span className="shrink-0 text-[11px] text-[var(--text-faint)] group-hover/thread:hidden">
          {formatThreadTime(thread.updatedAt, locale)}
        </span>
        <button
          onClick={(e) => {
            e.stopPropagation();
            onDelete();
          }}
          className="hidden shrink-0 items-center justify-center rounded-sm p-0.5 text-[var(--text-faint)] transition-colors hover:text-[var(--danger)] group-hover/thread:flex"
          title={intl.formatMessage({ id: "thread.delete" })}
        >
          <IconTrash size={11} stroke={1.8} />
        </button>
      </div>
    </>
  );
}
