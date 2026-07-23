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

export function buildMiniAppGeneratePrompt(params: {
  app: MiniAppRecord;
  requirement: string;
  allowWrite: boolean;
  mode: "generate" | "regenerate" | "patch";
}): string {
  const modeHint =
    params.mode === "generate"
      ? "这是首次生成：在现有脚手架上实现完整、可操作的小程序。"
      : params.mode === "regenerate"
        ? "这是重新生成：可大幅改写 server/web，但必须保持 Node.js MCP 包结构和可启动界面。"
        : "这是增量修改：在现有实现上按需求改动，尽量少破坏已可用功能。";
  const hasDatabase = Boolean(params.app.databaseId?.trim());
  const databaseGuidance = hasDatabase
    ? `本小程序已选择数据库 \`${params.app.databaseId}\`（${params.app.databaseName || "未命名"}）。数据库能力是本应用的一项可选依赖；禁止把密码或完整连接串写进代码。读写数据必须遵守宿主权限策略，${params.allowWrite ? "允许按需求执行授权范围内的写操作。" : "本次按只读方式实现，不要执行写操作。"}`
    : "本小程序未选择数据库。不要假设存在数据库，不要生成数据库连接代码；按需求使用浏览器状态、内存、本地文件或无需数据库的实现方式。";

  return `你正在为 CN-Codex 生成本地小程序（MiniApp）。当前工作目录已经是小程序包根目录。

## 强制约束（不可违反）
1. 技术栈必须是 **Node.js**（\`.mjs\`/\`.js\`）。MCP server、HTTP 页面服务、业务逻辑均用 Node；**禁止**以 Python 或其他语言作为运行主体。
2. **必须交付可用界面**：至少生成一个可通过浏览器打开的 Web UI，主功能必须能在界面中操作。只生成 API、MCP tools、命令行或说明文档均视为未完成。
3. 功能类型不设限：可以是工具、游戏、可视化、编辑器、计算器、媒体应用、业务系统或用户描述的任何其他功能，不要默认套用表单、CRUD 或数据库后台。
4. 保持/完善 MCP-like 包结构：
   - \`server/index.mjs\`：stdio MCP（tools/list + tools/call）+ 本地 HTTP 静态服务（\`web/\`）
   - \`web/\`：完整界面、样式和前端交互脚本
   - \`miniapp.json\`、\`.mcp.json\`、\`package.json\`、\`README.md\`
5. \`miniapp.json.pages\` 至少包含一个真实可访问页面；默认页必须指向实际入口。MCP tools 至少包含 \`list_pages\`、\`get_status\`、\`open_page\`，并按需求增加应用方法。
6. 业务方法统一返回 envelope：
\`\`\`json
{
  "ok": true,
  "code": "OK",
  "message": "说明",
  "data": {},
  "ui": { "action": "close_page", "pageId": "..." }
}
\`\`\`
7. 运行时使用内置 \`codey/node\`。需要依赖时用对应 npm 安装；优先使用少依赖、可离线启动的实现。
8. 端口由运行时环境变量 \`MINIAPP_PORT\` 注入或自动分配；不要写死端口。
9. 完成后更新 \`miniapp.json\` 的 \`pages\`、\`tools\`、\`description\`，并保持 name、slug、rootPath 等宿主识别字段有效。
10. 做启动与界面自检：检查 Node 语法，短暂启动服务并请求默认页面，确认返回有效 HTML 且界面资源可加载。不要留下脱离宿主管理的常驻进程，任务完成后宿主会自动启动小程序。

## 小程序信息
- 显示名称：${params.app.name}
- slug：\`${params.app.slug}\`
- 包路径：\`${params.app.rootPath}\`

## 数据能力
${databaseGuidance}

## 任务模式
${modeHint}

## 用户自然语言需求
${params.requirement.trim()}

## 建议步骤
1. 阅读现有脚手架文件（\`server/index.mjs\`、\`web/index.html\`、\`miniapp.json\`）。
2. 根据需求设计界面结构和核心交互，不要把脚手架示例页当成最终结果。
3. 实现页面、业务逻辑和 MCP tools；仅在确实需要时安装依赖。
4. 完成语法、启动、HTTP 页面和关键交互自检。
5. 最后用简短中文总结：实现了什么界面、核心功能、tools 和自检结果。

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
    const usesDatabase = Boolean(app.databaseId?.trim());

    const createResp = await standaloneThreadCreate();
    const threadId = createResp?.thread?.id;
    if (!threadId) {
      throw new Error(intl.formatMessage({ id: "settings.miniapp.err.threadFailed" }));
    }

    const prompt = buildMiniAppGeneratePrompt({
      app,
      requirement: requirementText,
      allowWrite: effectiveAllowWrite,
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
    // 仅在小程序选择数据库时打开本地知识库能力。
    store.setThreadSmartbrainEnabled(usesDatabase);
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
    try {
      if (app.status === "running") {
        await miniappStop(app.slug);
      }
      await standaloneChat(
        threadId,
        prompt,
        workdir,
        "chat",
        [],
        undefined,
        undefined,
        {
          smartbrainEnabled: usesDatabase,
        },
        userMessage.id,
      );
      await miniappStart(app.slug);
    } finally {
      window.dispatchEvent(new CustomEvent("cn-codex:miniapp-updated"));
    }
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
              placeholder="focus-timer"
            />
            <p className="text-[10px] text-[var(--text-faint)]">
              {intl.formatMessage({ id: "settings.miniapp.field.slugHint" })}
            </p>
          </div>
        </div>

        <div className="space-y-1">
          <label className="text-[11px] text-[var(--text-faint)]">
            {intl.formatMessage({ id: "settings.miniapp.field.database" })}
          </label>
          <select
            value={databaseId}
            onChange={(e) => {
              setDatabaseId(e.target.value);
              if (!e.target.value) setAllowWrite(false);
            }}
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
            <p className="text-[11px] text-[var(--text-faint)]">
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

        {databaseId && (
          <label className="inline-flex items-center gap-2 text-xs text-[var(--text-muted)]">
            <input
              type="checkbox"
              checked={allowWrite}
              onChange={(e) => setAllowWrite(e.target.checked)}
            />
            {intl.formatMessage({ id: "settings.miniapp.field.allowWrite" })}
          </label>
        )}

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
                          {app.databaseId
                            ? intl.formatMessage(
                                { id: "miniapp.boundDatabase" },
                                { name: app.databaseName || app.databaseId },
                              )
                            : intl.formatMessage({ id: "miniapp.noDatabase" })}
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
