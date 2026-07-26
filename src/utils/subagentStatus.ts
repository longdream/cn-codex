import type { ChatMessage, ToolCallItem } from "../stores/appStore";

export type SubagentRecordView = {
  id?: string;
  role?: string;
  status?: string;
  prompt?: string;
  cwd?: string;
  durationMs?: number | null;
  output?: string | null;
  error?: string | null;
};

export type SubagentToolOutput = {
  completed?: boolean;
  missing?: string[];
  agents: SubagentRecordView[];
  target?: string;
  closed?: boolean;
  previousStatus?: string;
  message?: string;
  status?: string;
  note?: string;
  submissionId?: string;
  resumed?: boolean;
  id?: string;
};

export type ActiveSubagentView = {
  id: string;
  role: string;
  status: string;
  prompt?: string;
  durationMs?: number | null;
  output?: string | null;
  error?: string | null;
  updatedAt: number;
  source: "running" | "output" | "live";
};

const SUBAGENT_TOOL_NAMES = new Set([
  "spawn_agent",
  "wait_agent",
  "send_input",
  "resume_agent",
  "list_agents",
  "close_agent",
]);

const TERMINAL_STATUSES = new Set([
  "completed",
  "failed",
  "timed_out",
  "closed",
  "cancelled",
]);

export function isSubagentToolName(name: string): boolean {
  return SUBAGENT_TOOL_NAMES.has(name);
}

export function isTerminalSubagentStatus(status?: string | null): boolean {
  if (!status) return false;
  return TERMINAL_STATUSES.has(status.trim().toLowerCase());
}

export function parseSubagentToolOutput(output?: string): SubagentToolOutput | null {
  if (!output) return null;
  const start = output.indexOf("{");
  if (start < 0) return null;
  try {
    const parsed = JSON.parse(output.slice(start)) as Record<string, unknown>;
    const agentsRaw = Array.isArray(parsed.agents)
      ? parsed.agents
      : parsed.agent && typeof parsed.agent === "object"
        ? [parsed.agent]
        : [];
    const agents = agentsRaw
      .filter((entry): entry is Record<string, unknown> => typeof entry === "object" && entry !== null)
      .map((entry) => ({
        id: typeof entry.id === "string" ? entry.id : undefined,
        role: typeof entry.role === "string" ? entry.role : undefined,
        status: typeof entry.status === "string" ? entry.status : undefined,
        prompt: typeof entry.prompt === "string" ? entry.prompt : undefined,
        cwd: typeof entry.cwd === "string" ? entry.cwd : undefined,
        durationMs: typeof entry.durationMs === "number"
          ? entry.durationMs
          : typeof entry.duration_ms === "number"
            ? entry.duration_ms
            : null,
        output: typeof entry.output === "string" ? entry.output : null,
        error: typeof entry.error === "string" ? entry.error : null,
      }));
    const missing = Array.isArray(parsed.missing)
      ? parsed.missing.filter((item): item is string => typeof item === "string")
      : [];
    return {
      completed: typeof parsed.completed === "boolean" ? parsed.completed : undefined,
      missing,
      agents,
      target: typeof parsed.target === "string" ? parsed.target : undefined,
      closed: typeof parsed.closed === "boolean" ? parsed.closed : undefined,
      previousStatus: typeof parsed.previousStatus === "string"
        ? parsed.previousStatus
        : typeof parsed.previous_status === "string"
          ? parsed.previous_status
          : undefined,
      message: typeof parsed.message === "string" ? parsed.message : undefined,
      status: typeof parsed.status === "string" ? parsed.status : undefined,
      note: typeof parsed.note === "string" ? parsed.note : undefined,
      submissionId: typeof parsed.submissionId === "string"
        ? parsed.submissionId
        : typeof parsed.submission_id === "string"
          ? parsed.submission_id
          : undefined,
      resumed: typeof parsed.resumed === "boolean" ? parsed.resumed : undefined,
      id: typeof parsed.id === "string" ? parsed.id : undefined,
    };
  } catch {
    return null;
  }
}

function parseToolArgs(argumentsText: string): Record<string, unknown> {
  try {
    const parsed = JSON.parse(argumentsText) as unknown;
    return parsed && typeof parsed === "object" && !Array.isArray(parsed)
      ? parsed as Record<string, unknown>
      : {};
  } catch {
    return {};
  }
}

function upsertAgent(
  map: Map<string, ActiveSubagentView>,
  next: ActiveSubagentView,
): void {
  const existing = map.get(next.id);
  if (!existing) {
    map.set(next.id, next);
    return;
  }
  // Prefer newer updates; keep richer role/prompt when the new event omits them.
  if (next.updatedAt < existing.updatedAt) {
    return;
  }
  map.set(next.id, {
    ...existing,
    ...next,
    role: next.role || existing.role,
    prompt: next.prompt || existing.prompt,
    durationMs: next.durationMs ?? existing.durationMs,
    output: next.output ?? existing.output,
    error: next.error ?? existing.error,
  });
}

function agentIdFromArgs(args: Record<string, unknown>): string | undefined {
  const candidates = [args.agent_id, args.target, args.id];
  for (const value of candidates) {
    if (typeof value === "string" && value.trim()) {
      return value.trim();
    }
  }
  return undefined;
}

function applyRecord(
  map: Map<string, ActiveSubagentView>,
  record: SubagentRecordView,
  updatedAt: number,
  fallbackRole?: string,
  fallbackPrompt?: string,
): void {
  const id = record.id?.trim();
  if (!id) return;
  upsertAgent(map, {
    id,
    role: (record.role || fallbackRole || "agent").trim() || "agent",
    status: (record.status || "running").trim() || "running",
    prompt: record.prompt || fallbackPrompt,
    durationMs: record.durationMs,
    output: record.output,
    error: record.error,
    updatedAt,
    source: "output",
  });
}

/**
 * Derive the latest known subagent states from chat tool cards.
 * Tool cards provide historical truth; optional live events overlay newer status.
 */
export function deriveActiveSubagents(
  messages: ChatMessage[],
  options?: {
    includeTerminalMs?: number;
    now?: number;
    liveAgents?: ActiveSubagentView[];
    dismissedIds?: Iterable<string>;
  },
): ActiveSubagentView[] {
  // Terminal agents stay visible briefly so the strip is useful after a turn ends.
  // Running agents are always kept.
  const includeTerminalMs = options?.includeTerminalMs ?? 120_000;
  const now = options?.now ?? Date.now();
  const map = new Map<string, ActiveSubagentView>();
  const dismissed = new Set(
    Array.from(options?.dismissedIds ?? [], (id) => id.trim()).filter(Boolean),
  );

  for (const message of messages) {
    const toolCalls = message.toolCalls;
    if (!toolCalls || toolCalls.length === 0) continue;
    const baseTs = message.timestamp || 0;

    toolCalls.forEach((call, index) => {
      if (!isSubagentToolName(call.name)) return;
      const updatedAt = baseTs + index;
      const args = parseToolArgs(call.arguments);
      const role = typeof args.role === "string" ? args.role : undefined;
      const prompt = typeof args.prompt === "string" ? args.prompt : undefined;
      const parsed = parseSubagentToolOutput(call.output);

      if (call.name === "spawn_agent") {
        if (call.status === "running") {
          const provisionalId =
            agentIdFromArgs(args)
            || (typeof call.displayLabel === "string" && call.displayLabel.trim()
              ? call.displayLabel.trim()
              : `pending-${call.id}`);
          upsertAgent(map, {
            id: provisionalId,
            role: role || "agent",
            status: "running",
            prompt,
            updatedAt,
            source: "running",
          });
        }
        for (const agent of parsed?.agents ?? []) {
          applyRecord(map, agent, updatedAt + 1, role, prompt);
        }
        return;
      }

      if (call.name === "wait_agent" || call.name === "list_agents") {
        for (const agent of parsed?.agents ?? []) {
          applyRecord(map, agent, updatedAt + 1);
        }
        if (call.name === "wait_agent" && call.status === "running") {
          const ids: string[] = [];
          const single = agentIdFromArgs(args);
          if (single) ids.push(single);
          if (Array.isArray(args.agent_ids)) {
            for (const value of args.agent_ids) {
              if (typeof value === "string" && value.trim()) ids.push(value.trim());
            }
          }
          for (const id of ids) {
            upsertAgent(map, {
              id,
              role: map.get(id)?.role || "agent",
              status: "running",
              prompt: map.get(id)?.prompt,
              updatedAt,
              source: "running",
            });
          }
        }
        return;
      }

      if (call.name === "send_input") {
        const target = parsed?.target || agentIdFromArgs(args);
        if (!target) return;
        const existing = map.get(target);
        upsertAgent(map, {
          id: target,
          role: existing?.role || "agent",
          status: parsed?.status || existing?.status || (call.status === "running" ? "running" : "running"),
          prompt: existing?.prompt,
          updatedAt,
          source: call.status === "running" ? "running" : "output",
        });
        return;
      }

      if (call.name === "resume_agent") {
        const target = parsed?.id || agentIdFromArgs(args);
        if (!target) return;
        const existing = map.get(target);
        const agent = parsed?.agents?.[0];
        upsertAgent(map, {
          id: target,
          role: agent?.role || existing?.role || "agent",
          status: parsed?.status || agent?.status || "running",
          prompt: agent?.prompt || existing?.prompt,
          durationMs: agent?.durationMs ?? existing?.durationMs,
          output: agent?.output ?? existing?.output,
          error: agent?.error ?? existing?.error,
          updatedAt,
          source: "output",
        });
        return;
      }

      if (call.name === "close_agent") {
        const target = parsed?.target || agentIdFromArgs(args);
        if (!target) return;
        const existing = map.get(target);
        const agent = parsed?.agents?.[0];
        upsertAgent(map, {
          id: target,
          role: agent?.role || existing?.role || "agent",
          status: parsed?.closed ? "closed" : (agent?.status || existing?.status || "closed"),
          prompt: agent?.prompt || existing?.prompt,
          durationMs: agent?.durationMs ?? existing?.durationMs,
          output: agent?.output ?? existing?.output,
          error: agent?.error ?? existing?.error,
          updatedAt,
          source: "output",
        });
      }
    });
  }

  // Live backend events win when they are newer than tool-card derived state.
  for (const live of options?.liveAgents ?? []) {
    if (!live?.id) continue;
    upsertAgent(map, {
      ...live,
      role: live.role || "agent",
      status: live.status || "running",
      source: "live",
    });
  }

  return Array.from(map.values())
    .filter((agent) => {
      if (dismissed.has(agent.id)) return false;
      if (!isTerminalSubagentStatus(agent.status)) return true;
      // Keep recently finished agents so the strip remains useful right after a turn.
      return now - agent.updatedAt <= includeTerminalMs;
    })
    .sort((a, b) => {
      const aRunning = !isTerminalSubagentStatus(a.status);
      const bRunning = !isTerminalSubagentStatus(b.status);
      if (aRunning !== bRunning) return aRunning ? -1 : 1;
      return b.updatedAt - a.updatedAt;
    });
}

export function countRunningSubagents(agents: ActiveSubagentView[]): number {
  return agents.filter((agent) => !isTerminalSubagentStatus(agent.status)).length;
}

export function subagentStatusTone(status?: string): string {
  switch ((status ?? "").toLowerCase()) {
    case "completed":
    case "closed":
      return "bg-[var(--accent-soft)] text-[var(--accent)]";
    case "failed":
    case "timed_out":
    case "cancelled":
      return "bg-[var(--danger-soft)] text-[var(--danger)]";
    case "running":
    default:
      return "bg-[var(--warning-soft,var(--chat-chip))] text-[var(--warning)]";
  }
}

export function summarizeToolCallAgents(call: ToolCallItem): SubagentRecordView[] {
  return parseSubagentToolOutput(call.output)?.agents ?? [];
}

export type LiveSubagentStatusPayload = {
  threadId?: string;
  id?: string;
  role?: string;
  status?: string;
  prompt?: string;
  durationMs?: number | null;
  output?: string | null;
  error?: string | null;
  updatedAt?: number;
};

/** Normalize a backend subagent-status event into ActiveSubagentView. */
export function liveSubagentFromPayload(
  payload: LiveSubagentStatusPayload,
  fallbackUpdatedAt = Date.now(),
): ActiveSubagentView | null {
  const id = typeof payload.id === "string" ? payload.id.trim() : "";
  if (!id) return null;
  return {
    id,
    role: (typeof payload.role === "string" && payload.role.trim()) || "agent",
    status: (typeof payload.status === "string" && payload.status.trim()) || "running",
    prompt: typeof payload.prompt === "string" ? payload.prompt : undefined,
    durationMs: typeof payload.durationMs === "number" ? payload.durationMs : null,
    output: typeof payload.output === "string" ? payload.output : null,
    error: typeof payload.error === "string" ? payload.error : null,
    updatedAt: typeof payload.updatedAt === "number" && Number.isFinite(payload.updatedAt)
      ? payload.updatedAt
      : fallbackUpdatedAt,
    source: "live",
  };
}
