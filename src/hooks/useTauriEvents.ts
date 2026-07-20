import { useEffect } from "react";
import { useIntl, type IntlShape } from "react-intl";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { fileReviewGet } from "../api/fileReview";
import { standaloneConfigWrite, standaloneChat } from "../api";
import {
  createRunSummaryMessage,
  useAppStore,
  type ChatMode,
  type FileChange,
  type FileChangeSnapshot,
  type PendingFileReview,
  type PatchProgressChange,
  type RunSummary,
  type ThreadGoal,
  type TokenUsage,
  type ToolCallItem,
} from "../stores/appStore";
import { resolveApproval } from "../api/approval";
import { decodePathRefRangeSnippet, PATH_REF_MIME } from "../utils/pathRefSnippet";
import { shouldAcceptEventSequence } from "../utils/turnEventSequence";

interface TurnEventPayload {
  threadId: string;
  eventSeq?: number;
  eventId?: string;
  status?: "completed" | "failed" | "cancelled";
  error?: string;
  goal?: ThreadGoal | null;
  turn: {
    id: string;
    mode?: ChatMode;
    cwd?: string;
    startedAt?: number;
    completedAt?: number;
    durationMs?: number;
    changedFiles?: FileChange[];
    changedFileSnapshots?: FileChangeSnapshot[];
    usage?: TokenUsage | null;
    goalBudgetTokens?: number | null;
    budgetLimited?: boolean | null;
  };
}

interface ReasoningDeltaPayload {
  threadId?: string;
  delta?: string;
  eventSeq?: number;
}

interface SequencedEventPayload {
  threadId?: string;
  eventSeq?: number;
  eventId?: string;
}

interface ServerErrorEventPayload {
  error?: { message?: string };
  message?: string;
  threadId?: string;
  retryable?: boolean;
  retryInMs?: number;
  attempt?: number;
  maxAttempts?: number;
}

interface ThreadGoalUpdatedPayload {
  threadId?: string;
  goal?: ThreadGoal | null;
}

interface ThreadGoalClearedPayload {
  threadId?: string;
}

interface RobotProgressUpdatedPayload {
  threadId?: string;
  robotState?: ThreadGoal["workflowProgress"] | null;
}

interface ThreadTokenUsageUpdatedPayload {
  threadId?: string;
  usage?: TokenUsage | null;
  inputTokens?: number;
  outputTokens?: number;
  totalTokens?: number;
  cachedTokens?: number;
  cacheCreationTokens?: number;
  reasoningTokens?: number;
  callCount?: number;
  lastSinglePromptTokens?: number;
  // 统一后的上下文占用分子（单次 prompt tokens），用于避免回退到累计值导致展示漂移。
  contextPromptTokens?: number;
  // 后端运行时上下文窗口大小，优先级高于前端模型静态配置。
  modelContextWindow?: number;
}

interface BrowserNavigationChangedPayload {
  url?: string;
  title?: string;
  canGoBack?: boolean;
  canGoForward?: boolean;
}

interface SmartbrainExtractionStartedPayload {
  source?: string;
  total?: number;
}

interface SmartbrainExtractionProgressPayload {
  source?: string;
  current?: number;
  total?: number;
}

function toTimestamp(value?: number | null): number | undefined {
  if (!value) return undefined;
  return value > 10_000_000_000 ? value : value * 1000;
}

function runSummaryFromTurn(turn: TurnEventPayload["turn"]): RunSummary | null {
  if (turn.durationMs == null && !(turn.changedFiles?.length) && turn.mode !== "goal") {
    return null;
  }

  return {
    turnId: turn.id,
    mode: turn.mode === "goal" ? "goal" : turn.mode === "plan" ? "plan" : "chat",
    cwd: turn.cwd,
    startedAt: toTimestamp(turn.startedAt),
    completedAt: toTimestamp(turn.completedAt),
    durationMs: turn.durationMs,
    changedFiles: turn.changedFiles ?? [],
    changedFileSnapshots: normalizeFileChangeSnapshots(turn.changedFileSnapshots),
    usage: normalizeTokenUsage(turn.usage),
    goalBudgetTokens: normalizeTokenBudget(turn.goalBudgetTokens),
    budgetLimited: Boolean(turn.budgetLimited),
  };
}

function normalizeFileChangeSnapshots(
  snapshots?: FileChangeSnapshot[] | null,
): FileChangeSnapshot[] | undefined {
  if (!Array.isArray(snapshots) || snapshots.length === 0) {
    return undefined;
  }

  const normalized = snapshots
    .map((snapshot) => ({
      path: String(snapshot.path ?? "").trim(),
      action: String(snapshot.action ?? "modified").trim() || "modified",
      beforeContent:
        typeof snapshot.beforeContent === "string" ? snapshot.beforeContent : undefined,
      afterContent:
        typeof snapshot.afterContent === "string" ? snapshot.afterContent : undefined,
    }))
    .filter((snapshot) => snapshot.path.length > 0);

  return normalized.length > 0 ? normalized : undefined;
}

function normalizeTokenUsage(usage?: TokenUsage | null): TokenUsage | undefined {
  if (!usage) {
    return undefined;
  }

  const promptTokens = Number(usage.promptTokens ?? 0);
  const completionTokens = Number(usage.completionTokens ?? 0);
  const totalTokens = Number(usage.totalTokens ?? promptTokens + completionTokens);
  const callCount = Number(usage.callCount ?? 0);
  const lastSinglePromptTokens = Number(usage.lastSinglePromptTokens ?? 0);
  const contextWindowTokens = Number(usage.contextWindowTokens ?? 0);
  const cachedTokens = Number(usage.cachedTokens ?? 0);
  const cacheCreationTokens = Number(usage.cacheCreationTokens ?? 0);
  const reasoningTokens = Number(usage.reasoningTokens ?? 0);
  if (
    promptTokens <= 0 &&
    completionTokens <= 0 &&
    totalTokens <= 0 &&
    callCount <= 0 &&
    lastSinglePromptTokens <= 0 &&
    contextWindowTokens <= 0
  ) {
    return undefined;
  }

  return {
    promptTokens: Math.max(0, promptTokens),
    completionTokens: Math.max(0, completionTokens),
    totalTokens: Math.max(0, totalTokens),
    ...(cachedTokens > 0 ? { cachedTokens: Math.max(0, Math.round(cachedTokens)) } : {}),
    ...(cacheCreationTokens > 0
      ? { cacheCreationTokens: Math.max(0, Math.round(cacheCreationTokens)) }
      : {}),
    ...(reasoningTokens > 0 ? { reasoningTokens: Math.max(0, Math.round(reasoningTokens)) } : {}),
    ...(callCount > 0 ? { callCount: Math.max(0, Math.round(callCount)) } : {}),
    ...(lastSinglePromptTokens > 0
      ? { lastSinglePromptTokens: Math.max(0, Math.round(lastSinglePromptTokens)) }
      : {}),
    ...(contextWindowTokens > 0
      ? { contextWindowTokens: Math.max(1, Math.round(contextWindowTokens)) }
      : {}),
  };
}

function normalizeTokenBudget(value?: number | null): number | undefined {
  const budget = Number(value ?? 0);
  if (!Number.isFinite(budget) || budget <= 0) {
    return undefined;
  }
  return Math.floor(budget);
}

function normalizeRealtimeTokenUsage(
  payload: ThreadTokenUsageUpdatedPayload,
): TokenUsage | undefined {
  if (payload.usage) {
    return normalizeTokenUsage({
      ...payload.usage,
      ...(typeof payload.modelContextWindow === "number"
        ? { contextWindowTokens: payload.modelContextWindow }
        : {}),
    });
  }
  const contextPromptTokens = Number(payload.contextPromptTokens ?? 0);
  return normalizeTokenUsage({
    promptTokens: Number(payload.inputTokens ?? 0),
    completionTokens: Number(payload.outputTokens ?? 0),
    totalTokens: Number(payload.totalTokens ?? 0),
    ...(typeof payload.cachedTokens === "number"
      ? { cachedTokens: payload.cachedTokens }
      : {}),
    ...(typeof payload.cacheCreationTokens === "number"
      ? { cacheCreationTokens: payload.cacheCreationTokens }
      : {}),
    ...(typeof payload.reasoningTokens === "number"
      ? { reasoningTokens: payload.reasoningTokens }
      : {}),
    ...(typeof payload.callCount === "number" ? { callCount: payload.callCount } : {}),
    ...(typeof payload.lastSinglePromptTokens === "number"
      ? { lastSinglePromptTokens: payload.lastSinglePromptTokens }
      : contextPromptTokens > 0
      ? { lastSinglePromptTokens: contextPromptTokens }
      : {}),
    ...(typeof payload.modelContextWindow === "number"
      ? { contextWindowTokens: payload.modelContextWindow }
      : {}),
  });
}

function hasWebFileChanges(changedFiles?: FileChange[] | null): boolean {
  if (!Array.isArray(changedFiles) || changedFiles.length === 0) {
    return false;
  }
  return changedFiles.some((file) => {
    const path = String(file.path ?? "").toLowerCase();
    return [".html", ".htm", ".css", ".js", ".jsx", ".mjs", ".cjs", ".ts", ".tsx", ".vue", ".svelte"]
      .some((ext) => path.endsWith(ext));
  });
}

function toolActivityLabel(
  calls: Array<{ name: string; arguments: string }>,
  intl: IntlShape,
): string {
  if (calls.length === 0) return intl.formatMessage({ id: "tool.executing" });
  const first = calls[0];
  const base = first.name;
  const labelMap: Record<string, string> = {
    shell: intl.formatMessage({ id: "tool.shell" }),
    shell_command: intl.formatMessage({ id: "tool.shell" }),
    exec_command: intl.formatMessage({ id: "tool.shell" }),
    read_file: intl.formatMessage({ id: "tool.readFile" }),
    write_file: intl.formatMessage({ id: "tool.writeFile" }),
    apply_patch: intl.formatMessage({ id: "tool.applyPatch" }),
    list_directory: intl.formatMessage({ id: "tool.listDirectory" }),
    tool_search: intl.formatMessage({ id: "tool.search" }),
    code_review: intl.formatMessage({ id: "tool.codeReview" }),
    browser_run: intl.formatMessage({ id: "tool.browserRun" }),
    image_generate: intl.formatMessage({ id: "tool.imageGenerate" }),
    echarts_report: intl.formatMessage({ id: "tool.echartsReport" }),
    view_image: intl.formatMessage({ id: "tool.viewImage" }),
    spawn_agent: intl.formatMessage({ id: "tool.spawnAgent" }),
    update_plan: intl.formatMessage({ id: "tool.updatePlan" }),
    build_entry_form: intl.formatMessage({ id: "tool.buildEntryForm" }),
    save_form_data: intl.formatMessage({ id: "tool.saveFormData" }),
  };
  const desc = base.startsWith("mcp__")
    ? intl.formatMessage({ id: "tool.mcpCall" })
    : (labelMap[base] ?? base);
  const detail = toolDisplayLabel(first.name, first.arguments);
  const suffix = calls.length > 1 ? ` (+${calls.length - 1})` : "";
  const shortDetail = detail.length > 40 ? detail.slice(0, 37) + "..." : detail;
  return `${desc}: ${shortDetail}${suffix}`;
}

function toolDisplayLabel(name: string, args: string): string {
  if (name === "apply_patch") {
    const path = firstPatchPath(args);
    if (path) {
      return path;
    }
  }

  try {
    const parsed = JSON.parse(args);
    if (name.startsWith("mcp__")) {
      return mcpDirectDisplayLabel(name);
    }
    switch (name) {
      case "shell":
      case "shell_command":
        return Array.isArray(parsed.command)
          ? parsed.command.join(" ")
          : String(parsed.command ?? name);
      case "exec_command":
        return String(parsed.cmd ?? "exec_command");
      case "write_stdin":
      case "close_exec_session":
        return parsed.session_id != null ? `session ${parsed.session_id}` : name;
      case "read_file":
        return parsed.path ?? "read_file";
      case "write_file":
        return parsed.path ?? "write_file";
      case "tool_search":
        return parsed.query ?? "tool_search";
      case "code_review":
        return parsed.base_ref
          ? `vs ${parsed.base_ref}`
          : Array.isArray(parsed.paths) && parsed.paths.length > 0
            ? `${parsed.paths.length} paths`
            : "working tree";
      case "apply_patch":
        return firstPatchPath(parsed.patch ?? parsed.command) ?? "apply_patch";
      case "list_directory":
        return parsed.path ?? ".";
      case "update_plan":
        return Array.isArray(parsed.plan) ? `${parsed.plan.length} steps` : "update_plan";
      case "build_entry_form": {
        const database = typeof parsed.database === "string" ? parsed.database : "";
        const table = typeof parsed.table === "string" ? parsed.table : "";
        if (database && table) return `${database}.${table}`;
        return database || table || "entry form";
      }
      case "save_form_data": {
        const database = typeof parsed.database === "string" ? parsed.database : "";
        const table = typeof parsed.table === "string" ? parsed.table : "";
        if (database && table) return `${database}.${table}`;
        return table || "save_form_data";
      }
      case "request_user_input":
        return Array.isArray(parsed.questions) ? `${parsed.questions.length} question(s)` : "request_user_input";
      case "request_permissions":
        return parsed.reason ?? "permissions";
      case "view_image":
        return parsed.path ?? "view_image";
      case "image_generate":
        return parsed.output_path ?? promptPreview(parsed.prompt) ?? "image_generate";
      case "echarts_report": {
        const title = typeof parsed.title === "string" ? parsed.title.trim() : "";
        if (title) {
          return title;
        }
        const chartType = typeof parsed.chart_type === "string" ? parsed.chart_type.trim() : "";
        return chartType || "echarts_report";
      }
      case "memory_list":
        return parsed.path ?? ".";
      case "memory_read":
      case "memory_write":
      case "memory_update":
      case "memory_forget":
        return parsed.path ?? name;
      case "memory_search":
        return parsed.query ?? "memory_search";
      case "mcp_list_servers":
        return "MCP servers";
      case "mcp_status":
        return parsed.server ?? "all";
      case "mcp_list_tools":
      case "mcp_list_resources":
      case "mcp_list_resource_templates":
      case "mcp_list_prompts":
        return parsed.server ?? "all";
      case "mcp_call_tool":
        return parsed.server && parsed.tool ? `${parsed.server}:${parsed.tool}` : "mcp_call_tool";
      case "mcp_read_resource":
        return parsed.server && parsed.uri ? `${parsed.server}:${parsed.uri}` : "mcp_read_resource";
      case "mcp_get_prompt":
        return parsed.server && parsed.prompt ? `${parsed.server}:${parsed.prompt}` : "mcp_get_prompt";
      case "apps_list":
        return parsed.connector_id ?? "all";
      case "list_available_plugins_to_install":
        return parsed.query ?? "plugins";
      case "request_plugin_install":
        return parsed.tool_id ?? parsed.name ?? "plugin";
      case "plugin_manage":
        return parsed.plugin_id ?? parsed.id ?? parsed.action ?? "plugins";
      case "browser_run": {
        const url = typeof parsed.url === "string" && parsed.url.trim() ? parsed.url : "browser";
        const actionCount = Array.isArray(parsed.actions) ? parsed.actions.length : 0;
        return actionCount > 0 ? `${url} (${actionCount} actions)` : url;
      }
      case "spawn_agent":
        return parsed.role ?? promptPreview(parsed.prompt) ?? "agent";
      case "wait_agent":
        return parsed.agent_id ?? (Array.isArray(parsed.agent_ids) ? `${parsed.agent_ids.length} agents` : "agents");
      case "send_input":
        return parsed.target ?? parsed.agent_id ?? parsed.id ?? "agent";
      case "resume_agent":
        return parsed.id ?? parsed.target ?? parsed.agent_id ?? "agent";
      case "list_agents":
        return parsed.status ?? "agents";
      case "close_agent":
        return parsed.target ?? parsed.agent_id ?? parsed.id ?? "agent";
      case "web_search":
        return parsed.query ?? "web_search";
      case "web_fetch":
        return parsed.url ?? "web_fetch";
      default:
        return name;
    }
  } catch {
    if (name === "apply_patch") {
      return firstPatchPath(args) ?? "apply_patch";
    }
    return name.startsWith("mcp__") ? mcpDirectDisplayLabel(name) : name;
  }
}

function promptPreview(value: unknown): string | null {
  if (typeof value !== "string") {
    return null;
  }
  const normalized = value.replace(/\s+/g, " ").trim();
  if (!normalized) {
    return null;
  }
  return normalized.length > 48 ? `${normalized.slice(0, 48)}...` : normalized;
}

function mcpDirectDisplayLabel(name: string): string {
  const parts = name.slice("mcp__".length).split("__");
  return parts.length >= 2 ? `${parts[0]}:${parts.slice(1).join("__")}` : name;
}

function firstPatchPath(value: unknown): string | null {
  if (typeof value !== "string") {
    return null;
  }

  for (const line of value.split(/\r?\n/)) {
    const match = line.match(/^\*\*\* (?:Add|Update|Delete) File: (.+)$/);
    if (match) {
      return match[1].replace(/\s+\*{3}\s*$/, "").trim();
    }
  }

  return null;
}

function normalizePatchProgressChanges(payload: {
  changes?: Array<Record<string, unknown>>;
  path?: string;
}): PatchProgressChange[] {
  if (Array.isArray(payload.changes)) {
    return payload.changes
      .map((change) => {
        const path = typeof change.path === "string" ? change.path.trim() : "";
        const action = typeof change.action === "string" ? change.action : "modified";
        const moveTo = typeof change.moveTo === "string"
          ? change.moveTo
          : typeof change.move_to === "string"
            ? change.move_to
            : undefined;
        const additions = Number(change.additions);
        const deletions = Number(change.deletions);
        return path
          ? {
            path,
            action,
            ...(moveTo ? { moveTo } : {}),
            ...(Number.isFinite(additions) ? { additions: Math.max(0, additions) } : {}),
            ...(Number.isFinite(deletions) ? { deletions: Math.max(0, deletions) } : {}),
          }
          : null;
      })
      .filter((change): change is PatchProgressChange => change !== null);
  }

  if (typeof payload.path === "string" && payload.path.trim()) {
    return [{ path: payload.path.trim(), action: "modified" }];
  }

  return [];
}

function normalizePendingFileReview(
  review: Awaited<ReturnType<typeof fileReviewGet>>,
): PendingFileReview {
  const files: PendingFileReview["files"] = [];
  if (Array.isArray(review.files)) {
    for (const file of review.files) {
      const path = typeof file.path === "string" ? file.path.trim() : "";
      if (!path) {
        continue;
      }
      files.push({
        path,
        action: typeof file.action === "string" ? file.action : "modified",
        moveTo: typeof file.moveTo === "string" ? file.moveTo : undefined,
        baseContent: typeof file.baseContent === "string" ? file.baseContent : undefined,
        candidateContent:
          typeof file.candidateContent === "string" ? file.candidateContent : undefined,
        editedContent: typeof file.editedContent === "string" ? file.editedContent : undefined,
        keep: file.keep !== false,
      });
    }
  }

  return {
    threadId: review.threadId,
    callId: review.callId,
    rawPatch: typeof review.rawPatch === "string" ? review.rawPatch : "",
    createdAtMs: Number(review.createdAtMs ?? Date.now()),
    updatedAtMs: Number(review.updatedAtMs ?? Date.now()),
    files,
    selectedPath: files[0]?.path ?? null,
    keepAll: files.length > 0 && files.every((file) => file.keep),
    status: "pending",
  };
}

function parseBrowserRunOutput(output?: string): {
  title?: string;
  finalUrl?: string;
} | null {
  if (!output) return null;

  const start = output.indexOf("{");
  const end = output.lastIndexOf("}");
  if (start < 0 || end <= start) return null;

  try {
    const parsed = JSON.parse(output.slice(start, end + 1)) as Record<string, unknown>;
    return {
      title: typeof parsed.title === "string" ? parsed.title : undefined,
      finalUrl: typeof parsed.finalUrl === "string" ? parsed.finalUrl : undefined,
    };
  } catch {
    return null;
  }
}

function optionIsRecommended(option: Record<string, unknown>): boolean {
  if (option.recommended === true || option.isRecommended === true) {
    return true;
  }
  const label = typeof option.label === "string" ? option.label : "";
  const description = typeof option.description === "string" ? option.description : "";
  const lowered = `${label} ${description}`.toLowerCase();
  return lowered.includes("recommended") || label.includes("推荐") || description.includes("推荐");
}

function preferredQuestionOptionLabel(question: Record<string, unknown>): string {
  const options = Array.isArray(question.options)
    ? question.options.filter((item): item is Record<string, unknown> => typeof item === "object" && item !== null)
    : [];
  if (options.length === 0) {
    return "";
  }
  const recommended = options.find(optionIsRecommended);
  if (recommended && typeof recommended.label === "string" && recommended.label.trim()) {
    return recommended.label;
  }
  const first = options.find((option) => typeof option.label === "string" && option.label.trim());
  return typeof first?.label === "string" ? first.label : "";
}

export function useTauriEvents() {
  const intl = useIntl();

  useEffect(() => {
    let cancelled = false;
    const unlisten: UnlistenFn[] = [];
    const reasoningByThread = new Map<string, string>();
    const lastEventSeqByThread = new Map<string, number>();
    const turnPhaseByThread = new Map<string, "created" | "sampling" | "toolRunning" | "completed">();
    const handledTerminalTurnIdsByThread = new Map<string, Set<string>>();

    const claimTerminalTurn = (threadId: string, turnId: string | null): boolean => {
      if (!turnId) {
        return turnPhaseByThread.get(threadId) !== "completed";
      }
      const handled = handledTerminalTurnIdsByThread.get(threadId) ?? new Set<string>();
      if (handled.has(turnId)) return false;
      handled.add(turnId);
      // Terminal IDs only protect against event replay; keep the cache bounded.
      if (handled.size > 16) {
        const oldest = handled.values().next().value;
        if (oldest) handled.delete(oldest);
      }
      handledTerminalTurnIdsByThread.set(threadId, handled);
      return true;
    };

    const acceptSequencedEvent = (payload: SequencedEventPayload, threadId: string): boolean => {
      const sequence = Number(payload.eventSeq ?? 0);
      const previous = lastEventSeqByThread.get(threadId) ?? 0;
      if (!shouldAcceptEventSequence(previous, payload.eventSeq)) {
        console.warn(`[event] ignore duplicate or stale event for thread ${threadId}: ${sequence} <= ${previous}`);
        return false;
      }
      if (!Number.isFinite(sequence) || sequence <= 0) return true;
      lastEventSeqByThread.set(threadId, sequence);
      return true;
    };

    const appendReasoningDelta = (payload: ReasoningDeltaPayload) => {
      const delta = payload.delta ?? "";
      if (!delta) {
        return;
      }
      const store = useAppStore.getState();
      const threadId = payload.threadId ?? store.currentThreadId ?? null;
      if (!threadId) {
        return;
      }
      if (!acceptSequencedEvent(payload, threadId)) {
        return;
      }
      const nextValue = `${reasoningByThread.get(threadId) ?? ""}${delta}`;
      reasoningByThread.set(threadId, nextValue);
      // 仅活跃线程更新 streamingLabel（后台线程无需更新 UI）。
      if (threadId === store.currentThreadId && store.isStreaming) {
        store.setStreamingLabel(`${intl.formatMessage({ id: "tool.reasoning" })}...`);
      }
    };

    /**
     * 后台线程 turn 结束后自动发送队列中的下一条消息。
     * 仅处理非活跃线程——活跃线程的队列发送由 ChatPage 的 effect 负责。
     */
    const autoSendNextQueuedForBackgroundThread = async (threadId: string) => {
      const store = useAppStore.getState();
      // 线程可能已被删除或变为活跃线程
      if (threadId === store.currentThreadId) return;
      const runtime = store.getThreadRuntimeState(threadId);
      if (!runtime || runtime.pendingMessageQueue.length === 0) return;
      // 仍在流式中则不发送（等本次 turn 完全收敛后再推进）
      if (runtime.isStreaming) return;

      const next = store.dequeueMessageForThread(threadId);
      if (!next) return;

      const cwd = store.workspaceCwd || store.projectRoot || store.userHomeDir;
      if (!cwd) return;

      // 标记后台线程开始新的 turn
      store.setStreamingForThread(threadId, true);
      store.clearStreamingTextForThread(threadId);
      store.setLiveTurnUsageForThread(threadId, null);
      store.setStreamingLabelForThread(threadId, intl.formatMessage({ id: "streaming.processing" }));

      try {
        await standaloneChat(
          threadId,
          next.text,
          cwd,
          next.mode,
          next.attachments,
          next.options?.goalBudgetTokens,
          next.options?.robotId,
        );
      } catch (err) {
        console.error(`[background-queue] Failed to send queued message for thread ${threadId}:`, err);
        const latestStore = useAppStore.getState();
        latestStore.setStreamingForThread(threadId, false);
        latestStore.addMessageToThread(threadId, {
          id: crypto.randomUUID(),
          role: "system",
          content: intl.formatMessage({ id: "chat.sendFailed" }),
          timestamp: Date.now(),
        });
      }
    };

    const setup = async () => {
      const listeners: Array<Promise<UnlistenFn>> = [
        listen<{ delta: string; threadId?: string; eventSeq?: number }>("agent-message-delta", (e) => {
          const store = useAppStore.getState();
          const threadId = e.payload.threadId ?? store.currentThreadId;
          if (!threadId) return;
          if (!acceptSequencedEvent(e.payload, threadId)) return;
          if (!store.isThreadStreaming(threadId)) {
            return;
          }
          const runtime = store.getThreadRuntimeState(threadId);
          if (runtime?.currentGoal?.status === "complete") {
            return;
          }
          const currentLen = runtime?.streamingText.length ?? 0;
          if (currentLen === 0) {
            store.setStreamingLabelForThread(threadId, intl.formatMessage({ id: "streaming.generating" }));
          } else if (currentLen > 200 && currentLen <= 220) {
            store.setStreamingLabelForThread(threadId, intl.formatMessage({ id: "streaming.structuring" }));
          } else if (currentLen > 800 && currentLen <= 820) {
            store.setStreamingLabelForThread(threadId, intl.formatMessage({ id: "streaming.summarizing" }));
          }
          store.appendStreamingTextForThread(threadId, e.payload.delta);
        }),

        listen<ReasoningDeltaPayload>("reasoning-text-delta", (e) => {
          appendReasoningDelta(e.payload);
        }),

        listen<ReasoningDeltaPayload>("reasoning-summary-delta", (e) => {
          appendReasoningDelta(e.payload);
        }),

        listen<TurnEventPayload>(
          "turn-started",
          (e) => {
            const store = useAppStore.getState();
            const threadId = e.payload.threadId ?? store.currentThreadId;
            if (!threadId) return;
            if (!acceptSequencedEvent(e.payload, threadId)) return;
            turnPhaseByThread.set(threadId, "sampling");
            store.setCurrentTurnIdForThread(threadId, e.payload.turn?.id ?? null);
            store.setStreamingForThread(threadId, true);
            store.clearStreamingTextForThread(threadId);
            store.setLiveTurnUsageForThread(threadId, null);
            store.setStreamingLabelForThread(threadId, intl.formatMessage({ id: "streaming.processing" }));
            reasoningByThread.set(threadId, "");
            if ("goal" in e.payload) {
              store.setCurrentGoalForThread(threadId, e.payload.goal ?? null);
            }
          },
        ),

        listen<{
          threadId: string;
          turnId?: string;
          eventSeq?: number;
          kind?: "skill" | "mcp";
          phase?: string;
          status?: "started" | "completed" | "failed";
          error?: string | null;
        }>("turn-loading", (e) => {
          const store = useAppStore.getState();
          const threadId = e.payload.threadId ?? store.currentThreadId;
          if (!threadId || !acceptSequencedEvent(e.payload, threadId)) return;
          if (threadId !== store.currentThreadId || !store.isThreadStreaming(threadId)) return;
          if (e.payload.status === "started") {
            const label = intl.formatMessage({
              id: e.payload.kind === "mcp"
                ? "streaming.loadingMcpTools"
                : "streaming.loadingSkills",
            });
            store.setStreamingLabelForThread(threadId, label);
          } else {
            store.setStreamingLabelForThread(threadId, intl.formatMessage({ id: "streaming.processing" }));
          }
        }),

        listen<ThreadTokenUsageUpdatedPayload>("thread-token-usage-updated", (e) => {
          const store = useAppStore.getState();
          const threadId = e.payload.threadId ?? store.currentThreadId;
          if (!threadId) return;
          const usage = normalizeRealtimeTokenUsage(e.payload);
          store.setLiveTurnUsageForThread(threadId, usage ?? null);
        }),

        listen<TurnEventPayload>(
          "turn-completed",
          (e) => {
            const store = useAppStore.getState();
            const threadId = e.payload.threadId ?? store.currentThreadId;
            if (!threadId) return;
            if (!acceptSequencedEvent(e.payload, threadId)) return;
            const isActive = threadId === store.currentThreadId;
            const completedTurnId = e.payload.turn?.id ?? null;
            const runtimeBeforeComplete = store.getThreadRuntimeState(threadId);
            const activeTurnId = runtimeBeforeComplete?.currentTurnId ?? null;
            const isStaleCompletion =
              !!completedTurnId && !!activeTurnId && completedTurnId !== activeTurnId;

            // 旧 turn 的 completed 事件晚到时，不能覆盖新 turn 的运行态。
            // 否则会把输入区错误地打回“空闲发送”按钮。
            if (isStaleCompletion) {
              console.warn(
                `[event] ignore stale turn-completed for thread ${threadId}: completed=${completedTurnId}, active=${activeTurnId}`,
              );
              return;
            }
            if (!claimTerminalTurn(threadId, completedTurnId)) return;
            turnPhaseByThread.set(threadId, "completed");

            // 推理过程消息
            const reasoningText = (reasoningByThread.get(threadId) ?? "").trim();
            if (reasoningText) {
              // 与本轮工具调用合并到同一张卡，避免再多出一张“工具调用”。
              store.appendToolCallsToThread(threadId, [
                {
                  id: `reasoning-call-${crypto.randomUUID()}`,
                  name: "reasoning",
                  arguments: "{}",
                  status: "success",
                  displayLabel: intl.formatMessage({
                    id: "tool.reasoning",
                    defaultMessage: "思考过程",
                  }),
                  output: reasoningText,
                },
              ]);
            }
            reasoningByThread.delete(threadId);

            // 先原子提交流式文本，再追加运行摘要，保持“回答在前、摘要在后”的顺序。
            store.markRunningToolCallsInterruptedForThread(
              threadId,
              "Turn completed before tool status settled.",
            );
            store.flushAndStopStreamingForThread(threadId, { commitStreamingText: true });

            // 运行摘要
            const summary = runSummaryFromTurn(e.payload.turn);
            if (summary) {
              store.addMessageToThread(threadId, createRunSummaryMessage(summary));
            }

            // Web 文件变更刷新（仅活跃线程触发 UI 事件）
            if (isActive && hasWebFileChanges(e.payload.turn.changedFiles)) {
              window.dispatchEvent(new CustomEvent("cn-codex:browser-refresh-requested"));
            }

            if ("goal" in e.payload) {
              store.setCurrentGoalForThread(threadId, e.payload.goal ?? null);
            }
            store.setLiveTurnUsageForThread(threadId, null);

            // 新会话标题回填（仅活跃线程需要检查当前 messages）
            const latestStore = useAppStore.getState();
            const currentSummary = latestStore.threads.find((t) => t.id === threadId);
            if (currentSummary && currentSummary.preview.trim().length === 0) {
              const threadRuntime = latestStore.getThreadRuntimeState(threadId);
              const userMessages = (threadRuntime?.messages ?? []).filter(
                (message) => message.role === "user",
              );
              if (userMessages.length === 1) {
                const preview = userMessages[0].content.trim().slice(0, 60);
                if (preview) {
                  latestStore.updateThreadSummary(threadId, {
                    preview,
                    updatedAt: Date.now(),
                  });
                }
              }
            }

            // 后台队列自动发送：turn 结束后检查该线程是否有排队消息。
            if (!isActive) {
              void autoSendNextQueuedForBackgroundThread(threadId);
            }
          },
        ),

        listen<TurnEventPayload>("turn-cancelled", (e) => {
          const store = useAppStore.getState();
          const threadId = e.payload.threadId ?? store.currentThreadId;
          if (!threadId || !acceptSequencedEvent(e.payload, threadId)) return;
          const terminalTurnId = e.payload.turn?.id ?? null;
          const activeTurnId = store.getThreadRuntimeState(threadId)?.currentTurnId ?? null;
          if (terminalTurnId && activeTurnId && terminalTurnId !== activeTurnId) return;
          if (!claimTerminalTurn(threadId, terminalTurnId)) return;
          turnPhaseByThread.set(threadId, "completed");
          store.markRunningToolCallsInterruptedForThread(threadId, "Turn cancelled by user.");
          store.flushAndStopStreamingForThread(threadId, { commitStreamingText: true });
          store.setLiveTurnUsageForThread(threadId, null);
          reasoningByThread.delete(threadId);
        }),

        listen<TurnEventPayload>("turn-failed", (e) => {
          const store = useAppStore.getState();
          const threadId = e.payload.threadId ?? store.currentThreadId;
          if (!threadId || !acceptSequencedEvent(e.payload, threadId)) return;
          const terminalTurnId = e.payload.turn?.id ?? null;
          const activeTurnId = store.getThreadRuntimeState(threadId)?.currentTurnId ?? null;
          if (terminalTurnId && activeTurnId && terminalTurnId !== activeTurnId) return;
          if (!claimTerminalTurn(threadId, terminalTurnId)) return;
          turnPhaseByThread.set(threadId, "completed");
          store.markRunningToolCallsInterruptedForThread(threadId, "Turn failed before completion.");
          store.flushAndStopStreamingForThread(threadId, { commitStreamingText: true });
          store.setLiveTurnUsageForThread(threadId, null);
          reasoningByThread.delete(threadId);
          store.addMessageToThread(threadId, {
            id: crypto.randomUUID(),
            role: "system",
            content: e.payload.error?.trim() || intl.formatMessage({ id: "chat.sendFailed" }),
            timestamp: Date.now(),
          });
        }),

        listen<ThreadGoalUpdatedPayload>("thread-goal-updated", (e) => {
          const store = useAppStore.getState();
          const threadId = e.payload.threadId ?? store.currentThreadId;
          if (!threadId) return;
          const goal = e.payload.goal ?? null;
          store.setCurrentGoalForThread(threadId, goal);
          if (goal?.status === "complete") {
            store.flushAndStopStreamingForThread(threadId, { commitStreamingText: true });
          }
        }),

        listen<ThreadGoalClearedPayload>("thread-goal-cleared", (e) => {
          const store = useAppStore.getState();
          const threadId = e.payload.threadId ?? store.currentThreadId;
          if (!threadId) return;
          store.setCurrentGoalForThread(threadId, null);
        }),

        listen<RobotProgressUpdatedPayload>("robot-progress-updated", (e) => {
          const store = useAppStore.getState();
          const threadId = e.payload.threadId ?? store.currentThreadId;
          if (!threadId) return;
          const runtime = store.getThreadRuntimeState(threadId);
          if (!runtime?.currentGoal) return;
          store.setCurrentGoalForThread(threadId, {
            ...runtime.currentGoal,
            workflowProgress: e.payload.robotState ?? undefined,
          });
        }),

        listen<BrowserNavigationChangedPayload>("browser-navigation-changed", (e) => {
          const nextUrl = typeof e.payload.url === "string" ? e.payload.url.trim() : "";
          const nextTitle = typeof e.payload.title === "string" ? e.payload.title : "";
          useAppStore.getState().setBrowserPanelState({
            ...(nextUrl ? { url: nextUrl } : {}),
            title: nextTitle,
            ...(typeof e.payload.canGoBack === "boolean"
              ? { canGoBack: e.payload.canGoBack }
              : {}),
            ...(typeof e.payload.canGoForward === "boolean"
              ? { canGoForward: e.payload.canGoForward }
              : {}),
          });
        }),

        listen<{
          threadId: string;
          eventSeq?: number;
          calls: Array<{ id: string; name: string; arguments: string }>;
        }>("tool-calls-start", (e) => {
          const store = useAppStore.getState();
          const threadId = e.payload.threadId ?? store.currentThreadId;
          if (!threadId) return;
          if (!acceptSequencedEvent(e.payload, threadId)) return;
          turnPhaseByThread.set(threadId, "toolRunning");
          const isActive = threadId === store.currentThreadId;

          // 提交待处理的流式文本
          const runtime = store.getThreadRuntimeState(threadId);
          const pendingText = runtime?.streamingText ?? "";
          if (pendingText) {
            store.addMessageToThread(threadId, {
              id: crypto.randomUUID(),
              role: "assistant",
              content: pendingText,
              timestamp: Date.now(),
            });
            store.clearStreamingTextForThread(threadId);
          }

          const items: ToolCallItem[] = e.payload.calls.map((c) => ({
            id: c.id,
            name: c.name,
            arguments: c.arguments,
            status: "running" as const,
            displayLabel: toolDisplayLabel(c.name, c.arguments),
          }));

          // 浏览器面板仅活跃线程触发 UI
          const browserCall = e.payload.calls.find((c) => c.name === "browser_run" || c.name === "web_search");
          if (browserCall && isActive) {
            const latestStore = useAppStore.getState();
            if (!latestStore.showSettings) {
              latestStore.setRightPanelTab("browser");
            }
            try {
              const parsed = JSON.parse(browserCall.arguments) as Record<string, unknown>;
              const url = typeof parsed.url === "string" && parsed.url.trim()
                ? parsed.url.trim()
                : null;
              latestStore.setBrowserPanelState({
                url,
                title: null,
                status: "running",
                canGoBack: false,
                canGoForward: false,
              });
            } catch {
              latestStore.setBrowserPanelState({
                status: "running",
                canGoBack: false,
                canGoForward: false,
              });
            }
            latestStore.setBrowserActive(true);
            latestStore.triggerBrowserSync();
          }

          // 同一用户轮次内多批 tool-calls-start 合并到一张工具调用卡。
          store.appendToolCallsToThread(threadId, items);
          store.setStreamingLabelForThread(threadId, toolActivityLabel(e.payload.calls, intl));

          // 超时兜底：5 分钟后如果工具仍为 running，自动收敛为 failed。
          for (const c of e.payload.calls) {
            setTimeout(() => {
              const latest = useAppStore.getState();
              const latestRuntime = latest.getThreadRuntimeState(threadId);
              for (const msg of latestRuntime?.messages ?? []) {
                const tc = msg.toolCalls?.find((t) => t.id === c.id && t.status === "running");
                if (tc) {
                  latest.updateToolCallStatusForThread(threadId, c.id, "failed", "Tool timed out (5 min).");
                  break;
                }
              }
            }, 5 * 60 * 1000);
          }
        }),

        listen<{ threadId: string; callId?: string; tool: string; exitCode?: number; output?: string }>(
          "tool-exec-end",
          (e) => {
            const store = useAppStore.getState();
            const threadId = e.payload.threadId ?? store.currentThreadId;
            if (!threadId) return;
            const isActive = threadId === store.currentThreadId;
            const status = e.payload.exitCode === 0 ? "success" : "failed";
            const output = e.payload.output;

            if (e.payload.tool === "browser_run" && isActive) {
              const parsed = parseBrowserRunOutput(output);
              store.setBrowserPanelState({
                url: parsed?.finalUrl ?? store.browserPanelUrl,
                title: parsed?.title ?? store.browserPanelTitle,
                status,
              });
            }

            if (e.payload.callId) {
              store.updateToolCallStatusForThread(threadId, e.payload.callId, status, output);
              return;
            }

            // 回退查找：在最近的消息中搜索匹配的 running 工具
            const runtime = store.getThreadRuntimeState(threadId);
            const msgs = runtime?.messages ?? [];
            for (let i = msgs.length - 1; i >= 0; i--) {
              const tc = msgs[i].toolCalls;
              if (!tc) continue;
              const runningIdx = tc.findIndex(
                (c) => c.status === "running" && c.name === e.payload.tool,
              );
              if (runningIdx >= 0) {
                store.updateToolCallStatusForThread(threadId, tc[runningIdx].id, status, output);
                return;
              }
            }
            console.warn("[event] tool-exec-end: no matching running tool found for", e.payload.tool);
          },
        ),

        listen<{ threadId: string; tool: string; exitCode?: number }>(
          "tool-exec-start",
          () => {},
        ),

        listen<{
          threadId?: string;
          callId?: string;
          itemId?: string;
          path?: string;
          changes?: Array<Record<string, unknown>>;
        }>("file-change-patch-updated", (e) => {
          const store = useAppStore.getState();
          const threadId = e.payload.threadId ?? store.currentThreadId;
          if (!threadId) return;
          const toolId = e.payload.callId ?? e.payload.itemId;
          if (!toolId) {
            return;
          }
          const changes = normalizePatchProgressChanges(e.payload);
          if (changes.length) {
            useAppStore.getState().updateToolCallPatchProgressForThread(threadId, toolId, changes);
          }
        }),

        listen<{
          threadId?: string;
          callId?: string;
          itemId?: string;
        }>("file-review-ready", (e) => {
          const store = useAppStore.getState();
          const threadId = e.payload.threadId ?? store.currentThreadId;
          if (!threadId) return;
          const callId = e.payload.callId ?? e.payload.itemId;
          if (!callId) {
            return;
          }
          // 后端只广播“审阅就绪”的轻量事件，完整内容在此按需拉取，
          // 避免大文件内容直接随事件广播导致前端卡顿。
          void fileReviewGet(threadId, callId)
            .then((review) => {
              const latestStore = useAppStore.getState();
              latestStore.upsertPendingFileReviewForThread(threadId, normalizePendingFileReview(review));
            })
            .catch((err) => {
              console.error("[event] file-review-ready: fetch review failed", err);
            });
        }),

        listen<{
          threadId?: string;
          callId?: string;
          status?: string;
          message?: string;
        }>("file-review-updated", (e) => {
          const store = useAppStore.getState();
          const threadId = e.payload.threadId ?? store.currentThreadId;
          if (!threadId) return;
          const callId = e.payload.callId;
          if (!callId) {
            return;
          }
          // 统一处理后端状态机事件：updated/applied/cancelled/failed。
          switch (e.payload.status) {
            case "updated":
              store.setPendingFileReviewStatusForThread(threadId, callId, "pending");
              break;
            case "applied":
            case "cancelled":
              store.removePendingFileReviewForThread(threadId, callId);
              break;
            case "failed":
              store.setPendingFileReviewStatusForThread(
                threadId,
                callId,
                "failed",
                e.payload.message ?? "Review apply failed.",
              );
              break;
            default:
              break;
          }
        }),

        listen<{ snippet?: string }>("document-detail-insert-snippet", (e) => {
          const snippet = e.payload.snippet;
          if (!snippet || !snippet.trim()) {
            return;
          }
          const decodedPathRef = decodePathRefRangeSnippet(snippet);
          if (decodedPathRef) {
            // 行号引用走“附件标签”链路，这样聊天区只展示标签，不注入整段正文。
            useAppStore.getState().addAttachedFile({
              kind: "pathRef",
              name: decodedPathRef.name,
              type: PATH_REF_MIME,
              size: 0,
              sourcePath: decodedPathRef.sourcePath,
              lineStart: decodedPathRef.lineStart,
              lineEnd: decodedPathRef.lineEnd,
            });
            return;
          }
          // 详情窗只负责产生片段，真正写入输入框仍复用主窗既有 queue/consume 链路。
          useAppStore.getState().queueComposerInsert(snippet);
        }),

        listen<{ threadId: string; eventSeq?: number; results: Array<{ id: string; tool: string; success: boolean; interrupted?: boolean }> }>(
          "tool-calls-end",
          (e) => {
            const store = useAppStore.getState();
            const threadId = e.payload.threadId ?? store.currentThreadId;
            if (!threadId) return;
            if (!acceptSequencedEvent(e.payload, threadId)) return;
            turnPhaseByThread.set(threadId, "sampling");
            for (const result of e.payload.results ?? []) {
              store.updateToolCallStatusForThread(
                threadId,
                result.id,
                result.success ? "success" : "failed",
                result.interrupted ? "Tool interrupted by user." : undefined,
              );
            }
            // 工具组已收敛后，旧的工具活动标签不能继续伪装成“正在读取”。
            // 模型可能还在等待下一轮响应，此时保留 streaming，但切回通用处理状态。
            if ((e.payload.results ?? []).length > 0 && store.isThreadStreaming(threadId)) {
              store.setStreamingLabelForThread(
                threadId,
                intl.formatMessage({ id: "streaming.processing" }),
              );
            }
          },
        ),

        listen<{ threadId: string }>("compaction-started", (e) => {
          const store = useAppStore.getState();
          const threadId = e.payload.threadId ?? store.currentThreadId;
          if (!threadId) return;

          // Pre-turn compaction happens before turn-started, so without this
          // explicit busy state the composer spins while the message list looks idle.
          store.setStreamingForThread(threadId, true);
          store.setStreamingLabelForThread(
            threadId,
            intl.formatMessage({ id: "streaming.compacting" }),
          );
        }),

        listen<{
          threadId: string;
          summaryLength: number;
          contextPromptTokens?: number;
          modelContextWindow?: number;
        }>("context-compacted", (e) => {
          const store = useAppStore.getState();
          const threadId = e.payload.threadId ?? store.currentThreadId;
          if (!threadId) return;

          // 压缩事件可能出现在 turn 间隙；此处主动刷新 live 用量，避免 UI 停留在压缩前数据。
          const compactedPromptTokens = Number(e.payload.contextPromptTokens ?? 0);
          const contextWindow = Number(e.payload.modelContextWindow ?? 0);
          if (compactedPromptTokens <= 0 && contextWindow <= 0) {
            return;
          }

          const runtime = store.getThreadRuntimeState(threadId);
          const liveUsage = runtime?.liveTurnUsage ?? null;
          const nextUsage = normalizeTokenUsage({
            promptTokens: compactedPromptTokens > 0
              ? compactedPromptTokens
              : Number(liveUsage?.promptTokens ?? 0),
            completionTokens: Number(liveUsage?.completionTokens ?? 0),
            totalTokens: compactedPromptTokens > 0
              ? compactedPromptTokens
              : Number(liveUsage?.totalTokens ?? 0),
            ...(typeof liveUsage?.callCount === "number"
              ? { callCount: liveUsage.callCount }
              : {}),
            ...(compactedPromptTokens > 0
              ? { lastSinglePromptTokens: compactedPromptTokens }
              : {}),
            ...(contextWindow > 0
              ? { contextWindowTokens: contextWindow }
              : {}),
          });
          if (nextUsage) {
            store.setLiveTurnUsageForThread(threadId, nextUsage);
          }
        }),

        listen<SmartbrainExtractionStartedPayload>(
          "smartbrain-extraction-started",
          (e) => {
            const total = Number(e.payload.total ?? 0);
            useAppStore.getState().setSmartbrainExtractionStatus({
              running: true,
              label: typeof e.payload.source === "string" ? e.payload.source : null,
              progress: total > 0 ? { current: 0, total } : null,
            });
          },
        ),

        listen<SmartbrainExtractionProgressPayload>(
          "smartbrain-extraction-progress",
          (e) => {
            const current = Number(e.payload.current ?? 0);
            const total = Number(e.payload.total ?? 0);
            useAppStore.getState().setSmartbrainExtractionStatus({
              running: true,
              label: typeof e.payload.source === "string" ? e.payload.source : null,
              progress: total > 0
                ? {
                  current: Math.max(0, current),
                  total: Math.max(1, total),
                }
                : null,
            });
          },
        ),

        listen("smartbrain-extraction-completed", () => {
          useAppStore.getState().setSmartbrainExtractionStatus({
            running: false,
            label: null,
            progress: null,
          });
        }),

        listen("smartbrain-extraction-failed", () => {
          useAppStore.getState().setSmartbrainExtractionStatus({
            running: false,
            label: null,
            progress: null,
          });
        }),

        listen<ServerErrorEventPayload>(
          "server-error",
          (e) => {
            const store = useAppStore.getState();
            const threadId = e.payload.threadId ?? store.currentThreadId;
            if (!threadId) return;
            console.error("[event] server-error:", e.payload);
            if (e.payload.retryable) {
              const retryInMs = Number(e.payload.retryInMs ?? 1_000);
              const waitSeconds = Math.max(1, Math.ceil(retryInMs / 1_000));
              const attempt = Number(e.payload.attempt ?? 0);
              const maxAttempts = Number(e.payload.maxAttempts ?? 0);
              const retrySuffix =
                attempt > 0 && maxAttempts > 0 ? ` (${attempt}/${maxAttempts})` : "";
              store.setStreamingForThread(threadId, true);
              store.setStreamingLabelForThread(threadId, `服务暂时不可用，${waitSeconds}s 后重试${retrySuffix}`);
              return;
            }
            const msg =
              e.payload.message ??
              e.payload.error?.message ??
              JSON.stringify(e.payload);
            reasoningByThread.delete(threadId);
            store.setLiveTurnUsageForThread(threadId, null);
            store.markRunningToolCallsInterruptedForThread(threadId, msg);
            store.flushAndStopStreamingForThread(threadId);
            store.addMessageToThread(threadId, {
              id: crypto.randomUUID(),
              role: "system",
              content: `Error: ${msg}`,
              timestamp: Date.now(),
            });
          },
        ),

        listen<{
          requestId?: string | number;
          id?: string | number;
          method?: string;
          params?: Record<string, unknown>;
        }>("server-request", (e) => {
          const method = e.payload.method ?? "";

          const isRobotWait = method === "robot_waiting_for_input";

          const isUserInput =
            method.includes("request_user_input") ||
            method.includes("requestUserInput");

          if (useAppStore.getState().autoApprove && isRobotWait) {
            const requestId = e.payload.requestId ?? e.payload.id ?? "";
            resolveApproval(requestId, { userReply: "请按你提出的推荐方案继续执行。" }).catch((err) =>
              console.error("Auto-approve robot input failed:", err),
            );
            return;
          }

          if (useAppStore.getState().autoApprove && !isUserInput) {
            const reqId = e.payload.requestId ?? e.payload.id ?? "";
            const isPermissions =
              method.includes("request_permissions") ||
              method.includes("requestPermissions");
            const decision = isPermissions
              ? { permissions: e.payload.params?.permissions ?? {}, scope: "turn", strict_auto_review: false }
              : { decision: "accept" };
            resolveApproval(reqId, decision).catch((err) =>
              console.error("Auto-approve failed:", err),
            );
            return;
          }

          // 自动审批 + 用户输入：优先选推荐项，未标记时回退到首项
          if (useAppStore.getState().autoApprove && isUserInput) {
            const questions = e.payload.params?.questions;
            if (
              Array.isArray(questions) &&
              questions.length > 0 &&
              questions.every(
                (q: Record<string, unknown>) =>
                  Array.isArray(q.options) && q.options.length > 0,
              )
            ) {
              const reqId = e.payload.requestId ?? e.payload.id ?? "";
              const answerEntries: Array<[string, { answers: string[] }]> = [];
              questions.forEach((q: Record<string, unknown>) => {
                const questionId = typeof q.id === "string" ? q.id : "";
                if (!questionId) {
                  return;
                }
                const preferredLabel = preferredQuestionOptionLabel(q);
                if (!preferredLabel) {
                  return;
                }
                answerEntries.push([questionId, { answers: [preferredLabel] }]);
              });
              if (answerEntries.length !== questions.length) {
                window.dispatchEvent(
                  new CustomEvent("cn-codex:server-request", { detail: e.payload }),
                );
                return;
              }
              const answers = Object.fromEntries(
                answerEntries,
              );
              resolveApproval(reqId, { answers }).catch((err) =>
                console.error("Auto-approve user input failed:", err),
              );
              return;
            }
          }

          window.dispatchEvent(
            new CustomEvent("cn-codex:server-request", { detail: e.payload }),
          );
        }),

        listen<{ robotId: string; name: string }>("robot-created", (e) => {
          window.dispatchEvent(new CustomEvent("robot-list-changed"));
          const store = useAppStore.getState();
          store.setSelectedRobotId(e.payload.robotId);
          store.setRobotCreateMode(false);
        }),

        listen<{ robotId: string }>("robot-deleted", (e) => {
          window.dispatchEvent(new CustomEvent("robot-list-changed"));
          const store = useAppStore.getState();
          if (store.selectedRobotId === e.payload.robotId) {
            store.setSelectedRobotId(null);
          }
        }),

        // 机器人提问倒计时等待
        listen<{
          threadId: string;
          callId: string;
          countdownMs: number;
          assistantText: string;
        }>("robot-waiting-for-input", (e) => {
          const store = useAppStore.getState();
          const threadId = e.payload.threadId;
          store.setRobotWaitCountdownForThread(threadId, {
            callId: e.payload.callId,
            threadId: e.payload.threadId,
            countdownMs: e.payload.countdownMs,
            startedAt: Date.now(),
            assistantText: e.payload.assistantText,
          });
        }),

        // 机器人倒计时结束（超时或用户回复）
        listen<{
          threadId: string;
          callId: string;
        }>("robot-wait-resolved", (e) => {
          const store = useAppStore.getState();
          // 活跃线程的 robotWaitCountdown 在顶层，后台线程在映射中。
          if (store.robotWaitCountdown?.callId === e.payload.callId) {
            store.setRobotWaitCountdown(null);
          }
          // 检查所有后台线程的 robotWaitCountdown
          for (const [tid, runtime] of Object.entries(store.threadRuntimeStates)) {
            if (runtime.robotWaitCountdown?.callId === e.payload.callId) {
              store.setRobotWaitCountdownForThread(tid, null);
            }
          }
        }),

        listen<{ index: number }>("active-endpoint-index", (e) => {
          const idx = e.payload.index;
          useAppStore.setState({ activeEndpointIndex: idx });
          standaloneConfigWrite([
            { keyPath: "active_endpoint_index", value: idx, mergeStrategy: "replace" },
          ]).catch((err) => console.error("Failed to persist active_endpoint_index:", err));
        }),

        listen<{
          threadId: string;
          path: string;
          content: string;
          updated?: boolean;
          revision?: number | null;
        }>(
          "plan-generated",
          (e) => {
            const store = useAppStore.getState();
            const threadId = e.payload.threadId ?? store.currentThreadId;
            if (!threadId) return;
            const revision = Number(e.payload.revision ?? 0);
            const nextPlan = {
              path: e.payload.path,
              content: e.payload.content,
              ...(Number.isFinite(revision) && revision > 0
                ? { revision: Math.floor(revision) }
                : {}),
              updatedAt: Date.now(),
            };
            store.applyToThread(threadId, (draft) => {
              const activePath = draft.activePlan?.path;
              const targetPath =
                e.payload.updated && activePath
                  ? activePath
                  : nextPlan.path;
              const duplicated = draft.messages.some(
                (message) =>
                  message.planFile?.path === nextPlan.path &&
                  message.planFile?.content === nextPlan.content &&
                  (message.planFile?.revision ?? 0) === (nextPlan.revision ?? 0),
              );
              if (!e.payload.updated && duplicated) {
                return {
                  latestPlanContent: nextPlan.content,
                  activePlan: nextPlan,
                };
              }

              let replaced = false;
              const nextMessages = draft.messages.map((message) => {
                const planPath = message.planFile?.path;
                if (!planPath) {
                  return message;
                }
                const shouldReplace =
                  planPath === targetPath ||
                  planPath === nextPlan.path;
                if (!shouldReplace) {
                  return message;
                }
                replaced = true;
                return {
                  ...message,
                  timestamp: Date.now(),
                  planFile: nextPlan,
                };
              });
              if (!replaced) {
                nextMessages.push({
                  id: `plan-${crypto.randomUUID()}`,
                  role: "assistant",
                  content: "",
                  timestamp: Date.now(),
                  planFile: nextPlan,
                });
              }

              return {
                messages: nextMessages,
                latestPlanContent: nextPlan.content,
                activePlan: nextPlan,
              };
            });
          },
        ),
      ];

      const fns = await Promise.all(listeners);

      if (cancelled) {
        fns.forEach((fn) => fn());
        return;
      }

      unlisten.push(...fns);
    };

    setup();

    return () => {
      cancelled = true;
      unlisten.forEach((fn) => fn());
    };
  }, [intl]);
}
