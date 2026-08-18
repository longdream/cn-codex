export interface PatchDiffEntry {
  // 一个补丁项可能同时命中旧路径/新路径（例如 rename）。
  paths: string[];
  // 作为 Diff 左侧输入的文本。
  beforeContent: string;
  // 作为 Diff 右侧输入的文本。
  afterContent: string;
}

function normalizeDirectivePath(value: string): string {
  return value.replace(/\s+\*{3}\s*$/, "").trim();
}

function normalizeDirectiveLine(line: string): string {
  const trimmed = line.trim();
  if (!trimmed.startsWith("*** ")) {
    return line;
  }
  const normalized = trimmed.replace(/\s+\*{3}\s*$/, "");
  return normalized === "*** End of Patch" ? "*** End Patch" : normalized;
}

function isBeginMarker(line: string): boolean {
  return normalizeDirectiveLine(line) === "*** Begin Patch";
}

function isEndMarker(line: string): boolean {
  return normalizeDirectiveLine(line) === "*** End Patch";
}

function isClassicContextDiffMarker(line: string): boolean {
  const trimmed = line.trim();
  return (
    trimmed === "***************" ||
    /^\*\*\*\s+\d+(?:,\d+)?\s+\*\*\*\*$/.test(trimmed) ||
    /^---\s+\d+(?:,\d+)?\s+----$/.test(trimmed)
  );
}

/**
 * 解析 apply_patch 文本得到每个文件的 before/after 内容块。
 * 说明：这里是“补丁级还原”，用于 Diff 可视化，不做完整文件重建。
 */
export function parsePatchDiffEntries(patch: string): PatchDiffEntry[] {
  const sourceLines = patch.replace(/\r\n/g, "\n").replace(/\r/g, "\n").split("\n");
  const beginIndexes = sourceLines
    .map((line, index) => (isBeginMarker(line) ? index : -1))
    .filter((index) => index >= 0);
  const endIndexes = sourceLines
    .map((line, index) => (isEndMarker(line) ? index : -1))
    .filter((index) => index >= 0);

  if (
    beginIndexes.length !== 1 ||
    endIndexes.length !== 1 ||
    beginIndexes[0] >= endIndexes[0]
  ) {
    return [];
  }

  const lines = sourceLines
    .slice(beginIndexes[0] + 1, endIndexes[0])
    .map(normalizeDirectiveLine);
  if (lines.some(isClassicContextDiffMarker)) {
    return [];
  }

  const result: PatchDiffEntry[] = [];
  let idx = 0;

  const isBoundary = (line: string): boolean => {
    return (
      line.startsWith("*** Add File: ") ||
      line.startsWith("*** Update File: ") ||
      line.startsWith("*** Delete File: ")
    );
  };

  while (idx < lines.length) {
    const line = lines[idx];

    if (line.startsWith("*** Add File: ")) {
      const path = normalizeDirectivePath(line.slice("*** Add File: ".length));
      if (!path) {
        return [];
      }
      idx += 1;
      const afterLines: string[] = [];
      while (idx < lines.length && !isBoundary(lines[idx])) {
        const body = lines[idx];
        if (!body.startsWith("+")) {
          return [];
        }
        afterLines.push(body.slice(1));
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
      const path = normalizeDirectivePath(line.slice("*** Delete File: ".length));
      if (!path) {
        return [];
      }
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
      const path = normalizeDirectivePath(line.slice("*** Update File: ".length));
      if (!path) {
        return [];
      }
      let moveTo: string | null = null;
      const beforeLines: string[] = [];
      const afterLines: string[] = [];
      let hasTextChanges = false;
      idx += 1;
      while (idx < lines.length && !isBoundary(lines[idx])) {
        const body = lines[idx];
        if (body.startsWith("*** Move to: ")) {
          moveTo = normalizeDirectivePath(body.slice("*** Move to: ".length));
          idx += 1;
          continue;
        }
        if (body.startsWith("*** Desc: ") || body === "*** End of File") {
          idx += 1;
          continue;
        }
        if (body === "```diff" || body === "```patch" || body === "```") {
          idx += 1;
          continue;
        }
        if (body.startsWith("diff --git ") || body.startsWith("index ")) {
          idx += 1;
          continue;
        }
        if (body.startsWith("--- ")) {
          if (lines[idx + 1]?.startsWith("+++ ")) {
            idx += 2;
            continue;
          }
          return [];
        }
        if (body.startsWith("@@")) {
          idx += 1;
          continue;
        }
        if (body.startsWith("-")) {
          beforeLines.push(body.slice(1));
          hasTextChanges = true;
        } else if (body.startsWith("+")) {
          afterLines.push(body.slice(1));
          hasTextChanges = true;
        } else if (body.startsWith(" ")) {
          const context = body.slice(1);
          beforeLines.push(context);
          afterLines.push(context);
        } else if (body.length === 0) {
          beforeLines.push("");
          afterLines.push("");
        } else {
          beforeLines.push(body);
          afterLines.push(body);
        }
        idx += 1;
      }
      if (!hasTextChanges && !moveTo) {
        return [];
      }
      result.push({
        paths: moveTo ? [path, moveTo] : [path],
        beforeContent: beforeLines.join("\n"),
        afterContent: afterLines.join("\n"),
      });
      continue;
    }

    return [];
  }

  return result;
}
