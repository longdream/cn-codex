import {
  IconAlertTriangle,
  IconExternalLink,
  IconFolderOpen,
  IconPencil,
  IconPlayerPlay,
  IconPlayerStop,
  IconRefresh,
  IconTrash,
} from "@tabler/icons-react";
import { useCallback, useEffect, useState } from "react";
import { useIntl } from "react-intl";
import {
  miniappDelete,
  miniappList,
  miniappOpenPage,
  miniappStart,
  miniappStop,
  type MiniAppRecord,
} from "../../api/miniapp";
import { standaloneThreadCreate } from "../../api/standalone";
import { revealInExplorer, windowOpenBrowser } from "../../api/window";
import { useAppStore } from "../../stores/appStore";

function statusTone(status: MiniAppRecord["status"]): string {
  switch (status) {
    case "running":
      return "text-green-400";
    case "error":
      return "text-red-400";
    case "generated":
    case "stopped":
      return "text-amber-300";
    default:
      return "text-[var(--text-faint)]";
  }
}

function buildJoinConversationDraft(app: MiniAppRecord): string {
  const rootPath = app.rootPath?.trim() || "(未配置 rootPath)";
  const dataSource = app.databaseId
    ? `- 数据库：${app.databaseName || app.databaseId}`
    : "- 数据：独立运行（未挂载数据库）";
  return [
    `请基于当前小程序包进行修改。`,
    `- 名称：${app.name}`,
    `- slug：${app.slug}`,
    `- MCP server 名：${app.slug}`,
    `- 路径：${rootPath}`,
    dataSource,
    `本对话已绑定到该小程序根目录作为专属工作目录，请直接读取并修改包内文件。`,
    `小程序必须保留可操作界面；需要查看时调用 open_page。只有已挂载数据库时才使用数据库能力。`,
    `需求：`,
  ].join("\n");
}

export function MiniAppSidePanel() {
  const intl = useIntl();
  const setRightPanelTab = useAppStore((s) => s.setRightPanelTab);
  const [apps, setApps] = useState<MiniAppRecord[]>([]);
  const [loading, setLoading] = useState(true);
  const [busySlug, setBusySlug] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const list = await miniappList();
      setApps(list);
    } catch (err) {
      setError(typeof err === "string" ? err : (err as Error).message);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void load();
    const handleUpdated = () => void load();
    window.addEventListener("cn-codex:miniapp-updated", handleUpdated);
    return () => {
      window.removeEventListener("cn-codex:miniapp-updated", handleUpdated);
    };
  }, [load]);

  const runAction = useCallback(
    async (slug: string, action: () => Promise<unknown>) => {
      setBusySlug(slug);
      setError(null);
      try {
        await action();
        await load();
      } catch (err) {
        setError(typeof err === "string" ? err : (err as Error).message);
      } finally {
        setBusySlug(null);
      }
    },
    [load],
  );

  const editMiniapp = useCallback(
    async (app: MiniAppRecord) => {
      const workdir = app.rootPath?.trim();
      if (!workdir) {
        throw new Error(intl.formatMessage({ id: "miniapp.err.noRootPath" }));
      }

      const store = useAppStore.getState();
      // 复用已绑定该小程序的编辑对话，避免重复创建。
      const existingThreadId = Object.entries(store.threadPreferences).find(
        ([, pref]) => pref?.miniappSlug === app.slug,
      )?.[0];
      if (existingThreadId && store.threads.some((t) => t.id === existingThreadId)) {
        // 用 loadThread 而非 setCurrentThread：缓存未命中（如应用重启后）时
        // 能从后端水合历史消息，避免复用对话打开为空白。
        await store.loadThread(existingThreadId);
        store.setShowSettings(false);
        return;
      }

      const createResp = await standaloneThreadCreate();
      const threadId = createResp?.thread?.id;
      if (!threadId) {
        throw new Error(intl.formatMessage({ id: "miniapp.err.threadFailed" }));
      }

      // 线程级绑定小程序目录：仅该对话使用小程序根目录作为 cwd，
      // 不再修改全局 workspaceCwd，普通对话的工作区不受影响。
      store.bindThreadMiniapp(threadId, {
        slug: app.slug,
        name: app.name,
        rootPath: workdir,
      });
      store.setCurrentThread(threadId);
      store.setThreadSmartbrainEnabled(true);
      store.addThread({
        id: threadId,
        preview: intl.formatMessage(
          { id: "miniapp.joinPreview" },
          { name: app.name, slug: app.slug },
        ),
        updatedAt: Date.now(),
        projectId: store.currentProjectId ?? undefined,
      });
      store.setShowSettings(false);
      store.queueComposerInsert(buildJoinConversationDraft(app));
    },
    [intl],
  );

  const openAppFolder = useCallback(
    (app: MiniAppRecord) => {
      const workdir = app.rootPath?.trim();
      if (!workdir) {
        return Promise.reject(new Error(intl.formatMessage({ id: "miniapp.err.noRootPath" })));
      }
      return revealInExplorer(workdir);
    },
    [intl],
  );

  return (
    <div className="flex min-h-0 flex-1 flex-col overflow-hidden">
      <div className="flex items-center gap-2 border-b border-[var(--border-subtle)] px-3 py-2">
        <span className="min-w-0 flex-1 truncate text-xs font-semibold text-[var(--text-strong)]">
          {intl.formatMessage({ id: "rightPanel.miniapp" })}
        </span>
        <button
          type="button"
          onClick={() => void load()}
          className="flex h-6 w-6 items-center justify-center rounded-[var(--radius-sm)] text-[var(--text-faint)] transition-colors hover:bg-[var(--surface-elevated)] hover:text-[var(--text-strong)]"
          title={intl.formatMessage({ id: "miniapp.refresh" })}
        >
          <IconRefresh size={14} stroke={1.8} className={loading ? "animate-spin" : undefined} />
        </button>
      </div>

      {error && (
        <div className="mx-3 mt-2 rounded-md border border-red-500/25 bg-red-500/10 px-2.5 py-2 text-[11px] text-red-300">
          {error}
        </div>
      )}

      <div className="min-h-0 flex-1 space-y-2 overflow-y-auto p-3">
        {loading && apps.length === 0 ? (
          <div className="text-xs text-[var(--text-faint)]">
            {intl.formatMessage({ id: "miniapp.loading" })}
          </div>
        ) : apps.length === 0 ? (
          <div className="space-y-2 rounded-md border border-[var(--border-subtle)] bg-[var(--bg-subtle)] px-3 py-4 text-xs text-[var(--text-faint)]">
            <div>{intl.formatMessage({ id: "miniapp.empty" })}</div>
            <div>{intl.formatMessage({ id: "miniapp.emptyHint" })}</div>
          </div>
        ) : (
          apps.map((app) => {
            const busy = busySlug === app.slug;
            const running = app.status === "running";
            return (
              <div
                key={app.id}
                className="space-y-2 rounded-md border border-[var(--border-subtle)] bg-[var(--surface-main)] px-3 py-2.5"
              >
                <div className="flex items-start justify-between gap-2">
                  <div className="min-w-0">
                    <div className="truncate text-xs font-semibold text-[var(--text-strong)]">
                      {app.name}
                    </div>
                    <div className="truncate text-[11px] text-[var(--text-faint)]">{app.slug}</div>
                  </div>
                  <div className={`shrink-0 text-[11px] ${statusTone(app.status)}`}>
                    {intl.formatMessage({ id: `miniapp.status.${app.status}` })}
                  </div>
                </div>
                <div className="text-[11px] text-[var(--text-muted)]">
                  {app.databaseId
                    ? intl.formatMessage(
                        { id: "miniapp.boundDatabase" },
                        { name: app.databaseName || app.databaseId },
                      )
                    : intl.formatMessage({ id: "miniapp.noDatabase" })}
                </div>
                {app.port ? (
                  <div className="text-[11px] text-[var(--text-faint)]">
                    {intl.formatMessage({ id: "miniapp.port" }, { port: app.port })}
                  </div>
                ) : null}
                {app.lastError ? (
                  <div className="flex items-start gap-1 text-[11px] text-red-300">
                    <IconAlertTriangle size={12} stroke={1.8} className="mt-0.5 shrink-0" />
                    <span className="min-w-0 break-words">{app.lastError}</span>
                  </div>
                ) : null}
                <div className="flex flex-wrap items-center gap-1.5">
                  {running ? (
                    <button
                      type="button"
                      disabled={busy}
                      onClick={() => void runAction(app.slug, () => miniappStop(app.slug))}
                      className="app-button-secondary flex items-center gap-1 text-[11px] disabled:opacity-50"
                    >
                      <IconPlayerStop size={12} stroke={1.8} />
                      {intl.formatMessage({ id: "miniapp.stop" })}
                    </button>
                  ) : (
                    <button
                      type="button"
                      disabled={busy}
                      onClick={() => void runAction(app.slug, () => miniappStart(app.slug))}
                      className="app-button-secondary flex items-center gap-1 text-[11px] disabled:opacity-50"
                    >
                      <IconPlayerPlay size={12} stroke={1.8} />
                      {intl.formatMessage({ id: "miniapp.start" })}
                    </button>
                  )}
                  <button
                    type="button"
                    disabled={busy}
                    onClick={() =>
                      void runAction(app.slug, async () => {
                        const result = await miniappOpenPage(app.slug);
                        setRightPanelTab("browser");
                        await windowOpenBrowser(result.url);
                      })
                    }
                    className="app-button-secondary flex items-center gap-1 text-[11px] disabled:opacity-50"
                  >
                    <IconExternalLink size={12} stroke={1.8} />
                    {intl.formatMessage({ id: "miniapp.openPage" })}
                  </button>
                  <button
                    type="button"
                    disabled={busy}
                    onClick={() => void runAction(app.slug, () => editMiniapp(app))}
                    className="app-button-secondary flex items-center gap-1 text-[11px] disabled:opacity-50"
                    title={intl.formatMessage({ id: "miniapp.editHint" })}
                  >
                    <IconPencil size={12} stroke={1.8} />
                    {intl.formatMessage({ id: "miniapp.edit" })}
                  </button>
                  <button
                    type="button"
                    disabled={busy || !app.rootPath?.trim()}
                    onClick={() => {
                      void openAppFolder(app).catch((err) =>
                        setError(typeof err === "string" ? err : (err as Error).message),
                      );
                    }}
                    className="app-button-secondary flex items-center gap-1 text-[11px] disabled:opacity-50"
                    title={intl.formatMessage({ id: "miniapp.openFolderHint" })}
                  >
                    <IconFolderOpen size={12} stroke={1.8} />
                    {intl.formatMessage({ id: "miniapp.openFolder" })}
                  </button>
                  <button
                    type="button"
                    disabled={busy}
                    onClick={() => {
                      if (
                        !confirm(
                          intl.formatMessage({ id: "miniapp.deleteConfirm" }, { name: app.name }),
                        )
                      ) {
                        return;
                      }
                      void runAction(app.slug, () => miniappDelete(app.slug));
                    }}
                    className="flex items-center gap-1 rounded-md px-2 py-1 text-[11px] text-red-300 hover:bg-red-500/10 disabled:opacity-50"
                  >
                    <IconTrash size={12} stroke={1.8} />
                    {intl.formatMessage({ id: "miniapp.delete" })}
                  </button>
                </div>
              </div>
            );
          })
        )}
      </div>
    </div>
  );
}
