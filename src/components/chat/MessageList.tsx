import {
  IconAlertTriangle,
  IconBrowser,
  IconCheck,
  IconChevronDown,
  IconChevronRight,
  IconClock,
  IconCopy,
  IconExternalLink,
  IconFile,
  IconFileDiff,
  IconFileText,
  IconFolderOpen,
  IconLoader2,
  IconMessage2,
  IconPencil,
  IconPhoto,
  IconRefresh,
  IconRoute,
  IconSearch,
  IconSettings,
  IconTargetArrow,
  IconTerminal2,
} from "@tabler/icons-react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { fileReviewApply, fileReviewCancel, fileReviewUpdate } from "../../api/fileReview";
import {
  revealInExplorer,
  windowOpenRunSummaryDiff,
  type RunSummaryDiffPayload,
} from "../../api/window";
import { useCallback, useEffect, useRef, useState } from "react";
import { useIntl } from "react-intl";
import ReactMarkdown, { type Components } from "react-markdown";
import remarkGfm from "remark-gfm";
import type { ChatMessage, RunSummary, ToolCallItem } from "../../stores/appStore";
import { useAppStore } from "../../stores/appStore";
import { formatDuration } from "../../utils/formatDuration";
import { CodeBlock } from "./CodeBlock";
import { PlanCard } from "./PlanCard";

interface MessageListProps {
  messages: ChatMessage[];
  streamingText: string;
  streamingLabel: string;
  isStreaming: boolean;
  onExecutePlan?: (planContent: string) => void;
}

export function MessageList({ messages, streamingText, streamingLabel, isStreaming, onExecutePlan }: MessageListProps) {
  const intl = useIntl();
  const initialized = useAppStore((state) => state.initialized);
  const initError = useAppStore((state) => state.initError);
  const retryInit = useAppStore((state) => state.retryInit);
  const bottomRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [messages, streamingText]);

  if (messages.length === 0 && !isStreaming) {
    if (!initialized && initError) {
      return (
        <div className="thin-scrollbar flex flex-1 items-center justify-center overflow-y-auto px-6 py-12">
          <div className="w-full max-w-md text-center">
            <div className="mx-auto mb-4 flex h-10 w-10 items-center justify-center rounded-[var(--radius-md)] bg-[var(--danger-soft)] text-[var(--danger)]">
              <IconAlertTriangle size={20} stroke={1.8} />
            </div>
            <h2 className="text-lg font-semibold text-[var(--text-strong)]">
              {intl.formatMessage({ id: "status.initFailed" })}
            </h2>
            <p className="mx-auto mt-2 max-w-sm text-[13px] leading-relaxed text-[var(--text-muted)]">
              {intl.formatMessage({ id: "status.initErrorHint" })}
            </p>
            <p className="mt-3 rounded-[var(--radius-sm)] bg-[var(--surface-raised)] px-3 py-2 text-left font-mono text-xs text-[var(--danger)] break-all">
              {initError}
            </p>
            <div className="mt-6 flex items-center justify-center gap-3">
              {retryInit && (
                <button onClick={retryInit} className="primary-button flex items-center gap-1.5">
                  <IconRefresh size={14} stroke={2} />
                  {intl.formatMessage({ id: "status.retry" })}
                </button>
              )}
              <button
                onClick={() => useAppStore.getState().setShowSettings(true)}
                className="flex items-center gap-1.5 rounded-[var(--radius-md)] border border-[var(--border-strong)] px-3 py-2 text-[13px] text-[var(--text-base)] transition-colors hover:bg-[var(--surface-elevated)]"
              >
                <IconSettings size={14} stroke={1.8} />
                {intl.formatMessage({ id: "status.openSettings" })}
              </button>
            </div>
          </div>
        </div>
      );
    }

    return null;
  }

  return (
    <div className="chat-dialog-surface thin-scrollbar min-h-0 flex-1 overflow-y-auto px-4 pb-7 pt-6 sm:px-8">
      <div className="mx-auto flex w-full max-w-[1180px] flex-col gap-5">
        {messages.map((message, index) => (
          <MessageRow
            key={message.id}
            message={message}
            messageIndex={index}
            sourceMessages={messages}
            onExecutePlan={onExecutePlan}
          />
        ))}

        {isStreaming && streamingText && (
          <article className="chat-answer group relative">
            <div className="chat-status-line mb-5 border-b border-[var(--chat-line)] pb-3">
              <IconLoader2 size={16} stroke={1.8} className="animate-spin text-[var(--accent)]" />
              <span>{streamingLabel || intl.formatMessage({ id: "chat.runSummary.running" })}</span>
              <IconChevronRight size={16} stroke={1.8} className="text-[var(--chat-faint)]" />
            </div>
            <div className="chat-prose max-w-[980px]">
              <MessageContent content={streamingText} />
              <span className="ml-1 inline-block h-3.5 w-1 animate-pulse rounded-sm bg-[var(--accent)] align-middle" />
            </div>
          </article>
        )}

        {isStreaming && !streamingText && (
          <div className="chat-status-line border-b border-[var(--chat-line)] pb-3">
            <span className="flex gap-1">
              <span className="h-1.5 w-1.5 animate-bounce rounded-full bg-[var(--chat-faint)]" style={{ animationDelay: "0ms" }} />
              <span className="h-1.5 w-1.5 animate-bounce rounded-full bg-[var(--chat-faint)]" style={{ animationDelay: "140ms" }} />
              <span className="h-1.5 w-1.5 animate-bounce rounded-full bg-[var(--chat-faint)]" style={{ animationDelay: "280ms" }} />
            </span>
            {streamingLabel || intl.formatMessage({ id: "chat.thinking" })}
            <IconChevronRight size={16} stroke={1.8} className="text-[var(--chat-faint)]" />
          </div>
        )}

        <div ref={bottomRef} />
      </div>
    </div>
  );
}

function MessageRow({
  message,
  messageIndex,
  sourceMessages,
  onExecutePlan,
}: {
  message: ChatMessage;
  messageIndex: number;
  sourceMessages: ChatMessage[];
  onExecutePlan?: (planContent: string) => void;
}) {
  if (message.planFile) {
    return (
      <PlanCard
        planFile={message.planFile}
        onExecute={(content) => onExecutePlan?.(content)}
      />
    );
  }

  if (message.runSummary) {
    return (
      <RunSummaryCard
        summary={message.runSummary}
        messageIndex={messageIndex}
        sourceMessages={sourceMessages}
      />
    );
  }

  if (message.toolCalls && message.toolCalls.length > 0) {
    return <ToolCallsCard calls={message.toolCalls} />;
  }

  if (message.role === "system" && message.commandStatus) {
    return <LegacyToolExecRow message={message} />;
  }

  if (message.role === "system") {
    return (
      <div className="chat-work-card max-w-[980px] border-[rgba(239,68,68,0.22)] bg-[var(--danger-soft)] px-4 py-3 text-[13px] leading-relaxed text-[var(--text-strong)]">
        <MessageContent content={message.content} />
      </div>
    );
  }

  if (message.role === "user") {
    return (
      <div className="flex justify-end py-1">
        <div className="chat-user-message group relative max-w-[min(88%,760px)] px-4 py-3 text-[13px] leading-relaxed">
          <MessageContent content={message.content} />
          <CopyButton text={message.content} />
        </div>
      </div>
    );
  }

  return (
    <article className="chat-answer group relative">
      <div className="chat-prose max-w-[980px] pr-10">
        <MessageContent content={message.content} />
      </div>
      <CopyButton text={message.content} />
    </article>
  );
}

function CopyButton({ text }: { text: string }) {
  const intl = useIntl();
  const [copied, setCopied] = useState(false);
  const handleCopy = useCallback(() => {
    navigator.clipboard.writeText(text).then(() => {
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    });
  }, [text]);

  return (
    <button
      onClick={handleCopy}
      className="chat-copy-button absolute right-0 top-0 flex items-center justify-center h-6 w-6 opacity-0 transition-[opacity,color,background] hover:bg-[var(--chat-chip)] hover:text-[var(--chat-prose)] group-hover:opacity-100"
      title={copied
        ? intl.formatMessage({ id: "chat.copied" })
        : intl.formatMessage({ id: "chat.copy" })}
    >
      {copied ? <IconCheck size={14} stroke={2} /> : <IconCopy size={14} stroke={2} />}
    </button>
  );
}

function RunSummaryCard({
  summary,
  messageIndex,
  sourceMessages,
}: {
  summary: RunSummary;
  messageIndex: number;
  sourceMessages: ChatMessage[];
}) {
  const intl = useIntl();
  const changedFiles = summary.changedFiles ?? [];
  const usage = summary.usage;
  const goalBudgetTokens = summary.goalBudgetTokens;
  const globalCwd = useAppStore((s) => s.workspaceCwd);
  const workspaceCwd = summary.cwd ?? globalCwd;
  const patchDiffEntries = useRef<RunSummaryPatchDiffEntry[]>([]);

  useEffect(() => {
    // 仅提取“当前 RunSummary 所属轮次”的 apply_patch 补丁，
    // 避免将历史轮次的文件差异误展示到当前按钮点击结果。
    patchDiffEntries.current = collectRunSummaryPatchDiffEntries(
      sourceMessages,
      messageIndex,
    );
  }, [messageIndex, sourceMessages]);

  const openDiffForFile = useCallback((file: { path: string; action: string }) => {
    const filePath = file.path;
    const absolutePath = toAbsolutePath(filePath, workspaceCwd);
    const openDiffWindow = (payload: RunSummaryDiffPayload) => {
      void windowOpenRunSummaryDiff(payload).catch((err) => {
        console.error("Open runsummary diff window failed:", err);
      });
    };
    const snapshotMatch = findRunSummarySnapshotEntry(
      filePath,
      summary.changedFileSnapshots,
    );
    const hasBeforeSnapshot = snapshotMatch?.beforeContent !== undefined;
    const hasAfterSnapshot = snapshotMatch?.afterContent !== undefined;
    if (
      snapshotMatch
      && (
        hasBeforeSnapshot
        || hasAfterSnapshot
      )
    ) {
      const normalizedAction = (file.action || snapshotMatch.action || "modified").toLowerCase();
      const canPersist = normalizedAction === "modified" && hasBeforeSnapshot && hasAfterSnapshot;
      openDiffWindow({
        path: absolutePath,
        beforeContent: snapshotMatch.beforeContent ?? "",
        afterContent: snapshotMatch.afterContent ?? "",
        fileAction: normalizedAction,
        diffSource: "snapshot",
        canPersist,
        persistHint: canPersist
          ? undefined
          : normalizedAction !== "modified"
            ? intl.formatMessage({ id: "diff.onlyModifiedSupported" })
            : intl.formatMessage({ id: "patchDiff.snapshotIncomplete" }),
      });
      return;
    }

    // 新增的独立 Diff 图标只做补丁详情预览，不影响原有“打开资源管理器”行为。
    const match = findRunSummaryPatchDiffEntry(filePath, patchDiffEntries.current);
    if (match) {
      openDiffWindow({
        path: absolutePath,
        beforeContent: match.beforeContent,
        afterContent: match.afterContent,
        fileAction: (file.action || "modified").toLowerCase(),
        diffSource: "patch",
        canPersist: false,
        persistHint: intl.formatMessage({ id: "patchDiff.patchViewOnly" }),
      });
      return;
    }

    // 某些文件改动可能来自 write_file / shell，无法映射到 apply_patch 文本；
    // 这里给出明确提示，避免用户误以为按钮失效。
    openDiffWindow({
      path: absolutePath,
      beforeContent: "",
      afterContent: "",
      fileAction: (file.action || "modified").toLowerCase(),
      diffSource: "empty",
      canPersist: false,
      persistHint: intl.formatMessage({ id: "patchDiff.noSnapshotData" }),
      emptyHint: intl.formatMessage({ id: "patchDiff.noDiffData" }),
    });
  }, [intl, summary.changedFileSnapshots, workspaceCwd]);

  return (
    <section className="max-w-[1100px]">
      <div className="chat-status-line border-b border-[var(--chat-line)] pb-3">
        {summary.budgetLimited ? (
          <IconAlertTriangle size={17} stroke={1.9} className="text-[var(--warning)]" />
        ) : (
          <IconCheck size={17} stroke={1.9} className="text-[var(--accent)]" />
        )}
        <span>
          {summary.budgetLimited
            ? intl.formatMessage({ id: "chat.runSummary.budgetLimited" })
            : intl.formatMessage({ id: "chat.runSummary.processed" })}
        </span>
        <span className="font-mono text-[0.95em]">{formatDuration(summary.durationMs)}</span>
        {usage && (
          <span className="font-mono text-[0.95em]">
            {formatTokenCount(usage.callCount ?? 0)} calls / {formatTokenCount(usage.totalTokens)} tokens
          </span>
        )}
        {goalBudgetTokens && (
          <span className="font-mono text-[0.95em]">
            / {formatTokenCount(goalBudgetTokens)}
          </span>
        )}
        <IconChevronRight size={17} stroke={1.8} className="text-[var(--chat-faint)]" />
        {summary.mode === "goal" && (
          <span className="ml-auto hidden items-center gap-1.5 text-[13px] sm:flex">
            <IconTargetArrow size={14} stroke={1.8} />
            {intl.formatMessage({ id: "chat.mode.goal" })}
          </span>
        )}
      </div>

      <div className="mt-5 grid gap-3">
        {changedFiles.length > 0 ? (
          changedFiles.map((file) => (
            <div
              key={`${file.action}:${file.path}`}
              className="chat-work-card flex cursor-pointer items-center gap-4 px-4 py-3 transition-colors hover:border-[var(--accent-border)]"
              onClick={() => void revealInExplorer(toAbsolutePath(file.path, workspaceCwd))}
              title={intl.formatMessage({ id: "chat.runSummary.revealFile" })}
            >
              <div className="flex h-12 w-12 flex-shrink-0 items-center justify-center rounded-[var(--radius-md)] bg-[var(--chat-chip)] text-[var(--chat-muted)]">
                <IconFileDiff size={24} stroke={1.65} />
              </div>
              <div className="min-w-0 flex-1">
                <div className="flex flex-wrap items-baseline gap-x-2 gap-y-1">
                  <span className="text-base font-semibold text-[var(--chat-prose)]">
                    {fileActionLabel(file.action, intl)}
                  </span>
                  <span className="truncate font-semibold text-[var(--chat-prose)]">
                    {basename(file.path)}
                  </span>
                </div>
                <div className="mt-0.5 truncate font-mono text-xs text-[var(--chat-muted)]" title={file.path}>
                  {file.path}
                </div>
              </div>
              <div className="flex items-center gap-1">
                <button
                  type="button"
                  className="flex h-8 w-8 flex-shrink-0 items-center justify-center rounded-[var(--radius-sm)] text-[var(--chat-muted)] transition-colors hover:bg-[var(--chat-chip)] hover:text-[var(--accent)]"
                  onClick={(e) => {
                    e.stopPropagation();
                    openDiffForFile(file);
                  }}
                  title={intl.formatMessage({ id: "patchDiff.viewDiffTitle" })}
                >
                  <IconFileDiff size={15} stroke={1.8} />
                </button>
                <button
                  type="button"
                  className="flex h-8 w-8 flex-shrink-0 items-center justify-center rounded-[var(--radius-sm)] text-[var(--chat-muted)] transition-colors hover:bg-[var(--chat-chip)] hover:text-[var(--accent)]"
                  onClick={(e) => {
                    e.stopPropagation();
                    void revealInExplorer(toAbsolutePath(file.path, workspaceCwd));
                  }}
                  title={intl.formatMessage({ id: "chat.runSummary.revealFile" })}
                >
                  <IconExternalLink size={15} stroke={1.8} />
                </button>
              </div>
            </div>
          ))
        ) : (
          <div className="chat-work-card px-4 py-3 text-[13px] text-[var(--chat-muted)]">
            {intl.formatMessage({ id: "chat.runSummary.noChangedFiles" })}
          </div>
        )}
      </div>

      {/* Save as Workflow button */}
      <div className="mt-3 flex justify-end">
        <button
          type="button"
          className="flex items-center gap-1.5 rounded-[var(--radius-sm)] border border-[var(--chat-line)] px-3 py-1.5 text-[12px] text-[var(--chat-muted)] transition-colors hover:border-[var(--accent-border)] hover:text-[var(--accent)]"
          onClick={() => {
            const currentThreadId = useAppStore.getState().currentThreadId;
            if (currentThreadId) {
              useAppStore.getState().setWorkflowExtractThreadId(currentThreadId);
            }
          }}
          title={intl.formatMessage({ id: "chat.runSummary.saveAsWorkflow" })}
        >
          <IconRoute size={14} stroke={1.8} />
          {intl.formatMessage({ id: "chat.runSummary.saveAsWorkflow" })}
        </button>
      </div>

    </section>
  );
}


function basename(path: string): string {
  return path.split(/[\\/]/).filter(Boolean).pop() ?? path;
}

interface RunSummaryPatchDiffEntry {
  // 一个补丁项可能同时命中旧路径/新路径（例如 rename）。
  paths: string[];
  // 作为 Diff 左侧输入的文本。
  beforeContent: string;
  // 作为 Diff 右侧输入的文本。
  afterContent: string;
}

interface RunSummarySnapshotEntry {
  path: string;
  action: string;
  beforeContent?: string;
  afterContent?: string;
}

function findRunSummarySnapshotEntry(
  filePath: string,
  snapshots?: RunSummarySnapshotEntry[],
): RunSummarySnapshotEntry | null {
  if (!Array.isArray(snapshots) || snapshots.length === 0) {
    return null;
  }
  // 反向查找保持“后写覆盖前写”，与补丁回退策略一致。
  for (let i = snapshots.length - 1; i >= 0; i -= 1) {
    const snapshot = snapshots[i];
    if (patchPathMatches(filePath, snapshot.path)) {
      return snapshot;
    }
  }
  return null;
}

function collectRunSummaryPatchDiffEntries(
  messages: ChatMessage[],
  summaryIndex: number,
): RunSummaryPatchDiffEntry[] {
  // 以“上一条 RunSummary”作为轮次边界，只解析当前轮消息中的补丁。
  let startIndex = 0;
  for (let i = summaryIndex - 1; i >= 0; i -= 1) {
    if (messages[i].runSummary) {
      startIndex = i + 1;
      break;
    }
  }

  const entries: RunSummaryPatchDiffEntry[] = [];
  for (let i = startIndex; i < summaryIndex; i += 1) {
    const message = messages[i];
    for (const call of message.toolCalls ?? []) {
      if (call.name !== "apply_patch") {
        continue;
      }
      const patchBody = patchTextFromToolCall(call);
      if (!patchBody) {
        continue;
      }
      entries.push(...parsePatchDiffEntries(patchBody));
    }
  }
  return entries;
}

function patchTextFromToolCall(call: ToolCallItem): string | null {
  const args = parseToolArgs(call);
  // apply_patch 在不同适配器下可能是 raw body，也可能放在 patch/command 字段。
  const patchCandidate =
    typeof args.patch === "string"
      ? args.patch
      : typeof args.command === "string"
        ? args.command
        : call.arguments;

  if (
    typeof patchCandidate !== "string" ||
    !patchCandidate.trim().startsWith("*** Begin Patch")
  ) {
    return null;
  }
  return patchCandidate;
}

function parsePatchDiffEntries(patch: string): RunSummaryPatchDiffEntry[] {
  // 解析 apply_patch 文本得到每个文件的 before/after 内容块。
  // 说明：这里是“补丁级还原”，用于 Diff 可视化，不做完整文件重建。
  const lines = patch.replace(/\r\n/g, "\n").replace(/\r/g, "\n").split("\n");
  const result: RunSummaryPatchDiffEntry[] = [];
  let idx = 0;

  const isBoundary = (line: string): boolean => {
    return (
      line.startsWith("*** Add File: ") ||
      line.startsWith("*** Update File: ") ||
      line.startsWith("*** Delete File: ") ||
      line.startsWith("*** End Patch")
    );
  };

  while (idx < lines.length) {
    const line = lines[idx];

    if (line.startsWith("*** Add File: ")) {
      const path = line.slice("*** Add File: ".length).trim();
      idx += 1;
      const afterLines: string[] = [];
      while (idx < lines.length && !isBoundary(lines[idx])) {
        const body = lines[idx];
        if (body.startsWith("+")) {
          afterLines.push(body.slice(1));
        }
        idx += 1;
      }
      result.push({
        paths: [path],
        beforeContent: "",
        afterContent: afterLines.join("\n"),
      });
      continue;
    }

    if (line.startsWith("*** Delete File: ")) {
      const path = line.slice("*** Delete File: ".length).trim();
      idx += 1;
      result.push({
        paths: [path],
        // 删除文件在补丁里通常不含完整正文，这里用占位确保弹窗有明确反馈。
        beforeContent: "[deleted file]",
        afterContent: "",
      });
      continue;
    }

    if (line.startsWith("*** Update File: ")) {
      const path = line.slice("*** Update File: ".length).trim();
      let moveTo: string | null = null;
      const beforeLines: string[] = [];
      const afterLines: string[] = [];
      idx += 1;
      while (idx < lines.length && !isBoundary(lines[idx])) {
        const body = lines[idx];
        if (body.startsWith("*** Move to: ")) {
          moveTo = body.slice("*** Move to: ".length).trim();
          idx += 1;
          continue;
        }
        if (body.startsWith("@@")) {
          idx += 1;
          continue;
        }
        if (body.startsWith("-")) {
          beforeLines.push(body.slice(1));
        } else if (body.startsWith("+")) {
          afterLines.push(body.slice(1));
        } else if (body.startsWith(" ")) {
          const context = body.slice(1);
          beforeLines.push(context);
          afterLines.push(context);
        }
        idx += 1;
      }
      result.push({
        paths: moveTo ? [path, moveTo] : [path],
        beforeContent: beforeLines.join("\n"),
        afterContent: afterLines.join("\n"),
      });
      continue;
    }

    idx += 1;
  }

  return result;
}

function normalizePathForDiffMatch(path: string): string {
  // 统一路径格式，兼容 Windows 与 Unix 分隔符差异。
  return path
    .trim()
    .replace(/\\/g, "/")
    .replace(/^\.\/+/, "")
    .toLowerCase();
}

function patchPathMatches(targetPath: string, patchPath: string): boolean {
  const target = normalizePathForDiffMatch(targetPath);
  const candidate = normalizePathForDiffMatch(patchPath);
  if (!target || !candidate) {
    return false;
  }
  return (
    target === candidate ||
    target.endsWith(`/${candidate}`) ||
    candidate.endsWith(`/${target}`)
  );
}

function findRunSummaryPatchDiffEntry(
  filePath: string,
  entries: RunSummaryPatchDiffEntry[],
): RunSummaryPatchDiffEntry | null {
  // 反向查找可保证“同轮多次修改同文件”时优先展示最后一次补丁形态。
  for (let i = entries.length - 1; i >= 0; i -= 1) {
    if (entries[i].paths.some((path) => patchPathMatches(filePath, path))) {
      return entries[i];
    }
  }
  return null;
}

function toAbsolutePath(filePath: string, cwd: string | null): string {
  if (!cwd) return filePath;
  if (/^[a-zA-Z]:[\\/]/.test(filePath) || filePath.startsWith("/") || filePath.startsWith("\\\\")) {
    return filePath;
  }
  const base = cwd.replace(/[\\/]+$/, "");
  return `${base}\\${filePath.replace(/\//g, "\\")}`;
}

function formatTokenCount(value?: number): string {
  const safe = Number.isFinite(value) ? Math.max(0, Math.round(value ?? 0)) : 0;
  return new Intl.NumberFormat(undefined, { maximumFractionDigits: 0 }).format(safe);
}


function fileActionLabel(action: string, intl: ReturnType<typeof useIntl>): string {
  const id = `chat.fileAction.${action}`;
  try {
    return intl.formatMessage({ id });
  } catch {
    return action;
  }
}

function LegacyToolExecRow({ message }: { message: ChatMessage }) {
  const status = message.commandStatus ?? "running";
  const isRunning = status === "running";
  const isSuccess = status === "success";

  return (
    <div className="chat-tool-card flex max-w-[980px] items-center gap-2 px-3 py-2 text-xs text-[var(--chat-muted)]">
      {isRunning ? (
        <IconLoader2 size={12} stroke={2} className="animate-spin text-[var(--warning)]" />
      ) : isSuccess ? (
        <span className="h-1.5 w-1.5 rounded-full bg-[var(--accent)]" />
      ) : (
        <span className="h-1.5 w-1.5 rounded-full bg-[var(--danger)]" />
      )}
      <IconTerminal2 size={12} stroke={1.8} />
      <code className="flex-1 truncate font-mono">{message.content}</code>
      {!isRunning && (
        <span className={isSuccess ? "text-[var(--accent)]" : "text-[var(--danger)]"}>
          {isSuccess ? "done" : "failed"}
        </span>
      )}
    </div>
  );
}

function ToolCallsCard({ calls }: { calls: ToolCallItem[] }) {
  const hasRunning = calls.some((c) => c.status === "running");
  const groups = groupToolCalls(calls);

  return (
    <div className="max-w-[980px] space-y-2">
      {groups.map((group, gi) => (
        <ToolGroup key={gi} group={group} forceExpanded={hasRunning} />
      ))}
    </div>
  );
}

interface ToolGroup {
  type:
    | "shell"
    | "shell_command"
    | "exec_command"
    | "write_stdin"
    | "close_exec_session"
    | "read_file"
    | "write_file"
    | "tool_search"
    | "code_review"
    | "apply_patch"
    | "list_directory"
    | "update_plan"
    | "request_user_input"
    | "request_permissions"
    | "view_image"
    | "image_generate"
    | "memory_list"
    | "memory_read"
    | "memory_search"
    | "memory_write"
    | "memory_update"
    | "memory_forget"
    | "mcp_list_servers"
    | "mcp_status"
    | "mcp_list_tools"
    | "mcp_call_tool"
    | "mcp_list_resources"
    | "mcp_read_resource"
    | "mcp_list_resource_templates"
    | "mcp_list_prompts"
    | "mcp_get_prompt"
    | "apps_list"
    | "list_available_plugins_to_install"
    | "request_plugin_install"
    | "plugin_manage"
    | "browser_run"
    | "spawn_agent"
    | "wait_agent"
    | "send_input"
    | "resume_agent"
    | "list_agents"
    | "close_agent"
    | "web_search"
    | "web_fetch";
  items: ToolCallItem[];
}

function groupToolCalls(calls: ToolCallItem[]): ToolGroup[] {
  const groups: ToolGroup[] = [];
  let current: ToolGroup | null = null;

  for (const call of calls) {
    const t = call.name as ToolGroup["type"];
    if (t === "shell" || t === "shell_command" || t === "exec_command") {
      groups.push({ type: t, items: [call] });
      current = null;
    } else {
      if (current && current.type === t) {
        current.items.push(call);
      } else {
        current = { type: t, items: [call] };
        groups.push(current);
      }
    }
  }
  return groups;
}

function toolGroupIcon(type: string) {
  if (type.startsWith("mcp__")) return <IconTerminal2 size={13} stroke={1.8} />;
  switch (type) {
    case "shell": return <IconTerminal2 size={13} stroke={1.8} />;
    case "shell_command": return <IconTerminal2 size={13} stroke={1.8} />;
    case "exec_command": return <IconTerminal2 size={13} stroke={1.8} />;
    case "write_stdin": return <IconTerminal2 size={13} stroke={1.8} />;
    case "close_exec_session": return <IconTerminal2 size={13} stroke={1.8} />;
    case "read_file": return <IconFileText size={13} stroke={1.8} />;
    case "write_file": return <IconPencil size={13} stroke={1.8} />;
    case "tool_search": return <IconSearch size={13} stroke={1.8} />;
    case "code_review": return <IconFileDiff size={13} stroke={1.8} />;
    case "apply_patch": return <IconFileDiff size={13} stroke={1.8} />;
    case "list_directory": return <IconFolderOpen size={13} stroke={1.8} />;
    case "update_plan": return <IconCheck size={13} stroke={1.8} />;
    case "request_user_input": return <IconMessage2 size={13} stroke={1.8} />;
    case "request_permissions": return <IconAlertTriangle size={13} stroke={1.8} />;
    case "view_image": return <IconPhoto size={13} stroke={1.8} />;
    case "image_generate": return <IconPhoto size={13} stroke={1.8} />;
    case "memory_list": return <IconFolderOpen size={13} stroke={1.8} />;
    case "memory_read": return <IconFileText size={13} stroke={1.8} />;
    case "memory_search": return <IconSearch size={13} stroke={1.8} />;
    case "memory_write": return <IconPencil size={13} stroke={1.8} />;
    case "memory_update": return <IconPencil size={13} stroke={1.8} />;
    case "memory_forget": return <IconFileDiff size={13} stroke={1.8} />;
    case "mcp_list_servers": return <IconTerminal2 size={13} stroke={1.8} />;
    case "mcp_status": return <IconTerminal2 size={13} stroke={1.8} />;
    case "mcp_list_tools": return <IconTerminal2 size={13} stroke={1.8} />;
    case "mcp_call_tool": return <IconTerminal2 size={13} stroke={1.8} />;
    case "mcp_list_resources": return <IconTerminal2 size={13} stroke={1.8} />;
    case "mcp_read_resource": return <IconTerminal2 size={13} stroke={1.8} />;
    case "mcp_list_resource_templates": return <IconTerminal2 size={13} stroke={1.8} />;
    case "mcp_list_prompts": return <IconTerminal2 size={13} stroke={1.8} />;
    case "mcp_get_prompt": return <IconTerminal2 size={13} stroke={1.8} />;
    case "apps_list": return <IconSettings size={13} stroke={1.8} />;
    case "list_available_plugins_to_install": return <IconSettings size={13} stroke={1.8} />;
    case "request_plugin_install": return <IconSettings size={13} stroke={1.8} />;
    case "plugin_manage": return <IconSettings size={13} stroke={1.8} />;
    case "browser_run": return <IconBrowser size={13} stroke={1.8} />;
    case "spawn_agent": return <IconMessage2 size={13} stroke={1.8} />;
    case "wait_agent": return <IconClock size={13} stroke={1.8} />;
    case "send_input": return <IconMessage2 size={13} stroke={1.8} />;
    case "resume_agent": return <IconRefresh size={13} stroke={1.8} />;
    case "list_agents": return <IconMessage2 size={13} stroke={1.8} />;
    case "close_agent": return <IconMessage2 size={13} stroke={1.8} />;
    case "web_search": return <IconSearch size={13} stroke={1.8} />;
    case "web_fetch": return <IconFileText size={13} stroke={1.8} />;
    default: return <IconFile size={13} stroke={1.8} />;
  }
}

function toolGroupSummary(group: ToolGroup): string {
  const n = group.items.length;
  if (group.type.startsWith("mcp__")) {
    return n === 1 ? `Called MCP tool ${group.items[0].displayLabel}` : `Called ${n} MCP tools`;
  }
  switch (group.type) {
    case "shell":
    case "shell_command":
    case "exec_command":
      return group.items[0].displayLabel;
    case "write_stdin":
      return n === 1 ? `Wrote stdin ${group.items[0].displayLabel}` : `Wrote stdin ${n} times`;
    case "close_exec_session":
      return n === 1 ? `Closed exec session ${group.items[0].displayLabel}` : `Closed exec sessions ${n} times`;
    case "read_file":
      return n === 1 ? `Read ${group.items[0].displayLabel}` : `Read ${n} files`;
    case "write_file":
      return n === 1 ? `Edited ${group.items[0].displayLabel}` : `Edited ${n} files`;
    case "tool_search":
      return n === 1 ? `Searched tools ${group.items[0].displayLabel}` : `Searched tools ${n} times`;
    case "code_review":
      return n === 1 ? `Reviewed code ${group.items[0].displayLabel}` : `Reviewed code ${n} times`;
    case "apply_patch":
      return n === 1 ? `Applied patch ${group.items[0].displayLabel}` : `Applied ${n} patches`;
    case "list_directory":
      return n === 1 ? `Listed ${group.items[0].displayLabel}` : `Listed ${n} directories`;
    case "update_plan":
      return n === 1 ? `Updated plan ${group.items[0].displayLabel}` : `Updated plan ${n} times`;
    case "request_user_input":
      return n === 1 ? `Asked user ${group.items[0].displayLabel}` : `Asked user ${n} times`;
    case "request_permissions":
      return n === 1 ? `Requested permissions ${group.items[0].displayLabel}` : `Requested permissions ${n} times`;
    case "view_image":
      return n === 1 ? `Viewed image ${group.items[0].displayLabel}` : `Viewed ${n} images`;
    case "image_generate":
      return n === 1 ? `Generated image ${group.items[0].displayLabel}` : `Generated ${n} images`;
    case "memory_list":
      return n === 1 ? `Listed memories ${group.items[0].displayLabel}` : `Listed ${n} memory paths`;
    case "memory_read":
      return n === 1 ? `Read memory ${group.items[0].displayLabel}` : `Read ${n} memories`;
    case "memory_search":
      return n === 1 ? `Searched memories ${group.items[0].displayLabel}` : `Searched ${n} memory queries`;
    case "memory_write":
      return n === 1 ? `Wrote memory ${group.items[0].displayLabel}` : `Wrote ${n} memories`;
    case "memory_update":
      return n === 1 ? `Updated memory ${group.items[0].displayLabel}` : `Updated ${n} memories`;
    case "memory_forget":
      return n === 1 ? `Forgot memory ${group.items[0].displayLabel}` : `Forgot ${n} memories`;
    case "mcp_list_servers":
      return "Listed MCP servers";
    case "mcp_status":
      return n === 1 ? `Checked MCP status ${group.items[0].displayLabel}` : `Checked MCP status ${n} times`;
    case "mcp_list_tools":
      return n === 1 ? `Listed MCP tools ${group.items[0].displayLabel}` : `Listed MCP tools ${n} times`;
    case "mcp_call_tool":
      return n === 1 ? `Called MCP tool ${group.items[0].displayLabel}` : `Called ${n} MCP tools`;
    case "mcp_list_resources":
      return n === 1 ? `Listed MCP resources ${group.items[0].displayLabel}` : `Listed MCP resources ${n} times`;
    case "mcp_read_resource":
      return n === 1 ? `Read MCP resource ${group.items[0].displayLabel}` : `Read ${n} MCP resources`;
    case "mcp_list_resource_templates":
      return n === 1 ? `Listed MCP resource templates ${group.items[0].displayLabel}` : `Listed MCP resource templates ${n} times`;
    case "mcp_list_prompts":
      return n === 1 ? `Listed MCP prompts ${group.items[0].displayLabel}` : `Listed MCP prompts ${n} times`;
    case "mcp_get_prompt":
      return n === 1 ? `Got MCP prompt ${group.items[0].displayLabel}` : `Got ${n} MCP prompts`;
    case "apps_list":
      return n === 1 ? `Listed apps ${group.items[0].displayLabel}` : `Listed apps ${n} times`;
    case "list_available_plugins_to_install":
      return n === 1 ? `Listed installable plugins ${group.items[0].displayLabel}` : `Listed installable plugins ${n} times`;
    case "request_plugin_install":
      return n === 1 ? `Installed plugin ${group.items[0].displayLabel}` : `Installed plugins ${n} times`;
    case "plugin_manage":
      return n === 1 ? `Managed plugin ${group.items[0].displayLabel}` : `Managed plugins ${n} times`;
    case "browser_run":
      return n === 1 ? `Browsed ${group.items[0].displayLabel}` : `Ran browser ${n} times`;
    case "spawn_agent":
      return n === 1 ? `Spawned agent ${group.items[0].displayLabel}` : `Spawned ${n} agents`;
    case "wait_agent":
      return n === 1 ? `Waited for agent ${group.items[0].displayLabel}` : `Waited for agents ${n} times`;
    case "send_input":
      return n === 1 ? `Messaged agent ${group.items[0].displayLabel}` : `Messaged agents ${n} times`;
    case "resume_agent":
      return n === 1 ? `Resumed agent ${group.items[0].displayLabel}` : `Resumed agents ${n} times`;
    case "list_agents":
      return n === 1 ? `Listed agents ${group.items[0].displayLabel}` : `Listed agents ${n} times`;
    case "close_agent":
      return n === 1 ? `Closed agent ${group.items[0].displayLabel}` : `Closed agents ${n} times`;
    case "web_search":
      return n === 1 ? `Searched ${group.items[0].displayLabel}` : `Searched ${n} queries`;
    case "web_fetch":
      return n === 1 ? `Fetched ${group.items[0].displayLabel}` : `Fetched ${n} pages`;
    default:
      return `${n} tool calls`;
  }
}

function parseToolArgs(item: ToolCallItem): Record<string, unknown> {
  try {
    return JSON.parse(item.arguments);
  } catch {
    if (item.name === "apply_patch" && item.arguments.trim().startsWith("*** Begin Patch")) {
      return { patch: item.arguments };
    }
    return {};
  }
}

function parseBrowserRunOutput(output?: string): {
  ok?: boolean;
  errorCode?: string;
  message?: string;
  hint?: string;
  title?: string;
  finalUrl?: string;
  browserMode?: string;
  durationMs?: number;
  actions: Array<Record<string, unknown>>;
  screenshots: string[];
  assetBundles: string[];
  tabs: Array<Record<string, unknown>>;
} | null {
  if (!output) return null;

  const start = output.indexOf("{");
  const end = output.lastIndexOf("}");
  if (start < 0 || end <= start) return null;

  try {
    const parsed = JSON.parse(output.slice(start, end + 1)) as Record<string, unknown>;
    return {
      ok: typeof parsed.ok === "boolean" ? parsed.ok : undefined,
      errorCode: typeof parsed.errorCode === "string" ? parsed.errorCode : undefined,
      message: typeof parsed.message === "string" ? parsed.message : undefined,
      hint: typeof parsed.hint === "string" ? parsed.hint : undefined,
      title: typeof parsed.title === "string" ? parsed.title : undefined,
      finalUrl: typeof parsed.finalUrl === "string" ? parsed.finalUrl : undefined,
      browserMode: typeof parsed.browserMode === "string" ? parsed.browserMode : undefined,
      durationMs: typeof parsed.durationMs === "number" ? parsed.durationMs : undefined,
      actions: Array.isArray(parsed.actions)
        ? parsed.actions.filter((item): item is Record<string, unknown> => typeof item === "object" && item !== null)
        : [],
      screenshots: Array.isArray(parsed.screenshots)
        ? parsed.screenshots.filter((item): item is string => typeof item === "string" && item.trim().length > 0)
        : [],
      assetBundles: Array.isArray(parsed.assetBundles)
        ? parsed.assetBundles.filter((item): item is string => typeof item === "string" && item.trim().length > 0)
        : [],
      tabs: Array.isArray(parsed.tabs)
        ? parsed.tabs.filter((item): item is Record<string, unknown> => typeof item === "object" && item !== null)
        : [],
    };
  } catch {
    return null;
  }
}

type ImageGeneratePreview = {
  outputPath?: string;
  absolutePath?: string;
  format?: string;
  dimensions?: string;
  size?: string;
  revisedPrompt?: string;
};

function parseImageGenerateOutput(output?: string): (ImageGeneratePreview & {
  images: ImageGeneratePreview[];
}) | null {
  if (!output) return null;

  const valueFor = (label: string) => {
    const match = output.match(new RegExp(`^${label}:\\s*(.+)$`, "im"));
    return match?.[1]?.trim() || undefined;
  };

  const fallback: ImageGeneratePreview = {
    outputPath: valueFor("Output path"),
    absolutePath: valueFor("Absolute path"),
    format: valueFor("Format"),
    dimensions: valueFor("Dimensions"),
    size: valueFor("Size"),
    revisedPrompt: valueFor("Revised prompt"),
  };
  const images = parseImageGenerateImagesJson(output);
  const first = images[0];

  return {
    ...fallback,
    ...first,
    outputPath: first?.outputPath ?? fallback.outputPath,
    absolutePath: first?.absolutePath ?? fallback.absolutePath,
    format: first?.format ?? fallback.format,
    dimensions: first?.dimensions ?? fallback.dimensions,
    size: first?.size ?? fallback.size,
    revisedPrompt: first?.revisedPrompt ?? fallback.revisedPrompt,
    images: images.length > 0
      ? images
      : fallback.outputPath || fallback.absolutePath
        ? [fallback]
        : [],
  };
}

function parseImageGenerateImagesJson(output: string): ImageGeneratePreview[] {
  const marker = "Images JSON:";
  const markerIndex = output.indexOf(marker);
  if (markerIndex < 0) return [];

  try {
    const parsed = JSON.parse(output.slice(markerIndex + marker.length).trim()) as Record<string, unknown>;
    const images = Array.isArray(parsed.images) ? parsed.images : [];
    return images
      .filter((item): item is Record<string, unknown> => typeof item === "object" && item !== null)
      .map((item) => ({
        outputPath: stringField(item, "outputPath"),
        absolutePath: stringField(item, "absolutePath"),
        format: stringField(item, "format"),
        dimensions: stringField(item, "dimensions"),
        size: stringField(item, "size"),
        revisedPrompt: stringField(item, "revisedPrompt"),
      }))
      .filter((item) => item.outputPath || item.absolutePath);
  } catch {
    return [];
  }
}

function stringField(record: Record<string, unknown>, key: string): string | undefined {
  const value = record[key];
  return typeof value === "string" && value.trim() ? value.trim() : undefined;
}

function ToolGroup({
  group,
  forceExpanded,
}: {
  group: ToolGroup;
  forceExpanded: boolean;
}) {
  const hasRunning = group.items.some((c) => c.status === "running");
  const allSuccess = group.items.every((c) => c.status === "success");
  const hasFailed = group.items.some((c) => c.status === "failed");
  const [localExpanded, setLocalExpanded] = useState(false);
  const isExpanded = localExpanded || (forceExpanded && hasRunning);

  const statusColor = hasRunning
    ? "text-[var(--warning)]"
    : hasFailed
      ? "text-[var(--danger)]"
      : "text-[var(--accent)]";

  const bulletColor = hasRunning
    ? "bg-[var(--warning)]"
    : hasFailed
      ? "bg-[var(--danger)]"
      : "bg-[var(--accent)]";

  const isMulti = group.items.length > 1;

  return (
    <div className="chat-tool-card overflow-hidden text-xs">
      <button
        onClick={() => setLocalExpanded(!localExpanded)}
        className="flex w-full items-center gap-2 px-3.5 py-2.5 text-left text-[var(--chat-muted)] transition-colors hover:bg-[var(--chat-chip)] hover:text-[var(--chat-prose)]"
      >
        {hasRunning ? (
          <IconLoader2 size={12} stroke={2} className="animate-spin text-[var(--warning)]" />
        ) : (
          <span className={`h-1.5 w-1.5 flex-shrink-0 rounded-full ${bulletColor}`} />
        )}
        {toolGroupIcon(group.type)}
        <span className="min-w-0 flex-1 truncate font-mono">{toolGroupSummary(group)}</span>
        {!hasRunning && (
          <span className={`flex-shrink-0 ${statusColor}`}>
            {allSuccess ? "done" : hasFailed ? "failed" : ""}
          </span>
        )}
        {isExpanded
          ? <IconChevronDown size={12} stroke={2} className="flex-shrink-0 opacity-50" />
          : <IconChevronRight size={12} stroke={2} className="flex-shrink-0 opacity-50" />
        }
      </button>

      {isExpanded && (
        <div className="space-y-1 border-t border-[var(--chat-line)] px-3.5 py-2">
          {isMulti ? (
            group.items.map((item) => (
              <MultiToolItem key={item.id} item={item} />
            ))
          ) : (
            <ToolDetailView item={group.items[0]} />
          )}
        </div>
      )}
    </div>
  );
}

function MultiToolItem({ item }: { item: ToolCallItem }) {
  const [expanded, setExpanded] = useState(false);
  const hasOutput = !!item.output && item.status !== "running";
  const hasPatchDetails = item.name === "apply_patch" && !!item.patchProgress?.length;
  const hasDetails = hasOutput || hasPatchDetails;

  return (
    <div>
      <button
        onClick={() => hasDetails && setExpanded(!expanded)}
        className={`flex w-full items-center gap-2 rounded-[var(--radius-sm)] py-1 pl-1 pr-2 text-left text-[var(--chat-faint)] ${hasDetails ? "cursor-pointer hover:bg-[var(--chat-chip)] hover:text-[var(--chat-muted)]" : "cursor-default"}`}
      >
        <span className="h-px w-3 bg-[var(--chat-line)]" />
        {item.status === "running" ? (
          <IconLoader2 size={10} stroke={2} className="animate-spin" />
        ) : item.status === "success" ? (
          <span className="h-1 w-1 rounded-full bg-[var(--accent)]" />
        ) : (
          <span className="h-1 w-1 rounded-full bg-[var(--danger)]" />
        )}
        <span className="min-w-0 flex-1 truncate font-mono">{item.displayLabel}</span>
        {hasDetails && (
          expanded
            ? <IconChevronDown size={10} stroke={2} className="flex-shrink-0 opacity-40" />
            : <IconChevronRight size={10} stroke={2} className="flex-shrink-0 opacity-40" />
        )}
      </button>
      {expanded && item.name === "apply_patch" && (
        <div className="ml-5 mt-1">
          <ToolDetailView item={item} />
        </div>
      )}
      {expanded && item.name !== "apply_patch" && item.output && (
        <pre className="chat-tool-output thin-scrollbar ml-5 mt-1 max-h-[150px] overflow-auto whitespace-pre-wrap break-all px-2.5 py-2 font-mono text-[11px] leading-relaxed text-[var(--chat-prose)]">
          {item.output}
        </pre>
      )}
    </div>
  );
}

function ToolDetailView({ item }: { item: ToolCallItem }) {
  const intl = useIntl();
  const args = parseToolArgs(item);
  const workspaceCwd = useAppStore((state) => state.workspaceCwd);
  const cmd = args.command;
  const execCmd = args.cmd as string | undefined;
  const sessionId = args.session_id as number | string | undefined;
  const path = args.path as string | undefined;
  const query = args.query as string | undefined;
  const url = args.url as string | undefined;
  const server = args.server as string | undefined;
  const tool = args.tool as string | undefined;
  const uri = args.uri as string | undefined;
  const baseRef = args.base_ref as string | undefined;
  const reviewPaths = Array.isArray(args.paths) ? args.paths.filter((item): item is string => typeof item === "string") : [];
  const prompt = args.prompt as string | undefined;
  const model = args.model as string | undefined;
  const size = args.size as string | undefined;
  const quality = args.quality as string | undefined;
  const background = args.background as string | undefined;
  const role = args.role as string | undefined;
  const agentId = args.agent_id as string | undefined;
  const sendInputTarget = (args.target ?? args.agent_id ?? args.id) as string | undefined;
  const sendInputMessage = args.message as string | undefined;
  const resumeAgentTarget = (args.id ?? args.target ?? args.agent_id) as string | undefined;
  const closeAgentTarget = (args.target ?? args.agent_id ?? args.id) as string | undefined;
  const agentIds = Array.isArray(args.agent_ids) ? args.agent_ids : [];
  const content = args.content as string | undefined;
  const browserActions = Array.isArray(args.actions) ? args.actions : [];
  const browserResult = item.name === "browser_run"
    ? parseBrowserRunOutput(item.output)
    : null;
  const imageGenerateResult = item.name === "image_generate"
    ? parseImageGenerateOutput(item.output)
    : null;
  const planItems = Array.isArray(args.plan)
    ? args.plan.filter((item): item is { step?: unknown; status?: unknown } => typeof item === "object" && item !== null)
    : [];
  const questionItems = Array.isArray(args.questions)
    ? args.questions.filter((item): item is { id?: unknown; header?: unknown; question?: unknown; options?: unknown } => typeof item === "object" && item !== null)
    : [];
  const patch = typeof args.patch === "string"
    ? args.patch
    : typeof args.command === "string"
      ? args.command
      : undefined;
  const patchProgress = item.patchProgress ?? [];
  const [outputCopied, setOutputCopied] = useState(false);
  const [planCopied, setPlanCopied] = useState(false);
  const planStepLines = planItems
    .map((planItem) => {
      const status = String(planItem.status ?? "pending").trim() || "pending";
      const step = String(planItem.step ?? "").trim();
      return step ? `- [${status}] ${step}` : null;
    })
    .filter((line): line is string => Boolean(line));

  const handleCopyOutput = useCallback(() => {
    if (!item.output) return;
    navigator.clipboard.writeText(item.output).then(() => {
      setOutputCopied(true);
      setTimeout(() => setOutputCopied(false), 2000);
    });
  }, [item.output]);
  const handleCopyPlanSteps = useCallback(() => {
    if (planStepLines.length === 0) return;
    navigator.clipboard.writeText(planStepLines.join("\n")).then(() => {
      setPlanCopied(true);
      setTimeout(() => setPlanCopied(false), 2000);
    });
  }, [planStepLines]);

  const imageSrc = item.name === "view_image" && path
    ? localImagePreviewSrc(path, workspaceCwd)
    : null;
  const generatedImages = item.name === "image_generate"
    ? imageGenerateResult?.images ?? []
    : [];
  const generatedImagePath = item.name === "image_generate"
    ? (generatedImages[0]?.outputPath
      ?? generatedImages[0]?.absolutePath
      ?? (typeof args.output_path === "string" && args.output_path.trim()
        ? args.output_path
        : imageGenerateResult?.outputPath ?? imageGenerateResult?.absolutePath))
    : null;
  const generatedImagePreviews = generatedImages.length > 0
    ? generatedImages
      .map((image, index) => {
        const imagePath = image.outputPath ?? image.absolutePath;
        return imagePath
          ? { image, index, imagePath, src: localImagePreviewSrc(imagePath, workspaceCwd) }
          : null;
      })
      .filter((item): item is { image: ImageGeneratePreview; index: number; imagePath: string; src: string | null } => item !== null)
    : generatedImagePath
      ? [{ image: imageGenerateResult ?? {}, index: 0, imagePath: generatedImagePath, src: localImagePreviewSrc(generatedImagePath, workspaceCwd) }]
      : [];

  return (
    <div className="space-y-2 py-1 text-[var(--chat-faint)]">
      {(item.name === "shell" || item.name === "shell_command") && cmd != null && (
        <pre className="chat-tool-output whitespace-pre-wrap break-all px-2.5 py-2 font-mono text-[var(--chat-prose)]">
          {Array.isArray(cmd) ? (cmd as string[]).join(" ") : String(cmd)}
        </pre>
      )}
      {item.name === "exec_command" && execCmd && (
        <pre className="chat-tool-output whitespace-pre-wrap break-all px-2.5 py-2 font-mono text-[var(--chat-prose)]">
          {execCmd}
        </pre>
      )}
      {item.name === "write_stdin" && (
        <div className="space-y-1.5">
          <div className="flex items-center gap-1.5">
            <IconTerminal2 size={11} stroke={1.8} />
            <span className="font-mono">{sessionId != null ? `session ${sessionId}` : "session"}</span>
          </div>
          {typeof args.chars === "string" && args.chars.length > 0 && (
            <pre className="chat-tool-output max-h-[120px] overflow-auto whitespace-pre-wrap break-all px-2.5 py-2 font-mono text-[var(--chat-prose)]">
              {args.chars.slice(0, 500)}{args.chars.length > 500 ? "..." : ""}
            </pre>
          )}
        </div>
      )}
      {item.name === "close_exec_session" && (
        <div className="flex items-center gap-1.5">
          <IconTerminal2 size={11} stroke={1.8} />
          <span className="font-mono">{sessionId != null ? `session ${sessionId}` : "session"}</span>
        </div>
      )}
      {item.name === "read_file" && path && (
        <div className="flex items-center gap-1.5">
          <IconFileText size={11} stroke={1.8} />
          <span className="font-mono">{path}</span>
        </div>
      )}
      {item.name === "write_file" && (
        <div className="space-y-0.5">
          <div className="flex items-center gap-1.5">
            <IconPencil size={11} stroke={1.8} />
            <span className="font-mono">{path ?? ""}</span>
          </div>
          {content && (
            <pre className="chat-tool-output max-h-[120px] overflow-auto whitespace-pre-wrap break-all px-2.5 py-2 font-mono text-[var(--chat-prose)]">
              {content.slice(0, 500)}{content.length > 500 ? "..." : ""}
            </pre>
          )}
        </div>
      )}
      {item.name === "apply_patch" && (
        <div className="space-y-1.5">
          <div className="flex items-center gap-1.5">
            <IconFileDiff size={11} stroke={1.8} />
            <span className="font-mono">{item.displayLabel}</span>
          </div>
          {patchProgress.length > 0 && (
            <div className="space-y-1">
              {patchProgress.map((change, index) => (
                <div key={`${index}:${change.path}:${change.moveTo ?? ""}`} className="flex items-center gap-2 text-[11px]">
                  <span className="rounded-[var(--radius-sm)] bg-[var(--chat-chip)] px-1.5 py-0.5 font-mono text-[11px] text-[var(--chat-muted)]">
                    {patchActionLabel(change.action)}
                  </span>
                  <span className="min-w-0 flex-1 truncate font-mono text-[var(--chat-prose)]">
                    {change.moveTo ? `${change.path} -> ${change.moveTo}` : change.path}
                  </span>
                </div>
              ))}
            </div>
          )}
          {patch && (
            <pre className="chat-tool-output max-h-[160px] overflow-auto whitespace-pre-wrap break-all px-2.5 py-2 font-mono text-[var(--chat-prose)]">
              {patch.slice(0, 800)}{patch.length > 800 ? "..." : ""}
            </pre>
          )}
          <PatchReviewPanel toolId={item.id} />
        </div>
      )}
      {item.name === "list_directory" && path && (
        <div className="flex items-center gap-1.5">
          <IconFolderOpen size={11} stroke={1.8} />
          <span className="font-mono">{path}</span>
        </div>
      )}
      {item.name === "update_plan" && (
        <div className="space-y-2">
          {typeof args.explanation === "string" && args.explanation.trim() && (
            <p className="text-xs text-[var(--chat-muted)]">{args.explanation.trim()}</p>
          )}
          {planStepLines.length > 0 && (
            <div className="flex justify-end">
              <button
                onClick={handleCopyPlanSteps}
                className="chat-copy-button flex items-center gap-1 rounded-[var(--radius-sm)] px-2 py-1 text-[11px] transition-[color,background] hover:bg-[var(--chat-chip)] hover:text-[var(--chat-prose)]"
                title={intl.formatMessage({ id: planCopied ? "chat.copied" : "chat.plan.copySteps" })}
              >
                {planCopied ? <IconCheck size={12} stroke={2} /> : <IconCopy size={12} stroke={2} />}
                {intl.formatMessage({ id: planCopied ? "chat.copied" : "chat.plan.copySteps" })}
              </button>
            </div>
          )}
          <div className="space-y-1 select-text">
            {planItems.map((planItem, index) => (
              <div key={`${index}:${String(planItem.step ?? "")}`} className="flex items-start gap-2">
                <span className="rounded-[var(--radius-sm)] bg-[var(--chat-chip)] px-1.5 py-0.5 font-mono text-[11px] text-[var(--chat-muted)]">
                  {String(planItem.status ?? "pending")}
                </span>
                <span className="min-w-0 flex-1 whitespace-pre-wrap break-words text-[var(--chat-prose)]">
                  {String(planItem.step ?? "")}
                </span>
              </div>
            ))}
          </div>
        </div>
      )}
      {item.name === "request_user_input" && (
        <div className="space-y-2">
          {questionItems.map((question, index) => {
            const options = Array.isArray(question.options)
              ? question.options.filter((option): option is { label?: unknown; description?: unknown } => typeof option === "object" && option !== null)
              : [];
            return (
              <div key={`${index}:${String(question.id ?? "")}`} className="space-y-1 rounded-[var(--radius-sm)] bg-[var(--chat-chip)] px-2.5 py-2">
                <div className="flex items-center gap-1.5">
                  <IconMessage2 size={11} stroke={1.8} />
                  <span className="font-mono text-[11px] text-[var(--chat-muted)]">
                    {String(question.header ?? question.id ?? `question ${index + 1}`)}
                  </span>
                </div>
                <p className="text-[11px] text-[var(--chat-prose)]">
                  {String(question.question ?? "")}
                </p>
                {options.length > 0 && (
                  <div className="flex flex-wrap gap-1">
                    {options.map((option, optionIndex) => (
                      <span
                        key={`${optionIndex}:${String(option.label ?? "")}`}
                        className="rounded-[var(--radius-sm)] bg-[var(--surface-main)] px-1.5 py-0.5 font-mono text-[11px] text-[var(--chat-muted)]"
                      >
                        {String(option.label ?? "")}
                      </span>
                    ))}
                  </div>
                )}
              </div>
            );
          })}
        </div>
      )}
      {item.name === "request_permissions" && (
        <div className="space-y-2">
          {typeof args.reason === "string" && args.reason.trim() && (
            <p className="text-[11px] text-[var(--chat-prose)]">{args.reason.trim()}</p>
          )}
          <pre className="chat-tool-output max-h-[160px] overflow-auto whitespace-pre-wrap break-all px-2.5 py-2 font-mono text-[var(--chat-prose)]">
            {JSON.stringify(args.permissions ?? {}, null, 2)}
          </pre>
        </div>
      )}
      {item.name === "view_image" && path && (
        <div className="space-y-2">
          <div className="flex items-center gap-1.5">
            <IconPhoto size={11} stroke={1.8} />
            <span className="font-mono break-all">{path}</span>
          </div>
          {imageSrc && (
            <img
              src={imageSrc}
              alt={path}
              className="max-h-[260px] max-w-full rounded-[var(--radius-sm)] border border-[var(--chat-line)] object-contain"
            />
          )}
        </div>
      )}
      {item.name === "image_generate" && (
        <div className="space-y-2">
          <div className="flex items-center gap-1.5">
            <IconPhoto size={11} stroke={1.8} />
            <span className="font-mono break-all">
              {generatedImagePreviews.length > 1
                ? `${generatedImagePreviews.length} images`
                : generatedImagePath ?? item.displayLabel}
            </span>
          </div>
          <div className="flex flex-wrap gap-1.5 text-[11px] text-[var(--chat-muted)]">
            {(model || imageGenerateResult?.format) && (
              <span className="rounded-[var(--radius-sm)] bg-[var(--chat-chip)] px-1.5 py-0.5">
                {model ?? imageGenerateResult?.format}
              </span>
            )}
            {(size || imageGenerateResult?.dimensions) && (
              <span className="rounded-[var(--radius-sm)] bg-[var(--chat-chip)] px-1.5 py-0.5">
                {size ?? imageGenerateResult?.dimensions}
              </span>
            )}
            {quality && (
              <span className="rounded-[var(--radius-sm)] bg-[var(--chat-chip)] px-1.5 py-0.5">
                {quality}
              </span>
            )}
            {background && (
              <span className="rounded-[var(--radius-sm)] bg-[var(--chat-chip)] px-1.5 py-0.5">
                {background}
              </span>
            )}
            {imageGenerateResult?.size && (
              <span className="rounded-[var(--radius-sm)] bg-[var(--chat-chip)] px-1.5 py-0.5">
                {imageGenerateResult.size}
              </span>
            )}
          </div>
          {prompt && (
            <p className="max-h-[4.5rem] overflow-hidden text-[var(--chat-prose)]">
              {prompt}
            </p>
          )}
          {imageGenerateResult?.revisedPrompt && imageGenerateResult.revisedPrompt !== prompt && (
            <p className="max-h-[4.5rem] overflow-hidden text-[var(--chat-muted)]">
              {imageGenerateResult.revisedPrompt}
            </p>
          )}
          {generatedImagePreviews.length > 0 && (
            <div className={generatedImagePreviews.length > 1 ? "grid gap-2 sm:grid-cols-2" : "space-y-2"}>
              {generatedImagePreviews.map(({ image, index, imagePath, src }) => (
                <div key={`${imagePath}:${index}`} className="space-y-1">
                  {generatedImagePreviews.length > 1 && (
                    <div className="flex items-center gap-1.5 text-[11px]">
                      <IconPhoto size={11} stroke={1.8} />
                      <span className="font-mono break-all">{imagePath}</span>
                    </div>
                  )}
                  {src && (
                    <img
                      src={src}
                      alt={image.outputPath ?? image.absolutePath ?? "generated image"}
                      className="max-h-[260px] max-w-full rounded-[var(--radius-sm)] border border-[var(--chat-line)] object-contain"
                    />
                  )}
                </div>
              ))}
            </div>
          )}
        </div>
      )}
      {item.name === "memory_list" && (
        <div className="flex items-center gap-1.5">
          <IconFolderOpen size={11} stroke={1.8} />
          <span className="font-mono">{path ?? "."}</span>
        </div>
      )}
      {item.name === "memory_read" && path && (
        <div className="flex items-center gap-1.5">
          <IconFileText size={11} stroke={1.8} />
          <span className="font-mono">{path}</span>
        </div>
      )}
      {item.name === "memory_search" && query && (
        <div className="flex items-center gap-1.5">
          <IconSearch size={11} stroke={1.8} />
          <span className="font-mono">{query}</span>
        </div>
      )}
      {item.name === "tool_search" && query && (
        <div className="flex items-center gap-1.5">
          <IconSearch size={11} stroke={1.8} />
          <span className="font-mono">{query}</span>
        </div>
      )}
      {item.name === "code_review" && (
        <div className="space-y-1.5">
          <div className="flex items-center gap-1.5">
            <IconFileDiff size={11} stroke={1.8} />
            <span className="font-mono">{baseRef ? `vs ${baseRef}` : "working tree"}</span>
          </div>
          <div className="flex flex-wrap gap-1.5 text-[11px] text-[var(--chat-muted)]">
            {reviewPaths.length > 0 && (
              <span className="rounded-[var(--radius-sm)] bg-[var(--chat-chip)] px-1.5 py-0.5">
                {reviewPaths.length} paths
              </span>
            )}
            {typeof args.max_diff_bytes === "number" && (
              <span className="rounded-[var(--radius-sm)] bg-[var(--chat-chip)] px-1.5 py-0.5">
                {args.max_diff_bytes} bytes
              </span>
            )}
            {args.include_untracked === false && (
              <span className="rounded-[var(--radius-sm)] bg-[var(--chat-chip)] px-1.5 py-0.5">
                tracked only
              </span>
            )}
          </div>
          {reviewPaths.length > 0 && (
            <div className="space-y-1">
              {reviewPaths.slice(0, 6).map((reviewPath) => (
                <div key={reviewPath} className="flex items-center gap-1.5">
                  <IconFileText size={10} stroke={1.8} />
                  <span className="font-mono break-all text-[var(--chat-prose)]">{reviewPath}</span>
                </div>
              ))}
              {reviewPaths.length > 6 && (
                <span className="text-[11px] text-[var(--chat-muted)]">
                  +{reviewPaths.length - 6} more
                </span>
              )}
            </div>
          )}
        </div>
      )}
      {item.name === "memory_write" && (
        <div className="space-y-0.5">
          <div className="flex items-center gap-1.5">
            <IconPencil size={11} stroke={1.8} />
            <span className="font-mono">{path ?? ""}</span>
          </div>
          {content && (
            <pre className="chat-tool-output max-h-[120px] overflow-auto whitespace-pre-wrap break-all px-2.5 py-2 font-mono text-[var(--chat-prose)]">
              {content.slice(0, 500)}{content.length > 500 ? "..." : ""}
            </pre>
          )}
        </div>
      )}
      {item.name === "memory_update" && (
        <div className="space-y-1.5">
          <div className="flex items-center gap-1.5">
            <IconPencil size={11} stroke={1.8} />
            <span className="font-mono">{path ?? ""}</span>
          </div>
          {typeof args.old_text === "string" && (
            <pre className="chat-tool-output max-h-[80px] overflow-auto whitespace-pre-wrap break-all px-2.5 py-2 font-mono text-[var(--chat-prose)]">
              {args.old_text.slice(0, 300)}{args.old_text.length > 300 ? "..." : ""}
            </pre>
          )}
        </div>
      )}
      {item.name === "memory_forget" && (
        <div className="space-y-1.5">
          <div className="flex items-center gap-1.5">
            <IconFileDiff size={11} stroke={1.8} />
            <span className="font-mono">{path ?? ""}</span>
          </div>
          <div className="flex flex-wrap gap-1.5 text-[11px] text-[var(--chat-muted)]">
            {typeof args.match_text === "string" && args.match_text.trim() && (
              <span className="rounded-[var(--radius-sm)] bg-[var(--chat-chip)] px-1.5 py-0.5">
                matching lines
              </span>
            )}
            {args.recursive === true && (
              <span className="rounded-[var(--radius-sm)] bg-[var(--chat-chip)] px-1.5 py-0.5">
                recursive
              </span>
            )}
          </div>
        </div>
      )}
      {item.name === "mcp_list_servers" && (
        <div className="flex items-center gap-1.5">
          <IconTerminal2 size={11} stroke={1.8} />
          <span className="font-mono">codey/config.toml</span>
        </div>
      )}
      {item.name === "mcp_status" && (
        <div className="space-y-1.5">
          <div className="flex items-center gap-1.5">
            <IconTerminal2 size={11} stroke={1.8} />
            <span className="font-mono">{server ?? "all"}</span>
          </div>
          <div className="flex flex-wrap gap-1.5 text-[11px] text-[var(--chat-muted)]">
            <span className="rounded-[var(--radius-sm)] bg-[var(--chat-chip)] px-1.5 py-0.5">
              {args.probe === false ? "configured only" : "probe"}
            </span>
          </div>
        </div>
      )}
      {(item.name === "mcp_list_tools" ||
        item.name === "mcp_list_resources" ||
        item.name === "mcp_list_resource_templates" ||
        item.name === "mcp_list_prompts") && (
        <div className="flex items-center gap-1.5">
          <IconTerminal2 size={11} stroke={1.8} />
          <span className="font-mono">{server ?? "all"}</span>
        </div>
      )}
      {item.name === "mcp_call_tool" && (
        <div className="flex items-center gap-1.5">
          <IconTerminal2 size={11} stroke={1.8} />
          <span className="font-mono">{server ?? ""}{tool ? `:${tool}` : ""}</span>
        </div>
      )}
      {item.name.startsWith("mcp__") && (
        <div className="flex items-center gap-1.5">
          <IconTerminal2 size={11} stroke={1.8} />
          <span className="font-mono break-all">{item.name}</span>
        </div>
      )}
      {item.name === "mcp_read_resource" && (
        <div className="flex items-center gap-1.5">
          <IconFileText size={11} stroke={1.8} />
          <span className="font-mono break-all">{server ?? ""}{uri ? `:${uri}` : ""}</span>
        </div>
      )}
      {item.name === "mcp_get_prompt" && (
        <div className="flex items-center gap-1.5">
          <IconTerminal2 size={11} stroke={1.8} />
          <span className="font-mono break-all">{server ?? ""}{prompt ? `:${prompt}` : ""}</span>
        </div>
      )}
      {item.name === "web_search" && query && (
        <div className="flex items-center gap-1.5">
          <IconSearch size={11} stroke={1.8} />
          <span className="font-mono">{query}</span>
        </div>
      )}
      {item.name === "web_fetch" && url && (
        <div className="flex items-center gap-1.5">
          <IconFileText size={11} stroke={1.8} />
          <span className="font-mono break-all">{url}</span>
        </div>
      )}
      {item.name === "spawn_agent" && (
        <div className="space-y-1.5">
          <div className="flex items-center gap-1.5">
            <IconMessage2 size={11} stroke={1.8} />
            <span className="font-mono">{role ?? "agent"}</span>
          </div>
          {prompt && (
            <p className="max-h-[4.5rem] overflow-hidden text-[var(--chat-prose)]">
              {prompt}
            </p>
          )}
        </div>
      )}
      {item.name === "wait_agent" && (
        <div className="flex items-center gap-1.5">
          <IconClock size={11} stroke={1.8} />
          <span className="font-mono">
            {agentId ?? (agentIds.length ? `${agentIds.length} agents` : "agents")}
          </span>
        </div>
      )}
      {item.name === "send_input" && (
        <div className="space-y-1.5">
          <div className="flex items-center gap-1.5">
            <IconMessage2 size={11} stroke={1.8} />
            <span className="font-mono">{sendInputTarget ?? "agent"}</span>
          </div>
          {sendInputMessage && (
            <p className="max-h-[4.5rem] overflow-hidden text-[var(--chat-prose)]">
              {sendInputMessage}
            </p>
          )}
        </div>
      )}
      {item.name === "resume_agent" && (
        <div className="flex items-center gap-1.5">
          <IconRefresh size={11} stroke={1.8} />
          <span className="font-mono">{resumeAgentTarget ?? "agent"}</span>
        </div>
      )}
      {item.name === "list_agents" && (
        <div className="flex items-center gap-1.5">
          <IconMessage2 size={11} stroke={1.8} />
          <span className="font-mono">{String(args.status ?? "agents")}</span>
        </div>
      )}
      {item.name === "close_agent" && (
        <div className="flex items-center gap-1.5">
          <IconMessage2 size={11} stroke={1.8} />
          <span className="font-mono">{closeAgentTarget ?? "agent"}</span>
        </div>
      )}
      {item.name === "browser_run" && (
        <div className="space-y-2">
          <div className="flex items-center gap-1.5">
            <IconBrowser size={11} stroke={1.8} />
            <span className="font-mono break-all">{url ?? browserResult?.finalUrl ?? "browser"}</span>
          </div>
          <div className="flex flex-wrap gap-1.5 text-[11px] text-[var(--chat-muted)]">
            {browserActions.length > 0 && (
              <span className="rounded-[var(--radius-sm)] bg-[var(--chat-chip)] px-1.5 py-0.5">
                {browserActions.length} actions
              </span>
            )}
            {browserResult?.durationMs != null && (
              <span className="rounded-[var(--radius-sm)] bg-[var(--chat-chip)] px-1.5 py-0.5">
                {browserResult.durationMs} ms
              </span>
            )}
            {browserResult?.browserMode && (
              <span className="rounded-[var(--radius-sm)] bg-[var(--chat-chip)] px-1.5 py-0.5">
                {browserResult.browserMode}
              </span>
            )}
            {browserResult?.title && (
              <span className="max-w-full truncate rounded-[var(--radius-sm)] bg-[var(--chat-chip)] px-1.5 py-0.5">
                {browserResult.title}
              </span>
            )}
            {browserResult?.tabs.length ? (
              <span className="rounded-[var(--radius-sm)] bg-[var(--chat-chip)] px-1.5 py-0.5">
                {browserResult.tabs.length} tabs
              </span>
            ) : null}
            {browserResult?.assetBundles.length ? (
              <span className="rounded-[var(--radius-sm)] bg-[var(--chat-chip)] px-1.5 py-0.5">
                {browserResult.assetBundles.length} asset bundles
              </span>
            ) : null}
          </div>
          {item.status === "failed" && !browserResult?.screenshots.length ? (
            <div className="rounded-[var(--radius-sm)] border border-[var(--danger-soft)] bg-[var(--danger-soft)] px-2 py-1.5">
              <p className="text-[11px] font-medium text-[var(--danger)]">
                {browserResult?.message ?? "Browser run 失败，未生成截图。"}
              </p>
              {browserResult?.hint ? (
                <p className="mt-1 text-[11px] text-[var(--chat-muted)]">{browserResult.hint}</p>
              ) : null}
              {browserResult?.errorCode ? (
                <p className="mt-1 font-mono text-[10px] text-[var(--chat-faint)]">
                  {browserResult.errorCode}
                </p>
              ) : null}
            </div>
          ) : null}
          {browserResult?.actions.length ? (
            <div className="space-y-1">
              {browserResult.actions.map((action, index) => (
                <div key={`${index}:${String(action.type ?? "")}`} className="flex items-center gap-2 text-[11px]">
                  <span className="rounded-[var(--radius-sm)] bg-[var(--chat-chip)] px-1.5 py-0.5 font-mono text-[11px] text-[var(--chat-muted)]">
                    {String(action.type ?? `#${index + 1}`)}
                  </span>
                  <span className="min-w-0 flex-1 truncate font-mono text-[var(--chat-prose)]">
                    {String(action.selector ?? action.url ?? action.path ?? action.key ?? action.title ?? action.text ?? action.tabIndex ?? action.activeIndex ?? "")}
                  </span>
                </div>
              ))}
            </div>
          ) : null}
          {browserResult?.screenshots.length ? (
            <div className="space-y-2">
              {browserResult.screenshots.map((shot) => {
                const src = localImagePreviewSrc(shot, workspaceCwd);
                return (
                  <div key={shot} className="space-y-1">
                    <div className="flex items-center gap-1.5">
                      <IconPhoto size={11} stroke={1.8} />
                      <span className="font-mono break-all">{shot}</span>
                    </div>
                    {src && (
                      <img
                        src={src}
                        alt={shot}
                        className="max-h-[260px] max-w-full rounded-[var(--radius-sm)] border border-[var(--chat-line)] object-contain"
                      />
                    )}
                  </div>
                );
              })}
            </div>
          ) : null}
          {browserResult?.assetBundles.length ? (
            <div className="space-y-1">
              {browserResult.assetBundles.map((manifestPath) => (
                <div key={manifestPath} className="flex items-center gap-1.5 text-[11px]">
                  <IconFileText size={11} stroke={1.8} />
                  <span className="font-mono break-all">{manifestPath}</span>
                </div>
              ))}
            </div>
          ) : null}
        </div>
      )}

      {item.output && item.status !== "running" && (
        <div className="group/output relative mt-1">
          <div className="mb-1 flex items-center justify-between text-[11px] text-[var(--chat-faint)]">
            <span>output</span>
            <button
              onClick={handleCopyOutput}
              className="chat-copy-button flex items-center gap-1 px-2 py-1 text-[11px] opacity-0 transition-[opacity,color,background] hover:bg-[var(--chat-chip)] hover:text-[var(--chat-prose)] group-hover/output:opacity-100"
              title={intl.formatMessage({
                id: outputCopied ? "chat.copied" : "chat.copy",
              })}
            >
              {outputCopied ? <IconCheck size={12} stroke={2} /> : <IconCopy size={12} stroke={2} />}
              {outputCopied
                ? intl.formatMessage({ id: "chat.copied" })
                : intl.formatMessage({ id: "chat.copy" })}
            </button>
          </div>
          <pre className="chat-tool-output thin-scrollbar max-h-[200px] overflow-auto whitespace-pre-wrap break-all px-2.5 py-2 font-mono text-[11px] leading-relaxed text-[var(--chat-prose)]">
            {item.output}
          </pre>
        </div>
      )}
    </div>
  );
}

function PatchReviewPanel({ toolId }: { toolId: string }) {
  const intl = useIntl();
  const review = useAppStore((state) => state.pendingFileReviews[toolId]);
  const setSelectedPath = useAppStore((state) => state.setPendingFileReviewSelectedPath);
  const setFileKeep = useAppStore((state) => state.setPendingFileReviewFileKeep);
  const setKeepAll = useAppStore((state) => state.setPendingFileReviewKeepAll);
  const setEditedContent = useAppStore((state) => state.setPendingFileReviewEditedContent);
  const setReviewStatus = useAppStore((state) => state.setPendingFileReviewStatus);
  const removeReview = useAppStore((state) => state.removePendingFileReview);
  const [requestError, setRequestError] = useState<string | null>(null);

  useEffect(() => {
    setRequestError(review?.error ?? null);
  }, [review?.error]);

  if (!review) {
    return null;
  }

  const selectedPath = review.selectedPath ?? review.files[0]?.path ?? null;
  const selectedFile = selectedPath
    ? review.files.find((file) => file.path === selectedPath) ?? review.files[0]
    : review.files[0];
  const keepCount = review.files.filter((file) => file.keep).length;
  const applying = review.status === "applying";

  const handleApply = useCallback(async () => {
    if (!review) {
      return;
    }
    if (review.files.filter((file) => file.keep).length === 0) {
      setRequestError(intl.formatMessage({ id: "patchDiff.selectAtLeastOne" }));
      return;
    }
    setRequestError(null);
    setReviewStatus(toolId, "applying");
    try {
      // 先把本地编辑过的内容回写到后端审阅缓存，再执行最终应用。
      // 这样可以保证 apply 阶段只负责“写盘决策”，不会丢失前端编辑结果。
      for (const file of review.files) {
        if (file.action === "deleted") {
          continue;
        }
        const candidate = file.candidateContent ?? "";
        const edited = file.editedContent ?? candidate;
        if (edited !== candidate) {
          await fileReviewUpdate(
            review.threadId,
            review.callId,
            file.path,
            undefined,
            edited,
          );
        }
      }
      await fileReviewApply(
        review.threadId,
        review.callId,
        review.keepAll,
        review.files.filter((file) => file.keep).map((file) => file.path),
      );
      removeReview(toolId);
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      setReviewStatus(toolId, "failed", message);
      setRequestError(message);
    }
  }, [intl, review, removeReview, setReviewStatus, toolId]);

  const handleCancel = useCallback(async () => {
    if (!review || applying) {
      return;
    }
    setRequestError(null);
    try {
      await fileReviewCancel(review.threadId, review.callId);
      removeReview(toolId);
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      setReviewStatus(toolId, "failed", message);
      setRequestError(message);
    }
  }, [applying, review, removeReview, setReviewStatus, toolId]);

  const selectedAfterContent = selectedFile?.editedContent ?? selectedFile?.candidateContent ?? "";
  const selectedBeforeContent = selectedFile?.baseContent ?? "";

  return (
    <div className="patch-review-panel mt-2">
      <div className="flex items-center justify-between gap-2">
        <div className="text-[11px] text-[var(--chat-muted)]">
          {intl.formatMessage(
            { id: "patchDiff.reviewPending" },
            { keepCount, total: review.files.length },
          )}
        </div>
        <label className="flex items-center gap-1.5 text-[11px] text-[var(--chat-muted)]">
          <input
            type="checkbox"
            checked={review.keepAll}
            disabled={applying || review.files.length === 0}
            onChange={(e) => setKeepAll(toolId, e.target.checked)}
          />
          {intl.formatMessage({ id: "patchDiff.keepAll" })}
        </label>
      </div>

      <div className="mt-2 grid gap-2 md:grid-cols-[220px_minmax(0,1fr)]">
        <div className="patch-review-file-list thin-scrollbar max-h-[240px] overflow-auto rounded-[var(--radius-sm)] border border-[var(--chat-line)] bg-[var(--chat-paper)] p-1.5">
          {review.files.map((file) => (
            <button
              key={`${file.path}:${file.moveTo ?? ""}`}
              type="button"
              className={`patch-review-file-item w-full text-left ${
                selectedPath === file.path ? "is-selected" : ""
              }`}
              onClick={() => setSelectedPath(toolId, file.path)}
            >
              <div className="flex items-center gap-1.5">
                <input
                  type="checkbox"
                  checked={file.keep}
                  disabled={applying}
                  onChange={(e) => {
                    e.stopPropagation();
                    setFileKeep(toolId, file.path, e.target.checked);
                  }}
                  onClick={(e) => e.stopPropagation()}
                />
                <span className="rounded-[var(--radius-sm)] bg-[var(--chat-chip)] px-1.5 py-0.5 font-mono text-[10px] text-[var(--chat-muted)]">
                  {patchActionLabel(file.action)}
                </span>
              </div>
              <div className="mt-1 truncate font-mono text-[11px] text-[var(--chat-prose)]" title={file.path}>
                {file.moveTo ? `${file.path} -> ${file.moveTo}` : file.path}
              </div>
            </button>
          ))}
        </div>

        <div className="space-y-2">
          {selectedFile ? (
            <div className="grid gap-2 md:grid-cols-2">
              <div>
                <div className="mb-1 text-[11px] text-[var(--chat-muted)]">
                  {intl.formatMessage({ id: "patchDiff.before" })}
                </div>
                <pre className="chat-tool-output thin-scrollbar max-h-[220px] overflow-auto whitespace-pre-wrap break-all px-2.5 py-2 font-mono text-[11px] leading-relaxed text-[var(--chat-prose)]">
                  {selectedFile.action === "created"
                    ? intl.formatMessage({ id: "patchDiff.newFile" })
                    : selectedBeforeContent || intl.formatMessage({ id: "patchDiff.empty" })}
                </pre>
              </div>
              <div>
                <div className="mb-1 text-[11px] text-[var(--chat-muted)]">
                  {intl.formatMessage({ id: "patchDiff.after" })}
                </div>
                {selectedFile.action === "deleted" ? (
                  <pre className="chat-tool-output thin-scrollbar max-h-[220px] overflow-auto whitespace-pre-wrap break-all px-2.5 py-2 font-mono text-[11px] leading-relaxed text-[var(--chat-prose)]">
                    {intl.formatMessage({ id: "patchDiff.willBeDeleted" })}
                  </pre>
                ) : (
                  <textarea
                    className="patch-review-editor thin-scrollbar h-[220px] w-full rounded-[var(--radius-sm)] border border-[var(--chat-line)] bg-[var(--chat-paper)] px-2.5 py-2 font-mono text-[11px] leading-relaxed text-[var(--chat-prose)]"
                    value={selectedAfterContent}
                    disabled={applying}
                    onChange={(e) =>
                      setEditedContent(toolId, selectedFile.path, e.target.value)
                    }
                  />
                )}
              </div>
            </div>
          ) : (
            <div className="chat-tool-output px-2.5 py-2 text-[11px] text-[var(--chat-muted)]">
              {intl.formatMessage({ id: "patchDiff.noReviewFiles" })}
            </div>
          )}
        </div>
      </div>

      <div className="mt-2 flex items-center justify-end gap-2">
        <button
          type="button"
          className="secondary-button rounded-[var(--radius-sm)] px-2.5 py-1.5 text-[11px]"
          disabled={applying}
          onClick={() => void handleCancel()}
        >
          {intl.formatMessage({ id: "patchDiff.cancel" })}
        </button>
        <button
          type="button"
          className="primary-button rounded-[var(--radius-sm)] px-2.5 py-1.5 text-[11px]"
          disabled={applying || keepCount === 0}
          onClick={() => void handleApply()}
        >
          {intl.formatMessage({ id: applying ? "patchDiff.applying" : "patchDiff.apply" })}
        </button>
      </div>

      {requestError && (
        <p className="mt-2 rounded-[var(--radius-sm)] bg-[var(--danger-soft)] px-2.5 py-1.5 text-[11px] text-[var(--danger)]">
          {requestError}
        </p>
      )}
    </div>
  );
}

function patchActionLabel(action: string): string {
  switch (action) {
    case "created":
      return "add";
    case "deleted":
      return "delete";
    case "renamed":
      return "move";
    case "modified":
      return "edit";
    default:
      return action;
  }
}

function localImagePreviewSrc(path: string, workspaceCwd: string | null): string | null {
  // 兼容 Windows 扩展路径前缀，防止 convertFileSrc 生成损坏的 asset URL。
  const trimmed = normalizeLocalImagePath(path);
  if (!trimmed) {
    return null;
  }

  const absolute = /^[A-Za-z]:[\\/]/.test(trimmed) || trimmed.startsWith("\\\\") || trimmed.startsWith("/");
  const resolved = absolute
    ? trimmed
    : workspaceCwd
      ? `${workspaceCwd.replace(/[\\/]+$/, "")}\\${trimmed.replace(/^[\\/]+/, "")}`
      : null;

  return resolved ? convertFileSrc(resolved) : null;
}

function normalizeLocalImagePath(raw: string): string {
  const trimmed = raw.trim();
  if (!trimmed) return "";
  if (trimmed.startsWith("\\\\?\\UNC\\")) {
    return `\\\\${trimmed.slice("\\\\?\\UNC\\".length)}`;
  }
  if (trimmed.startsWith("\\\\?\\")) {
    return trimmed.slice("\\\\?\\".length);
  }
  return trimmed;
}

const markdownComponents: Components = {
  h1: ({ children }) => (
    <h1 className="mt-6 mb-2 text-lg font-semibold text-[var(--chat-prose)]">{children}</h1>
  ),
  h2: ({ children }) => (
    <h2 className="mt-5 mb-2 text-[16px] font-semibold text-[var(--chat-prose)]">{children}</h2>
  ),
  h3: ({ children }) => (
    <h3 className="mt-4 mb-1.5 text-[15px] font-semibold text-[var(--chat-prose)]">{children}</h3>
  ),
  p: ({ children }) => <p className="whitespace-pre-wrap break-words">{children}</p>,
  ul: ({ children }) => (
    <ul className="my-3 ml-6 list-disc space-y-2 text-[var(--chat-prose)]">{children}</ul>
  ),
  ol: ({ children }) => (
    <ol className="my-3 ml-6 list-decimal space-y-2 text-[var(--chat-prose)]">{children}</ol>
  ),
  li: ({ children }) => <li className="leading-relaxed">{children}</li>,
  a: ({ href, children }) => (
    <a
      href={href}
      target="_blank"
      rel="noreferrer"
      className="text-[var(--accent)] underline decoration-[0.08em] underline-offset-2 hover:opacity-80"
    >
      {children}
    </a>
  ),
  blockquote: ({ children }) => (
    <blockquote className="my-3 border-l-2 border-[var(--chat-line)] pl-3 text-[var(--chat-muted)]">
      {children}
    </blockquote>
  ),
  pre: ({ children }) => <>{children}</>,
  code: ({ className, children }) => {
    const code = String(children).replace(/\n$/, "");
    const language = /language-([\w-]+)/.exec(className ?? "")?.[1] ?? "";
    const isBlockCode = Boolean(language) || code.includes("\n");
    if (isBlockCode) {
      return <CodeBlock code={code} language={language} />;
    }
    return <code className="chat-inline-code">{code}</code>;
  },
  table: ({ children }) => (
    <div className="thin-scrollbar my-3 overflow-x-auto">
      <table className="chat-md-table">{children}</table>
    </div>
  ),
  thead: ({ children }) => <thead className="bg-[var(--chat-card-solid)]">{children}</thead>,
  tbody: ({ children }) => <tbody>{children}</tbody>,
  tr: ({ children }) => <tr className="border-b border-[var(--chat-line)] last:border-b-0">{children}</tr>,
  th: ({ children }) => (
    <th className="border-r border-[var(--chat-line)] px-3 py-2 text-left font-semibold last:border-r-0">
      {children}
    </th>
  ),
  td: ({ children }) => (
    <td className="border-r border-[var(--chat-line)] px-3 py-2 align-top last:border-r-0">{children}</td>
  ),
};

function MessageContent({ content }: { content: string }) {
  if (!content) return null;

  return (
    <ReactMarkdown remarkPlugins={[remarkGfm]} components={markdownComponents}>
      {content}
    </ReactMarkdown>
  );
}
