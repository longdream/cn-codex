import { useMemo, useState } from "react";
import {
  IconChevronDown,
  IconChevronRight,
  IconCpu,
  IconLoader2,
  IconX,
} from "@tabler/icons-react";
import { useIntl } from "react-intl";
import { standaloneSubagentClose } from "../../api/standalone";
import { useAppStore } from "../../stores/appStore";
import { formatDuration } from "../../utils/formatDuration";
import {
  countRunningSubagents,
  deriveActiveSubagents,
  isTerminalSubagentStatus,
  subagentStatusTone,
} from "../../utils/subagentStatus";

function statusLabel(status: string | undefined, intl: ReturnType<typeof useIntl>): string {
  const normalized = (status ?? "running").trim().toLowerCase();
  const key = `tool.subagent.status.${normalized}`;
  try {
    return intl.formatMessage({ id: key, defaultMessage: status ?? "running" });
  } catch {
    return status ?? "running";
  }
}

/**
 * Compact strip above the composer showing active/recent subagents for this chat.
 * Merges tool-card history with live subagent-status events.
 */
export function ActiveSubagentsBar() {
  const intl = useIntl();
  const messages = useAppStore((s) => s.messages);
  const liveSubagents = useAppStore((s) => s.liveSubagents);
  const subagentEnabled = useAppStore((s) => s.subagentEnabled);
  const currentThreadId = useAppStore((s) => s.currentThreadId);
  const removeLiveSubagentForThread = useAppStore((s) => s.removeLiveSubagentForThread);
  const [expanded, setExpanded] = useState(true);
  const [openDetails, setOpenDetails] = useState<Record<string, boolean>>({});
  const [dismissedIds, setDismissedIds] = useState<string[]>([]);
  const [closingIds, setClosingIds] = useState<Record<string, boolean>>({});
  const [actionError, setActionError] = useState<string | null>(null);

  const agents = useMemo(
    () => deriveActiveSubagents(messages, {
      liveAgents: Object.values(liveSubagents ?? {}),
      dismissedIds,
    }),
    [messages, liveSubagents, dismissedIds],
  );

  if (!subagentEnabled || agents.length === 0) {
    return null;
  }

  const runningCount = countRunningSubagents(agents);
  const title = runningCount > 0
    ? intl.formatMessage(
      { id: "chat.subagentBar.running" },
      { count: runningCount },
    )
    : intl.formatMessage(
      { id: "chat.subagentBar.recent" },
      { count: agents.length },
    );

  const dismissAgent = (agentId: string) => {
    setDismissedIds((prev) => (prev.includes(agentId) ? prev : [...prev, agentId]));
    if (currentThreadId) {
      removeLiveSubagentForThread(currentThreadId, agentId);
    }
    setOpenDetails((prev) => {
      if (!prev[agentId]) return prev;
      const next = { ...prev };
      delete next[agentId];
      return next;
    });
  };

  const closeAgent = async (agentId: string) => {
    if (!currentThreadId || closingIds[agentId]) return;
    setActionError(null);
    setClosingIds((prev) => ({ ...prev, [agentId]: true }));
    try {
      await standaloneSubagentClose(currentThreadId, agentId);
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err ?? "unknown error");
      setActionError(
        intl.formatMessage(
          { id: "chat.subagentBar.closeFailed" },
          { error: message },
        ),
      );
    } finally {
      setClosingIds((prev) => {
        const next = { ...prev };
        delete next[agentId];
        return next;
      });
    }
  };

  return (
    <div className="border-t border-[var(--chat-line)] bg-[var(--chat-bg-secondary)]">
      <button
        type="button"
        onClick={() => setExpanded((prev) => !prev)}
        className="flex w-full items-center gap-2 px-4 py-2 text-left text-xs text-[var(--chat-muted)] transition-colors hover:bg-[var(--chat-chip)]/40"
        aria-expanded={expanded}
      >
        <span className="flex h-5 w-5 flex-shrink-0 items-center justify-center rounded-[var(--radius-sm)] bg-[var(--accent-soft)] text-[var(--accent-strong)]">
          {runningCount > 0
            ? <IconLoader2 size={12} stroke={2} className="animate-spin" />
            : <IconCpu size={12} stroke={1.9} />}
        </span>
        <span className="min-w-0 flex-1 truncate font-medium text-[var(--chat-prose)]">
          {title}
        </span>
        <span className="rounded-[var(--radius-sm)] bg-[var(--chat-chip)] px-1.5 py-0.5 text-[10px] text-[var(--chat-faint)]">
          {intl.formatMessage({ id: "tool.subagent.isolated" })}
        </span>
        {expanded
          ? <IconChevronDown size={13} stroke={1.8} className="opacity-50" />
          : <IconChevronRight size={13} stroke={1.8} className="opacity-50" />}
      </button>

      {expanded && (
        <div className="space-y-1.5 px-4 pb-2.5">
          {actionError && (
            <p className="text-[11px] text-[var(--danger)]">{actionError}</p>
          )}
          {agents.map((agent) => {
            const terminal = isTerminalSubagentStatus(agent.status);
            const detailOpen = !!openDetails[agent.id];
            const detailText = agent.error || agent.output || agent.prompt || "";
            const hasDetail = detailText.trim().length > 0;
            const closing = !!closingIds[agent.id];

            return (
              <div
                key={agent.id}
                className="rounded-[var(--radius-sm)] border border-[var(--chat-line)] bg-[var(--chat-bg)] px-2.5 py-1.5"
              >
                <div className="flex items-center gap-2 text-[11px]">
                  {terminal
                    ? <IconCpu size={12} stroke={1.8} className="flex-shrink-0 text-[var(--chat-faint)]" />
                    : <IconLoader2 size={12} stroke={2} className="flex-shrink-0 animate-spin text-[var(--warning)]" />}
                  <span className="min-w-0 flex-1 truncate font-medium text-[var(--chat-prose)]">
                    {agent.role}
                    <span className="ml-1.5 font-mono text-[10px] text-[var(--chat-faint)]">
                      {agent.id}
                    </span>
                  </span>
                  <span className={`rounded-[var(--radius-sm)] px-1.5 py-0.5 text-[10px] ${subagentStatusTone(agent.status)}`}>
                    {closing
                      ? intl.formatMessage({ id: "chat.subagentBar.closing" })
                      : statusLabel(agent.status, intl)}
                  </span>
                  {typeof agent.durationMs === "number" && agent.durationMs >= 0 && (
                    <span className="text-[10px] text-[var(--chat-faint)]">
                      {formatDuration(agent.durationMs)}
                    </span>
                  )}
                  {hasDetail && (
                    <button
                      type="button"
                      onClick={() => setOpenDetails((prev) => ({
                        ...prev,
                        [agent.id]: !prev[agent.id],
                      }))}
                      className="rounded-[var(--radius-sm)] px-1 py-0.5 text-[10px] text-[var(--chat-muted)] transition-colors hover:bg-[var(--chat-chip)] hover:text-[var(--chat-prose)]"
                      title={intl.formatMessage({
                        id: detailOpen
                          ? "chat.subagentBar.collapse"
                          : "chat.subagentBar.expand",
                      })}
                    >
                      {detailOpen
                        ? <IconChevronDown size={12} stroke={1.8} />
                        : <IconChevronRight size={12} stroke={1.8} />}
                    </button>
                  )}
                  {terminal ? (
                    <button
                      type="button"
                      onClick={() => dismissAgent(agent.id)}
                      className="rounded-[var(--radius-sm)] p-0.5 text-[var(--chat-faint)] transition-colors hover:bg-[var(--chat-chip)] hover:text-[var(--chat-prose)]"
                      title={intl.formatMessage({ id: "chat.subagentBar.dismiss" })}
                    >
                      <IconX size={12} stroke={1.8} />
                    </button>
                  ) : (
                    <button
                      type="button"
                      onClick={() => void closeAgent(agent.id)}
                      disabled={closing || !currentThreadId}
                      className="rounded-[var(--radius-sm)] p-0.5 text-[var(--chat-faint)] transition-colors hover:bg-[var(--danger-soft)] hover:text-[var(--danger)] disabled:opacity-40"
                      title={intl.formatMessage({ id: "chat.subagentBar.close" })}
                    >
                      {closing
                        ? <IconLoader2 size={12} stroke={2} className="animate-spin" />
                        : <IconX size={12} stroke={1.8} />}
                    </button>
                  )}
                </div>

                {!detailOpen && agent.prompt && (
                  <p className="mt-1 max-h-[2.6rem] overflow-hidden text-[11px] leading-relaxed text-[var(--chat-muted)]">
                    {agent.prompt}
                  </p>
                )}
                {!detailOpen && agent.error && (
                  <p className="mt-1 max-h-[2.6rem] overflow-hidden text-[11px] text-[var(--danger)]">
                    {agent.error}
                  </p>
                )}
                {!detailOpen && !agent.error && agent.output && terminal && (
                  <p className="mt-1 max-h-[2.6rem] overflow-hidden text-[11px] leading-relaxed text-[var(--chat-muted)]">
                    {agent.output}
                  </p>
                )}

                {detailOpen && hasDetail && (
                  <div className="mt-1.5 space-y-1">
                    {agent.prompt && (
                      <p className="text-[11px] leading-relaxed text-[var(--chat-muted)]">
                        {agent.prompt}
                      </p>
                    )}
                    {agent.error ? (
                      <pre className="max-h-40 overflow-auto whitespace-pre-wrap break-words rounded-[var(--radius-sm)] bg-[var(--danger-soft)]/40 px-2 py-1.5 text-[11px] leading-relaxed text-[var(--danger)]">
                        {agent.error}
                      </pre>
                    ) : agent.output ? (
                      <pre className="max-h-40 overflow-auto whitespace-pre-wrap break-words rounded-[var(--radius-sm)] bg-[var(--chat-chip)]/50 px-2 py-1.5 text-[11px] leading-relaxed text-[var(--chat-prose)]">
                        {agent.output}
                      </pre>
                    ) : null}
                  </div>
                )}
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}
