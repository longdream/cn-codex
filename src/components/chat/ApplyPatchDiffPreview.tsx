import { useMemo } from "react";
import { buildPatchLineDiff } from "../../utils/lineDiff";
import { parsePatchDiffEntries } from "../../utils/parsePatchDiff";
import { PatchDiffLines } from "../diff/PatchDiffLines";

interface ApplyPatchDiffPreviewProps {
  patch: string;
}

function fileLabel(paths: string[]): string {
  if (paths.length >= 2) {
    return `${paths[0]} -> ${paths[1]}`;
  }
  return paths[0] ?? "";
}

export function ApplyPatchDiffPreview({ patch }: ApplyPatchDiffPreviewProps) {
  const entries = useMemo(() => parsePatchDiffEntries(patch), [patch]);
  const fileDiffs = useMemo(
    () =>
      entries.map((entry) => ({
        label: fileLabel(entry.paths),
        lines: buildPatchLineDiff(entry.beforeContent, entry.afterContent),
      })),
    [entries],
  );

  if (fileDiffs.length === 0) {
    return null;
  }

  return (
    <div className="chat-tool-output max-h-[220px] overflow-auto">
      {fileDiffs.map((fileDiff, index) => (
        <div key={`${index}:${fileDiff.label}`}>
          {fileDiff.label && (
            <div className="sticky top-0 z-[1] border-b border-[var(--chat-line)] bg-[var(--chat-chip)] px-2.5 py-1 font-mono text-[10px] text-[var(--chat-muted)]">
              {fileDiff.label}
            </div>
          )}
          <PatchDiffLines
            lines={fileDiff.lines}
            className="patch-diff-lines-inline"
          />
        </div>
      ))}
    </div>
  );
}
