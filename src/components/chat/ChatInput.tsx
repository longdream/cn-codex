import {
  IconArrowUp,
  IconBrain,
  IconChevronDown,
  IconCpu,
  IconFile,
  IconFolder,
  IconMessage2,
  IconPaperclip,
  IconPlugConnected,
  IconPlus,
  IconRobot,
  IconShieldCheck,
  IconSquare,
  IconTargetArrow,
  IconX,
} from "@tabler/icons-react";
import { useCallback, useEffect, useMemo, useRef, useState, type DragEvent } from "react";
import { useIntl } from "react-intl";
import { useAppStore, type ChatMode, type ChatSendOptions } from "../../stores/appStore";
import {
  SlashCommandPanel,
  getDefaultSlashCommands,
  type SlashCommand,
} from "./SlashCommandPanel";
import type { AttachedFile } from "../../types/provider";
import { invoke } from "@tauri-apps/api/core";
import { readFileForAttach } from "../../api/window";
import { robotList } from "../../api/robot";
import type { RobotSummary } from "../../types/robot";
import { skillList } from "../../api/skill";
import type { SkillSummary } from "../../types/skill";

/** 支持的文档 MIME 类型和扩展名 */
const DOCUMENT_ACCEPT = ".pdf,.md,.txt,.docx,.doc,.csv,.json,.yaml,.yml,.toml,.xml,.html";
const IMAGE_ACCEPT = "image/*";
const ALL_ACCEPT = `${IMAGE_ACCEPT},${DOCUMENT_ACCEPT}`;

function guessTypeFromExt(name: string): string {
  const ext = name.split(".").pop()?.toLowerCase() ?? "";
  const map: Record<string, string> = {
    pdf: "application/pdf",
    docx: "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
    doc: "application/msword",
    pptx: "application/vnd.openxmlformats-officedocument.presentationml.presentation",
    ppt: "application/vnd.ms-powerpoint",
    xlsx: "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
    xls: "application/vnd.ms-excel",
    csv: "text/csv",
    json: "application/json",
    xml: "application/xml",
    html: "text/html",
    md: "text/markdown",
    txt: "text/plain",
    yaml: "text/yaml",
    yml: "text/yaml",
    toml: "application/toml",
  };
  return map[ext] ?? "application/octet-stream";
}

export interface ChatSendExtendedOptions extends ChatSendOptions {
  robotId?: string;
  robotCreateMode?: boolean;
  robotModifyMode?: boolean;
}

interface ChatInputProps {
  onSend: (
    text: string,
    mode: ChatMode,
    attachments: AttachedFile[],
    options?: ChatSendExtendedOptions,
  ) => void;
  onInterrupt?: () => void;
  isStreaming: boolean;
  disabled: boolean;
  mode: ChatMode;
  onGoalCommand?: (command: ParsedGoalCommand) => void;
  isGeneralMode?: boolean;
}

/**
 * 校验当前选择的机器人是否仍存在于机器人列表中。
 * 删除机器人后用于快速回收失效 selection，避免继续提交不存在的 robotId。
 */
export function resolveSelectedRobotId(
  robots: Array<Pick<RobotSummary, "id">>,
  selectedRobotId: string | null,
): string | null {
  if (!selectedRobotId) {
    return null;
  }
  return robots.some((robot) => robot.id === selectedRobotId) ? selectedRobotId : null;
}

export function ChatInput({
  onSend,
  onInterrupt,
  isStreaming,
  disabled,
  mode,
  onGoalCommand,
  isGeneralMode = false,
}: ChatInputProps) {
  const intl = useIntl();
  const [text, setText] = useState("");
  const [showSlash, setShowSlash] = useState(false);
  const [showModelMenu, setShowModelMenu] = useState(false);
  const [showRobotMenu, setShowRobotMenu] = useState(false);
  const [robots, setRobots] = useState<RobotSummary[]>([]);
  const [skills, setSkills] = useState<SkillSummary[]>([]);
  const [robotModifyMode, setRobotModifyMode] = useState(false);
  const [visionWarning, setVisionWarning] = useState(false);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);

  const initialized = useAppStore((s) => s.initialized);
  const workspaceCwd = useAppStore((s) => s.workspaceCwd);
  const attachedFiles = useAppStore((s) => s.attachedFiles);
  const pendingComposerInsert = useAppStore((s) => s.pendingComposerInsert);
  const currentGoal = useAppStore((s) => s.currentGoal);
  const autoApprove = useAppStore((s) => s.autoApprove);
  const setAutoApprove = useAppStore((s) => s.setAutoApprove);
  const consumeComposerInsert = useAppStore((s) => s.consumeComposerInsert);
  const selectedRobotId = useAppStore((s) => s.selectedRobotId);
  const robotCreateMode = useAppStore((s) => s.robotCreateMode);
  const setSelectedRobotId = useAppStore((s) => s.setSelectedRobotId);
  const setRobotCreateMode = useAppStore((s) => s.setRobotCreateMode);
  // 目标模式运行态：只在 goal + active 时视为“整体执行中”。
  // 聊天模式不受该状态影响。
  const goalRunning = mode === "goal" && currentGoal?.status === "active";

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

  useEffect(() => {
    robotList().then(setRobots).catch(() => setRobots([]));
    const handler = () => {
      robotList().then(setRobots).catch(() => setRobots([]));
    };
    window.addEventListener("robot-list-changed", handler);
    return () => window.removeEventListener("robot-list-changed", handler);
  }, []);

  useEffect(() => {
    skillList().then(setSkills).catch(() => setSkills([]));
  }, []);

  const selectedRobotName = useMemo(
    () => robots.find((r) => r.id === selectedRobotId)?.name ?? null,
    [robots, selectedRobotId],
  );

  useEffect(() => {
    if (!selectedRobotId) {
      return;
    }
    const validSelectedRobotId = resolveSelectedRobotId(robots, selectedRobotId);
    if (!validSelectedRobotId) {
      // 当机器人被设置页删除后，主动清空选择与修改态，防止继续提交无效 robotId。
      setSelectedRobotId(null);
      setRobotModifyMode(false);
    }
  }, [robots, selectedRobotId, setSelectedRobotId]);

  useEffect(() => {
    if (!pendingComposerInsert) {
      return;
    }
    // 预览面板把代码片段投递到 store 后，这里统一并入输入框并恢复焦点。
    const payload = consumeComposerInsert();
    if (!payload) {
      return;
    }
    setText((prev) => (prev.trim().length > 0 ? `${prev}\n\n${payload}` : payload));
    setShowSlash(false);
    requestAnimationFrame(() => {
      const element = textareaRef.current;
      if (!element) return;
      element.focus();
      element.style.height = "auto";
      element.style.height = `${Math.min(element.scrollHeight, 200)}px`;
    });
  }, [consumeComposerInsert, pendingComposerInsert]);

  const setMode = useCallback((nextMode: ChatMode) => {
    useAppStore.getState().setChatMode(nextMode);
    textareaRef.current?.focus();
  }, []);

  const appendSystemMessage = useCallback((content: string) => {
    useAppStore.getState().addMessage({
      id: crypto.randomUUID(),
      role: "system",
      content,
      timestamp: Date.now(),
    });
  }, []);

  const activateRobotModifyMode = useCallback(() => {
    if (!selectedRobotId) {
      appendSystemMessage(intl.formatMessage({ id: "chat.robot.modifyNeedSelection" }));
      return false;
    }
    setRobotModifyMode(true);
    useAppStore.getState().setChatMode("goal");
    requestAnimationFrame(() => textareaRef.current?.focus());
    return true;
  }, [appendSystemMessage, intl, selectedRobotId]);

  const slashSkillQuery = useMemo(() => parseSkillSelectionQuery(text), [text]);
  const slashCommandQuery = useMemo(() => {
    const trimmed = text.trim();
    if (!trimmed.startsWith("/")) {
      return "";
    }
    return trimmed.slice(1).trim();
  }, [text]);

  const slashCommands = useMemo<SlashCommand[]>(() => {
    const defaults = getDefaultSlashCommands();
    const clearCommand = defaults.find((command) => command.name === "clear");
    if (clearCommand) {
      clearCommand.action = () => {
        useAppStore.getState().setMessages([]);
        useAppStore.getState().clearStreamingText();
      };
    }
    const modelCommand = defaults.find((command) => command.name === "model");
    if (modelCommand) {
      modelCommand.action = () => {
        useAppStore.getState().setShowSettings(true);
      };
    }
    const goalCommand = defaults.find((command) => command.name === "goal");
    if (goalCommand) {
      goalCommand.action = () => {
        setMode("goal");
      };
    }
    const skillCommand = defaults.find((command) => command.name === "skill");
    if (skillCommand) {
      skillCommand.description = intl.formatMessage({ id: "chat.slashSkill" });
      skillCommand.action = () => {
        const nextText = "/skill ";
        setText(nextText);
        setShowSlash(true);
        requestAnimationFrame(() => {
          const element = textareaRef.current;
          if (!element) return;
          element.focus();
          element.setSelectionRange(nextText.length, nextText.length);
          element.style.height = "auto";
          element.style.height = `${Math.min(element.scrollHeight, 200)}px`;
        });
      };
      skillCommand.closeOnSelect = false;
    }
    const modifyRobotCommand = defaults.find(
      (command) => command.name === "modifyrobot",
    );
    if (modifyRobotCommand) {
      modifyRobotCommand.description = intl.formatMessage({ id: "chat.slashModifyRobot" });
      modifyRobotCommand.action = () => {
        const activated = activateRobotModifyMode();
        if (activated) {
          setText("");
        }
      };
    }
    return defaults;
  }, [activateRobotModifyMode, intl, setMode]);

  const slashSkillCommands = useMemo<SlashCommand[]>(() => {
    if (slashSkillQuery == null) {
      return [];
    }
    const query = slashSkillQuery.toLowerCase();
    return skills
      .filter((skill) => {
        if (!query) return true;
        const tags = Array.isArray(skill.tags) ? skill.tags.join(" ") : "";
        const haystack = `${skill.id} ${skill.name} ${skill.description} ${tags}`.toLowerCase();
        return haystack.includes(query);
      })
      .map((skill) => {
        const description = skill.description?.trim()
          ? `${skill.name} · ${skill.description}`
          : skill.name;
        return {
          name: `skill:${skill.id}`,
          trigger: `skill ${skill.id}`,
          description,
          searchText: `${skill.id} ${skill.name} ${skill.description} ${(skill.tags ?? []).join(" ")}`,
          action: () => {
            const nextText = `/skill ${skill.id} `;
            setText(nextText);
            requestAnimationFrame(() => {
              const element = textareaRef.current;
              if (!element) return;
              element.focus();
              element.setSelectionRange(nextText.length, nextText.length);
              element.style.height = "auto";
              element.style.height = `${Math.min(element.scrollHeight, 200)}px`;
            });
          },
        } satisfies SlashCommand;
      });
  }, [skills, slashSkillQuery]);

  const slashPanelCommands = useMemo(() => {
    if (slashSkillQuery != null) {
      return slashSkillCommands;
    }
    return slashCommands;
  }, [slashCommands, slashSkillCommands, slashSkillQuery]);

  const slashPanelQuery = useMemo(() => {
    if (slashSkillQuery != null) {
      return slashSkillQuery;
    }
    return slashCommandQuery;
  }, [slashCommandQuery, slashSkillQuery]);

  const resetComposerAfterSubmit = useCallback(() => {
    setText("");
    setShowSlash(false);
    useAppStore.getState().clearAttachedFiles();
    if (textareaRef.current) {
      textareaRef.current.style.height = "auto";
    }
  }, []);

  const handleSkillCommandSubmit = useCallback((trimmed: string, filesToSend: AttachedFile[]) => {
    const skillCommand = parseSkillCommand(trimmed);
    if (skillCommand) {
      if (!skillCommand.objective) {
        setShowSlash(true);
        requestAnimationFrame(() => textareaRef.current?.focus());
        return true;
      }
      const payload = buildSkillScopedPrompt(skillCommand.skillId, skillCommand.objective);
      onSend(payload, mode, filesToSend);
      resetComposerAfterSubmit();
      return true;
    }

    if (/^\/skill(?:\s+[\s\S]*)?$/i.test(trimmed)) {
      if (skills.length === 0) {
        appendSystemMessage(intl.formatMessage({ id: "chat.skill.noneAvailable" }));
        setShowSlash(false);
      } else {
        setShowSlash(true);
      }
      requestAnimationFrame(() => textareaRef.current?.focus());
      return true;
    }

    return false;
  }, [appendSystemMessage, intl, mode, onSend, resetComposerAfterSubmit, skills.length]);

  const handleModifyRobotCommandSubmit = useCallback((trimmed: string, filesToSend: AttachedFile[]) => {
    const modifyCommand = parseModifyRobotCommand(trimmed);
    if (!modifyCommand) {
      return false;
    }

    const activated = activateRobotModifyMode();
    if (!activated) {
      setShowSlash(false);
      setText("");
      return true;
    }

    if (!modifyCommand.objective) {
      setShowSlash(false);
      setText("");
      requestAnimationFrame(() => textareaRef.current?.focus());
      return true;
    }

    onSend(modifyCommand.objective, mode, filesToSend, {
      robotModifyMode: true,
      robotId: selectedRobotId!,
    });
    setRobotModifyMode(false);
    resetComposerAfterSubmit();
    return true;
  }, [activateRobotModifyMode, mode, onSend, resetComposerAfterSubmit, selectedRobotId]);

  const handleSubmit = useCallback(() => {
    // 仅目标模式在 active 时禁止提交；聊天模式行为保持不变。
    if (isStreaming || goalRunning) return;
    const trimmed = text.trim();
    if ((!trimmed && attachedFiles.length === 0) || disabled) return;
    const filesToSend = attachedFiles;

    if (handleModifyRobotCommandSubmit(trimmed, filesToSend)) {
      return;
    }
    if (handleSkillCommandSubmit(trimmed, filesToSend)) {
      return;
    }

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
    } else if (robotCreateMode) {
      onSend(trimmed, mode, filesToSend, { robotCreateMode: true });
    } else if (robotModifyMode && selectedRobotId) {
      onSend(trimmed, mode, filesToSend, { robotModifyMode: true, robotId: selectedRobotId });
      setRobotModifyMode(false);
    } else if (selectedRobotId) {
      onSend(trimmed, "goal", filesToSend, { robotId: selectedRobotId });
    } else {
      onSend(trimmed, mode, filesToSend);
    }
    resetComposerAfterSubmit();
  }, [
    attachedFiles,
    currentGoal,
    disabled,
    goalRunning,
    handleModifyRobotCommandSubmit,
    handleSkillCommandSubmit,
    isStreaming,
    mode,
    onGoalCommand,
    onSend,
    resetComposerAfterSubmit,
    robotModifyMode,
    selectedRobotId,
    text,
  ]);

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
        if (!isStreaming && !goalRunning) {
          handleSubmit();
        }
      }
      if (event.key === "Escape") {
        setShowSlash(false);
        setShowModelMenu(false);
        setShowRobotMenu(false);
      }
    },
    [goalRunning, handleSubmit, isStreaming],
  );

  const handleChange = useCallback((event: React.ChangeEvent<HTMLTextAreaElement>) => {
    const value = event.target.value;
    setText(value);
    setShowSlash(shouldShowSlashPanel(value));
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

  const [smartBrainAdded, setSmartBrainAdded] = useState<Set<number>>(new Set());

  const handleAddToSmartBrain = useCallback(async (file: AttachedFile, index: number) => {
    if (!file.sourcePath || smartBrainAdded.has(index)) return;
    try {
      await invoke("smartbrain_upload_knowledge", { filePath: file.sourcePath });
      setSmartBrainAdded((prev) => new Set(prev).add(index));
    } catch (err) {
      console.error("Add to SmartBrain failed:", err);
    }
  }, [smartBrainAdded]);

  // --- 拖拽支持 ---
  const [isDragging, setIsDragging] = useState(false);
  const dragCounter = useRef(0);

  const handleDragEnter = useCallback((e: DragEvent) => {
    e.preventDefault();
    e.stopPropagation();
    dragCounter.current += 1;
    if (e.dataTransfer.types.includes("Files") || e.dataTransfer.types.includes("application/x-cn-codex-file")) {
      setIsDragging(true);
    }
  }, []);

  const handleDragLeave = useCallback((e: DragEvent) => {
    e.preventDefault();
    e.stopPropagation();
    dragCounter.current -= 1;
    if (dragCounter.current <= 0) {
      dragCounter.current = 0;
      setIsDragging(false);
    }
  }, []);

  const handleDragOver = useCallback((e: DragEvent) => {
    e.preventDefault();
    e.stopPropagation();
  }, []);

  const handleDrop = useCallback((e: DragEvent) => {
    e.preventDefault();
    e.stopPropagation();
    setIsDragging(false);
    dragCounter.current = 0;

    const projectFileData = e.dataTransfer.getData("application/x-cn-codex-file");
    if (projectFileData) {
      try {
        const { path, name } = JSON.parse(projectFileData) as { path: string; name: string };
        void readFileForAttach(path).then((result) => {
          const attached: AttachedFile = {
            name: result.name || name,
            type: result.mimeType,
            dataUrl: result.dataUrl,
            size: result.size,
            sourcePath: result.sourcePath,
          };
          useAppStore.getState().addAttachedFile(attached);
        }).catch((err) => {
          console.error("Failed to read project file:", err);
        });
      } catch {
        console.error("Failed to parse project file data");
      }
      return;
    }

    const files = e.dataTransfer.files;
    if (!files || files.length === 0) return;

    for (const file of Array.from(files)) {
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
          type: file.type || guessTypeFromExt(file.name),
          dataUrl: reader.result as string,
          size: file.size,
        };
        useAppStore.getState().addAttachedFile(attached);
      };
      reader.readAsDataURL(file);
    }
  }, [activeEntry, providerModels]);

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
    <div
      className="chat-composer-zone relative flex-shrink-0 px-4 pb-4 pt-3 sm:px-8"
      onDragEnter={handleDragEnter}
      onDragLeave={handleDragLeave}
      onDragOver={handleDragOver}
      onDrop={handleDrop}
    >
      {isDragging && (
        <div className="pointer-events-none absolute inset-0 z-50 flex items-center justify-center rounded-[var(--radius-lg)] border-2 border-dashed border-[var(--accent)] bg-[var(--accent-soft)]">
          <span className="text-[13px] font-medium text-[var(--accent-strong)]">
            {intl.formatMessage({ id: "chat.dropFiles" })}
          </span>
        </div>
      )}
      {showSlash && (
        <SlashCommandPanel
          query={slashPanelQuery}
          commands={slashPanelCommands}
          onSelect={(command) => {
            command.action();
            if (command.name === "plan" || command.name === "help" || command.name === "compact") {
              if (!goalRunning) {
                onSend(`/${command.name}`, mode, []);
              }
              setText("");
            }
            if (command.closeOnSelect !== false) {
              setShowSlash(false);
            }
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
                      className={`flex w-full items-center gap-2.5 px-3 py-2 text-left text-[13px] transition-colors hover:bg-[var(--surface-elevated)] ${
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
                      className={`flex w-full items-center gap-2.5 px-3 py-2 text-left text-[13px] transition-colors hover:bg-[var(--surface-elevated)] ${
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
          {isGeneralMode ? (
            <div className="inline-flex items-center gap-1.5 rounded-full border border-[var(--chat-line)] bg-[var(--chat-chip)] px-3 py-1">
              <IconMessage2 size={12} stroke={1.8} className="text-[var(--accent)]" />
              <span className="text-[11px] font-medium text-[var(--chat-prose)]">
                {intl.formatMessage({ id: "chat.mode.chat" })}
              </span>
            </div>
          ) : (
          <div className="inline-flex rounded-full border border-[var(--chat-line)] bg-[var(--chat-chip)] p-1">
            <button
              type="button"
              aria-pressed={mode === "chat"}
              onClick={() => setMode("chat")}
              className={`flex h-6 items-center gap-1 rounded-full px-2.5 text-[11px] font-medium transition-colors ${
                mode === "chat"
                  ? "bg-[var(--chat-card-solid)] text-[var(--chat-prose)] shadow-[var(--shadow-soft)]"
                  : "text-[var(--chat-muted)] hover:text-[var(--chat-prose)]"
              }`}
            >
              <IconMessage2 size={12} stroke={1.8} />
              {intl.formatMessage({ id: "chat.mode.chat" })}
            </button>
            <button
              type="button"
              aria-pressed={mode === "goal"}
              onClick={() => setMode("goal")}
              className={`flex h-6 items-center gap-1 rounded-full px-2.5 text-[11px] font-medium transition-colors ${
                mode === "goal"
                  ? "bg-[var(--accent-soft)] text-[var(--accent-strong)]"
                  : "text-[var(--chat-muted)] hover:text-[var(--chat-prose)]"
              }`}
            >
              <IconTargetArrow size={13} stroke={1.8} />
              {intl.formatMessage({ id: "chat.mode.goal" })}
            </button>
          </div>
          )}

          {!isGeneralMode && (
            <div className="relative">
              <button
                type="button"
                onClick={() => setShowRobotMenu((v) => !v)}
                className={`flex h-6 items-center gap-1 rounded-full border px-2.5 text-[11px] font-medium transition-colors ${
                  selectedRobotId || robotCreateMode
                    ? "border-[var(--accent-border)] bg-[var(--accent-soft)] text-[var(--accent-strong)]"
                    : "border-[var(--chat-line)] bg-[var(--chat-chip)] text-[var(--chat-muted)] hover:text-[var(--chat-prose)]"
                }`}
              >
                <IconRobot size={12} stroke={1.8} />
                <span className="max-w-[120px] truncate">
                  {robotCreateMode
                    ? intl.formatMessage({ id: "chat.robot.creating" })
                    : robotModifyMode
                      ? intl.formatMessage({ id: "chat.robot.modifying" })
                      : selectedRobotName ?? intl.formatMessage({ id: "chat.robot.none" })}
                </span>
                <IconChevronDown size={10} stroke={2} className="opacity-60" />
              </button>

              {showRobotMenu && (
                <div className="absolute left-0 top-full z-30 mt-1 min-w-[200px] rounded-[var(--radius-md)] border border-[var(--chat-line)] bg-[var(--chat-card-solid)] py-1 shadow-lg">
                  <button
                    onClick={() => {
                      setSelectedRobotId(null);
                      setRobotCreateMode(false);
                      setShowRobotMenu(false);
                    }}
                    className={`flex w-full items-center gap-2 px-3 py-2 text-left text-[12px] transition-colors hover:bg-[var(--surface-elevated)] ${
                      !selectedRobotId && !robotCreateMode ? "text-[var(--accent-strong)]" : "text-[var(--text-base)]"
                    }`}
                  >
                    {intl.formatMessage({ id: "chat.robot.none" })}
                  </button>
                  {robots.map((robot) => (
                    <button
                      key={robot.id}
                      onClick={() => {
                        setSelectedRobotId(robot.id);
                        setShowRobotMenu(false);
                      }}
                      className={`flex w-full items-center gap-2 px-3 py-2 text-left text-[12px] transition-colors hover:bg-[var(--surface-elevated)] ${
                        selectedRobotId === robot.id ? "text-[var(--accent-strong)]" : "text-[var(--text-base)]"
                      }`}
                    >
                      <IconRobot size={13} stroke={1.6} className="shrink-0 opacity-70" />
                      <div className="min-w-0">
                        <span className="block truncate font-medium">{robot.name}</span>
                        {robot.description && (
                          <span className="block truncate text-[11px] text-[var(--text-faint)]">
                            {robot.description}
                          </span>
                        )}
                      </div>
                    </button>
                  ))}
                  <div className="my-1 border-t border-[var(--chat-line)]" />
                  <button
                    onClick={() => {
                      setRobotCreateMode(true);
                      setShowRobotMenu(false);
                      textareaRef.current?.focus();
                    }}
                    className="flex w-full items-center gap-2 px-3 py-2 text-left text-[12px] text-[var(--accent-strong)] transition-colors hover:bg-[var(--surface-elevated)]"
                  >
                    <IconPlus size={13} stroke={1.8} className="shrink-0" />
                    {intl.formatMessage({ id: "chat.robot.create" })}
                  </button>
                </div>
              )}
            </div>
          )}

          {!isGeneralMode && mode === "goal" && !selectedRobotId && !robotCreateMode && (
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
                {file.sourcePath && !file.type.startsWith("image/") && (
                  <button
                    type="button"
                    onClick={() => handleAddToSmartBrain(file, idx)}
                    disabled={smartBrainAdded.has(idx)}
                    className={`absolute -left-1 -top-1 flex h-4 w-4 items-center justify-center rounded-full shadow transition-opacity group-hover:opacity-100 ${
                      smartBrainAdded.has(idx)
                        ? "bg-[var(--accent)] text-white opacity-100"
                        : "bg-[var(--chat-card-solid)] text-[var(--chat-faint)] opacity-0 hover:text-[var(--accent)]"
                    }`}
                    title={intl.formatMessage({ id: smartBrainAdded.has(idx) ? "chat.addedToSmartBrain" : "chat.addToSmartBrain" })}
                  >
                    <IconBrain size={10} stroke={2} />
                  </button>
                )}
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
            placeholder={intl.formatMessage({ id: robotModifyMode ? "chat.robot.modifyPlaceholder" : robotCreateMode ? "chat.robot.placeholder" : "chat.placeholder" })}
            disabled={disabled}
            rows={2}
            className="chat-composer-input max-h-[200px] min-h-[78px] w-full flex-1 resize-none bg-transparent py-2 text-[13px] leading-relaxed text-[var(--chat-prose)] placeholder:text-[var(--chat-faint)] outline-none disabled:opacity-50"
          />

          {isStreaming || goalRunning ? (
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

            <button
              type="button"
              onClick={() => setAutoApprove(!autoApprove)}
              className={`flex items-center gap-1 rounded-full px-2 py-1 transition-colors ${
                autoApprove
                  ? "bg-[var(--accent-soft)] text-[var(--accent)]"
                  : "hover:bg-[var(--chat-chip)] hover:text-[var(--chat-prose)]"
              }`}
              title={intl.formatMessage({ id: autoApprove ? "chat.autoApproveOn" : "chat.autoApproveOff" })}
            >
              <IconShieldCheck size={12} stroke={1.8} className="flex-shrink-0" />
              <span className="truncate">{intl.formatMessage({ id: "chat.autoApprove" })}</span>
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

export interface ParsedSkillCommand {
  skillId: string;
  objective: string;
}

export function parseSkillCommand(value: string): ParsedSkillCommand | null {
  const match = value.trim().match(/^\/skill(?:\s+(\S+))(?:\s+([\s\S]+))?$/i);
  if (!match) {
    return null;
  }
  return {
    skillId: match[1].trim(),
    objective: match[2]?.trim() ?? "",
  };
}

export function parseSkillSelectionQuery(value: string): string | null {
  const match = value.trim().match(/^\/skill(?:\s+([\s\S]*))?$/i);
  if (!match) {
    return null;
  }
  const body = match[1]?.trim() ?? "";
  if (!body) {
    return "";
  }
  if (/\s/.test(body)) {
    return null;
  }
  return body;
}

export function buildSkillScopedPrompt(skillId: string, objective: string): string {
  const normalizedSkillId = skillId.trim();
  const normalizedObjective = objective.trim();
  if (!normalizedObjective) {
    return normalizedObjective;
  }
  return `请优先使用 skill "${normalizedSkillId}"，然后完成以下需求：\n${normalizedObjective}`;
}

export interface ParsedModifyRobotCommand {
  objective: string;
}

export function parseModifyRobotCommand(value: string): ParsedModifyRobotCommand | null {
  const match = value.trim().match(/^\/(?:modifyrobot|modify)(?:\s+([\s\S]+))?$/i);
  if (!match) {
    return null;
  }
  return {
    objective: match[1]?.trim() ?? "",
  };
}

export function shouldShowSlashPanel(value: string): boolean {
  const trimmed = value.trim();
  if (!trimmed.startsWith("/")) {
    return false;
  }
  if (parseSkillSelectionQuery(trimmed) != null) {
    return true;
  }
  return !trimmed.includes(" ");
}
