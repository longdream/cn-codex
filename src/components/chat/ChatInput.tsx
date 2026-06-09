import {
  IconArrowUp,
  IconChevronDown,
  IconCpu,
  IconFile,
  IconFolder,
  IconMessage2,
  IconPaperclip,
  IconPlugConnected,
  IconSquare,
  IconTargetArrow,
  IconX,
} from "@tabler/icons-react";
import { useCallback, useMemo, useRef, useState } from "react";
import { useIntl } from "react-intl";
import { useAppStore, type ChatMode, type ChatSendOptions } from "../../stores/appStore";
import { SlashCommandPanel, getDefaultSlashCommands } from "./SlashCommandPanel";
import type { AttachedFile } from "../../types/provider";

/** 支持的文档 MIME 类型和扩展名 */
const DOCUMENT_ACCEPT = ".pdf,.md,.txt,.docx,.doc,.csv,.json,.yaml,.yml,.toml,.xml,.html";
const IMAGE_ACCEPT = "image/*";
const ALL_ACCEPT = `${IMAGE_ACCEPT},${DOCUMENT_ACCEPT}`;

interface ChatInputProps {
  onSend: (
    text: string,
    mode: ChatMode,
    attachments: AttachedFile[],
    options?: ChatSendOptions,
  ) => void;
  onInterrupt?: () => void;
  isStreaming: boolean;
  disabled: boolean;
  mode: ChatMode;
  onGoalCommand?: (command: ParsedGoalCommand) => void;
}

export function ChatInput({
  onSend,
  onInterrupt,
  isStreaming,
  disabled,
  mode,
  onGoalCommand,
}: ChatInputProps) {
  const intl = useIntl();
  const [text, setText] = useState("");
  const [showSlash, setShowSlash] = useState(false);
  const [showModelMenu, setShowModelMenu] = useState(false);
  const [visionWarning, setVisionWarning] = useState(false);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);

  const initialized = useAppStore((s) => s.initialized);
  const workspaceCwd = useAppStore((s) => s.workspaceCwd);
  const attachedFiles = useAppStore((s) => s.attachedFiles);
  const currentGoal = useAppStore((s) => s.currentGoal);

  // 供应商相关
  const providers = useAppStore((s) => s.providers);
  const activeProviderId = useAppStore((s) => s.activeProviderId);
  const activeProvider = useMemo(
    () => providers.find((p) => p.id === activeProviderId) ?? null,
    [providers, activeProviderId],
  );
  const providerModels = activeProvider?.models ?? [];

  // 兼容旧的 configuredModels
  const configuredModels = useAppStore((s) => s.configuredModels);
  const activeModelId = useAppStore((s) => s.activeModelId);
  const currentModel = useAppStore((s) => s.currentModel);
  const defaultProvider = intl.formatMessage({ id: "app.defaultProvider" });

  // 显示的模型名称：优先取旧的 activeEntry，否则取供应商默认模型
  const activeEntry = configuredModels.find((m) => m.id === activeModelId) ?? null;
  const displayModel = activeEntry?.label
    ?? currentModel
    ?? providerModels[0]?.label
    ?? activeProvider?.name
    ?? defaultProvider;

  const setMode = useCallback((nextMode: ChatMode) => {
    useAppStore.getState().setChatMode(nextMode);
    textareaRef.current?.focus();
  }, []);

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
    if ((!trimmed && attachedFiles.length === 0) || disabled) return;
    const filesToSend = attachedFiles;

    const goalCommand = parseGoalCommand(trimmed);
    if (goalCommand) {
      useAppStore.getState().setChatMode("goal");
      if (goalCommand.action === "show") {
        onGoalCommand?.(goalCommand);
        setText("");
        setShowSlash(false);
        textareaRef.current?.focus();
        return;
      }
      if (goalCommand.action === "edit" && !goalCommand.objective) {
        const editCommand = buildGoalEditCommand(currentGoal);
        if (editCommand) {
          setText(editCommand);
          setShowSlash(false);
          requestAnimationFrame(() => {
            const element = textareaRef.current;
            if (!element) return;
            element.focus();
            element.setSelectionRange("/goal edit ".length, editCommand.length);
            element.style.height = "auto";
            element.style.height = `${Math.min(element.scrollHeight, 200)}px`;
          });
          return;
        }
      }
      if (goalCommand.action !== "set") {
        onGoalCommand?.(goalCommand);
      } else if (goalCommand.objective) {
        onSend(goalCommand.objective, "goal", filesToSend, {
          goalBudgetTokens: goalCommand.goalBudgetTokens,
        });
      }
      if (goalCommand.action !== "set" || !goalCommand.objective) {
        useAppStore.getState().clearAttachedFiles();
      }
    } else {
      onSend(trimmed, mode, filesToSend);
    }
    setText("");
    setShowSlash(false);
    useAppStore.getState().clearAttachedFiles();
    if (textareaRef.current) {
      textareaRef.current.style.height = "auto";
    }
  }, [attachedFiles, currentGoal, disabled, mode, onGoalCommand, onSend, text]);

  const goalStatusLabel = useMemo(() => {
    if (!currentGoal) {
      return intl.formatMessage({ id: "chat.mode.goalActive" });
    }

    return intl.formatMessage({ id: `chat.goalStatus.${currentGoal.status}` });
  }, [currentGoal, intl]);

  const goalStatusClass = currentGoal?.status === "paused"
    ? "border-[rgba(239,180,40,0.35)] bg-[rgba(239,180,40,0.12)] text-[var(--warning)]"
    : currentGoal?.status === "budgetLimited" || currentGoal?.status === "usageLimited"
      ? "border-[rgba(239,68,68,0.35)] bg-[var(--danger-soft)] text-[var(--danger)]"
      : "border-[var(--accent-border)] bg-[var(--accent-soft)] text-[var(--accent-strong)]";

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

  // 附件处理：支持图片和文档
  const handleAttachClick = useCallback(() => {
    fileInputRef.current?.click();
  }, []);

  const handleFileChange = useCallback((event: React.ChangeEvent<HTMLInputElement>) => {
    const files = event.target.files;
    if (!files || files.length === 0) return;

    for (const file of Array.from(files)) {
      // 图片类型检查视觉支持
      if (file.type.startsWith("image/") && !activeEntry?.supportsVision) {
        const providerVision = providerModels.some((m) => m.supportsVision);
        if (!providerVision) {
          setVisionWarning(true);
          setTimeout(() => setVisionWarning(false), 4000);
        }
      }

      const reader = new FileReader();
      reader.onload = () => {
        const attached: AttachedFile = {
          name: file.name,
          type: file.type,
          dataUrl: reader.result as string,
          size: file.size,
        };
        useAppStore.getState().addAttachedFile(attached);
      };
      reader.readAsDataURL(file);
    }
    event.target.value = "";
  }, [activeEntry, providerModels]);

  const handleRemoveFile = useCallback((index: number) => {
    useAppStore.getState().removeAttachedFile(index);
  }, []);

  const handleModelSelect = useCallback((modelId: string) => {
    useAppStore.getState().setActiveModelId(modelId);
    setShowModelMenu(false);
  }, []);

  // 供应商模型选择（使用新供应商系统）
  const handleProviderModelSelect = useCallback((modelId: string) => {
    // 将供应商模型同步为 configuredModels 格式
    if (!activeProvider) return;
    const model = activeProvider.models.find((m) => m.id === modelId);
    if (!model) return;

    const entryId = `${activeProvider.id}:${modelId}`;
    const store = useAppStore.getState();
    const existing = store.configuredModels;
    const newEntry = {
      id: entryId,
      provider: activeProvider.id,
      model: modelId,
      label: `${activeProvider.name} / ${model.label}`,
      supportsVision: model.supportsVision,
    };
    const idx = existing.findIndex((m) => m.id === entryId);
    const updated = idx >= 0
      ? existing.map((m, i) => (i === idx ? newEntry : m))
      : [...existing, newEntry];
    store.setConfiguredModels(updated);
    store.setActiveModelId(entryId);
    store.setCurrentModel(modelId);
    setShowModelMenu(false);
  }, [activeProvider]);

  const cwdLeaf = workspaceCwd
    ? workspaceCwd.split(/[\\/]/).filter(Boolean).pop() ?? workspaceCwd
    : null;

  /** 格式化文件大小 */
  const formatSize = (bytes: number) => {
    if (bytes < 1024) return `${bytes}B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)}KB`;
    return `${(bytes / (1024 * 1024)).toFixed(1)}MB`;
  };

  return (
    <div className="chat-composer-zone relative flex-shrink-0 px-4 pb-4 pt-3 sm:px-8">
      {showSlash && (
        <SlashCommandPanel
          query={text}
          commands={slashCommands}
          onSelect={(command) => {
            command.action();
            if (command.name === "goal") {
              setMode("goal");
            } else if (command.name === "plan" || command.name === "help" || command.name === "compact") {
              onSend(`/${command.name}`, mode, []);
            }
            setText("");
            setShowSlash(false);
          }}
          onClose={() => setShowSlash(false)}
        />
      )}

      {/* 模型选择菜单 */}
      {showModelMenu && (
        <div className="absolute bottom-full left-0 right-0 z-20 mx-auto max-w-[1180px] px-4 pb-2 sm:px-8">
          <div className="rounded-[var(--radius-md)] border border-[var(--chat-line)] bg-[var(--chat-card-solid)] shadow-lg">
            {/* 供应商模型列表 */}
            {providerModels.length > 0 && (
              <>
                <div className="px-3 py-2 text-[11px] font-medium uppercase tracking-wider text-[var(--text-faint)]">
                  {activeProvider?.name} {intl.formatMessage({ id: "settings.provider.models" })}
                </div>
                <div className="max-h-[200px] overflow-y-auto">
                  {providerModels.map((m) => (
                    <button
                      key={m.id}
                      onClick={() => handleProviderModelSelect(m.id)}
                      className={`flex w-full items-center gap-2.5 px-3 py-2 text-left text-sm transition-colors hover:bg-[var(--surface-elevated)] ${
                        activeEntry?.model === m.id ? "bg-[var(--accent-soft)] text-[var(--accent-strong)]" : "text-[var(--text-base)]"
                      }`}
                    >
                      <IconCpu size={13} stroke={1.8} className="flex-shrink-0 opacity-60" />
                      <div className="min-w-0 flex-1">
                        <span className="block truncate font-medium">{m.label}</span>
                        <span className="block truncate text-[11px] text-[var(--text-faint)]">
                          {m.id}{m.supportsVision && " · 📷 vision"}
                        </span>
                      </div>
                      {activeEntry?.model === m.id && (
                        <span className="h-1.5 w-1.5 flex-shrink-0 rounded-full bg-[var(--accent)]" />
                      )}
                    </button>
                  ))}
                </div>
              </>
            )}
            {/* 兼容旧的 configuredModels */}
            {configuredModels.length > 0 && providerModels.length === 0 && (
              <>
                <div className="px-3 py-2 text-[11px] font-medium uppercase tracking-wider text-[var(--text-faint)]">
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
                          {m.provider}{m.supportsVision && " · 📷 vision"}
                        </span>
                      </div>
                      {m.id === activeModelId && (
                        <span className="h-1.5 w-1.5 flex-shrink-0 rounded-full bg-[var(--accent)]" />
                      )}
                    </button>
                  ))}
                </div>
              </>
            )}
          </div>
        </div>
      )}

      {visionWarning && (
        <div className="absolute bottom-full left-0 right-0 z-10 mx-auto max-w-[1180px] px-4 pb-2 sm:px-8">
          <div className="rounded-[var(--radius-sm)] border border-[rgba(239,180,40,0.3)] bg-[rgba(239,180,40,0.1)] px-3 py-2 text-xs text-[var(--warning)]">
            {intl.formatMessage({ id: "chat.visionNotSupported" })}
          </div>
        </div>
      )}

      <div className="mx-auto max-w-[1180px]">
        <div className="chat-composer-shell px-4 pb-3 pt-3">
          <div className="mb-2 flex flex-wrap items-center justify-between gap-2">
          <div className="inline-flex rounded-full border border-[var(--chat-line)] bg-[var(--chat-chip)] p-1">
            <button
              type="button"
              aria-pressed={mode === "chat"}
              onClick={() => setMode("chat")}
              className={`flex h-7 items-center gap-1.5 rounded-full px-3 text-[11px] font-medium transition-colors ${
                mode === "chat"
                  ? "bg-[var(--chat-card-solid)] text-[var(--chat-prose)] shadow-[var(--shadow-soft)]"
                  : "text-[var(--chat-muted)] hover:text-[var(--chat-prose)]"
              }`}
            >
              <IconMessage2 size={13} stroke={1.8} />
              {intl.formatMessage({ id: "chat.mode.chat" })}
            </button>
            <button
              type="button"
              aria-pressed={mode === "goal"}
              onClick={() => setMode("goal")}
              className={`flex h-7 items-center gap-1.5 rounded-full px-3 text-[11px] font-medium transition-colors ${
                mode === "goal"
                  ? "bg-[var(--accent-soft)] text-[var(--accent-strong)]"
                  : "text-[var(--chat-muted)] hover:text-[var(--chat-prose)]"
              }`}
            >
              <IconTargetArrow size={13} stroke={1.8} />
              {intl.formatMessage({ id: "chat.mode.goal" })}
            </button>
          </div>

          {mode === "goal" && (
            <span className={`rounded-full border px-2.5 py-1 text-[11px] font-medium ${goalStatusClass}`}>
              {goalStatusLabel}
            </span>
          )}
        </div>

        {/* 附件预览区 */}
        {attachedFiles.length > 0 && (
          <div className="mb-2 flex flex-wrap items-center gap-2">
            {attachedFiles.map((file, idx) => (
              <div key={idx} className="group relative flex items-center gap-2 rounded-[var(--radius-md)] border border-[var(--chat-line)] bg-[var(--chat-chip)] px-2 py-1.5">
                {file.type.startsWith("image/") ? (
                  <div className="h-10 w-10 overflow-hidden rounded-[var(--radius-sm)]">
                    <img src={file.dataUrl} alt="" className="h-full w-full object-cover" />
                  </div>
                ) : (
                  <IconFile size={16} stroke={1.5} className="text-[var(--chat-muted)]" />
                )}
                <div className="max-w-[150px]">
                  <p className="truncate text-[11px] text-[var(--chat-prose)]">{file.name}</p>
                  <p className="text-[11px] text-[var(--chat-faint)]">{formatSize(file.size)}</p>
                </div>
                <button
                  type="button"
                  onClick={() => handleRemoveFile(idx)}
                  className="absolute -right-1 -top-1 flex h-4 w-4 items-center justify-center rounded-full bg-[var(--chat-card-solid)] text-[var(--chat-faint)] opacity-0 shadow transition-opacity group-hover:opacity-100 hover:text-[var(--danger)]"
                >
                  <IconX size={10} stroke={2} />
                </button>
              </div>
            ))}
          </div>
        )}

        {/* 输入框主体 */}
        <div className="flex min-h-[82px] items-end gap-3 rounded-[var(--radius-lg)] px-1 py-1 transition-colors">
          {/* 附件按钮 */}
          <button
            type="button"
            onClick={handleAttachClick}
            className="mb-1 flex h-9 w-9 flex-shrink-0 items-center justify-center rounded-full border border-[var(--chat-line)] text-[var(--chat-muted)] transition-colors hover:bg-[var(--chat-chip)] hover:text-[var(--chat-prose)]"
            title={intl.formatMessage({ id: "chat.attachFile" })}
          >
            <IconPaperclip size={15} stroke={1.8} />
          </button>
          <input
            ref={fileInputRef}
            type="file"
            accept={ALL_ACCEPT}
            multiple
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
            rows={2}
            className="chat-composer-input max-h-[200px] min-h-[78px] w-full flex-1 resize-none bg-transparent py-2 text-base leading-relaxed text-[var(--chat-prose)] placeholder:text-[var(--chat-faint)] outline-none disabled:opacity-50"
          />

          {isStreaming ? (
            <button
              type="button"
              onClick={onInterrupt}
              className="mb-1 flex h-9 w-9 flex-shrink-0 items-center justify-center rounded-full border border-[rgba(239,68,68,0.3)] bg-[var(--danger-soft)] text-[var(--danger)] transition-opacity hover:opacity-80"
            >
              <IconSquare size={12} stroke={2.5} />
            </button>
          ) : (
            <button
              type="button"
              onClick={handleSubmit}
              disabled={disabled || (!text.trim() && attachedFiles.length === 0)}
              className="mb-1 flex h-9 w-9 flex-shrink-0 items-center justify-center rounded-full bg-[var(--accent)] text-white transition-opacity hover:opacity-90 disabled:opacity-30"
            >
              <IconArrowUp size={14} stroke={2.5} />
            </button>
          )}
        </div>

        {/* 底部状态栏 */}
        <div className="mt-2 flex flex-wrap items-center justify-between gap-3 border-t border-[var(--chat-line)] px-1 pt-3 text-[11px] text-[var(--chat-muted)]">
          <div className="flex min-w-0 flex-wrap items-center gap-3">
            <span className="flex items-center gap-1.5">
              <span className={`h-1.5 w-1.5 rounded-full ${initialized ? "bg-[var(--accent)]" : "bg-[var(--warning)] animate-pulse"}`} />
              <IconPlugConnected size={12} stroke={1.8} />
              {initialized
                ? intl.formatMessage({ id: "status.connected" })
                : intl.formatMessage({ id: "status.initializing" })}
            </span>

            {/* 模型选择器 */}
            <button
              type="button"
              onClick={() => {
                if (providerModels.length > 0 || configuredModels.length > 0) {
                  setShowModelMenu((v) => !v);
                } else {
                  useAppStore.getState().setShowSettings(true);
                }
              }}
              className="flex min-w-0 items-center gap-1 rounded-full px-2 py-1 transition-colors hover:bg-[var(--chat-chip)] hover:text-[var(--chat-prose)]"
              title={intl.formatMessage({ id: "chat.modelSelector" })}
            >
              <IconCpu size={12} stroke={1.8} className="flex-shrink-0" />
              <span className="max-w-[190px] truncate">{displayModel}</span>
              <IconChevronDown size={10} stroke={2} className="flex-shrink-0 opacity-60" />
            </button>
          </div>
          <div className="flex min-w-0 items-center gap-2">
            {cwdLeaf && (
              <span className="flex min-w-0 items-center gap-1 truncate" title={workspaceCwd ?? undefined}>
                <IconFolder size={12} stroke={1.8} className="flex-shrink-0" />
                <span className="truncate">{cwdLeaf}</span>
              </span>
            )}
          </div>
        </div>
        </div>
      </div>
    </div>
  );
}

export interface ParsedGoalCommand {
  action: "set" | "show" | "pause" | "resume" | "clear" | "edit";
  objective: string;
  goalBudgetTokens?: number;
}

export function buildGoalEditCommand(
  goal: { objective?: string | null } | null | undefined,
): string | null {
  const objective = goal?.objective?.trim();
  return objective ? `/goal edit ${objective}` : null;
}

export function parseGoalCommand(value: string): ParsedGoalCommand | null {
  const match = value.trim().match(/^\/goal(?:\s+([\s\S]+))?$/i);
  if (!match) {
    return null;
  }

  const body = match[1]?.trim() ?? "";
  if (!body) {
    return { action: "show", objective: "" };
  }

  const normalizedBody = body.toLowerCase();
  if (normalizedBody === "pause" || normalizedBody === "paused") {
    return { action: "pause", objective: "" };
  }
  if (normalizedBody === "resume" || normalizedBody === "active") {
    return { action: "resume", objective: "" };
  }
  if (normalizedBody === "clear") {
    return { action: "clear", objective: "" };
  }

  const editMatch = body.match(/^edit(?:\s+([\s\S]*))?$/i);
  if (editMatch) {
    return parseGoalObjective("edit", editMatch[1]?.trim() ?? "");
  }

  return parseGoalObjective("set", body);
}

function parseGoalObjective(
  action: "set" | "edit",
  body: string,
): ParsedGoalCommand {
  const tokenMatch = body.match(/^--tokens(?:=|\s+)(\S+)(?:\s+([\s\S]*))?$/i);
  if (!tokenMatch) {
    return { action, objective: body };
  }

  const goalBudgetTokens = parseGoalTokenBudget(tokenMatch[1]);
  if (goalBudgetTokens == null) {
    return { action, objective: body };
  }

  return {
    action,
    objective: tokenMatch[2]?.trim() ?? "",
    goalBudgetTokens,
  };
}

function parseGoalTokenBudget(value: string): number | undefined {
  const normalized = value.replace(/[,_]/g, "").trim();
  const match = normalized.match(/^(\d+(?:\.\d+)?)([kKmM])?$/);
  if (!match) {
    return undefined;
  }

  const base = Number(match[1]);
  if (!Number.isFinite(base) || base <= 0) {
    return undefined;
  }

  const multiplier = match[2]?.toLowerCase() === "m"
    ? 1_000_000
    : match[2]?.toLowerCase() === "k"
      ? 1_000
      : 1;
  const tokens = Math.round(base * multiplier);
  return tokens > 0 ? tokens : undefined;
}
