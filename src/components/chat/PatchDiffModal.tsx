import { IconX } from "@tabler/icons-react";
import { useEffect, useMemo } from "react";
import {
  buildPatchLineDiff,
  type PatchDiffLine,
} from "../../utils/lineDiff";

interface PatchDiffModalProps {
  open: boolean;
  titlePath: string;
  beforeContent: string;
  afterContent: string;
  onClose: () => void;
}

function linePrefix(type: PatchDiffLine["type"]): string {
  if (type === "add") return "+";
  if (type === "remove") return "-";
  return " ";
}

export function PatchDiffModal({
  open,
  titlePath,
  beforeContent,
  afterContent,
  onClose,
}: PatchDiffModalProps) {
  const lines = useMemo(
    () => buildPatchLineDiff(beforeContent, afterContent),
    [beforeContent, afterContent],
  );

  useEffect(() => {
    if (!open) {
      return;
    }
    // 弹窗打开时支持 Esc 关闭，交互和主窗口其他弹层保持一致。
    const handler = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        onClose();
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [onClose, open]);

  if (!open) {
    return null;
  }

  return (
    <div
      className="patch-diff-backdrop"
      onClick={() => onClose()}
      role="presentation"
    >
      <div
        className="patch-diff-dialog"
        onClick={(event) => event.stopPropagation()}
      >
        <div className="patch-diff-header">
          <div className="min-w-0">
            <div className="text-[12px] font-semibold text-[var(--chat-prose)]">
              Diff 预览（Keep 才会写盘）
            </div>
            <div
              className="mt-0.5 truncate font-mono text-[11px] text-[var(--chat-faint)]"
              title={titlePath}
            >
              {titlePath}
            </div>
          </div>
          <button
            type="button"
            onClick={() => onClose()}
            className="patch-diff-close"
            title="关闭"
          >
            <IconX size={14} stroke={1.8} />
          </button>
        </div>

        <div className="patch-diff-body thin-scrollbar">
          {lines.map((line, index) => (
            <div
              key={`${index}:${line.type}:${line.oldLineNumber ?? 0}:${line.newLineNumber ?? 0}`}
              className={`patch-diff-line patch-diff-line--${line.type}`}
            >
              <span className="patch-diff-line-number">
                {line.oldLineNumber ?? ""}
              </span>
              <span className="patch-diff-line-number">
                {line.newLineNumber ?? ""}
              </span>
              <span className="patch-diff-line-prefix">{linePrefix(line.type)}</span>
              <span className="patch-diff-line-text">{line.text}</span>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}
