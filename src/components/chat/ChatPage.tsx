import { useCallback, useState } from "react";
import { useIntl } from "react-intl";
import { IconFolder, IconSparkles, IconCopy, IconCheck } from "@tabler/icons-react";
import {
  standaloneChat,
  standaloneThreadGoalClear,
  standaloneThreadGoalEdit,
  standaloneThreadGoalStatus,
  standaloneThreadCreate,
} from "../../api";
import {
  useAppStore,
  type ChatMode,
  type ChatSendOptions,
  type ThreadGoal,
} from "../../stores/appStore";
import type { AttachedFile } from "../../types/provider";
import { ChatInput, type ParsedGoalCommand } from "./ChatInput";
import { MessageList } from "./MessageList";

export function ChatPage() {
  const intl = useIntl();
  const currentThreadId = useAppStore((s) => s.currentThreadId);
  const currentProjectId = useAppStore((s) => s.currentProjectId);
  const messages = useAppStore((s) => s.messages);
  const streamingText = useAppStore((s) => s.streamingText);
  const isStreaming = useAppStore((s) => s.isStreaming);
  const initialized = useAppStore((s) => s.initialized);
  const workspaceCwd = useAppStore((s) => s.workspaceCwd);
  const chatMode = useAppStore((s) => s.chatMode);

  const handleSend = useCallback(
    async (
      text: string,
      mode: ChatMode,
      attachments: AttachedFile[] = [],
      options: ChatSendOptions = {},
    ) => {
      const cwd = useAppStore.getState().workspaceCwd;
      if (!cwd) return;
      const displayText = formatUserMessageDisplay(text, attachments);

      let threadId = currentThreadId;
      const userMessage = {
        id: crypto.randomUUID(),
        role: "user" as const,
        content: displayText,
        timestamp: Date.now(),
      };

      if (!threadId) {
        try {
          const resp = await standaloneThreadCreate();
          threadId = resp?.thread?.id ?? null;
          if (threadId) {
            useAppStore.getState().startNewThreadWithMessage(threadId, userMessage);
            useAppStore.getState().addThread({
              id: threadId,
              preview: displayText.slice(0, 60),
              updatedAt: Date.now(),
              archived: false,
              projectId: useAppStore.getState().currentProjectId ?? undefined,
            });
          }
        } catch (err) {
          console.error("Failed to create thread:", err);
          return;
        }
      } else {
        useAppStore.getState().addMessage(userMessage);
      }
      if (!threadId) return;

      try {
        await standaloneChat(
          threadId,
          text,
          cwd,
          mode,
          attachments,
          options.goalBudgetTokens,
        );
      } catch (err) {
        useAppStore.getState().setStreaming(false);
        useAppStore.getState().addMessage({
          id: crypto.randomUUID(),
          role: "assistant",
          content: `Error: ${err}`,
          timestamp: Date.now(),
        });
      }
    },
    [currentThreadId],
  );

  const handleInterrupt = useCallback(async () => {
    useAppStore.getState().setStreaming(false);
  }, []);

  const addSystemMessage = useCallback((content: string) => {
    useAppStore.getState().addMessage({
      id: crypto.randomUUID(),
      role: "system",
      content,
      timestamp: Date.now(),
    });
  }, []);

  const handleGoalCommand = useCallback(async (command: ParsedGoalCommand) => {
    const state = useAppStore.getState();
    const threadId = state.currentThreadId;
    const currentGoal = state.currentGoal;
    if (!threadId) {
      addSystemMessage(intl.formatMessage({ id: "chat.goalCommand.noGoal" }));
      return;
    }

    if (command.action === "show") {
      if (!currentGoal) {
        addSystemMessage(intl.formatMessage({ id: "chat.goalCommand.noGoal" }));
        return;
      }
      addSystemMessage(
        formatGoalSummaryMessage(currentGoal, {
          title: intl.formatMessage({ id: "chat.goalCommand.summaryTitle" }),
          status: intl.formatMessage({ id: "chat.goalCommand.summaryStatus" }),
          objective: intl.formatMessage({ id: "chat.goalCommand.summaryObjective" }),
          tokens: intl.formatMessage({ id: "chat.goalCommand.summaryTokens" }),
          commands: intl.formatMessage({ id: "chat.goalCommand.summaryCommands" }),
          statusLabel: intl.formatMessage({ id: `chat.goalStatus.${currentGoal.status}` }),
        }),
      );
      return;
    }

    try {
      if (command.action === "clear") {
        await standaloneThreadGoalClear(threadId);
        useAppStore.getState().setCurrentGoal(null);
        useAppStore.getState().setChatMode("chat");
        addSystemMessage(intl.formatMessage({ id: "chat.goalCommand.cleared" }));
        return;
      }

      if (command.action === "edit") {
        if (!command.objective) {
          addSystemMessage(intl.formatMessage({
            id: currentGoal
              ? "chat.goalCommand.editNeedsObjective"
              : "chat.goalCommand.noGoal",
          }));
          return;
        }
        const resp = await standaloneThreadGoalEdit(
          threadId,
          command.objective,
          command.goalBudgetTokens,
        );
        useAppStore.getState().setCurrentGoal(resp.goal as ThreadGoal);
        addSystemMessage(intl.formatMessage({ id: "chat.goalCommand.edited" }));
        return;
      }

      const status = command.action === "pause" ? "paused" : "active";
      const resp = await standaloneThreadGoalStatus(threadId, status);
      useAppStore.getState().setCurrentGoal(resp.goal as ThreadGoal);
      addSystemMessage(
        intl.formatMessage({
          id: command.action === "pause"
            ? "chat.goalCommand.paused"
            : "chat.goalCommand.resumed",
        }),
      );
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err);
      addSystemMessage(`Error: ${message}`);
    }
  }, [addSystemMessage, intl]);

  const hasProject = !!currentProjectId && !!workspaceCwd;
  const showEmpty = messages.length === 0 && !isStreaming;

  if (!hasProject) {
    return (
      <div className="flex min-h-0 flex-1 flex-col items-center justify-center px-6 py-12">
        <div className="w-full max-w-sm text-center">
          <div className="mx-auto mb-4 flex h-12 w-12 items-center justify-center rounded-[var(--radius-lg)] bg-[var(--accent-soft)] text-[var(--accent)]">
            <IconFolder size={24} stroke={1.5} />
          </div>
          <p className="text-sm text-[var(--text-muted)]">
            {intl.formatMessage({ id: "project.noProjectSelected" })}
          </p>
        </div>
      </div>
    );
  }

  const [copyDone, setCopyDone] = useState(false);

  const handleCopyAll = useCallback(() => {
    const lines = messages.map((m) => {
      const role = m.role === "user" ? "User" : m.role === "assistant" ? "AI" : m.role;
      let text = `[${role}] ${m.content}`;
      if (m.toolCalls) {
        for (const tc of m.toolCalls) {
          text += `\n  [Tool Call] ${tc.name}(${tc.arguments ?? ""})`;
        }
      }
      if (m.toolName) {
        text = `[Tool: ${m.toolName}] ${m.content}`;
      }
      return text;
    });
    navigator.clipboard.writeText(lines.join("\n\n")).then(() => {
      setCopyDone(true);
      setTimeout(() => setCopyDone(false), 2000);
    });
  }, [messages]);

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      {showEmpty ? (
        <EmptyState />
      ) : (
        <>
          {messages.length > 0 && (
            <div className="flex items-center justify-end px-4 py-1.5 border-b border-[var(--chat-line)]">
              <button
                onClick={handleCopyAll}
                className="flex items-center gap-1.5 rounded-[var(--radius-sm)] px-2.5 py-1 text-[11px] text-[var(--chat-muted)] hover:bg-[var(--chat-chip)] hover:text-[var(--chat-prose)] transition-colors"
                title={intl.formatMessage({ id: "chat.copyAll" })}
              >
                {copyDone ? <IconCheck size={13} stroke={1.8} /> : <IconCopy size={13} stroke={1.8} />}
                {intl.formatMessage({ id: copyDone ? "chat.copied" : "chat.copyAll" })}
              </button>
            </div>
          )}
          <MessageList
            messages={messages}
            streamingText={streamingText}
            isStreaming={isStreaming}
          />
        </>
      )}

      <ChatInput
        onSend={handleSend}
        onInterrupt={handleInterrupt}
        isStreaming={isStreaming}
        disabled={!initialized || !hasProject}
        mode={chatMode}
        onGoalCommand={handleGoalCommand}
      />
    </div>
  );
}

function formatUserMessageDisplay(text: string, attachments: AttachedFile[]): string {
  if (attachments.length === 0) {
    return text;
  }

  const attachmentLines = attachments.map((file) => {
    const kind = file.type.startsWith("image/") ? "Image" : "File";
    return `- ${kind}: ${file.name}`;
  });
  const attachmentText = `Attachments:\n${attachmentLines.join("\n")}`;
  return text.trim() ? `${text}\n\n${attachmentText}` : attachmentText;
}

interface GoalSummaryLabels {
  title: string;
  status: string;
  objective: string;
  tokens: string;
  commands: string;
  statusLabel: string;
}

export function formatGoalSummaryMessage(
  goal: ThreadGoal,
  labels: GoalSummaryLabels,
): string {
  const tokenBudget = Number(goal.tokenBudget ?? 0);
  const tokenBudgetText = tokenBudget > 0
    ? ` / ${formatGoalTokenCount(tokenBudget)}`
    : "";

  return [
    labels.title,
    `${labels.status}: ${labels.statusLabel}`,
    `${labels.objective}: ${goal.objective}`,
    `${labels.tokens}: ${formatGoalTokenCount(goal.tokensUsed)}${tokenBudgetText}`,
    `${labels.commands}: ${goalCommandHint(goal.status)}`,
  ].join("\n");
}

function goalCommandHint(status: ThreadGoal["status"]): string {
  switch (status) {
    case "active":
      return "/goal edit, /goal pause, /goal clear";
    case "paused":
    case "blocked":
    case "usageLimited":
      return "/goal edit, /goal resume, /goal clear";
    case "budgetLimited":
    case "complete":
      return "/goal edit, /goal clear";
  }
}

function formatGoalTokenCount(value?: number | null): string {
  const safe = Number.isFinite(value) ? Math.max(0, Math.round(value ?? 0)) : 0;
  return new Intl.NumberFormat(undefined, { maximumFractionDigits: 0 }).format(safe);
}

function EmptyState() {
  const intl = useIntl();
  const currentModel = useAppStore((s) => s.currentModel);
  const configPath = useAppStore((s) => s.configPath);
  const defaultProvider = intl.formatMessage({ id: "app.defaultProvider" });

  return (
    <div className="thin-scrollbar flex flex-1 items-center justify-center overflow-y-auto px-6 py-12">
      <div className="w-full max-w-lg text-center">
        <div className="mx-auto mb-4 flex h-10 w-10 items-center justify-center rounded-[var(--radius-md)] bg-[var(--accent-soft)] text-[var(--accent)]">
          <IconSparkles size={20} stroke={1.8} />
        </div>
        <h2 className="text-xl font-semibold text-[var(--text-strong)]">
          {intl.formatMessage({ id: "chat.emptyTitle" })}
        </h2>
        <p className="mx-auto mt-2 max-w-md text-sm leading-relaxed text-[var(--text-muted)]">
          {intl.formatMessage({ id: "chat.emptyDescription" })}
        </p>
        <div className="mt-8 grid gap-3 sm:grid-cols-2">
          <div className="rounded-[var(--radius-lg)] border border-[var(--border-subtle)] bg-[var(--surface-panel)] px-4 py-3 text-left">
            <div className="mb-2 text-[var(--text-faint)]">
              <IconSparkles size={16} stroke={1.8} />
            </div>
            <p className="text-[11px] font-medium uppercase tracking-wide text-[var(--text-faint)]">
              {intl.formatMessage({ id: "chat.emptyCardProvider" })}
            </p>
            <p className="mt-1 break-all text-sm text-[var(--text-strong)]">
              {currentModel ?? defaultProvider}
            </p>
          </div>
          <div className="rounded-[var(--radius-lg)] border border-[var(--border-subtle)] bg-[var(--surface-panel)] px-4 py-3 text-left">
            <div className="mb-2 text-[var(--text-faint)]">
              <IconFolder size={16} stroke={1.8} />
            </div>
            <p className="text-[11px] font-medium uppercase tracking-wide text-[var(--text-faint)]">
              {intl.formatMessage({ id: "chat.emptyCardConfig" })}
            </p>
            <p className="mt-1 break-all text-sm text-[var(--text-strong)]">
              {configPath ?? "codey/config.toml"}
            </p>
          </div>
        </div>
      </div>
    </div>
  );
}
