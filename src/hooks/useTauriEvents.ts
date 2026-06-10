import { useEffect } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import {
  createRunSummaryMessage,
  useAppStore,
  type ChatMode,
  type FileChange,
  type PatchProgressChange,
  type RunSummary,
  type ThreadGoal,
  type TokenUsage,
  type ToolCallItem,
} from "../stores/appStore";

interface TurnEventPayload {
  threadId: string;
  goal?: ThreadGoal | null;
  turn: {
    id: string;
    mode?: ChatMode;
    startedAt?: number;
    completedAt?: number;
    durationMs?: number;
    changedFiles?: FileChange[];
    usage?: TokenUsage | null;
    goalBudgetTokens?: number | null;
    budgetLimited?: boolean | null;
  };
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
    mode: turn.mode === "goal" ? "goal" : "chat",
    startedAt: toTimestamp(turn.startedAt),
    completedAt: toTimestamp(turn.completedAt),
    durationMs: turn.durationMs,
    changedFiles: turn.changedFiles ?? [],
    usage: normalizeTokenUsage(turn.usage),
    goalBudgetTokens: normalizeTokenBudget(turn.goalBudgetTokens),
    budgetLimited: Boolean(turn.budgetLimited),
  };
}

function normalizeTokenUsage(usage?: TokenUsage | null): TokenUsage | undefined {
  if (!usage) {
    return undefined;
  }

  const promptTokens = Number(usage.promptTokens ?? 0);
  const completionTokens = Number(usage.completionTokens ?? 0);
  const totalTokens = Number(usage.totalTokens ?? promptTokens + completionTokens);
  if (promptTokens <= 0 && completionTokens <= 0 && totalTokens <= 0) {
    return undefined;
  }

  return {
    promptTokens: Math.max(0, promptTokens),
    completionTokens: Math.max(0, completionTokens),
    totalTokens: Math.max(0, totalTokens),
  };
}

function normalizeTokenBudget(value?: number | null): number | undefined {
  const budget = Number(value ?? 0);
  if (!Number.isFinite(budget) || budget <= 0) {
    return undefined;
  }
  return Math.floor(budget);
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

export function useTauriEvents() {
  useEffect(() => {
    let cancelled = false;
    const unlisten: UnlistenFn[] = [];

    const setup = async () => {
      const listeners: Array<Promise<UnlistenFn>> = [
        listen<{ delta: string }>("agent-message-delta", (e) => {
          const delta = e.payload.delta;
          useAppStore.getState().appendStreamingText(delta);
        }),

        listen<TurnEventPayload>(
          "turn-started",
          (e) => {
            useAppStore.getState().setCurrentTurnId(e.payload.turn?.id ?? null);
            useAppStore.getState().setStreaming(true);
            useAppStore.getState().clearStreamingText();
          },
        ),

        listen<TurnEventPayload>(
          "turn-completed",
          (e) => {
            const store = useAppStore.getState();
            const text = store.streamingText;
            if (text) {
              store.addMessage({
                id: crypto.randomUUID(),
                role: "assistant",
                content: text,
                timestamp: Date.now(),
              });
            }
            const summary = runSummaryFromTurn(e.payload.turn);
            if (summary) {
              store.addMessage(createRunSummaryMessage(summary));
            }
            if ("goal" in e.payload) {
              store.setCurrentGoal(e.payload.goal ?? null);
            }
            // turn 已结束但仍有 running 工具时，做一次兜底收敛，避免 UI 长时间转圈。
            store.markRunningToolCallsInterrupted(
              "Turn completed before tool status settled.",
            );
            store.clearStreamingText();
            store.setStreaming(false);
            store.setCurrentTurnId(null);
          },
        ),

        listen<{
          threadId: string;
          calls: Array<{ id: string; name: string; arguments: string }>;
        }>("tool-calls-start", (e) => {
          const store = useAppStore.getState();
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

          const browserCall = e.payload.calls.find((c) => c.name === "browser_run");
          if (browserCall) {
            // 自动打开右面板并切到 browser tab
            useAppStore.getState().setRightPanelTab("browser");
            try {
              const parsed = JSON.parse(browserCall.arguments) as Record<string, unknown>;
              const url = typeof parsed.url === "string" && parsed.url.trim()
                ? parsed.url.trim()
                : null;
              useAppStore.getState().setBrowserPanelState({
                url,
                title: null,
                status: "running",
              });
            } catch {
              useAppStore.getState().setBrowserPanelState({
                status: "running",
              });
            }
          }

          store.addMessage({
            id: `tcg-${Date.now()}`,
            role: "system",
            content: "",
            timestamp: Date.now(),
            toolCalls: items,
          });
        }),

        listen<{ threadId: string; callId?: string; tool: string; exitCode?: number; output?: string }>(
          "tool-exec-end",
          (e) => {
            const status = e.payload.exitCode === 0 ? "success" : "failed";
            const store = useAppStore.getState();
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
          const toolId = e.payload.callId ?? e.payload.itemId;
          if (!toolId) {
            return;
          }
          const changes = normalizePatchProgressChanges(e.payload);
          if (changes.length) {
            useAppStore.getState().updateToolCallPatchProgress(toolId, changes);
          }
        }),

        listen<{ threadId: string; results: Array<{ id: string; tool: string; success: boolean; interrupted?: boolean }> }>(
          "tool-calls-end",
          (e) => {
            const store = useAppStore.getState();
            for (const result of e.payload.results ?? []) {
              store.updateToolCallStatus(
                result.id,
                result.success ? "success" : "failed",
                result.interrupted ? "Tool interrupted by user." : undefined,
              );
            }
          },
        ),

        listen<{ error?: { message?: string }; message?: string; threadId?: string }>(
          "server-error",
          (e) => {
            console.error("[event] server-error:", e.payload);
            const msg =
              e.payload.message ??
              e.payload.error?.message ??
              JSON.stringify(e.payload);
            useAppStore.getState().addMessage({
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
          window.dispatchEvent(
            new CustomEvent("cn-codex:server-request", { detail: e.payload }),
          );
        }),
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
  }, []);
}
