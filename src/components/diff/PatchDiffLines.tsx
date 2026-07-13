import type { PatchDiffLine } from "../../utils/lineDiff";

function linePrefix(type: PatchDiffLine["type"]): string {
  if (type === "add") return "+";
  if (type === "remove") return "-";
  return " ";
}

interface PatchDiffLinesProps {
  lines: PatchDiffLine[];
  emptyHint?: string;
  className?: string;
}

export function PatchDiffLines({ lines, emptyHint, className }: PatchDiffLinesProps) {
  if (lines.length === 0) {
    return emptyHint ? (
      <div className="px-3 py-3 text-[11px] text-[var(--chat-muted)]">{emptyHint}</div>
    ) : null;
  }

  return (
    <div className={className ?? "patch-diff-lines-scroll thin-scrollbar"}>
      {lines.map((line, index) => (
        <div
          key={`${index}:${line.type}:${line.oldLineNumber ?? 0}:${line.newLineNumber ?? 0}`}
          className={`patch-diff-line patch-diff-line--${line.type}`}
        >
          <span className="patch-diff-line-number">{line.oldLineNumber ?? ""}</span>
          <span className="patch-diff-line-number">{line.newLineNumber ?? ""}</span>
          <span className="patch-diff-line-prefix">{linePrefix(line.type)}</span>
          <span className="patch-diff-line-text">{line.text}</span>
        </div>
      ))}
    </div>
  );
}
