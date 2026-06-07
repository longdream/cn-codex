import { useCallback } from "react";
import { useIntl } from "react-intl";
import { IconFolder, IconSparkles } from "@tabler/icons-react";
import {
  standaloneChat,
  standaloneThreadCreate,
} from "../../api";
import { useAppStore } from "../../stores/appStore";
import { ChatInput } from "./ChatInput";
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

  const handleSend = useCallback(
    async (text: string) => {
      const cwd = useAppStore.getState().workspaceCwd;
      if (!cwd) return;

      let threadId = currentThreadId;
      const userMessage = {
        id: crypto.randomUUID(),
        role: "user" as const,
        content: text,
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
              preview: text.slice(0, 60),
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
        await standaloneChat(threadId, text, cwd);
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

  const hasProject = !!currentProjectId && !!workspaceCwd;
  const showEmpty = messages.length === 0 && !isStreaming;

  console.log("[ChatPage] render — messages:", messages.length, "streaming:", isStreaming, "threadId:", currentThreadId, "hasProject:", hasProject, "showEmpty:", showEmpty);

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

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      {showEmpty ? (
        <EmptyState />
      ) : (
        <MessageList
          messages={messages}
          streamingText={streamingText}
          isStreaming={isStreaming}
        />
      )}

      <ChatInput
        onSend={handleSend}
        onInterrupt={handleInterrupt}
        isStreaming={isStreaming}
        disabled={!initialized || !hasProject}
      />
    </div>
  );
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
