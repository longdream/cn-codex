import {
  IconArrowUp,
  IconChevronDown,
  IconCpu,
  IconFolder,
  IconPhoto,
  IconPlugConnected,
  IconSquare,
  IconX,
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
  const [showModelMenu, setShowModelMenu] = useState(false);
  const [attachedImage, setAttachedImage] = useState<string | null>(null);
  const [visionWarning, setVisionWarning] = useState(false);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);

  const initialized = useAppStore((s) => s.initialized);
  const currentModel = useAppStore((s) => s.currentModel);
  const workspaceCwd = useAppStore((s) => s.workspaceCwd);
  const configuredModels = useAppStore((s) => s.configuredModels);
  const activeModelId = useAppStore((s) => s.activeModelId);
  const defaultProvider = intl.formatMessage({ id: "app.defaultProvider" });

  const activeEntry = configuredModels.find((m) => m.id === activeModelId) ?? null;
  const displayModel = activeEntry?.label ?? currentModel ?? defaultProvider;

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
    setAttachedImage(null);
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
        setShowModelMenu(false);
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

  const handleImageClick = useCallback(() => {
    if (!activeEntry?.supportsVision) {
      setVisionWarning(true);
      setTimeout(() => setVisionWarning(false), 4000);
      return;
    }
    fileInputRef.current?.click();
  }, [activeEntry]);

  const handleFileChange = useCallback((event: React.ChangeEvent<HTMLInputElement>) => {
    const file = event.target.files?.[0];
    if (!file) return;
    const reader = new FileReader();
    reader.onload = () => {
      setAttachedImage(reader.result as string);
    };
    reader.readAsDataURL(file);
    event.target.value = "";
  }, []);

  const handleModelSelect = useCallback((modelId: string) => {
    useAppStore.getState().setActiveModelId(modelId);
    setShowModelMenu(false);
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

      {showModelMenu && configuredModels.length > 0 && (
        <div className="absolute bottom-full left-0 right-0 z-20 mx-auto max-w-3xl px-5 pb-1">
          <div className="rounded-[var(--radius-md)] border border-[var(--border-subtle)] bg-[var(--surface-panel)] shadow-lg">
            <div className="px-3 py-2 text-[10px] font-medium uppercase tracking-wider text-[var(--text-faint)]">
              {intl.formatMessage({ id: "chat.modelSelectorHint" })}
            </div>
            <div className="max-h-[200px] overflow-y-auto">
              {configuredModels.map((m) => (
                <button
                  key={m.id}
                  onClick={() => handleModelSelect(m.id)}
                  className={`flex w-full items-center gap-2.5 px-3 py-2 text-left text-sm transition-colors hover:bg-[var(--surface-elevated)] ${
                    m.id === activeModelId ? "bg-[var(--accent-soft)] text-[var(--accent-strong)]" : "text-[var(--text-base)]"
                  }`}
                >
                  <IconCpu size={13} stroke={1.8} className="flex-shrink-0 opacity-60" />
                  <div className="min-w-0 flex-1">
                    <span className="block truncate font-medium">{m.label}</span>
                    <span className="block truncate text-[11px] text-[var(--text-faint)]">
                      {m.provider}
                      {m.supportsVision && " · 📷 vision"}
                    </span>
                  </div>
                  {m.id === activeModelId && (
                    <span className="h-1.5 w-1.5 flex-shrink-0 rounded-full bg-[var(--accent)]" />
                  )}
                </button>
              ))}
            </div>
          </div>
        </div>
      )}

      {visionWarning && (
        <div className="absolute bottom-full left-0 right-0 z-10 mx-auto max-w-3xl px-5 pb-1">
          <div className="rounded-[var(--radius-sm)] border border-[rgba(239,180,40,0.3)] bg-[rgba(239,180,40,0.1)] px-3 py-2 text-xs text-[var(--warning)]">
            {intl.formatMessage({ id: "chat.visionNotSupported" })}
          </div>
        </div>
      )}

      <div className="mx-auto max-w-3xl px-5 py-2.5">
        {attachedImage && (
          <div className="mb-2 flex items-center gap-2">
            <div className="group relative h-16 w-16 overflow-hidden rounded-[var(--radius-sm)] border border-[var(--border-subtle)]">
              <img src={attachedImage} alt="" className="h-full w-full object-cover" />
              <button
                onClick={() => setAttachedImage(null)}
                className="absolute inset-0 flex items-center justify-center bg-black/50 opacity-0 transition-opacity group-hover:opacity-100"
                title={intl.formatMessage({ id: "chat.removeImage" })}
              >
                <IconX size={16} stroke={2} className="text-white" />
              </button>
            </div>
            <span className="text-[11px] text-[var(--text-faint)]">
              {intl.formatMessage({ id: "chat.imageAttached" })}
            </span>
          </div>
        )}

        <div className="flex items-end rounded-[var(--radius-lg)] border border-[var(--border-subtle)] bg-[var(--surface-contrast)] px-3 py-1.5 focus-within:border-[var(--accent-border)] transition-colors">
          <button
            onClick={handleImageClick}
            className="mb-0.5 mr-1.5 flex h-[28px] w-[28px] flex-shrink-0 items-center justify-center rounded-[var(--radius-sm)] text-[var(--text-faint)] transition-colors hover:bg-[var(--surface-elevated)] hover:text-[var(--text-muted)]"
            title={intl.formatMessage({ id: "chat.attachImage" })}
          >
            <IconPhoto size={15} stroke={1.8} />
          </button>
          <input
            ref={fileInputRef}
            type="file"
            accept="image/*"
            className="hidden"
            onChange={handleFileChange}
          />

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

            <button
              onClick={() => {
                if (configuredModels.length > 0) {
                  setShowModelMenu((v) => !v);
                } else {
                  useAppStore.getState().setShowSettings(true);
                }
              }}
              className="flex items-center gap-1 rounded px-1 py-0.5 transition-colors hover:bg-[var(--surface-elevated)] hover:text-[var(--text-muted)]"
              title={configuredModels.length > 0
                ? intl.formatMessage({ id: "chat.modelSelector" })
                : intl.formatMessage({ id: "chat.noModelsConfigured" })}
            >
              <IconCpu size={10} stroke={1.8} />
              <span className="max-w-[160px] truncate">{displayModel}</span>
              {configuredModels.length > 1 && (
                <IconChevronDown size={8} stroke={2} className="opacity-50" />
              )}
            </button>
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
