import { useState } from "react";
import { useIntl } from "react-intl";
import {
  IconClipboardList,
  IconExternalLink,
  IconPlayerPlay,
} from "@tabler/icons-react";
import ReactMarkdown, { type Components } from "react-markdown";
import remarkGfm from "remark-gfm";
import { standalonePlanOpen } from "../../api/standalone";
import { useAppStore } from "../../stores/appStore";
import type { PlanFile } from "../../stores/appStore";
import { CodeBlock } from "./CodeBlock";

interface PlanCardProps {
  planFile: PlanFile;
  onExecute: (planContent: string) => void;
}

const markdownComponents: Components = {
  code({ className, children, ...rest }) {
    const match = /language-(\w+)/.exec(className ?? "");
    const code = String(children).replace(/\n$/, "");
    if (match) {
      return <CodeBlock language={match[1]} code={code} />;
    }
    return (
      <code className={className} {...rest}>
        {children}
      </code>
    );
  },
};

export function PlanCard({ planFile, onExecute }: PlanCardProps) {
  const intl = useIntl();
  const [collapsed, setCollapsed] = useState(false);
  const isStreaming = useAppStore((s) => s.isStreaming);

  const fileName = planFile.path.split(/[\\/]/).pop() ?? planFile.path;

  const handleOpenFile = async () => {
    try {
      await standalonePlanOpen(planFile.path);
    } catch (err) {
      console.error("Failed to open plan file:", err);
    }
  };

  return (
    <div className="chat-work-card max-w-[980px] overflow-hidden">
      <button
        type="button"
        onClick={() => setCollapsed((v) => !v)}
        className="flex w-full items-center gap-3 border-b border-[var(--chat-line)] px-4 py-3 text-left transition-colors hover:bg-[var(--chat-chip)]"
      >
        <div className="flex h-8 w-8 flex-shrink-0 items-center justify-center rounded-[var(--radius-md)] bg-[var(--accent-soft)] text-[var(--accent)]">
          <IconClipboardList size={18} stroke={1.8} />
        </div>
        <div className="min-w-0 flex-1">
          <div className="text-[13px] font-semibold text-[var(--text-strong)]">
            {intl.formatMessage({ id: "chat.plan.title" })}
          </div>
          <div className="truncate text-[11px] text-[var(--chat-muted)]">
            {fileName}
          </div>
        </div>
      </button>

      {!collapsed && (
        <>
          <div className="max-h-[400px] overflow-y-auto px-4 py-3">
            <div className="prose-plan text-[13px] leading-relaxed text-[var(--chat-prose)]">
              <ReactMarkdown
                remarkPlugins={[remarkGfm]}
                components={markdownComponents}
              >
                {planFile.content}
              </ReactMarkdown>
            </div>
          </div>

          <div className="flex items-center gap-2 border-t border-[var(--chat-line)] px-4 py-2.5">
            <button
              type="button"
              onClick={() => onExecute(planFile.content)}
              disabled={isStreaming}
              className="flex items-center gap-1.5 rounded-full bg-[var(--accent)] px-3.5 py-1.5 text-[12px] font-medium text-white transition-opacity hover:opacity-90 disabled:opacity-40"
            >
              <IconPlayerPlay size={14} stroke={2} />
              {intl.formatMessage({ id: "chat.plan.execute" })}
            </button>
            <button
              type="button"
              onClick={handleOpenFile}
              className="flex items-center gap-1.5 rounded-full border border-[var(--chat-line)] bg-[var(--chat-chip)] px-3 py-1.5 text-[12px] font-medium text-[var(--chat-muted)] transition-colors hover:text-[var(--chat-prose)]"
            >
              <IconExternalLink size={13} stroke={1.8} />
              {intl.formatMessage({ id: "chat.plan.openFile" })}
            </button>
          </div>
        </>
      )}
    </div>
  );
}
