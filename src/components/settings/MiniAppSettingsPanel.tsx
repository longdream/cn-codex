import {
  IconAlertTriangle,
  IconApps,
  IconExternalLink,
  IconLoader2,
  IconPlayerPlay,
  IconPlayerStop,
  IconPlus,
  IconRefresh,
  IconSparkles,
  IconTrash,
} from "@tabler/icons-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useIntl } from "react-intl";
import {
  miniappCreate,
  miniappDelete,
  miniappList,
  miniappOpenPage,
  miniappStart,
  miniappStop,
  type MiniAppRecord,
} from "../../api/miniapp";
import { standaloneChat, standaloneThreadCreate } from "../../api/standalone";
import { windowOpenBrowser } from "../../api/window";
import { useAppStore } from "../../stores/appStore";
import {
  loadSmartbrainDbSources,
  type SmartbrainDbSource,
} from "./smartbrainDatabaseState";

const SLUG_RE = /^[a-z][a-z0-9_-]*$/;

type PageType = "entry" | "list" | "query";

function statusLabelId(status: MiniAppRecord["status"]): string {
  switch (status) {
    case "running":
      return "miniapp.status.running";
    case "error":
      return "miniapp.status.error";
    case "generated":
      return "miniapp.status.generated";
    case "stopped":
      return "miniapp.status.stopped";
    default:
      return "miniapp.status.draft";
  }
}

function statusTone(status: MiniAppRecord["status"]): string {
  switch (status) {
    case "running":
      return "text-green-400";
    case "error":
      return "text-red-400";
    case "generated":
      return "text-sky-300";
    case "stopped":
      return "text-[var(--text-faint)]";
    default:
      return "text-[var(--text-muted)]";
  }
}

function buildMiniAppGeneratePrompt(params: {
  app: MiniAppRecord;
  requirement: string;
  allowWrite: boolean;
  pageTypes: PageType[];
  mode: "generate" | "regenerate" | "patch";
}): string {
  const pageTypeLabels = params.pageTypes
    .map((t) => {
      if (t === "entry") return "录入页";
      if (t === "list") return "列表页";
      return "查询页";
    })
    .join("、");

  const modeHint =
    params.mode === "generate"
      ? "这是首次生成：在现有脚手架上实现完整业务。"
      : params.mode === "regenerate"
        ? "这是重新生成：可大幅改写 server/web，但必须保持 Node.js MCP 包结构与 databaseId 绑定。"
        : "这是增量修改：在现有实现上按需求改动，尽量少破坏已可用功能。";

  return `你正在为 CN-Codex 生成本地小程序（MiniApp）。当前工作目录已经是小程序包根目录。

## 强制约束（不可违反）
1. 技术栈必须是 **Node.js**（\`.mjs\`/\`.js\`）。MCP server、HTTP 页面服务、业务逻辑均用 Node；**禁止**以 Python 或其他语言作为运行主体。
2. 保持/完善 MCP-like 包结构：
   - \`server/index.mjs\`：stdio MCP（tools/list + tools/call）+ 本地 HTTP 静态服务（\`web/\`）
   - \`web/\`：页面
   - \`miniapp.json\`、\`.mcp.json\`、\`package.json\`、\`README.md\`
3. MCP tools **至少**包含：\`list_pages\`、\`get_status\`、\`open_page\`，以及按需求生成的业务方法（如 \`submit_form\`、\`search\`）。
4. 业务方法统一返回 envelope：
\`\`\`json
{
  "ok": true,
  "code": "OK",
  "message": "说明",
  "data": {},
  "ui": { "action": "close_page", "pageId": "..." }
}
\`\`\`
5. 数据库只能通过已配置的 \`databaseId\` 引用；**禁止**把密码、完整连接串写进代码。读/写库请使用宿主工具 \`smartbrain_sql_query\`（受权限策略约束），不要自建连接脚本。
6. 运行时使用内置 \`codey/node\`（相对路径通常在 \`../../node/node.exe\` 或环境中的 node）。需要依赖时用该 node 对应的 npm 安装。
7. 端口由运行时环境变量 \`MINIAPP_PORT\` 注入或自动分配；不要写死生产端口到配置外。
8. 完成后更新 \`miniapp.json\` 的 \`pages\`/\`tools\`/\`description\`，保证可被宿主列表发现。

## 小程序信息
- 中文名称：${params.app.name}
- slug：\`${params.app.slug}\`
- databaseId：\`${params.app.databaseId}\`
- databaseName：\`${params.app.databaseName || "(未填)"}\`
- 是否需要写库能力：${params.allowWrite ? "是（INSERT/UPDATE 等需符合库权限）" : "否（尽量只读）"}
- 预置页面类型：${pageTypeLabels || "由需求自行决定"}
- 包路径：\`${params.app.rootPath}\`

## 任务模式
${modeHint}

## 用户自然语言需求
${params.requirement.trim()}

## 建议步骤
1. 阅读现有脚手架文件（\`server/index.mjs\`、\`web/index.html\`、\`miniapp.json\`）。
2. 若需要表结构，用 \`smartbrain_sql_query\` 查询（database 参数填显示名或物理库名），先确认权限允许。
3. 实现页面与业务 MCP tools，必要时 \`npm install\`。
4. 尽量本地自检（语法/启动说明），最后用简短中文总结：已实现的页面、tools、如何启动。

现在开始。`;
}

export function MiniAppSettingsPanel() {
  const intl = useIntl();
  const [apps, setApps] = useState<MiniAppRecord[]>([]);
  const [sources, setSources] = useState<SmartbrainDbSource[]>([]);
  const [loading, setLoading] = useState(true);
  const [busySlug, setBusySlug] = useState<string | null>(null);
  const [generating, setGenerating] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [info, setInfo] = useState<string | null>(null);

  const [name, setName] = useState("");
  const [slug, setSlug] = useState("");
  const [databaseId, setDatabaseId] = useState("");
  const [requirement, setRequirement] = useState("");
  const [allowWrite, setAllowWrite] = useState(false);
  const [pageTypes, setPageTypes] = useState<PageType[]>(["entry"]);
  const [selectedSlug, setSelectedSlug] = useState<string | null>(null);

  const enabledSources = useMemo(
    () => sources.filter((s) => s.enabled),
    [sources],
  );

  const refresh = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const [list, dbs] = await Promise.all([
        miniappList(),
        loadSmartbrainDbSources().catch(() => [] as SmartbrainDbSource[]),
      ]);
      setApps(list);
      setSources(dbs);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const selectedDb = enabledSources.find((s) => s.id === databaseId) ?? null;

  const togglePageType = (type: PageType) => {
    setPageTypes((prev) =>
      prev.includes(type) ? prev.filter((t) => t !== type) : [...prev, type],
    );
  };

  const fillFromApp = (app: MiniAppRecord) => {
    setSelectedSlug(app.slug);
    setName(app.name);
    setSlug(app.slug);
    setDatabaseId(app.databaseId);
    setRequirement(app.description || "");
  };

  const validateForm = (): string | null => {
    if (!name.trim()) {
      return intl.formatMessage({ id: "settings.miniapp.err.nameRequired" });
    }
    if (!SLUG_RE.test(slug.trim())) {
      return intl.formatMessage({ id: "settings.miniapp.err.slugInvalid" });
    }
    if (!databaseId.trim()) {
      return intl.formatMessage({ id: "settings.miniapp.err.databaseRequired" });
    }
    if (!requirement.trim()) {
      return intl.formatMessage({ id: "settings.miniapp.err.requirementRequired" });
    }
    return null;
  };

  const launchMainChainGenerate = async (
    app: MiniAppRecord,
    mode: "generate" | "regenerate" | "patch",
    options?: {
      requirementText?: string;
      allowWrite?: boolean;
      pageTypes?: PageType[];
    },
  ) => {
    const cwd =
      useAppStore.getState().workspaceCwd ||
      useAppStore.getState().projectRoot ||
      useAppStore.getState().userHomeDir;
    if (!cwd) {
      throw new Error(intl.formatMessage({ id: "settings.miniapp.err.noWorkspace" }));
    }

    const requirementText = (options?.requirementText ?? requirement).trim();
    if (!requirementText) {
      throw new Error(intl.formatMessage({ id: "settings.miniapp.err.requirementRequired" }));
    }
    const effectiveAllowWrite = options?.allowWrite ?? allowWrite;
    const effectivePageTypes = options?.pageTypes ?? pageTypes;

    const createResp = await standaloneThreadCreate();
    const threadId = createResp?.thread?.id;
    if (!threadId) {
      throw new Error(intl.formatMessage({ id: "settings.miniapp.err.threadFailed" }));
    }

    const prompt = buildMiniAppGeneratePrompt({
      app,
      requirement: requirementText,
      allowWrite: effectiveAllowWrite,
      pageTypes: effectivePageTypes,
      mode,
    });
    const preview = `[小程序生成] ${app.name} (${app.slug})`.slice(0, 60);
    const userMessage = {
      id: crypto.randomUUID(),
      role: "user" as const,
      content: prompt,
      timestamp: Date.now(),
    };

    const store = useAppStore.getState();
    // 先打开本地知识库，再切换线程，确保 threadPreferences 继承 smartbrain=true。
    store.setThreadSmartbrainEnabled(true);
    store.startNewThreadWithMessage(threadId, userMessage);
    store.addThread({
      id: threadId,
      preview,
      updatedAt: Date.now(),
      projectId: store.currentProjectId ?? undefined,
    });
    store.setShowSettings(false);
    store.setRightPanelTab("miniapp");

    // 工作目录用小程序根目录，便于 Agent 直接改包内文件
    const workdir = app.rootPath?.trim() || cwd;
    await standaloneChat(
      threadId,
      prompt,
      workdir,
      "chat",
      [],
      undefined,
      undefined,
      {
        smartbrainEnabled: true,
      },
      userMessage.id,
    );
  };

  const handleCreateScaffoldOnly = async () => {
    const formError = validateForm();
    if (formError) {
      setError(formError);
      return;
    }
    setGenerating(true);
    setError(null);
    setInfo(null);
    try {
      const app = await miniappCreate({
        name: name.trim(),
        slug: slug.trim().toLowerCase(),
        description: requirement.trim(),
        databaseId: databaseId.trim(),
        databaseName: selectedDb?.name || selectedDb?.databaseName || "",
      });
      setInfo(intl.formatMessage({ id: "settings.miniapp.scaffoldCreated" }, { name: app.name }));
      setSelectedSlug(app.slug);
      await refresh();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setGenerating(false);
    }
  };

  const handleGenerate = async (mode: "generate" | "regenerate" | "patch") => {
    const formError = validateForm();
    if (formError) {
      setError(formError);
      return;
    }
    setGenerating(true);
    setError(null);
    setInfo(null);
    try {
      const slugKey = slug.trim().toLowerCase();
      let app: MiniAppRecord | undefined = apps.find(
        (a) => a.slug === slugKey || a.slug === selectedSlug,
      );
      let effectiveMode = mode;
      if (!app) {
        if (mode === "patch") {
          throw new Error(intl.formatMessage({ id: "settings.miniapp.err.notFound" }));
        }
        app = await miniappCreate({
          name: name.trim(),
          slug: slugKey,
          description: requirement.trim(),
          databaseId: databaseId.trim(),
          databaseName: selectedDb?.name || selectedDb?.databaseName || "",
        });
        effectiveMode = "generate";
      } else if (mode === "generate") {
        // 已有包再次点「生成」视为重新生成
        effectiveMode = "regenerate";
      }
      setSelectedSlug(app.slug);
      setInfo(
        intl.formatMessage({ id: "settings.miniapp.generateStarted" }, { name: app.name }),
      );
      await launchMainChainGenerate(app, effectiveMode, {
        requirementText: requirement.trim(),
        allowWrite,
        pageTypes,
      });
      await refresh();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setGenerating(false);
    }
  };

  const runAppAction = async (
    slugKey: string,
    action: () => Promise<unknown>,
  ) => {
    setBusySlug(slugKey);
    setError(null);
    try {
      await action();
      await refresh();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusySlug(null);
    }
  };

  return (
    <div className="space-y-4">
      <section className="settings-card space-y-3">
        <div className="flex items-start gap-3">
          <div className="flex h-10 w-10 items-center justify-center rounded-[var(--radius-md)] bg-[var(--accent-soft)] text-[var(--accent-strong)]">
            <IconApps size={18} stroke={1.9} />
          </div>
          <div>
            <h4 className="text-[13px] font-semibold text-[var(--text-strong)]">
              {intl.formatMessage({ id: "settings.miniapp.title" })}
            </h4>
            <p className="mt-1 text-xs text-[var(--text-faint)]">
              {intl.formatMessage({ id: "settings.miniapp.description" })}
            </p>
          </div>
        </div>

        <div className="grid gap-3 md:grid-cols-2">
          <div className="space-y-1">
            <label className="text-[11px] text-[var(--text-faint)]">
              {intl.formatMessage({ id: "settings.miniapp.field.name" })} *
            </label>
            <input
              value={name}
              onChange={(e) => setName(e.target.value)}
              className="app-input w-full"
              placeholder={intl.formatMessage({ id: "settings.miniapp.field.namePlaceholder" })}
            />
          </div>
          <div className="space-y-1">
            <label className="text-[11px] text-[var(--text-faint)]">
              {intl.formatMessage({ id: "settings.miniapp.field.slug" })} *
            </label>
            <input
              value={slug}
              onChange={(e) => setSlug(e.target.value.toLowerCase().replace(/\s+/g, "-"))}
              className="app-input w-full font-mono text-xs"
              placeholder="contract-app"
            />
            <p className="text-[10px] text-[var(--text-faint)]">
              {intl.formatMessage({ id: "settings.miniapp.field.slugHint" })}
            </p>
          </div>
        </div>

        <div className="space-y-1">
          <label className="text-[11px] text-[var(--text-faint)]">
            {intl.formatMessage({ id: "settings.miniapp.field.database" })} *
          </label>
          <select
            value={databaseId}
            onChange={(e) => setDatabaseId(e.target.value)}
            className="app-input w-full"
          >
            <option value="">
              {intl.formatMessage({ id: "settings.miniapp.field.databasePlaceholder" })}
            </option>
            {enabledSources.map((source) => (
              <option key={source.id} value={source.id}>
                {source.name || source.databaseName || source.id}
              </option>
            ))}
          </select>
          {enabledSources.length === 0 && (
            <p className="text-[11px] text-amber-300/90">
              {intl.formatMessage({ id: "settings.miniapp.field.noDatabase" })}
            </p>
          )}
        </div>

        <div className="space-y-1">
          <label className="text-[11px] text-[var(--text-faint)]">
            {intl.formatMessage({ id: "settings.miniapp.field.requirement" })} *
          </label>
          <textarea
            value={requirement}
            onChange={(e) => setRequirement(e.target.value)}
            rows={5}
            className="app-input w-full resize-y"
            placeholder={intl.formatMessage({
              id: "settings.miniapp.field.requirementPlaceholder",
            })}
          />
        </div>

        <div className="flex flex-wrap items-center gap-3">
          <label className="inline-flex items-center gap-2 text-xs text-[var(--text-muted)]">
            <input
              type="checkbox"
              checked={allowWrite}
              onChange={(e) => setAllowWrite(e.target.checked)}
            />
            {intl.formatMessage({ id: "settings.miniapp.field.allowWrite" })}
          </label>
          {(["entry", "list", "query"] as const).map((type) => (
            <label
              key={type}
              className="inline-flex items-center gap-1.5 text-xs text-[var(--text-muted)]"
            >
              <input
                type="checkbox"
                checked={pageTypes.includes(type)}
                onChange={() => togglePageType(type)}
              />
              {intl.formatMessage({ id: `settings.miniapp.pageType.${type}` })}
            </label>
          ))}
        </div>

        <div className="flex flex-wrap gap-2">
          <button
            type="button"
            disabled={generating}
            onClick={() => void handleGenerate("generate")}
            className="inline-flex items-center gap-1.5 rounded-lg bg-[var(--accent-strong)] px-3 py-1.5 text-xs font-medium text-white disabled:opacity-50"
          >
            {generating ? (
              <IconLoader2 size={14} className="animate-spin" />
            ) : (
              <IconSparkles size={14} stroke={1.8} />
            )}
            {intl.formatMessage({ id: "settings.miniapp.action.generate" })}
          </button>
          <button
            type="button"
            disabled={generating || !selectedSlug}
            onClick={() => void handleGenerate("patch")}
            className="app-button-secondary inline-flex items-center gap-1.5 px-3 py-1.5 text-xs disabled:opacity-50"
          >
            <IconRefresh size={14} stroke={1.8} />
            {intl.formatMessage({ id: "settings.miniapp.action.patch" })}
          </button>
          <button
            type="button"
            disabled={generating}
            onClick={() => void handleCreateScaffoldOnly()}
            className="app-button-secondary inline-flex items-center gap-1.5 px-3 py-1.5 text-xs disabled:opacity-50"
          >
            <IconPlus size={14} stroke={1.8} />
            {intl.formatMessage({ id: "settings.miniapp.action.scaffoldOnly" })}
          </button>
        </div>

        {error && (
          <div className="flex items-start gap-2 rounded-md border border-red-500/30 bg-red-500/10 px-3 py-2 text-xs text-red-200">
            <IconAlertTriangle size={14} className="mt-0.5 shrink-0" />
            <span>{error}</span>
          </div>
        )}
        {info && !error && (
          <div className="rounded-md border border-[var(--border-subtle)] bg-[var(--surface-soft)] px-3 py-2 text-xs text-[var(--text-muted)]">
            {info}
          </div>
        )}
      </section>

      <section className="settings-card space-y-3">
        <div className="flex items-center justify-between gap-2">
          <h5 className="text-xs font-semibold text-[var(--text-strong)]">
            {intl.formatMessage({ id: "settings.miniapp.listTitle" })}
          </h5>
          <button
            type="button"
            onClick={() => void refresh()}
            className="app-button-secondary inline-flex items-center gap-1 px-2 py-1 text-[11px]"
          >
            <IconRefresh size={12} stroke={1.8} />
            {intl.formatMessage({ id: "miniapp.refresh" })}
          </button>
        </div>

        {loading ? (
          <p className="text-xs text-[var(--text-faint)]">
            {intl.formatMessage({ id: "miniapp.loading" })}
          </p>
        ) : apps.length === 0 ? (
          <p className="text-xs text-[var(--text-faint)]">
            {intl.formatMessage({ id: "miniapp.empty" })}
          </p>
        ) : (
          <div className="space-y-2">
            {apps.map((app) => {
              const busy = busySlug === app.slug;
              return (
                <div
                  key={app.id}
                  className={`rounded-[var(--radius-md)] border px-3 py-2 ${
                    selectedSlug === app.slug
                      ? "border-[var(--accent-border)] bg-[var(--accent-soft)]/30"
                      : "border-[var(--border-subtle)] bg-[var(--surface-main)]"
                  }`}
                >
                  <button
                    type="button"
                    className="w-full text-left"
                    onClick={() => fillFromApp(app)}
                  >
                    <div className="flex items-center justify-between gap-2">
                      <div className="min-w-0">
                        <div className="truncate text-xs font-medium text-[var(--text-strong)]">
                          {app.name}
                          <span className="ml-2 font-mono text-[10px] text-[var(--text-faint)]">
                            {app.slug}
                          </span>
                        </div>
                        <div className="mt-0.5 text-[11px] text-[var(--text-faint)]">
                          {intl.formatMessage(
                            { id: "miniapp.boundDatabase" },
                            { name: app.databaseName || app.databaseId || "-" },
                          )}
                          {app.port
                            ? ` · ${intl.formatMessage({ id: "miniapp.port" }, { port: app.port })}`
                            : ""}
                        </div>
                      </div>
                      <span className={`shrink-0 text-[11px] ${statusTone(app.status)}`}>
                        {intl.formatMessage({ id: statusLabelId(app.status) })}
                      </span>
                    </div>
                    {app.lastError ? (
                      <p className="mt-1 line-clamp-2 text-[11px] text-red-300/90">
                        {app.lastError}
                      </p>
                    ) : null}
                  </button>
                  <div className="mt-2 flex flex-wrap gap-1.5">
                    {app.status === "running" ? (
                      <button
                        type="button"
                        disabled={busy}
                        onClick={() =>
                          void runAppAction(app.slug, () => miniappStop(app.slug))
                        }
                        className="inline-flex items-center gap-1 rounded-md px-2 py-1 text-[11px] text-[var(--text-muted)] hover:bg-[var(--surface-elevated)] disabled:opacity-50"
                      >
                        <IconPlayerStop size={12} stroke={1.8} />
                        {intl.formatMessage({ id: "miniapp.stop" })}
                      </button>
                    ) : (
                      <button
                        type="button"
                        disabled={busy}
                        onClick={() =>
                          void runAppAction(app.slug, () => miniappStart(app.slug))
                        }
                        className="inline-flex items-center gap-1 rounded-md px-2 py-1 text-[11px] text-[var(--text-muted)] hover:bg-[var(--surface-elevated)] disabled:opacity-50"
                      >
                        <IconPlayerPlay size={12} stroke={1.8} />
                        {intl.formatMessage({ id: "miniapp.start" })}
                      </button>
                    )}
                    <button
                      type="button"
                      disabled={busy}
                      onClick={() =>
                        void runAppAction(app.slug, async () => {
                          const result = await miniappOpenPage(app.slug);
                          if (result.url) {
                            useAppStore.getState().setRightPanelTab("browser");
                            useAppStore.getState().setRightPanelVisible(true);
                            await windowOpenBrowser(result.url);
                          }
                        })
                      }
                      className="inline-flex items-center gap-1 rounded-md px-2 py-1 text-[11px] text-[var(--text-muted)] hover:bg-[var(--surface-elevated)] disabled:opacity-50"
                    >
                      <IconExternalLink size={12} stroke={1.8} />
                      {intl.formatMessage({ id: "miniapp.openPage" })}
                    </button>
                    <button
                      type="button"
                      disabled={busy || generating}
                      onClick={() => {
                        void (async () => {
                          fillFromApp(app);
                          // fillFromApp 是异步 setState；本轮直接用 app 字段，避免读到旧 requirement。
                          const reqText = (app.description || "").trim();
                          if (!reqText.trim()) {
                            setError(
                              intl.formatMessage({
                                id: "settings.miniapp.err.requirementRequired",
                              }),
                            );
                            return;
                          }
                          setGenerating(true);
                          setError(null);
                          setInfo(
                            intl.formatMessage(
                              { id: "settings.miniapp.generateStarted" },
                              { name: app.name },
                            ),
                          );
                          try {
                            await launchMainChainGenerate(
                              {
                                ...app,
                                description: reqText,
                              },
                              "regenerate",
                              { requirementText: reqText },
                            );
                            await refresh();
                          } catch (err) {
                            setError(err instanceof Error ? err.message : String(err));
                          } finally {
                            setGenerating(false);
                          }
                        })();
                      }}
                      className="inline-flex items-center gap-1 rounded-md px-2 py-1 text-[11px] text-[var(--text-muted)] hover:bg-[var(--surface-elevated)] disabled:opacity-50"
                    >
                      <IconSparkles size={12} stroke={1.8} />
                      {intl.formatMessage({ id: "settings.miniapp.action.regenerate" })}
                    </button>
                    <button
                      type="button"
                      disabled={busy}
                      onClick={() => {
                        if (
                          !confirm(
                            intl.formatMessage(
                              { id: "miniapp.deleteConfirm" },
                              { name: app.name },
                            ),
                          )
                        ) {
                          return;
                        }
                        void runAppAction(app.slug, () => miniappDelete(app.slug));
                      }}
                      className="inline-flex items-center gap-1 rounded-md px-2 py-1 text-[11px] text-red-300 hover:bg-red-500/10 disabled:opacity-50"
                    >
                      <IconTrash size={12} stroke={1.8} />
                      {intl.formatMessage({ id: "miniapp.delete" })}
                    </button>
                  </div>
                </div>
              );
            })}
          </div>
        )}
      </section>
    </div>
  );
}
