import { useCallback, useEffect, useRef, useState } from "react";
import { useIntl } from "react-intl";
import { IconClock, IconFolder, IconSparkles, IconCopy, IconCheck } from "@tabler/icons-react";
import { formatDuration } from "../../utils/formatDuration";
import { open } from "@tauri-apps/plugin-dialog";
import {
  standaloneChat,
  standaloneTurnInterrupt,
  standaloneThreadGoalClear,
  standaloneThreadGoalEdit,
  standaloneThreadGoalStatus,
  standaloneThreadCreate,
} from "../../api";
import {
  useAppStore,
  GENERAL_PROJECT_ID,
  type ChatMode,
  type ThreadGoal,
} from "../../stores/appStore";
import type { AttachedFile } from "../../types/provider";
import { ChatInput, type ParsedGoalCommand, type ChatSendExtendedOptions } from "./ChatInput";
import { MessageList } from "./MessageList";

export function ChatPage() {
  const intl = useIntl();
  const currentThreadId = useAppStore((s) => s.currentThreadId);
  const currentProjectId = useAppStore((s) => s.currentProjectId);
  const messages = useAppStore((s) => s.messages);
  const streamingText = useAppStore((s) => s.streamingText);
  const streamingLabel = useAppStore((s) => s.streamingLabel);
  const isStreaming = useAppStore((s) => s.isStreaming);
  const initialized = useAppStore((s) => s.initialized);
  const workspaceCwd = useAppStore((s) => s.workspaceCwd);
  const chatMode = useAppStore((s) => s.chatMode);

  const handleSend = useCallback(
    async (
      text: string,
      mode: ChatMode,
      attachments: AttachedFile[] = [],
      options: ChatSendExtendedOptions = {},
    ) => {
      const state = useAppStore.getState();
      const cwd = state.workspaceCwd || state.projectRoot || state.userHomeDir;
      if (!cwd) return;
      let actualMode: ChatMode | "robot-create" | "robot-modify" = mode;
      if (options.robotCreateMode) {
        actualMode = "robot-create" as ChatMode;
      } else if (options.robotModifyMode) {
        actualMode = "robot-modify" as ChatMode;
      }
      const goalRunning = actualMode === "goal" && state.currentGoal?.status === "active";
      if (goalRunning && state.isStreaming) return;
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
              // 新线程在创建当次就写入 preview，确保侧边栏能立即显示主题。
              preview: displayText.slice(0, 60),
              updatedAt: Date.now(),
              projectId: useAppStore.getState().currentProjectId ?? undefined,
            });
          }
        } catch (err) {
          console.error("Failed to create thread:", err);
          return;
        }
      } else {
        useAppStore.getState().addMessage(userMessage);
        const currentThread = state.threads.find((thread) => thread.id === threadId);
        const shouldSyncPreview =
          state.messages.length === 0 &&
          !!currentThread &&
          currentThread.preview.trim().length === 0;
        if (shouldSyncPreview) {
          // 仅在“空白新会话发送第一条消息”时回填 preview：
          // 1) 修复实时标题显示；
          // 2) 避免批量改动历史会话（按需求仅修复未来会话）。
          useAppStore.getState().updateThreadSummary(threadId, {
            preview: displayText.slice(0, 60),
            updatedAt: Date.now(),
          });
        }
      }
      if (!threadId) return;

      try {
        await standaloneChat(
          threadId,
          text,
          cwd,
          actualMode as "chat" | "plan" | "goal" | "robot-create" | "robot-modify",
          attachments,
          options.goalBudgetTokens,
          options.robotId,
        );
      } catch (err) {
        const store = useAppStore.getState();
        store.setStreaming(false);
        store.addMessage({
          id: crypto.randomUUID(),
          role: "assistant",
          content: intl.formatMessage({ id: "chat.sendFailed" }),
          timestamp: Date.now(),
        });
        // Goal 模式下后端已回退为 paused，前端同步状态
        if (actualMode === "goal" && store.currentGoal) {
          store.setCurrentGoal({ ...store.currentGoal, status: "paused" });
        }
      }
    },
    [currentThreadId, intl],
  );

  const handleInterrupt = useCallback(async () => {
    const store = useAppStore.getState();
    const threadId = store.currentThreadId;
    const currentGoal = store.currentGoal;
    const shouldPauseGoal =
      store.chatMode === "goal" && !!threadId && currentGoal?.status === "active";

    // 先做前端状态收敛，保证点击“停止”后转圈立即结束。
    store.markRunningToolCallsInterrupted(intl.formatMessage({ id: "chat.toolInterrupted" }));
    const partialText = store.streamingText;
    if (partialText) {
      store.addMessage({
        id: crypto.randomUUID(),
        role: "assistant",
        content: `${partialText}\n\n_(${intl.formatMessage({ id: "chat.interrupted" })})_`,
        timestamp: Date.now(),
      });
      store.clearStreamingText();
    }
    store.setStreaming(false);
    store.setCurrentTurnId(null);
    if (shouldPauseGoal && currentGoal) {
      // 先乐观切到 paused，让目标模式按钮立即回到可发送（绿色）状态。
      store.setCurrentGoal({ ...currentGoal, status: "paused" });
    }

    // 对齐 Codex：停止=中断当前执行；目标 active 时额外切到 paused（不是 complete）。
    const interruptPromise = standaloneTurnInterrupt().catch((err) => {
      console.error("Failed to interrupt turn:", err);
    });

    if (shouldPauseGoal && threadId) {
      try {
        const resp = await standaloneThreadGoalStatus(threadId, "paused");
        store.setCurrentGoal(resp.goal as ThreadGoal);
      } catch (err) {
        console.error("Failed to pause active goal after interrupt:", err);
      }
    }

    await interruptPromise;
  }, [intl]);

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

  // 排队消息自动发送：当 isStreaming 从 true 变为 false 时，自动取出队首消息发送
  const wasStreamingRef = useRef(false);
  useEffect(() => {
    if (wasStreamingRef.current && !isStreaming) {
      const store = useAppStore.getState();
      const next = store.dequeueMessage();
      if (next) {
        handleSend(next.text, next.mode, next.attachments, next.options);
      }
    }
    wasStreamingRef.current = isStreaming;
  }, [isStreaming, handleSend]);

  const handleJumpQueue = useCallback(async (messageId: string) => {
    const store = useAppStore.getState();
    const queue = store.pendingMessageQueue;
    const target = queue.find((m) => m.id === messageId);
    if (!target) return;

    store.removeQueuedMessage(messageId);

    if (store.isStreaming) {
      store.markRunningToolCallsInterrupted(intl.formatMessage({ id: "chat.toolInterrupted" }));
      const partialText = store.streamingText;
      if (partialText) {
        store.addMessage({
          id: crypto.randomUUID(),
          role: "assistant",
          content: `${partialText}\n\n_(${intl.formatMessage({ id: "chat.interrupted" })})_`,
          timestamp: Date.now(),
        });
        store.clearStreamingText();
      }
      store.setStreaming(false);
      store.setCurrentTurnId(null);
      standaloneTurnInterrupt().catch((err) => {
        console.error("Failed to interrupt turn:", err);
      });
    }

    // 跳过 wasStreamingRef 的自动触发，直接手动发送目标消息
    wasStreamingRef.current = false;
    handleSend(target.text, target.mode, target.attachments, target.options);
  }, [handleSend, intl]);

  // 注意：所有 Hook 必须在任何条件 return 之前声明，避免项目切换时触发 Hook 顺序错误。
  const [copyDone, setCopyDone] = useState(false);

  const [elapsedMs, setElapsedMs] = useState(0);
  const timerRef = useRef<number | null>(null);
  const lastTickRef = useRef<number>(0);

  useEffect(() => {
    if (isStreaming) {
      lastTickRef.current = Date.now();
      timerRef.current = window.setInterval(() => {
        const now = Date.now();
        setElapsedMs((prev) => prev + (now - lastTickRef.current));
        lastTickRef.current = now;
      }, 1000);
    } else if (timerRef.current !== null) {
      clearInterval(timerRef.current);
      timerRef.current = null;
    }
    return () => {
      if (timerRef.current !== null) {
        clearInterval(timerRef.current);
        timerRef.current = null;
      }
    };
  }, [isStreaming]);

  useEffect(() => {
    setElapsedMs(0);
  }, [currentThreadId]);

  const handleCopyAll = useCallback(() => {
    const userLabel = intl.formatMessage({ id: "chat.role.user" });
    const assistantLabel = intl.formatMessage({ id: "chat.role.assistant" });
    const toolCallLabel = intl.formatMessage({ id: "chat.toolCall" });
    const lines = messages.map((m) => {
      const role = m.role === "user"
        ? userLabel
        : m.role === "assistant"
          ? assistantLabel
          : m.role;
      let text = `[${role}] ${m.content}`;
      if (m.toolCalls) {
        for (const tc of m.toolCalls) {
          text += `\n  [${toolCallLabel}] ${tc.name}(${tc.arguments ?? ""})`;
        }
      }
      return text;
    });
    navigator.clipboard.writeText(lines.join("\n\n")).then(() => {
      setCopyDone(true);
      setTimeout(() => setCopyDone(false), 2000);
    });
  }, [intl, messages]);

  const isGeneralMode = currentProjectId === GENERAL_PROJECT_ID;
  const hasProject = (!!currentProjectId && !!workspaceCwd) || isGeneralMode;
  const effectiveMode = chatMode;
  const showEmpty = messages.length === 0 && !isStreaming;

  const handleExecutePlan = useCallback(
    (planContent: string) => {
      useAppStore.getState().setChatMode("goal");
      const prefix = "请根据以下计划执行实施：\n\n";
      handleSend(`${prefix}${planContent}`, "goal");
    },
    [handleSend],
  );

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

  if (!hasProject) {
    return (
      <div
        onClick={handleAddProject}
        title={intl.formatMessage({ id: "chat.selectProjectDir" })}
        className="flex min-h-0 flex-1 cursor-pointer flex-col items-center justify-center px-6 py-12 transition-colors hover:bg-[var(--accent-soft)]/20"
      >
        <div className="w-full max-w-sm text-center">
          <div className="mx-auto mb-4 flex h-12 w-12 items-center justify-center rounded-[var(--radius-lg)] bg-[var(--accent-soft)] text-[var(--accent)]">
            <IconFolder size={24} stroke={1.5} />
          </div>
          <p className="text-[13px] text-[var(--text-muted)]">
            {intl.formatMessage({ id: "project.noProjectSelected" })}
          </p>
        </div>
      </div>
    );
  }

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      {messages.length > 0 && (
        <div className="flex items-center justify-end gap-2 px-4 py-1.5 border-b border-[var(--chat-line)]">
          {elapsedMs > 0 && (
            <span
              className="flex items-center gap-1 font-mono text-[11px] text-[var(--chat-muted)] select-none"
              title={intl.formatMessage({ id: "chat.elapsedTooltip" })}
            >
              <IconClock size={12} stroke={1.5} />
              {formatDuration(elapsedMs)}
            </span>
          )}
          <button
            onClick={handleCopyAll}
            className="chat-copy-button flex items-center justify-center h-6 w-6 transition-[color,background]"
            title={intl.formatMessage({ id: copyDone ? "chat.copied" : "chat.copyAll" })}
          >
            {copyDone ? <IconCheck size={14} stroke={2} /> : <IconCopy size={14} stroke={2} />}
          </button>
        </div>
      )}

      {showEmpty ? (
        <EmptyState />
      ) : (
        <>
          <MessageList
            messages={messages}
            streamingText={streamingText}
            streamingLabel={streamingLabel}
            isStreaming={isStreaming}
            onExecutePlan={handleExecutePlan}
          />
        </>
      )}

      <ChatInput
        onSend={handleSend}
        onInterrupt={handleInterrupt}
        onJumpQueue={handleJumpQueue}
        isStreaming={isStreaming}
        disabled={!initialized || !hasProject}
        mode={effectiveMode}
        onGoalCommand={handleGoalCommand}
        isGeneralMode={isGeneralMode}
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
        <h2 className="text-[13px] font-semibold text-[var(--text-strong)]">
          {intl.formatMessage({ id: "chat.emptyTitle" })}
        </h2>
        <p className="mx-auto mt-2 max-w-md text-xs leading-relaxed text-[var(--text-muted)]">
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
            <p className="mt-1 break-all text-xs text-[var(--text-strong)]">
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
            <p className="mt-1 break-all text-xs text-[var(--text-strong)]">
              {configPath ?? "codey/config.toml"}
            </p>
          </div>
        </div>
      </div>
    </div>
  );
}
