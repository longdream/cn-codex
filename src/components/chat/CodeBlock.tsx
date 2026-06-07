import { IconCheck, IconCopy } from "@tabler/icons-react";
import { useCallback, useState } from "react";
import { useIntl } from "react-intl";

interface CodeBlockProps {
  code: string;
  language: string;
}

export function CodeBlock({ code, language }: CodeBlockProps) {
  const intl = useIntl();
  const [copied, setCopied] = useState(false);

  const handleCopy = useCallback(() => {
    navigator.clipboard.writeText(code).then(() => {
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    });
  }, [code]);

  return (
    <div className="my-2 overflow-hidden rounded-[var(--radius-md)] border border-[var(--border-subtle)] bg-[var(--surface-contrast)]">
      <div className="flex items-center justify-between border-b border-[var(--border-subtle)] px-3 py-1.5">
        <span className="text-[11px] text-[var(--text-faint)]">
          {language || "text"}
        </span>
        <button
          onClick={handleCopy}
          className="flex items-center gap-1.5 rounded-[var(--radius-sm)] px-2 py-1 text-[11px] text-[var(--text-faint)] transition-colors hover:bg-[var(--surface-elevated)] hover:text-[var(--text-muted)]"
        >
          {copied ? <IconCheck size={12} stroke={2} /> : <IconCopy size={12} stroke={2} />}
          {copied
            ? intl.formatMessage({ id: "chat.copied" })
            : intl.formatMessage({ id: "chat.copy" })}
        </button>
      </div>
      <pre className="thin-scrollbar overflow-x-auto px-3 py-3 text-xs leading-relaxed">
        <code className="text-[var(--text-base)]">{code}</code>
      </pre>
    </div>
  );
}
