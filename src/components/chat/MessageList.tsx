import {
  IconAlertTriangle,
  IconCheck,
  IconChevronDown,
  IconChevronRight,
  IconCopy,
  IconFile,
  IconFileText,
  IconFolderOpen,
  IconLoader2,
  IconPencil,
  IconRefresh,
  IconSettings,
  IconTerminal2,
} from "@tabler/icons-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { useIntl } from "react-intl";
import type { ChatMessage, ToolCallItem } from "../../stores/appStore";
import { useAppStore } from "../../stores/appStore";
import { CodeBlock } from "./CodeBlock";

interface MessageListProps {
  messages: ChatMessage[];
  streamingText: string;
  isStreaming: boolean;
}

export function MessageList({ messages, streamingText, isStreaming }: MessageListProps) {
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
            <p className="mx-auto mt-2 max-w-sm text-sm leading-relaxed text-[var(--text-muted)]">
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
                className="flex items-center gap-1.5 rounded-[var(--radius-md)] border border-[var(--border-strong)] px-3 py-2 text-sm text-[var(--text-base)] transition-colors hover:bg-[var(--surface-elevated)]"
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
    <div className="thin-scrollbar min-h-0 flex-1 overflow-y-auto px-5 py-4">
      <div className="mx-auto max-w-3xl space-y-3">
        {messages.map((message) => (
          <MessageRow key={message.id} message={message} />
        ))}

        {isStreaming && streamingText && (
          <div className="text-sm leading-relaxed text-[var(--text-base)]">
            <MessageContent content={streamingText} />
            <span className="ml-1 inline-block h-3.5 w-1 animate-pulse rounded-sm bg-[var(--accent)] align-middle" />
          </div>
        )}

        {isStreaming && !streamingText && (
          <div className="flex items-center gap-2 text-sm text-[var(--text-muted)]">
            <span className="flex gap-1">
              <span className="h-1.5 w-1.5 animate-bounce rounded-full bg-[var(--text-faint)]" style={{ animationDelay: "0ms" }} />
              <span className="h-1.5 w-1.5 animate-bounce rounded-full bg-[var(--text-faint)]" style={{ animationDelay: "140ms" }} />
              <span className="h-1.5 w-1.5 animate-bounce rounded-full bg-[var(--text-faint)]" style={{ animationDelay: "280ms" }} />
            </span>
            {intl.formatMessage({ id: "chat.thinking" })}
          </div>
        )}

        <div ref={bottomRef} />
      </div>
    </div>
  );
}

function MessageRow({ message }: { message: ChatMessage }) {
  if (message.toolCalls && message.toolCalls.length > 0) {
    return <ToolCallsCard calls={message.toolCalls} />;
  }

  if (message.role === "system" && message.commandStatus) {
    return <LegacyToolExecRow message={message} />;
  }

  if (message.role === "system") {
    return (
      <div className="rounded-[var(--radius-md)] border border-[rgba(239,68,68,0.2)] bg-[var(--danger-soft)] px-3 py-2 text-sm text-[var(--text-strong)]">
        <MessageContent content={message.content} />
      </div>
    );
  }

  if (message.role === "user") {
    return (
      <div className="group relative rounded-[var(--radius-md)] bg-[var(--surface-elevated)] px-4 py-2.5 text-sm leading-relaxed text-[var(--text-strong)]">
        <MessageContent content={message.content} />
        <CopyButton text={message.content} />
      </div>
    );
  }

  return (
    <div className="group relative text-sm leading-relaxed text-[var(--text-base)]">
      <MessageContent content={message.content} />
      <CopyButton text={message.content} />
    </div>
  );
}

function CopyButton({ text }: { text: string }) {
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
      className="absolute right-2 top-2 flex items-center gap-1 rounded-[var(--radius-sm)] bg-[var(--surface-raised)] px-1.5 py-1 text-[11px] text-[var(--text-faint)] opacity-0 transition-opacity hover:text-[var(--text-muted)] group-hover:opacity-100"
    >
      {copied ? <IconCheck size={12} stroke={2} /> : <IconCopy size={12} stroke={2} />}
      {copied ? "Copied" : "Copy"}
    </button>
  );
}

function LegacyToolExecRow({ message }: { message: ChatMessage }) {
  const status = message.commandStatus ?? "running";
  const isRunning = status === "running";
  const isSuccess = status === "success";

  return (
    <div className="flex items-center gap-2 rounded-[var(--radius-sm)] bg-[var(--surface-raised)] px-3 py-1.5 text-xs text-[var(--text-muted)]">
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
    <div className="space-y-1">
      {groups.map((group, gi) => (
        <ToolGroup key={gi} group={group} forceExpanded={hasRunning} />
      ))}
    </div>
  );
}

interface ToolGroup {
  type: "shell" | "read_file" | "write_file" | "list_directory";
  items: ToolCallItem[];
}

function groupToolCalls(calls: ToolCallItem[]): ToolGroup[] {
  const groups: ToolGroup[] = [];
  let current: ToolGroup | null = null;

  for (const call of calls) {
    const t = call.name as ToolGroup["type"];
    if (t === "shell") {
      groups.push({ type: "shell", items: [call] });
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
  switch (type) {
    case "shell": return <IconTerminal2 size={13} stroke={1.8} />;
    case "read_file": return <IconFileText size={13} stroke={1.8} />;
    case "write_file": return <IconPencil size={13} stroke={1.8} />;
    case "list_directory": return <IconFolderOpen size={13} stroke={1.8} />;
    default: return <IconFile size={13} stroke={1.8} />;
  }
}

function toolGroupSummary(group: ToolGroup): string {
  const n = group.items.length;
  switch (group.type) {
    case "shell":
      return group.items[0].displayLabel;
    case "read_file":
      return n === 1 ? `Read ${group.items[0].displayLabel}` : `Read ${n} files`;
    case "write_file":
      return n === 1 ? `Edited ${group.items[0].displayLabel}` : `Edited ${n} files`;
    case "list_directory":
      return n === 1 ? `Listed ${group.items[0].displayLabel}` : `Listed ${n} directories`;
    default:
      return `${n} tool calls`;
  }
}

function parseToolArgs(item: ToolCallItem): Record<string, unknown> {
  try {
    return JSON.parse(item.arguments);
  } catch {
    return {};
  }
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
    <div className="rounded-[var(--radius-sm)] bg-[var(--surface-raised)] text-xs">
      <button
        onClick={() => setLocalExpanded(!localExpanded)}
        className="flex w-full items-center gap-2 px-3 py-1.5 text-left text-[var(--text-muted)] hover:text-[var(--text-base)] transition-colors"
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
        <div className="border-t border-[var(--border-subtle)] px-3 py-1 space-y-0.5">
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

  return (
    <div>
      <button
        onClick={() => hasOutput && setExpanded(!expanded)}
        className={`flex w-full items-center gap-2 py-0.5 text-left text-[var(--text-faint)] ${hasOutput ? "cursor-pointer hover:text-[var(--text-muted)]" : "cursor-default"}`}
      >
        <span className="text-[var(--text-faint)]">└</span>
        {item.status === "running" ? (
          <IconLoader2 size={10} stroke={2} className="animate-spin" />
        ) : item.status === "success" ? (
          <span className="h-1 w-1 rounded-full bg-[var(--accent)]" />
        ) : (
          <span className="h-1 w-1 rounded-full bg-[var(--danger)]" />
        )}
        <span className="min-w-0 flex-1 truncate font-mono">{item.displayLabel}</span>
        {hasOutput && (
          expanded
            ? <IconChevronDown size={10} stroke={2} className="flex-shrink-0 opacity-40" />
            : <IconChevronRight size={10} stroke={2} className="flex-shrink-0 opacity-40" />
        )}
      </button>
      {expanded && item.output && (
        <pre className="thin-scrollbar ml-4 mt-0.5 max-h-[150px] overflow-auto whitespace-pre-wrap break-all rounded bg-[var(--surface-main)] px-2 py-1 font-mono text-[11px] text-[var(--text-base)]">
          {item.output}
        </pre>
      )}
    </div>
  );
}

function ToolDetailView({ item }: { item: ToolCallItem }) {
  const args = parseToolArgs(item);
  const cmd = args.command;
  const path = args.path as string | undefined;
  const content = args.content as string | undefined;
  const [outputCopied, setOutputCopied] = useState(false);

  const handleCopyOutput = useCallback(() => {
    if (!item.output) return;
    navigator.clipboard.writeText(item.output).then(() => {
      setOutputCopied(true);
      setTimeout(() => setOutputCopied(false), 2000);
    });
  }, [item.output]);

  return (
    <div className="space-y-1 py-1 text-[var(--text-faint)]">
      {item.name === "shell" && cmd != null && (
        <pre className="whitespace-pre-wrap break-all rounded bg-[var(--surface-main)] px-2 py-1 font-mono text-[var(--text-base)]">
          {Array.isArray(cmd) ? (cmd as string[]).join(" ") : String(cmd)}
        </pre>
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
            <pre className="max-h-[120px] overflow-auto whitespace-pre-wrap break-all rounded bg-[var(--surface-main)] px-2 py-1 font-mono text-[var(--text-base)]">
              {content.slice(0, 500)}{content.length > 500 ? "..." : ""}
            </pre>
          )}
        </div>
      )}
      {item.name === "list_directory" && path && (
        <div className="flex items-center gap-1.5">
          <IconFolderOpen size={11} stroke={1.8} />
          <span className="font-mono">{path}</span>
        </div>
      )}

      {item.output && item.status !== "running" && (
        <div className="group/output relative mt-1">
          <div className="flex items-center justify-between text-[10px] text-[var(--text-faint)] mb-0.5">
            <span>output</span>
            <button
              onClick={handleCopyOutput}
              className="flex items-center gap-0.5 rounded px-1 py-0.5 opacity-0 transition-opacity hover:bg-[var(--surface-elevated)] group-hover/output:opacity-100"
            >
              {outputCopied ? <IconCheck size={10} stroke={2} /> : <IconCopy size={10} stroke={2} />}
              {outputCopied ? "Copied" : "Copy"}
            </button>
          </div>
          <pre className="thin-scrollbar max-h-[200px] overflow-auto whitespace-pre-wrap break-all rounded bg-[var(--surface-main)] px-2 py-1.5 font-mono text-[11px] leading-relaxed text-[var(--text-base)]">
            {item.output}
          </pre>
        </div>
      )}
    </div>
  );
}

function MessageContent({ content }: { content: string }) {
  const parts = content.split(/(```[\s\S]*?```)/g);

  return (
    <>
      {parts.map((part, index) => {
        if (part.startsWith("```") && part.endsWith("```")) {
          const lines = part.slice(3, -3);
          const firstNewline = lines.indexOf("\n");
          const language = firstNewline > 0 ? lines.slice(0, firstNewline).trim() : "";
          const code = firstNewline > 0 ? lines.slice(firstNewline + 1) : lines;
          return <CodeBlock key={index} code={code} language={language} />;
        }

        return <MarkdownText key={index} text={part} />;
      })}
    </>
  );
}

function MarkdownText({ text }: { text: string }) {
  if (!text) return null;

  const lines = text.split("\n");
  const elements: React.ReactNode[] = [];
  let listItems: string[] = [];
  let listKey = 0;

  const flushList = () => {
    if (listItems.length > 0) {
      elements.push(
        <ul key={`list-${listKey++}`} className="my-1.5 ml-4 list-disc space-y-0.5 text-[var(--text-base)]">
          {listItems.map((item, i) => (
            <li key={i}><InlineMarkdown text={item} /></li>
          ))}
        </ul>
      );
      listItems = [];
    }
  };

  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];

    const headingMatch = line.match(/^(#{1,3})\s+(.+)$/);
    if (headingMatch) {
      flushList();
      const level = headingMatch[1].length;
      const headingText = headingMatch[2];
      const cls = level === 1
        ? "text-base font-bold mt-3 mb-1.5"
        : level === 2
          ? "text-sm font-semibold mt-2.5 mb-1"
          : "text-sm font-medium mt-2 mb-0.5";
      elements.push(
        <div key={`h-${i}`} className={`${cls} text-[var(--text-strong)]`}>
          <InlineMarkdown text={headingText} />
        </div>
      );
      continue;
    }

    const listMatch = line.match(/^[-*]\s+(.+)$/);
    if (listMatch) {
      listItems.push(listMatch[1]);
      continue;
    }

    const numberedMatch = line.match(/^\d+\.\s+(.+)$/);
    if (numberedMatch) {
      listItems.push(numberedMatch[1]);
      continue;
    }

    flushList();

    if (line.trim() === "") {
      if (i > 0 && lines[i - 1].trim() !== "") {
        elements.push(<div key={`br-${i}`} className="h-2" />);
      }
      continue;
    }

    elements.push(
      <span key={`t-${i}`} className="whitespace-pre-wrap break-words">
        <InlineMarkdown text={line} />
        {i < lines.length - 1 && lines[i + 1].trim() !== "" ? "\n" : ""}
      </span>
    );
  }

  flushList();

  return <>{elements}</>;
}

function InlineMarkdown({ text }: { text: string }) {
  const parts = text.split(/(\*\*[^*]+\*\*|`[^`]+`)/g);

  return (
    <>
      {parts.map((part, i) => {
        if (part.startsWith("**") && part.endsWith("**")) {
          return <strong key={i} className="font-semibold text-[var(--text-strong)]">{part.slice(2, -2)}</strong>;
        }
        if (part.startsWith("`") && part.endsWith("`")) {
          return (
            <code key={i} className="rounded bg-[var(--surface-raised)] px-1 py-0.5 text-[0.85em] font-mono text-[var(--accent)]">
              {part.slice(1, -1)}
            </code>
          );
        }
        return <span key={i}>{part}</span>;
      })}
    </>
  );
}
