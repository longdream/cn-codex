import {
  IconArrowUp,
  IconCpu,
  IconFolder,
  IconPlugConnected,
  IconSquare,
} from "@tabler/icons-react";
import { useCallback, useMemo, useRef, useState } from "react";
import { useIntl } from "react-intl";
import { useAppStore } from "../../stores/appStore";
import { SlashCommandPanel, getDefaultSlashCommands } from "./SlashCommandPanel";

interface ChatInputProps {
  onSend: (text: string) => void;
  onInterrupt?: () => void;
  isStreaming: boolean;
  disabled: boolean;
}

export function ChatInput({ onSend, onInterrupt, isStreaming, disabled }: ChatInputProps) {
  const intl = useIntl();
  const [text, setText] = useState("");
  const [showSlash, setShowSlash] = useState(false);
  const textareaRef = useRef<HTMLTextAreaElement>(null);

  const initialized = useAppStore((s) => s.initialized);
  const currentModel = useAppStore((s) => s.currentModel);
  const workspaceCwd = useAppStore((s) => s.workspaceCwd);
  const defaultProvider = intl.formatMessage({ id: "app.defaultProvider" });

  const slashCommands = useMemo(() => {
    const defaults = getDefaultSlashCommands();
    defaults.find((command) => command.name === "clear")!.action = () => {
      useAppStore.getState().setMessages([]);
      useAppStore.getState().clearStreamingText();
    };
    defaults.find((command) => command.name === "model")!.action = () => {
      useAppStore.getState().setShowSettings(true);
    };
    return defaults;
  }, []);

  const handleSubmit = useCallback(() => {
    const trimmed = text.trim();
    if (!trimmed || disabled) return;

    onSend(trimmed);
    setText("");
    setShowSlash(false);
    if (textareaRef.current) {
      textareaRef.current.style.height = "auto";
    }
  }, [disabled, onSend, text]);

  const handleKeyDown = useCallback(
    (event: React.KeyboardEvent) => {
      if (event.key === "Enter" && !event.shiftKey) {
        event.preventDefault();
        handleSubmit();
      }
      if (event.key === "Escape") {
        setShowSlash(false);
      }
    },
    [handleSubmit],
  );

  const handleChange = useCallback((event: React.ChangeEvent<HTMLTextAreaElement>) => {
    const value = event.target.value;
    setText(value);
    setShowSlash(value.startsWith("/") && !value.includes(" "));
  }, []);

  const handleInput = useCallback(() => {
    const element = textareaRef.current;
    if (element) {
      element.style.height = "auto";
      element.style.height = `${Math.min(element.scrollHeight, 200)}px`;
    }
  }, []);

  const cwdLeaf = workspaceCwd
    ? workspaceCwd.split(/[\\/]/).filter(Boolean).pop() ?? workspaceCwd
    : null;

  return (
    <div className="relative flex-shrink-0 border-t border-[var(--border-subtle)] bg-[var(--surface-main)]">
      {showSlash && (
        <SlashCommandPanel
          query={text}
          commands={slashCommands}
          onSelect={(command) => {
            command.action();
            if (command.name === "plan" || command.name === "goal" || command.name === "help") {
              onSend(`/${command.name}`);
            }
            setText("");
            setShowSlash(false);
          }}
          onClose={() => setShowSlash(false)}
        />
      )}

      <div className="mx-auto max-w-3xl px-5 py-2.5">
        <div className="flex items-end rounded-[var(--radius-lg)] border border-[var(--border-subtle)] bg-[var(--surface-contrast)] px-3 py-1.5 focus-within:border-[var(--accent-border)] transition-colors">
          <textarea
            ref={textareaRef}
            value={text}
            onChange={handleChange}
            onKeyDown={handleKeyDown}
            onInput={handleInput}
            placeholder={intl.formatMessage({ id: "chat.placeholder" })}
            disabled={disabled}
            rows={1}
            className="max-h-[200px] min-h-[28px] w-full flex-1 resize-none bg-transparent py-1 text-sm leading-relaxed text-[var(--text-strong)] placeholder:text-[var(--text-faint)] outline-none disabled:opacity-50"
          />

          {isStreaming ? (
            <button
              onClick={onInterrupt}
              className="mb-0.5 ml-2 flex h-[30px] w-[30px] flex-shrink-0 items-center justify-center rounded-full border border-[rgba(239,68,68,0.3)] bg-[var(--danger-soft)] text-[var(--danger)] transition-opacity hover:opacity-80"
            >
              <IconSquare size={12} stroke={2.5} />
            </button>
          ) : (
            <button
              onClick={handleSubmit}
              disabled={disabled || !text.trim()}
              className="mb-0.5 ml-2 flex h-[30px] w-[30px] flex-shrink-0 items-center justify-center rounded-full bg-[var(--accent)] text-white transition-opacity hover:opacity-90 disabled:opacity-30"
            >
              <IconArrowUp size={14} stroke={2.5} />
            </button>
          )}
        </div>

        <div className="mt-1.5 flex items-center justify-between px-1 text-[10px] text-[var(--text-faint)]">
          <div className="flex items-center gap-3">
            <span className="flex items-center gap-1">
              <span className={`h-1.5 w-1.5 rounded-full ${initialized ? "bg-[var(--accent)]" : "bg-[var(--warning)] animate-pulse"}`} />
              <IconPlugConnected size={10} stroke={1.8} />
              {initialized
                ? intl.formatMessage({ id: "status.connected" })
                : intl.formatMessage({ id: "status.initializing" })}
            </span>
            <span className="flex items-center gap-1">
              <IconCpu size={10} stroke={1.8} />
              {currentModel ?? defaultProvider}
            </span>
          </div>
          <div className="flex items-center gap-2">
            {cwdLeaf && (
              <span className="flex items-center gap-1" title={workspaceCwd ?? undefined}>
                <IconFolder size={10} stroke={1.8} />
                {cwdLeaf}
              </span>
            )}
            <span>v0.1.0</span>
          </div>
        </div>
      </div>
    </div>
  );
}
