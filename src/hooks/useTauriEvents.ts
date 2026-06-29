import { useEffect } from "react";
import { useIntl, type IntlShape } from "react-intl";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { fileReviewGet } from "../api/fileReview";
import { standaloneConfigWrite } from "../api";
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

interface TurnEventPayload {
  threadId: string;
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
  if (
    promptTokens <= 0 &&
    completionTokens <= 0 &&
    totalTokens <= 0 &&
    callCount <= 0 &&
    lastSinglePromptTokens <= 0
  ) {
    return undefined;
  }

  return {
    promptTokens: Math.max(0, promptTokens),
    completionTokens: Math.max(0, completionTokens),
    totalTokens: Math.max(0, totalTokens),
    ...(callCount > 0 ? { callCount: Math.max(0, Math.round(callCount)) } : {}),
    ...(lastSinglePromptTokens > 0
      ? { lastSinglePromptTokens: Math.max(0, Math.round(lastSinglePromptTokens)) }
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
      return match[1].trim();
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
        return path ? { path, action, ...(moveTo ? { moveTo } : {}) } : null;
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
      if (payload.threadId && payload.threadId !== store.currentThreadId) {
        return;
      }
      const nextValue = `${reasoningByThread.get(threadId) ?? ""}${delta}`;
      reasoningByThread.set(threadId, nextValue);
      if (store.isStreaming) {
        store.setStreamingLabel(`${intl.formatMessage({ id: "tool.reasoning" })}...`);
      }
    };

    const setup = async () => {
      const listeners: Array<Promise<UnlistenFn>> = [
        listen<{ delta: string; threadId?: string }>("agent-message-delta", (e) => {
          const store = useAppStore.getState();
          if (e.payload.threadId && e.payload.threadId !== store.currentThreadId) {
            return;
          }
          if (!store.isStreaming) {
            return;
          }
          if (store.currentGoal?.status === "complete") {
            return;
          }
          const currentLen = store.streamingText.length;
          if (currentLen === 0) {
            store.setStreamingLabel(intl.formatMessage({ id: "streaming.generating" }));
          } else if (currentLen > 200 && currentLen <= 220) {
            store.setStreamingLabel(intl.formatMessage({ id: "streaming.structuring" }));
          } else if (currentLen > 800 && currentLen <= 820) {
            store.setStreamingLabel(intl.formatMessage({ id: "streaming.summarizing" }));
          }
          store.appendStreamingText(e.payload.delta);
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
            if (e.payload.threadId && e.payload.threadId !== store.currentThreadId) {
              return;
            }
            store.setCurrentTurnId(e.payload.turn?.id ?? null);
            store.setStreaming(true);
            store.clearStreamingText();
            store.setStreamingLabel(intl.formatMessage({ id: "streaming.processing" }));
            if (e.payload.threadId) {
              reasoningByThread.set(e.payload.threadId, "");
            }
            if ("goal" in e.payload) {
              store.setCurrentGoal(e.payload.goal ?? null);
            }
          },
        ),

        listen<TurnEventPayload>(
          "turn-completed",
          (e) => {
            const store = useAppStore.getState();
            if (e.payload.threadId && e.payload.threadId !== store.currentThreadId) {
              return;
            }
            const threadId = e.payload.threadId;
            if (threadId) {
              const reasoningText = (reasoningByThread.get(threadId) ?? "").trim();
              if (reasoningText) {
                store.addMessage({
                  id: `reasoning-${crypto.randomUUID()}`,
                  role: "system",
                  content: "",
                  timestamp: Date.now(),
                  toolCalls: [
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
                  ],
                });
              }
              reasoningByThread.delete(threadId);
            }
            if (store.isStreaming) {
              const text = store.streamingText;
              if (text) {
                store.addMessage({
                  id: crypto.randomUUID(),
                  role: "assistant",
                  content: text,
                  timestamp: Date.now(),
                });
              }
            }
            const summary = runSummaryFromTurn(e.payload.turn);
            if (summary) {
              store.addMessage(createRunSummaryMessage(summary));
            }
            if (hasWebFileChanges(e.payload.turn.changedFiles)) {
              window.dispatchEvent(new CustomEvent("cn-codex:browser-refresh-requested"));
            }
            if ("goal" in e.payload) {
              store.setCurrentGoal(e.payload.goal ?? null);
            }
            const latestStore = useAppStore.getState();
            const currentSummary = latestStore.threads.find((thread) => thread.id === e.payload.threadId);
            if (currentSummary && currentSummary.preview.trim().length === 0) {
              const userMessages = latestStore.messages.filter((message) => message.role === "user");
              if (userMessages.length === 1) {
                const preview = userMessages[0].content.trim().slice(0, 60);
                if (preview) {
                  // turn-completed 兜底：只在“仅有 1 条 user 消息”的新会话场景回填标题，
                  // 既保证实时可见，又避免把历史旧会话批量回填（符合“仅修复未来会话”约束）。
                  latestStore.updateThreadSummary(e.payload.threadId, {
                    preview,
                    updatedAt: Date.now(),
                  });
                }
              }
            }
            // turn 已结束但仍有 running 工具时，做一次兜底收敛，避免 UI 长时间转圈。
            store.markRunningToolCallsInterrupted(
              "Turn completed before tool status settled.",
            );
            store.flushAndStopStreaming();
          },
        ),

        listen<ThreadGoalUpdatedPayload>("thread-goal-updated", (e) => {
          const store = useAppStore.getState();
          if (e.payload.threadId && e.payload.threadId !== store.currentThreadId) {
            return;
          }
          const goal = e.payload.goal ?? null;
          store.setCurrentGoal(goal);
          if (goal?.status === "complete") {
            store.flushAndStopStreaming({ commitStreamingText: true });
          }
        }),

        listen<ThreadGoalClearedPayload>("thread-goal-cleared", (e) => {
          const store = useAppStore.getState();
          if (e.payload.threadId && e.payload.threadId !== store.currentThreadId) {
            return;
          }
          store.setCurrentGoal(null);
        }),

        listen<{
          threadId: string;
          calls: Array<{ id: string; name: string; arguments: string }>;
        }>("tool-calls-start", (e) => {
          const store = useAppStore.getState();
          if (e.payload.threadId && e.payload.threadId !== store.currentThreadId) {
            return;
          }
          const pendingText = store.streamingText;
          if (pendingText) {
            store.addMessage({
              id: crypto.randomUUID(),
              role: "assistant",
              content: pendingText,
              timestamp: Date.now(),
            });
            store.clearStreamingText();
          }

          const items: ToolCallItem[] = e.payload.calls.map((c) => ({
            id: c.id,
            name: c.name,
            arguments: c.arguments,
            status: "running" as const,
            displayLabel: toolDisplayLabel(c.name, c.arguments),
          }));

          const browserCall = e.payload.calls.find((c) => c.name === "browser_run" || c.name === "web_search");
          if (browserCall) {
            // 设置面板打开时不自动拉起右侧栏，避免内置浏览器遮挡设置层。
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
              });
            } catch {
              latestStore.setBrowserPanelState({
                status: "running",
              });
            }
            latestStore.setBrowserActive(true);
            latestStore.triggerBrowserSync();
          }

          store.addMessage({
            id: `tcg-${Date.now()}`,
            role: "system",
            content: "",
            timestamp: Date.now(),
            toolCalls: items,
          });
          store.setStreamingLabel(toolActivityLabel(e.payload.calls, intl));
        }),

        listen<{ threadId: string; callId?: string; tool: string; exitCode?: number; output?: string }>(
          "tool-exec-end",
          (e) => {
            const store = useAppStore.getState();
            if (e.payload.threadId && e.payload.threadId !== store.currentThreadId) {
              return;
            }
            const status = e.payload.exitCode === 0 ? "success" : "failed";
            const output = e.payload.output;

            if (e.payload.tool === "browser_run") {
              const parsed = parseBrowserRunOutput(output);
              store.setBrowserPanelState({
                url: parsed?.finalUrl ?? store.browserPanelUrl,
                title: parsed?.title ?? store.browserPanelTitle,
                status,
              });
            }

            if (e.payload.callId) {
              store.updateToolCallStatus(e.payload.callId, status, output);
              return;
            }

            const msgs = store.messages;
            for (let i = msgs.length - 1; i >= 0; i--) {
              const tc = msgs[i].toolCalls;
              if (!tc) continue;
              const runningIdx = tc.findIndex(
                (c) => c.status === "running" && c.name === e.payload.tool,
              );
              if (runningIdx >= 0) {
                store.updateToolCallStatus(tc[runningIdx].id, status, output);
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
          if (e.payload.threadId && e.payload.threadId !== store.currentThreadId) {
            return;
          }
          const toolId = e.payload.callId ?? e.payload.itemId;
          if (!toolId) {
            return;
          }
          const changes = normalizePatchProgressChanges(e.payload);
          if (changes.length) {
            useAppStore.getState().updateToolCallPatchProgress(toolId, changes);
          }
        }),

        listen<{
          threadId?: string;
          callId?: string;
          itemId?: string;
        }>("file-review-ready", (e) => {
          const store = useAppStore.getState();
          if (e.payload.threadId && e.payload.threadId !== store.currentThreadId) {
            return;
          }
          const threadId = e.payload.threadId;
          const callId = e.payload.callId ?? e.payload.itemId;
          if (!threadId || !callId) {
            return;
          }
          // 后端只广播“审阅就绪”的轻量事件，完整内容在此按需拉取，
          // 避免大文件内容直接随事件广播导致前端卡顿。
          void fileReviewGet(threadId, callId)
            .then((review) => {
              const latestStore = useAppStore.getState();
              if (threadId && latestStore.currentThreadId && threadId !== latestStore.currentThreadId) {
                return;
              }
              latestStore.upsertPendingFileReview(normalizePendingFileReview(review));
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
          if (e.payload.threadId && e.payload.threadId !== store.currentThreadId) {
            return;
          }
          const callId = e.payload.callId;
          if (!callId) {
            return;
          }
          // 统一处理后端状态机事件：updated/applied/cancelled/failed。
          // 这样即使用户在别的入口触发 apply/cancel，本地 UI 也能实时收敛。
          switch (e.payload.status) {
            case "updated":
              store.setPendingFileReviewStatus(callId, "pending");
              break;
            case "applied":
            case "cancelled":
              store.removePendingFileReview(callId);
              break;
            case "failed":
              store.setPendingFileReviewStatus(
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
          // 详情窗只负责产生片段，真正写入输入框仍复用主窗既有 queue/consume 链路。
          useAppStore.getState().queueComposerInsert(snippet);
        }),

        listen<{ threadId: string; results: Array<{ id: string; tool: string; success: boolean; interrupted?: boolean }> }>(
          "tool-calls-end",
          (e) => {
            const store = useAppStore.getState();
            if (e.payload.threadId && e.payload.threadId !== store.currentThreadId) {
              return;
            }
            for (const result of e.payload.results ?? []) {
              store.updateToolCallStatus(
                result.id,
                result.success ? "success" : "failed",
                result.interrupted ? "Tool interrupted by user." : undefined,
              );
            }
          },
        ),

        listen<{ threadId: string }>("compaction-started", () => {}),

        listen<{ threadId: string; summaryLength: number }>("context-compacted", () => {}),

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
            if (e.payload.threadId && e.payload.threadId !== store.currentThreadId) {
              return;
            }
            console.error("[event] server-error:", e.payload);
            if (e.payload.retryable) {
              const retryInMs = Number(e.payload.retryInMs ?? 1_000);
              const waitSeconds = Math.max(1, Math.ceil(retryInMs / 1_000));
              const attempt = Number(e.payload.attempt ?? 0);
              const maxAttempts = Number(e.payload.maxAttempts ?? 0);
              const retrySuffix =
                attempt > 0 && maxAttempts > 0 ? ` (${attempt}/${maxAttempts})` : "";
              store.setStreaming(true);
              store.setStreamingLabel(`429 限流，${waitSeconds}s 后重试${retrySuffix}`);
              return;
            }
            const msg =
              e.payload.message ??
              e.payload.error?.message ??
              JSON.stringify(e.payload);
            if (e.payload.threadId) {
              reasoningByThread.delete(e.payload.threadId);
            }
            store.markRunningToolCallsInterrupted(msg);
            store.flushAndStopStreaming();
            store.addMessage({
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
          const isUserInput =
            method.includes("request_user_input") ||
            method.includes("requestUserInput");

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
            if (e.payload.threadId && e.payload.threadId !== store.currentThreadId) {
              return;
            }
            const revision = Number(e.payload.revision ?? 0);
            const nextPlan = {
              path: e.payload.path,
              content: e.payload.content,
              ...(Number.isFinite(revision) && revision > 0
                ? { revision: Math.floor(revision) }
                : {}),
              updatedAt: Date.now(),
            };
            useAppStore.setState((state) => {
              const activePath = state.activePlan?.path;
              const targetPath =
                e.payload.updated && activePath
                  ? activePath
                  : nextPlan.path;
              const duplicated = state.messages.some(
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
              const nextMessages = state.messages.map((message) => {
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
