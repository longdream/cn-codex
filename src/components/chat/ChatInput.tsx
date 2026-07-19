import {
  IconArrowUp,
  IconBrowser,
  IconBrain,
  IconChevronDown,
  IconClipboardList,
  IconCpu,
  IconFile,
  IconFolder,
  IconPaperclip,
  IconPencil,
  IconPlayerSkipForward,
  IconPlugConnected,
  IconPlus,
  IconRobot,
  IconShieldCheck,
  IconSquare,
  IconTargetArrow,
  IconPuzzle,
  IconServer,
  IconX,
} from "@tabler/icons-react";
import { useCallback, useEffect, useMemo, useRef, useState, type DragEvent } from "react";
import { useIntl } from "react-intl";
import {
  DEFAULT_MODEL_CONTEXT_LENGTH,
  VISION_FALLBACK_KIND_LOCAL_OCR,
  useAppStore,
  type ChatMode,
  type ChatSendOptions,
  type QueuedMessage,
} from "../../stores/appStore";
import {
  SlashCommandPanel,
  getDefaultSlashCommands,
  type SlashCommand,
} from "./SlashCommandPanel";
import {
  type AttachedFile,
  type BinaryAttachedFile,
  type PathRefAttachedFile,
  isBinaryAttachedFile,
  isPathRefAttachedFile,
  isWebSnippetAttachedFile,
} from "../../types/provider";
import { invoke } from "@tauri-apps/api/core";
import { pluginList } from "../../api/plugin";
import { robotList } from "../../api/robot";
import type { RobotSummary } from "../../types/robot";
import { skillList } from "../../api/skill";
import type { SkillSummary } from "../../types/skill";
import type { PluginSummary } from "../../types/plugin";
import { standaloneConfigRead } from "../../api/standalone";
import { formatWebSnippet } from "../../utils/formatWebSnippet";
import { derivePlanExecutionProgress } from "../../utils/planExecutionProgress";
import {
  filterProviderModels,
  resolveModelPickerProviderId,
  resolveProviderModelId,
} from "../../utils/chatModelSelection";
import {
  buildPathRefPromptValue,
  formatPathRefLineRange,
  PATH_REF_MIME,
} from "../../utils/pathRefSnippet";
import { LanGroupChatLauncher } from "../lan/LanGroupChatLauncher";
import { PlanExecutionProgress } from "./PlanExecutionProgress";

/** 支持的文档 MIME 类型和扩展名 */
const DOCUMENT_ACCEPT = ".pdf,.md,.txt,.docx,.doc,.csv,.json,.yaml,.yml,.toml,.xml,.html";
const IMAGE_ACCEPT = "image/*";
const ALL_ACCEPT = `${IMAGE_ACCEPT},${DOCUMENT_ACCEPT}`;
const COMPUTER_USE_PLUGIN_ID = "computer-use";

type AttachMenuView = "root" | "skill" | "plugin" | "mcp";

interface McpServerOption {
  name: string;
  command?: string;
  args?: string[];
  disabled?: boolean;
  source?: "config" | "plugin";
}

interface ClipboardImageItemLike {
  type: string;
  getAsFile: () => File | null;
}

interface BrowserFileAttachmentInput {
  file: File;
  fallbackName?: string;
}

interface PreparedSendPayload {
  text: string;
  attachments: BinaryAttachedFile[];
}

function buildAttachedPathBlock(files: AttachedFile[]): string {
  const uniquePaths = Array.from(
    new Set(
      files
        .filter(isPathRefAttachedFile)
        // 路径引用允许携带行号范围，发送时统一编码为 path#Lx-Ly 形式，避免把正文塞入输入框。
        .map((file) => buildPathRefPromptValue(file).trim())
        .filter(Boolean),
    ),
  );
  if (uniquePaths.length === 0) {
    return "";
  }
  return `AttachedPaths:\n${uniquePaths.map((sourcePath) => `- ${sourcePath}`).join("\n")}`;
}

function buildWebSnippetBlock(files: AttachedFile[]): string {
  const blocks = files
    .filter(isWebSnippetAttachedFile)
    .map((file) => formatWebSnippet({
      url: file.url,
      selector: file.selector,
      selectorCandidates: file.selectorCandidates ?? [],
      sourcePath: file.sourcePath ?? null,
      tagName: file.tagName,
      text: file.text,
      rect: file.rect,
    }).text.trim())
    .filter(Boolean);
  return blocks.join("\n\n");
}

function prepareSendPayload(rawText: string, files: AttachedFile[]): PreparedSendPayload {
  const text = rawText.trim();
  const attachments = files.filter(isBinaryAttachedFile);
  const extraBlocks = [buildAttachedPathBlock(files), buildWebSnippetBlock(files)].filter((block) => block.length > 0);
  if (extraBlocks.length === 0) {
    return { text, attachments };
  }
  const snippetBlock = extraBlocks.join("\n\n");
  return {
    text: text ? `${text}\n\n${snippetBlock}` : snippetBlock,
    attachments,
  };
}

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

function imageExtensionFromMimeType(mimeType: string): string {
  const normalized = mimeType.toLowerCase().trim();
  switch (normalized) {
    case "image/jpeg":
    case "image/jpg":
      return "jpg";
    case "image/svg+xml":
      return "svg";
    case "image/vnd.microsoft.icon":
    case "image/x-icon":
      return "ico";
    default:
      break;
  }
  if (!normalized.startsWith("image/")) {
    return "png";
  }
  const subtype = normalized.slice("image/".length).split(";")[0].replace(/[^a-z0-9]/gi, "");
  return subtype || "png";
}

export function buildPastedImageName(mimeType: string, seed = Date.now(), sequence = 1): string {
  const safeSequence = Number.isFinite(sequence) && sequence > 0 ? Math.floor(sequence) : 1;
  return `pasted-image-${seed}-${safeSequence}.${imageExtensionFromMimeType(mimeType)}`;
}

function pathRefPrimaryText(file: PathRefAttachedFile): string {
  const lineRange = formatPathRefLineRange(file);
  return lineRange ? `${file.name} (${lineRange})` : file.name;
}

export function extractClipboardImageFiles(
  items: ArrayLike<ClipboardImageItemLike>,
  seed = Date.now(),
): BrowserFileAttachmentInput[] {
  const imageFiles: BrowserFileAttachmentInput[] = [];
  let imageCount = 0;
  for (const item of Array.from(items)) {
    if (!item.type?.toLowerCase().startsWith("image/")) {
      continue;
    }
    const file = item.getAsFile();
    if (!file) {
      continue;
    }
    imageCount += 1;
    const trimmedName = file.name.trim();
    imageFiles.push({
      file,
      fallbackName: trimmedName ? undefined : buildPastedImageName(file.type || item.type, seed, imageCount),
    });
  }
  return imageFiles;
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
    attachments: BinaryAttachedFile[],
    options?: ChatSendExtendedOptions,
  ) => void;
  onInterrupt?: () => void;
  onJumpQueue?: (id: string) => void;
  isStreaming: boolean;
  isDispatching: boolean;
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
  onJumpQueue,
  isStreaming,
  isDispatching,
  disabled,
  mode,
  onGoalCommand,
  isGeneralMode = false,
}: ChatInputProps) {
  const intl = useIntl();
  const [text, setText] = useState("");
  const [showSlash, setShowSlash] = useState(false);
  const [showModelMenu, setShowModelMenu] = useState(false);
  const [modelPickerProviderId, setModelPickerProviderId] = useState<string | null>(null);
  const [modelSearchQuery, setModelSearchQuery] = useState("");
  const [showRobotMenu, setShowRobotMenu] = useState(false);
  const [robots, setRobots] = useState<RobotSummary[]>([]);
  const [skills, setSkills] = useState<SkillSummary[]>([]);
  const [plugins, setPlugins] = useState<PluginSummary[]>([]);
  const [mcpServers, setMcpServers] = useState<McpServerOption[]>([]);
  const [showAttachMenu, setShowAttachMenu] = useState(false);
  const [attachMenuView, setAttachMenuView] = useState<AttachMenuView>("root");
  const [attachSearchQuery, setAttachSearchQuery] = useState("");
  const [robotModifyMode, setRobotModifyMode] = useState(false);
  const [visionWarning, setVisionWarning] = useState(false);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);
  const modelMenuRef = useRef<HTMLDivElement>(null);
  const attachMenuRef = useRef<HTMLDivElement>(null);

  const initialized = useAppStore((s) => s.initialized);
  const workspaceCwd = useAppStore((s) => s.workspaceCwd);
  const messages = useAppStore((s) => s.messages);
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
  const liveTurnUsage = useAppStore((s) => s.liveTurnUsage);
  const pendingMessageQueue = useAppStore((s) => s.pendingMessageQueue);
  const removeQueuedMessage = useAppStore((s) => s.removeQueuedMessage);
  // 目标模式运行态：只在 goal + active 时视为“整体执行中”。
  // 聊天模式不受该状态影响。
  // Goal lifecycle state and transport state are separate: an active goal can
  // be idle between turns. Only an actual dispatch/stream makes the composer busy.
  const composerBusy = isStreaming || isDispatching;
  const goalRunning =
    mode === "goal" && currentGoal?.status === "active" && composerBusy;
  const planExecutionProgress = useMemo(
    () => derivePlanExecutionProgress(messages, composerBusy, currentGoal?.workflowProgress),
    [composerBusy, currentGoal?.workflowProgress, messages],
  );

  // 供应商相关
  const providers = useAppStore((s) => s.providers);
  const activeProviderId = useAppStore((s) => s.activeProviderId);
  const overrideProviderId = useAppStore((s) => s.overrideProviderId);
  const overrideModelId = useAppStore((s) => s.overrideModelId);
  const smartbrainEnabled = useAppStore((s) => s.smartbrainEnabled);
  const setThreadModelOverride = useAppStore((s) => s.setThreadModelOverride);
  const setThreadSmartbrainEnabled = useAppStore((s) => s.setThreadSmartbrainEnabled);

  // 兼容旧的 configuredModels
  const configuredModels = useAppStore((s) => s.configuredModels);
  const activeModelId = useAppStore((s) => s.activeModelId);
  const currentModel = useAppStore((s) => s.currentModel);
  const defaultProvider = intl.formatMessage({ id: "app.defaultProvider" });

  const effectiveProviderId = overrideProviderId ?? activeProviderId;
  const activeProvider = useMemo(
    () => providers.find((p) => p.id === effectiveProviderId) ?? null,
    [providers, effectiveProviderId],
  );
  const providerModels = activeProvider?.models ?? [];

  // 模型只能在当前有效供应商内解析，避免旧 activeModelId 把 Z-API 拉回 Grok。
  const activeEntry = configuredModels.find((m) => m.id === activeModelId) ?? null;
  const effectiveModelId = resolveProviderModelId(activeProvider, {
    overrideModelId,
    currentModel,
    legacyModelId: activeEntry?.model,
  });
  const activeProviderModel = useMemo(() => {
    if (effectiveModelId) {
      return providerModels.find((model) => model.id === effectiveModelId) ?? null;
    }
    return providerModels[0] ?? null;
  }, [effectiveModelId, providerModels]);
  const usingThreadOverride = Boolean(overrideProviderId || overrideModelId);
  const pickerProviderId = modelPickerProviderId ?? effectiveProviderId ?? providers[0]?.id ?? null;
  const pickerProvider = useMemo(
    () => providers.find((provider) => provider.id === pickerProviderId) ?? null,
    [pickerProviderId, providers],
  );
  const filteredPickerModels = useMemo(
    () => filterProviderModels(pickerProvider?.models ?? [], modelSearchQuery),
    [modelSearchQuery, pickerProvider],
  );
  const displayModel = activeProvider && activeProviderModel
    ? `${activeProvider.name} / ${activeProviderModel.label}`
    : activeEntry?.label
      ?? currentModel
      ?? providerModels[0]?.label
      ?? activeProvider?.name
      ?? defaultProvider;

  const modelContextWindow = useMemo(() => {
    // 优先使用后端实时上报的窗口值，确保展示口径与运行时配置一致。
    const runtimeWindow = Number(liveTurnUsage?.contextWindowTokens ?? 0);
    if (runtimeWindow > 0) {
      return runtimeWindow;
    }

    if (activeProviderModel?.contextLength) {
      return activeProviderModel.contextLength;
    }

    if (effectiveModelId) {
      for (const provider of providers) {
        const matchedModel = provider.models.find((model) => model.id === effectiveModelId);
        if (matchedModel?.contextLength) {
          return matchedModel.contextLength;
        }
      }
    }

    return providerModels[0]?.contextLength ?? DEFAULT_MODEL_CONTEXT_LENGTH;
  }, [activeProviderModel, effectiveModelId, liveTurnUsage?.contextWindowTokens, providerModels, providers]);

  const contextUsedTokens = useMemo(() => {
    if (liveTurnUsage) {
      // 上下文占用只使用“单次请求 prompt tokens”，不再回退累计 promptTokens，
      // 避免累计值在多次工具调用后把占用显示拉高到假 100%。
      const liveUsed = Number(liveTurnUsage.lastSinglePromptTokens ?? 0);
      if (liveUsed > 0) {
        return liveUsed;
      }
    }
    for (let index = messages.length - 1; index >= 0; index -= 1) {
      const usage = messages[index].runSummary?.usage;
      if (!usage) {
        continue;
      }
      // 历史回填同样保持一致口径：只认 lastSinglePromptTokens。
      const used = Number(usage.lastSinglePromptTokens ?? 0);
      if (used > 0) {
        return used;
      }
    }
    return 0;
  }, [liveTurnUsage, messages]);

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

  useEffect(() => {
    pluginList()
      .then((list) => {
        // Computer Use 改为 MCP 入口，避免插件列表与 MCP 列表重复展示。
        setPlugins(list.filter((plugin) => plugin.id !== COMPUTER_USE_PLUGIN_ID));
      })
      .catch(() => setPlugins([]));
  }, []);

  useEffect(() => {
    const loadMcpServers = async () => {
      try {
        const resp = await standaloneConfigRead();
        const cfg = (resp?.config ?? {}) as Record<string, unknown>;
        const mcpServersCfg = (cfg.mcp_servers ?? cfg.mcpServers ?? {}) as Record<
          string,
          Record<string, unknown>
        >;
        const parsed: McpServerOption[] = Object.entries(mcpServersCfg)
          .map(([name, val]) => ({
            name,
            command: typeof val.command === "string" ? val.command : "",
            args: Array.isArray(val.args) ? val.args.map(String) : [],
            disabled: Boolean(val.disabled),
            source: "config" as const,
          }))
          .sort((a, b) => a.name.localeCompare(b.name));

        // 即便配置尚未写入，也保证 Computer Use MCP 可被直接添加到对话框。
        if (!parsed.some((server) => server.name === COMPUTER_USE_PLUGIN_ID)) {
          parsed.unshift({
            name: COMPUTER_USE_PLUGIN_ID,
            command: "node",
            args: ["scripts/computer-use-mcp-server.mjs"],
            disabled: false,
            source: "plugin",
          });
        } else {
          // 历史错误配置可能把 client 库写成了 MCP 入口，这里在 UI 层做纠正。
          for (const server of parsed) {
            if (server.name !== COMPUTER_USE_PLUGIN_ID) continue;
            const joined = `${server.command ?? ""} ${(server.args ?? []).join(" ")}`.toLowerCase();
            if (joined.includes("computer-use-client")) {
              server.command = "node";
              server.args = ["scripts/computer-use-mcp-server.mjs"];
              server.source = "plugin";
            }
          }
        }
        setMcpServers(parsed);
      } catch {
        setMcpServers([{
          name: COMPUTER_USE_PLUGIN_ID,
          command: "node",
          args: ["scripts/computer-use-mcp-server.mjs"],
          disabled: false,
          source: "plugin",
        }]);
      }
    };
    void loadMcpServers();
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
        if (!window.confirm(intl.formatMessage({ id: "chat.confirmClearHistory" }))) {
          return;
        }
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
    const planCommand = defaults.find((command) => command.name === "plan");
    if (planCommand) {
      planCommand.action = () => {
        setMode("plan");
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

  const handleEditQueuedMessage = useCallback((queuedMessageId: string) => {
    const queuedMessage = pendingMessageQueue.find((message) => message.id === queuedMessageId);
    if (!queuedMessage) {
      return;
    }
    removeQueuedMessage(queuedMessageId);
    setText(queuedMessage.text);
    setShowSlash(false);
    useAppStore.getState().setChatMode(queuedMessage.mode);
    useAppStore.getState().setAttachedFiles([...queuedMessage.attachments]);

    if (queuedMessage.options?.robotCreateMode) {
      setRobotCreateMode(true);
      setRobotModifyMode(false);
    } else if (queuedMessage.options?.robotId) {
      setSelectedRobotId(queuedMessage.options.robotId);
      setRobotModifyMode(Boolean(queuedMessage.options.robotModifyMode));
    } else {
      setSelectedRobotId(null);
      setRobotCreateMode(false);
      setRobotModifyMode(false);
    }

    requestAnimationFrame(() => {
      const element = textareaRef.current;
      if (!element) return;
      element.focus();
      element.style.height = "auto";
      element.style.height = `${Math.min(element.scrollHeight, 200)}px`;
    });
  }, [pendingMessageQueue, removeQueuedMessage, setRobotCreateMode, setSelectedRobotId]);

  const sendPrepared = useCallback(
    (
      rawText: string,
      sendMode: ChatMode,
      filesToSend: AttachedFile[],
      options?: ChatSendExtendedOptions,
    ) => {
      const payload = prepareSendPayload(rawText, filesToSend);
      onSend(payload.text, sendMode, payload.attachments, options);
    },
    [onSend],
  );

  const handleSkillCommandSubmit = useCallback((trimmed: string, filesToSend: AttachedFile[]) => {
    const skillCommand = parseSkillCommand(trimmed);
    if (skillCommand) {
      if (!skillCommand.objective) {
        setShowSlash(true);
        requestAnimationFrame(() => textareaRef.current?.focus());
        return true;
      }
      const payload = intl.formatMessage(
        { id: "chat.skillPrompt" },
        { skillId: skillCommand.skillId, objective: skillCommand.objective },
      );
      sendPrepared(payload, mode, filesToSend);
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
  }, [appendSystemMessage, intl, mode, resetComposerAfterSubmit, sendPrepared, skills.length]);

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

    sendPrepared(modifyCommand.objective, mode, filesToSend, {
      robotModifyMode: true,
      robotId: selectedRobotId!,
    });
    setRobotModifyMode(false);
    resetComposerAfterSubmit();
    return true;
  }, [activateRobotModifyMode, mode, resetComposerAfterSubmit, selectedRobotId, sendPrepared]);

  const handleSubmit = useCallback(() => {
    const trimmed = text.trim();
    if ((!trimmed && attachedFiles.length === 0) || disabled) return;
    const filesToSend = attachedFiles;

    // 使用点击时的实时状态，避免 turn 边界时旧渲染闭包导致误判。
    const currentStoreState = useAppStore.getState();
    if (isDispatching || currentStoreState.isStreaming) {
      const queuedPayload = prepareSendPayload(trimmed, filesToSend);
      const queuedMsg: QueuedMessage = {
        id: crypto.randomUUID(),
        text: queuedPayload.text,
        mode,
        attachments: [...queuedPayload.attachments],
        options: robotModifyMode && selectedRobotId
          ? { robotModifyMode: true, robotId: selectedRobotId }
          : selectedRobotId
            ? { robotId: selectedRobotId }
            : robotCreateMode
              ? { robotCreateMode: true }
              : undefined,
        timestamp: Date.now(),
      };
      useAppStore.getState().enqueueMessage(queuedMsg);
      resetComposerAfterSubmit();
      return;
    }

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
        sendPrepared(goalCommand.objective, "goal", filesToSend, {
          goalBudgetTokens: goalCommand.goalBudgetTokens,
        });
      }
      if (goalCommand.action !== "set" || !goalCommand.objective) {
        useAppStore.getState().clearAttachedFiles();
      }
    } else if (robotCreateMode) {
      sendPrepared(trimmed, mode, filesToSend, { robotCreateMode: true });
    } else if (robotModifyMode && selectedRobotId) {
      sendPrepared(trimmed, mode, filesToSend, { robotModifyMode: true, robotId: selectedRobotId });
      setRobotModifyMode(false);
    } else if (selectedRobotId) {
      sendPrepared(trimmed, "goal", filesToSend, { robotId: selectedRobotId });
    } else {
      sendPrepared(trimmed, mode, filesToSend);
    }
    resetComposerAfterSubmit();
  }, [
    attachedFiles,
    currentGoal,
    disabled,
    handleModifyRobotCommandSubmit,
    handleSkillCommandSubmit,
    isDispatching,
    mode,
    onGoalCommand,
    resetComposerAfterSubmit,
    robotCreateMode,
    robotModifyMode,
    selectedRobotId,
    sendPrepared,
    text,
  ]);

  const goalStatusLabel = useMemo(() => {
    if (!currentGoal) {
      return intl.formatMessage({ id: "chat.mode.goalActive" });
    }

    const displayedStatus = currentGoal.status === "active" && !composerBusy
      ? "paused"
      : currentGoal.status;
    return intl.formatMessage({ id: `chat.goalStatus.${displayedStatus}` });
  }, [composerBusy, currentGoal, intl]);

  const goalStatusClass = currentGoal?.status === "paused" || (
    currentGoal?.status === "active" && !composerBusy
  )
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
        setShowRobotMenu(false);
        setShowAttachMenu(false);
      }
    },
    [goalRunning, handleSubmit],
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
  const closeAttachMenu = useCallback(() => {
    setShowAttachMenu(false);
    setAttachMenuView("root");
    setAttachSearchQuery("");
  }, []);

  const insertComposerText = useCallback((snippet: string, options?: { replaceEmpty?: boolean }) => {
    const nextSnippet = snippet.trim();
    if (!nextSnippet) {
      return;
    }
    setText((prev) => {
      const trimmedPrev = prev.trim();
      if (!trimmedPrev) {
        return nextSnippet;
      }
      if (options?.replaceEmpty) {
        return `${trimmedPrev}\n${nextSnippet}`;
      }
      return `${prev.replace(/\s+$/, "")}\n${nextSnippet}`;
    });
    setShowSlash(false);
    closeAttachMenu();
    requestAnimationFrame(() => {
      const element = textareaRef.current;
      if (!element) return;
      element.focus();
      element.style.height = "auto";
      element.style.height = `${Math.min(element.scrollHeight, 200)}px`;
      const caret = element.value.length;
      element.setSelectionRange(caret, caret);
    });
  }, [closeAttachMenu]);

  const handleSelectSkill = useCallback((skill: SkillSummary) => {
    insertComposerText(`/skill ${skill.id} `);
  }, [insertComposerText]);

  const handleSelectPlugin = useCallback((plugin: PluginSummary) => {
    const label = plugin.displayName || plugin.name || plugin.id;
    insertComposerText(
      intl.formatMessage(
        { id: "chat.pluginPrompt" },
        { pluginId: plugin.id, pluginName: label },
      ),
    );
  }, [insertComposerText, intl]);

  const handleSelectMcp = useCallback((server: McpServerOption) => {
    insertComposerText(
      intl.formatMessage(
        { id: "chat.mcpPrompt" },
        { mcpName: server.name },
      ),
    );
  }, [insertComposerText, intl]);

  const filteredAttachSkills = useMemo(() => {
    const query = attachSearchQuery.trim().toLowerCase();
    if (!query) return skills;
    return skills.filter((skill) => {
      const tags = Array.isArray(skill.tags) ? skill.tags.join(" ") : "";
      return `${skill.id} ${skill.name} ${skill.description} ${tags}`.toLowerCase().includes(query);
    });
  }, [attachSearchQuery, skills]);

  const filteredAttachPlugins = useMemo(() => {
    const query = attachSearchQuery.trim().toLowerCase();
    const enabledPlugins = plugins.filter((plugin) => plugin.enabled && !plugin.error);
    if (!query) return enabledPlugins;
    return enabledPlugins.filter((plugin) => {
      const keywords = Array.isArray(plugin.keywords) ? plugin.keywords.join(" ") : "";
      return `${plugin.id} ${plugin.name} ${plugin.displayName} ${plugin.description ?? ""} ${keywords}`
        .toLowerCase()
        .includes(query);
    });
  }, [attachSearchQuery, plugins]);

  const filteredAttachMcps = useMemo(() => {
    const query = attachSearchQuery.trim().toLowerCase();
    const enabledServers = mcpServers.filter((server) => !server.disabled);
    if (!query) return enabledServers;
    return enabledServers.filter((server) => {
      const commandText = `${server.command ?? ""} ${(server.args ?? []).join(" ")}`.toLowerCase();
      return `${server.name} ${commandText}`.includes(query);
    });
  }, [attachSearchQuery, mcpServers]);

  const handleAttachClick = useCallback(() => {
    setShowAttachMenu((prev) => {
      const next = !prev;
      if (next) {
        setAttachMenuView("root");
        setAttachSearchQuery("");
        setShowModelMenu(false);
        setShowRobotMenu(false);
        setShowSlash(false);
      }
      return next;
    });
  }, []);

  const warnVisionUnsupportedIfNeeded = useCallback((mimeType: string) => {
    if (!mimeType.startsWith("image/")) {
      return;
    }
    if (activeProviderModel?.supportsVision || activeEntry?.supportsVision) {
      return;
    }
    if (activeProviderModel?.visionFallbackKind === VISION_FALLBACK_KIND_LOCAL_OCR) {
      return;
    }
    if (
      activeProviderModel?.visionFallbackProviderId
      && activeProviderModel?.visionFallbackModelId
    ) {
      return;
    }
    const providerVision = providerModels.some((m) => m.supportsVision);
    if (!providerVision) {
      setVisionWarning(true);
      setTimeout(() => setVisionWarning(false), 4000);
    }
  }, [activeEntry, activeProviderModel, providerModels]);

  const addBrowserFileAttachment = useCallback((file: File, fallbackName?: string) => {
    const resolvedName = file.name.trim() || fallbackName || `attachment-${Date.now()}`;
    const resolvedType = file.type || guessTypeFromExt(resolvedName);
    warnVisionUnsupportedIfNeeded(resolvedType);

    const reader = new FileReader();
    reader.onload = () => {
      const attached: AttachedFile = {
        kind: "binary",
        name: resolvedName,
        type: resolvedType,
        dataUrl: reader.result as string,
        size: file.size,
      };
      useAppStore.getState().addAttachedFile(attached);
    };
    reader.readAsDataURL(file);
  }, [warnVisionUnsupportedIfNeeded]);

  const addBrowserFileAttachments = useCallback((files: BrowserFileAttachmentInput[]) => {
    for (const fileEntry of files) {
      addBrowserFileAttachment(fileEntry.file, fileEntry.fallbackName);
    }
  }, [addBrowserFileAttachment]);

  const handleFileChange = useCallback((event: React.ChangeEvent<HTMLInputElement>) => {
    const files = event.target.files;
    if (!files || files.length === 0) return;
    addBrowserFileAttachments(Array.from(files).map((file) => ({ file })));
    event.target.value = "";
  }, [addBrowserFileAttachments]);

  const handleRemoveFile = useCallback((index: number) => {
    useAppStore.getState().removeAttachedFile(index);
  }, []);

  const [smartBrainAdded, setSmartBrainAdded] = useState<Set<number>>(new Set());

  const handleAddToSmartBrain = useCallback(async (file: AttachedFile, index: number) => {
    if (isWebSnippetAttachedFile(file) || !file.sourcePath || smartBrainAdded.has(index)) return;
    try {
      await invoke("smartbrain_upload_knowledge", { filePath: file.sourcePath });
      setSmartBrainAdded((prev) => new Set(prev).add(index));
    } catch (err) {
      console.error("Add to Local Knowledge Base failed:", err);
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
        const sourcePath = typeof path === "string" ? path.trim() : "";
        if (!sourcePath) {
          console.error("Project file path is empty");
          return;
        }
        const fallbackName = sourcePath.split(/[\\/]/).filter(Boolean).pop() ?? sourcePath;
        const attached: AttachedFile = {
          kind: "pathRef",
          name: (name || "").trim() || fallbackName,
          type: PATH_REF_MIME,
          size: 0,
          sourcePath,
        };
        useAppStore.getState().addAttachedFile(attached);
      } catch {
        console.error("Failed to parse project file data");
      }
      return;
    }

    const files = e.dataTransfer.files;
    if (!files || files.length === 0) return;
    addBrowserFileAttachments(Array.from(files).map((file) => ({ file })));
  }, [addBrowserFileAttachments]);

  const handlePaste = useCallback((event: React.ClipboardEvent<HTMLTextAreaElement>) => {
    const clipboardItems = event.clipboardData?.items;
    if (!clipboardItems || clipboardItems.length === 0) {
      return;
    }
    const imageFiles = extractClipboardImageFiles(clipboardItems);
    if (imageFiles.length === 0) {
      return;
    }
    event.preventDefault();
    addBrowserFileAttachments(imageFiles);
  }, [addBrowserFileAttachments]);

  // 对话框内切换模型：仅写入本对话覆盖，不改全局供应商/模型。
  const handleThreadProviderModelSelect = useCallback((providerId: string, modelId: string) => {
    setThreadModelOverride(providerId, modelId);
    setShowModelMenu(false);
    setModelSearchQuery("");
  }, [setThreadModelOverride]);

  const handleModelPickerProviderChange = useCallback((providerId: string) => {
    setModelPickerProviderId(providerId);
    setModelSearchQuery("");
    const provider = providers.find((item) => item.id === providerId) ?? null;
    const modelId = resolveProviderModelId(provider, {
      overrideModelId: providerId === overrideProviderId ? overrideModelId : null,
      currentModel: providerId === activeProviderId ? currentModel : null,
      legacyModelId: null,
    });
    setThreadModelOverride(providerId, modelId);
  }, [activeProviderId, currentModel, overrideModelId, overrideProviderId, providers, setThreadModelOverride]);

  const handleUseGlobalModel = useCallback(() => {
    setThreadModelOverride(null, null);
    setShowModelMenu(false);
    setModelPickerProviderId(activeProviderId);
    setModelSearchQuery("");
  }, [activeProviderId, setThreadModelOverride]);

  const handleModelMenuToggle = useCallback(() => {
    if (showModelMenu) {
      setShowModelMenu(false);
      return;
    }
    setModelPickerProviderId(resolveModelPickerProviderId(
      providers.map((provider) => provider.id),
      effectiveProviderId,
      activeProviderId,
    ));
    setModelSearchQuery("");
    setShowModelMenu(true);
  }, [activeProviderId, effectiveProviderId, providers, showModelMenu]);

  // 点击菜单外任意区域关闭模型选择弹层，避免必须先选中模型才能退出。
  useEffect(() => {
    if (!showModelMenu) {
      return;
    }
    const handlePointerDown = (event: MouseEvent) => {
      const target = event.target as Node | null;
      if (!target || modelMenuRef.current?.contains(target)) {
        return;
      }
      setShowModelMenu(false);
      setModelSearchQuery("");
    };
    window.addEventListener("mousedown", handlePointerDown);
    return () => {
      window.removeEventListener("mousedown", handlePointerDown);
    };
  }, [showModelMenu]);

  // 点击菜单外任意区域关闭附件菜单。
  useEffect(() => {
    if (!showAttachMenu) {
      return;
    }
    const handlePointerDown = (event: MouseEvent) => {
      const target = event.target as Node | null;
      if (!target || attachMenuRef.current?.contains(target)) {
        return;
      }
      closeAttachMenu();
    };
    window.addEventListener("mousedown", handlePointerDown);
    return () => {
      window.removeEventListener("mousedown", handlePointerDown);
    };
  }, [closeAttachMenu, showAttachMenu]);

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
            if (command.name === "help" || command.name === "compact") {
              if (!goalRunning) {
                onSend(`/${command.name}`, mode, []);
              }
              setText("");
            }
            if (command.name === "plan") {
              setText("");
            }
            if (command.closeOnSelect !== false) {
              setShowSlash(false);
            }
          }}
          onClose={() => setShowSlash(false)}
        />
      )}

      {visionWarning && (
        <div className="absolute bottom-full left-0 right-0 z-10 mx-auto max-w-[1180px] px-4 pb-2 sm:px-8">
          <div className="rounded-[var(--radius-sm)] border border-[rgba(239,180,40,0.3)] bg-[rgba(239,180,40,0.1)] px-3 py-2 text-xs text-[var(--warning)]">
            {intl.formatMessage({ id: "chat.visionNotSupported" })}
          </div>
        </div>
      )}

      <div className="mx-auto max-w-[1180px]">
        {/* 排队消息列表 */}
        {pendingMessageQueue.length > 0 && (
          <div className="chat-queue-list mb-1.5 space-y-1">
            {pendingMessageQueue.map((qm, idx) => (
              <div
                key={qm.id}
                className={`chat-queue-item flex items-center gap-2 rounded-[var(--radius-md)] border px-3 py-1.5 ${
                  idx === 0
                    ? "border-[var(--accent-border)] bg-[var(--accent-soft)]"
                    : "border-[var(--chat-line)] bg-[var(--chat-chip)]"
                }`}
              >
                {idx === 0 && (
                  <span className="flex-shrink-0 rounded-full bg-[var(--accent)] px-1.5 py-0.5 text-[9px] font-semibold uppercase leading-none text-white">
                    {intl.formatMessage({ id: "chat.queueNext" })}
                  </span>
                )}
                <span className="min-w-0 flex-1 truncate text-[12px] text-[var(--chat-prose)]">
                  {qm.text.length > 60 ? `${qm.text.slice(0, 60)}...` : qm.text}
                </span>
                <button
                  type="button"
                  onClick={() => onJumpQueue?.(qm.id)}
                  className="flex h-5 w-5 flex-shrink-0 items-center justify-center rounded-full border border-[var(--accent-border)] bg-[var(--accent-soft)] text-[var(--accent)] transition-colors hover:bg-[var(--accent)] hover:text-white"
                  title={intl.formatMessage({ id: "chat.queueJump" })}
                >
                  <IconPlayerSkipForward size={10} stroke={2.5} />
                </button>
                <button
                  type="button"
                  onClick={() => handleEditQueuedMessage(qm.id)}
                  className="flex h-5 w-5 flex-shrink-0 items-center justify-center rounded-full border border-[var(--chat-line)] text-[var(--chat-faint)] transition-colors hover:border-[var(--accent-border)] hover:bg-[var(--accent-soft)] hover:text-[var(--accent-strong)]"
                  title={intl.formatMessage({ id: "chat.queueEdit" })}
                  aria-label={intl.formatMessage({ id: "chat.queueEdit" })}
                >
                  <IconPencil size={10} stroke={2.5} />
                </button>
                <button
                  type="button"
                  onClick={() => removeQueuedMessage(qm.id)}
                  className="flex h-5 w-5 flex-shrink-0 items-center justify-center rounded-full text-[var(--chat-faint)] transition-colors hover:bg-[var(--danger-soft)] hover:text-[var(--danger)]"
                  title={intl.formatMessage({ id: "chat.queueRemove" })}
                >
                  <IconX size={10} stroke={2.5} />
                </button>
              </div>
            ))}
          </div>
        )}

        {planExecutionProgress && (
          <PlanExecutionProgress progress={planExecutionProgress} />
        )}

        <div className="chat-composer-shell px-4 pb-3 pt-3">
          <div className="mb-2 flex flex-wrap items-center justify-between gap-2">
          <div className="inline-flex min-w-0 flex-1 flex-wrap items-center gap-2">
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
              {intl.formatMessage({ id: "chat.mode.chat" })}
            </button>
            <button
              type="button"
              aria-pressed={mode === "plan"}
              onClick={() => setMode("plan")}
              className={`flex h-6 items-center gap-1 rounded-full px-2.5 text-[11px] font-medium transition-colors ${
                mode === "plan"
                  ? "bg-[var(--chat-card-solid)] text-[var(--chat-prose)] shadow-[var(--shadow-soft)]"
                  : "text-[var(--chat-muted)] hover:text-[var(--chat-prose)]"
              }`}
            >
              <IconClipboardList size={13} stroke={1.8} />
              {intl.formatMessage({ id: "chat.mode.plan" })}
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
                <div className="absolute left-0 bottom-full z-30 mb-1 w-[180px] max-h-[200px] overflow-y-auto rounded-[var(--radius-md)] border border-[var(--chat-line)] bg-[var(--chat-card-solid)] py-1 shadow-lg">
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
                      <span className="truncate font-medium">{robot.name}</span>
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

          <LanGroupChatLauncher />
          </div>

        {/* 附件预览区 */}
        {attachedFiles.length > 0 && (
          <div className="mb-2 flex flex-wrap items-center gap-2">
            {attachedFiles.map((file, idx) => {
              const isImageAttachment = isBinaryAttachedFile(file) && file.type.startsWith("image/");
              const isPathRefAttachment = isPathRefAttachedFile(file);
              const isWebSnippetAttachment = isWebSnippetAttachedFile(file);
              const pathRefLineRange = isPathRefAttachment ? formatPathRefLineRange(file) : null;
              const chipTitle = isPathRefAttachment
                ? (pathRefLineRange ? `${file.sourcePath}\n${pathRefLineRange}` : file.sourcePath)
                : isWebSnippetAttachment
                  ? `${file.url}\n${file.selector}`
                  : undefined;
              const secondaryText = isPathRefAttachment
                ? (pathRefLineRange ? `${pathRefLineRange} · ${file.sourcePath}` : file.sourcePath)
                : isWebSnippetAttachment
                  ? file.selector || file.url
                  : formatSize(file.size);
              const canAddToSmartBrain = !isWebSnippetAttachment && !!file.sourcePath && !file.type.startsWith("image/");
              return (
                <div
                  key={idx}
                  className="group relative flex items-center gap-2 rounded-[var(--radius-md)] border border-[var(--chat-line)] bg-[var(--chat-chip)] px-2 py-1.5"
                  title={chipTitle}
                >
                  {isImageAttachment ? (
                    <div className="h-10 w-10 overflow-hidden rounded-[var(--radius-sm)]">
                      <img src={file.dataUrl} alt="" className="h-full w-full object-cover" />
                    </div>
                  ) : isWebSnippetAttachment ? (
                    <IconBrowser size={16} stroke={1.5} className="text-[var(--accent)]" />
                  ) : isPathRefAttachment ? (
                    <IconFolder size={16} stroke={1.5} className="text-[var(--accent)]" />
                  ) : (
                    <IconFile size={16} stroke={1.5} className="text-[var(--chat-muted)]" />
                  )}
                  <div className="max-w-[220px]">
                    <p className="truncate text-[11px] text-[var(--chat-prose)]">
                      {isPathRefAttachment ? pathRefPrimaryText(file) : file.name}
                    </p>
                    <p className="truncate text-[11px] text-[var(--chat-faint)]">
                      {secondaryText}
                    </p>
                  </div>
                  {canAddToSmartBrain && (
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
              );
            })}
          </div>
        )}

        {/* 输入框主体 */}
        <div className="flex min-h-[82px] items-end gap-3 rounded-[var(--radius-lg)] px-1 py-1 transition-colors">
          {/* 附件按钮 */}
          <div ref={attachMenuRef} className="relative mb-1">
            <button
              type="button"
              onClick={handleAttachClick}
              className={`flex h-9 w-9 flex-shrink-0 items-center justify-center rounded-full border transition-colors ${
                showAttachMenu
                  ? "border-[var(--accent-border)] bg-[var(--accent-soft)] text-[var(--accent-strong)]"
                  : "border-[var(--chat-line)] text-[var(--chat-muted)] hover:bg-[var(--chat-chip)] hover:text-[var(--chat-prose)]"
              }`}
              title={intl.formatMessage({ id: "chat.attachMenu" })}
              aria-expanded={showAttachMenu}
            >
              <IconPaperclip size={15} stroke={1.8} />
            </button>

            {showAttachMenu && (
              <div className="absolute bottom-full left-0 z-40 mb-2 w-[280px] overflow-hidden rounded-[var(--radius-md)] border border-[var(--chat-line)] bg-[var(--chat-card-solid)] shadow-lg">
                {attachMenuView === "root" ? (
                  <div className="py-1">
                    <button
                      type="button"
                      onClick={() => {
                        closeAttachMenu();
                        fileInputRef.current?.click();
                      }}
                      className="flex w-full items-center gap-2 px-3 py-2 text-left text-[12px] text-[var(--chat-prose)] transition-colors hover:bg-[var(--surface-elevated)]"
                    >
                      <IconFile size={14} stroke={1.8} className="shrink-0 text-[var(--chat-muted)]" />
                      <span>{intl.formatMessage({ id: "chat.attachFile" })}</span>
                    </button>
                    <button
                      type="button"
                      onClick={() => {
                        setAttachMenuView("skill");
                        setAttachSearchQuery("");
                      }}
                      className="flex w-full items-center gap-2 px-3 py-2 text-left text-[12px] text-[var(--chat-prose)] transition-colors hover:bg-[var(--surface-elevated)]"
                    >
                      <IconCpu size={14} stroke={1.8} className="shrink-0 text-[var(--chat-muted)]" />
                      <span>{intl.formatMessage({ id: "chat.attachSkill" })}</span>
                    </button>
                    <button
                      type="button"
                      onClick={() => {
                        setAttachMenuView("plugin");
                        setAttachSearchQuery("");
                      }}
                      className="flex w-full items-center gap-2 px-3 py-2 text-left text-[12px] text-[var(--chat-prose)] transition-colors hover:bg-[var(--surface-elevated)]"
                    >
                      <IconPuzzle size={14} stroke={1.8} className="shrink-0 text-[var(--chat-muted)]" />
                      <span>{intl.formatMessage({ id: "chat.attachPlugin" })}</span>
                    </button>
                    <button
                      type="button"
                      onClick={() => {
                        setAttachMenuView("mcp");
                        setAttachSearchQuery("");
                      }}
                      className="flex w-full items-center gap-2 px-3 py-2 text-left text-[12px] text-[var(--chat-prose)] transition-colors hover:bg-[var(--surface-elevated)]"
                    >
                      <IconServer size={14} stroke={1.8} className="shrink-0 text-[var(--chat-muted)]" />
                      <span>{intl.formatMessage({ id: "chat.attachMcp" })}</span>
                    </button>
                  </div>
                ) : (
                  <div className="flex max-h-[280px] flex-col">
                    <div className="flex items-center justify-between gap-2 border-b border-[var(--chat-line)] px-3 py-2">
                      <button
                        type="button"
                        onClick={() => {
                          setAttachMenuView("root");
                          setAttachSearchQuery("");
                        }}
                        className="text-[11px] text-[var(--chat-muted)] transition-colors hover:text-[var(--chat-prose)]"
                      >
                        {intl.formatMessage({ id: "chat.attachBack" })}
                      </button>
                      <span className="text-[11px] font-medium text-[var(--chat-prose)]">
                        {intl.formatMessage({
                          id:
                            attachMenuView === "skill"
                              ? "chat.attachSkill"
                              : attachMenuView === "plugin"
                                ? "chat.attachPlugin"
                                : "chat.attachMcp",
                        })}
                      </span>
                    </div>
                    <div className="border-b border-[var(--chat-line)] px-3 py-2">
                      <input
                        type="text"
                        value={attachSearchQuery}
                        onChange={(event) => setAttachSearchQuery(event.target.value)}
                        placeholder={intl.formatMessage({ id: "chat.attachSearchPlaceholder" })}
                        className="w-full rounded-[var(--radius-sm)] border border-[var(--chat-line)] bg-transparent px-2 py-1.5 text-[12px] text-[var(--chat-prose)] outline-none placeholder:text-[var(--chat-faint)]"
                        autoFocus
                      />
                    </div>
                    <div className="thin-scrollbar flex-1 overflow-y-auto py-1">
                      {attachMenuView === "skill" && (
                        filteredAttachSkills.length === 0 ? (
                          <p className="px-3 py-3 text-[12px] text-[var(--chat-faint)]">
                            {intl.formatMessage({ id: "chat.skill.noneAvailable" })}
                          </p>
                        ) : (
                          filteredAttachSkills.map((skill) => (
                            <button
                              key={skill.id}
                              type="button"
                              onClick={() => handleSelectSkill(skill)}
                              className="flex w-full flex-col gap-0.5 px-3 py-2 text-left transition-colors hover:bg-[var(--surface-elevated)]"
                            >
                              <span className="truncate text-[12px] font-medium text-[var(--chat-prose)]">
                                {skill.name || skill.id}
                              </span>
                              <span className="truncate text-[11px] text-[var(--chat-faint)]">
                                {skill.description || skill.id}
                              </span>
                            </button>
                          ))
                        )
                      )}

                      {attachMenuView === "plugin" && (
                        filteredAttachPlugins.length === 0 ? (
                          <p className="px-3 py-3 text-[12px] text-[var(--chat-faint)]">
                            {intl.formatMessage({ id: "chat.plugin.noneAvailable" })}
                          </p>
                        ) : (
                          filteredAttachPlugins.map((plugin) => (
                            <button
                              key={plugin.id}
                              type="button"
                              onClick={() => handleSelectPlugin(plugin)}
                              className="flex w-full flex-col gap-0.5 px-3 py-2 text-left transition-colors hover:bg-[var(--surface-elevated)]"
                            >
                              <span className="truncate text-[12px] font-medium text-[var(--chat-prose)]">
                                {plugin.displayName || plugin.name || plugin.id}
                              </span>
                              <span className="truncate text-[11px] text-[var(--chat-faint)]">
                                {plugin.description || plugin.id}
                              </span>
                            </button>
                          ))
                        )
                      )}

                      {attachMenuView === "mcp" && (
                        filteredAttachMcps.length === 0 ? (
                          <p className="px-3 py-3 text-[12px] text-[var(--chat-faint)]">
                            {intl.formatMessage({ id: "chat.mcp.noneAvailable" })}
                          </p>
                        ) : (
                          filteredAttachMcps.map((server) => (
                            <button
                              key={server.name}
                              type="button"
                              onClick={() => handleSelectMcp(server)}
                              className="flex w-full flex-col gap-0.5 px-3 py-2 text-left transition-colors hover:bg-[var(--surface-elevated)]"
                            >
                              <span className="truncate text-[12px] font-medium text-[var(--chat-prose)]">
                                {server.name}
                              </span>
                              <span className="truncate text-[11px] text-[var(--chat-faint)]">
                                {server.name === COMPUTER_USE_PLUGIN_ID
                                  ? intl.formatMessage({ id: "chat.mcp.computerUseHint" })
                                  : `${server.command ?? ""} ${(server.args ?? []).join(" ")}`.trim() || "MCP"}
                              </span>
                            </button>
                          ))
                        )
                      )}
                    </div>
                  </div>
                )}
              </div>
            )}
          </div>
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
            onPaste={handlePaste}
            placeholder={intl.formatMessage({
              id: composerBusy
                ? "chat.queuePlaceholder"
                : robotModifyMode
                  ? "chat.robot.modifyPlaceholder"
                  : robotCreateMode
                    ? "chat.robot.placeholder"
                    : "chat.inputPlaceholder",
            })}
            disabled={disabled}
            rows={2}
            className="chat-composer-input max-h-[200px] min-h-[78px] w-full flex-1 resize-none bg-transparent py-2 text-[13px] leading-relaxed text-[var(--chat-prose)] placeholder:text-[var(--chat-faint)] outline-none disabled:opacity-50"
          />

          {composerBusy && !text.trim() ? (
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
            <div ref={modelMenuRef} className="relative">
              <button
                type="button"
                onClick={() => {
                  // 两段联动选择器只基于 providers；旧 configuredModels 仅作显示兼容，
                  // 没有供应商时直接进入设置，避免打开空菜单。
                  if (providers.length > 0) {
                    handleModelMenuToggle();
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

              {/* 模型选择菜单：紧贴触发按钮上方，供应商 + 可搜索模型两段联动 */}
              {showModelMenu && (
                <div className="absolute bottom-full left-0 z-30 mb-1 w-[min(360px,calc(100vw-2rem))] max-w-[calc(100vw-2rem)] overflow-hidden rounded-[var(--radius-md)] border border-[var(--chat-line)] bg-[var(--chat-card-solid)] shadow-lg">
                  <div className="flex items-center justify-between gap-2 border-b border-[var(--chat-line)] px-3 py-2">
                    <div className="min-w-0">
                      <div className="text-[11px] font-medium uppercase tracking-wider text-[var(--text-faint)]">
                        {intl.formatMessage({ id: "chat.modelSelectorHint" })}
                      </div>
                      <div className="truncate text-[11px] text-[var(--text-muted)]">
                        {usingThreadOverride
                          ? intl.formatMessage({ id: "chat.modelScope.thread" })
                          : intl.formatMessage({ id: "chat.modelScope.global" })}
                      </div>
                    </div>
                    {usingThreadOverride && (
                      <button
                        type="button"
                        onClick={handleUseGlobalModel}
                        className="rounded-full px-2 py-1 text-[11px] text-[var(--accent)] transition-colors hover:bg-[var(--accent-soft)]"
                      >
                        {intl.formatMessage({ id: "chat.modelUseGlobal" })}
                      </button>
                    )}
                  </div>

                  {providers.length > 0 ? (
                    <div className="space-y-3 p-3">
                      <label className="block min-w-0 space-y-1.5">
                        <span className="block text-[11px] font-medium text-[var(--text-muted)]">
                          {intl.formatMessage({ id: "chat.modelProviderLabel" })}
                        </span>
                        <select
                          value={pickerProviderId ?? ""}
                          onChange={(event) => handleModelPickerProviderChange(event.target.value)}
                          className="h-9 w-full min-w-0 rounded-[var(--radius-sm)] border border-[var(--chat-line)] bg-[var(--surface-elevated)] px-2.5 text-[13px] text-[var(--text-base)] outline-none focus:border-[var(--accent)]"
                        >
                          {providers.map((provider) => (
                            <option key={provider.id} value={provider.id}>
                              {provider.name}{provider.id === activeProviderId
                                ? ` · ${intl.formatMessage({ id: "chat.modelGlobalBadge" })}`
                                : ""}
                            </option>
                          ))}
                        </select>
                      </label>

                      <div className="min-w-0 space-y-1.5">
                        <label htmlFor="chat-model-search" className="block text-[11px] font-medium text-[var(--text-muted)]">
                          {intl.formatMessage({ id: "chat.modelLabel" })}
                        </label>
                        <input
                          id="chat-model-search"
                          type="text"
                          value={modelSearchQuery}
                          onChange={(event) => setModelSearchQuery(event.target.value)}
                          placeholder={intl.formatMessage({ id: "chat.modelSearchPlaceholder" })}
                          autoComplete="off"
                          className="h-9 w-full min-w-0 rounded-[var(--radius-sm)] border border-[var(--chat-line)] bg-[var(--surface-elevated)] px-2.5 text-[13px] text-[var(--text-base)] outline-none placeholder:text-[var(--text-faint)] focus:border-[var(--accent)]"
                        />
                        <div className="max-h-[min(220px,32vh)] overflow-y-auto rounded-[var(--radius-sm)] border border-[var(--chat-line)] bg-[var(--chat-card-solid)] py-1">
                          {filteredPickerModels.length > 0 ? (
                            filteredPickerModels.map((model) => {
                              const selected = pickerProvider?.id === effectiveProviderId
                                && model.id === effectiveModelId;
                              return (
                                <button
                                  key={`${pickerProvider?.id ?? "provider"}:${model.id}`}
                                  type="button"
                                  onClick={() => pickerProvider
                                    && handleThreadProviderModelSelect(pickerProvider.id, model.id)}
                                  className={`flex w-full items-center gap-2.5 px-2.5 py-2 text-left text-[13px] transition-colors hover:bg-[var(--surface-elevated)] ${
                                    selected
                                      ? "bg-[var(--accent-soft)] text-[var(--accent-strong)]"
                                      : "text-[var(--text-base)]"
                                  }`}
                                >
                                  <IconCpu size={13} stroke={1.8} className="flex-shrink-0 opacity-60" />
                                  <span className="min-w-0 flex-1">
                                    <span className="block truncate font-medium">{model.label}</span>
                                    <span className="block truncate text-[11px] text-[var(--text-faint)]">
                                      {model.id}{model.supportsVision && " · 📷 vision"}
                                    </span>
                                  </span>
                                  {selected && (
                                    <span className="h-1.5 w-1.5 flex-shrink-0 rounded-full bg-[var(--accent)]" />
                                  )}
                                </button>
                              );
                            })
                          ) : (
                            <div className="px-3 py-4 text-center text-xs text-[var(--text-faint)]">
                              {pickerProvider?.models.length
                                ? intl.formatMessage({ id: "chat.modelSearchEmpty" })
                                : intl.formatMessage({ id: "chat.modelEmptyProvider" })}
                            </div>
                          )}
                        </div>
                      </div>
                    </div>
                  ) : (
                    <div className="px-3 py-4 text-center text-xs text-[var(--text-faint)]">
                      {intl.formatMessage({ id: "chat.modelEmpty" })}
                    </div>
                  )}
                </div>
              )}
            </div>

            <button
              type="button"
              onClick={() => setThreadSmartbrainEnabled(!smartbrainEnabled)}
              className={`flex items-center gap-1 rounded-full px-2 py-1 transition-colors ${
                smartbrainEnabled
                  ? "bg-[var(--accent-soft)] text-[var(--accent)]"
                  : "hover:bg-[var(--chat-chip)] hover:text-[var(--chat-prose)]"
              }`}
              title={intl.formatMessage({
                id: smartbrainEnabled ? "chat.smartbrainOn" : "chat.smartbrainOff",
              })}
            >
              <IconBrain size={12} stroke={1.8} className="flex-shrink-0" />
              <span className="truncate">{intl.formatMessage({ id: "chat.smartbrain" })}</span>
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
            {contextUsedTokens > 0 && modelContextWindow > 0 && (
              <ContextUsageRing usedTokens={contextUsedTokens} windowTokens={modelContextWindow} />
            )}
            <span className="hidden text-[var(--chat-faint)] sm:inline">
              {intl.formatMessage({ id: "chat.inputHint" })}
            </span>
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

function ContextUsageRing({
  usedTokens,
  windowTokens,
}: {
  usedTokens: number;
  windowTokens: number;
}) {
  const intl = useIntl();
  const ratio = Math.max(0, Math.min(1, usedTokens / windowTokens));
  // 让 tooltip 与圆环百分比保持同一口径，避免出现“百分比 100% 但文本超过上限”的认知冲突。
  const clampedUsedTokens = Math.max(0, Math.min(usedTokens, windowTokens));
  const radius = 7;
  const strokeWidth = 2;
  const normalizedRadius = radius - strokeWidth / 2;
  const circumference = 2 * Math.PI * normalizedRadius;
  const dashOffset = circumference * (1 - ratio);
  const formatter = new Intl.NumberFormat(undefined, { maximumFractionDigits: 0 });
  const percentage = ratio * 100;
  const percentageText = percentage >= 99.95
    ? "100%"
    : `${percentage.toFixed(percentage < 10 ? 1 : 0)}%`;
  const progressColor = ratio >= 0.9
    ? "var(--danger)"
    : ratio >= 0.75
      ? "var(--warning)"
      : "var(--accent)";

  return (
    <span
      className="flex items-center gap-1.5 rounded-full border border-[var(--chat-line)] px-2 py-0.5 text-[var(--chat-muted)]"
      title={intl.formatMessage(
        { id: "chat.contextUsage" },
        {
          used: formatter.format(clampedUsedTokens),
          total: formatter.format(windowTokens),
        },
      )}
    >
      <svg width="16" height="16" viewBox="0 0 16 16" aria-hidden>
        <circle
          cx="8"
          cy="8"
          r={normalizedRadius}
          fill="none"
          stroke="var(--chat-line)"
          strokeWidth={strokeWidth}
        />
        <circle
          cx="8"
          cy="8"
          r={normalizedRadius}
          fill="none"
          stroke={progressColor}
          strokeWidth={strokeWidth}
          strokeLinecap="round"
          strokeDasharray={`${circumference} ${circumference}`}
          strokeDashoffset={dashOffset}
          transform="rotate(-90 8 8)"
        />
      </svg>
      <span className="font-mono tabular-nums">{percentageText}</span>
    </span>
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
  return `Please prioritize skill "${normalizedSkillId}", then complete the following:\n${normalizedObjective}`;
}

export function buildPluginScopedPrompt(pluginId: string, pluginName?: string): string {
  const normalizedPluginId = pluginId.trim();
  const displayName = (pluginName ?? normalizedPluginId).trim() || normalizedPluginId;
  return `Please prioritize plugin "${displayName}" (id: ${normalizedPluginId}), then complete the following:`;
}

export function buildMcpScopedPrompt(mcpName: string): string {
  const normalizedMcpName = mcpName.trim();
  return `Please prioritize MCP server "${normalizedMcpName}", then complete the following:`;
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
