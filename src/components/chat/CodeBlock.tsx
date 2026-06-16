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
    <div className="chat-code-block my-3 overflow-hidden">
      <div className="flex items-center justify-between border-b border-[var(--chat-line)] px-3 py-1.5">
        <span className="text-[11px] text-[var(--chat-faint)]">
          {language || "text"}
        </span>
        <button
          onClick={handleCopy}
          className="chat-copy-button flex items-center gap-1 px-2 py-1 text-[11px] transition-[color,background]"
          title={intl.formatMessage({ id: copied ? "chat.copied" : "chat.copy" })}
        >
          {copied ? <IconCheck size={12} stroke={2} /> : <IconCopy size={12} stroke={2} />}
          {copied
            ? intl.formatMessage({ id: "chat.copied" })
            : intl.formatMessage({ id: "chat.copy" })}
        </button>
      </div>
      <pre className="thin-scrollbar overflow-x-auto px-3 py-3 text-[13px] leading-relaxed">
        <code className="text-[var(--chat-prose)]">{code}</code>
      </pre>
    </div>
  );
}
