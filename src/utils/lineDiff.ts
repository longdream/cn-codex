export type PatchDiffLineType = "context" | "add" | "remove";

export interface PatchDiffLine {
  type: PatchDiffLineType;
  text: string;
  oldLineNumber?: number;
  newLineNumber?: number;
}

const MAX_LCS_LINE_COUNT = 1800;
const MAX_LCS_CELL_COUNT = 1_200_000;

function normalizeEol(text: string): string {
  return text.replace(/\r\n/g, "\n").replace(/\r/g, "\n");
}

function splitLines(text: string): string[] {
  // 统一按 LF 拆行，保证 Windows / macOS / Linux 文本都能稳定对齐。
  const normalized = normalizeEol(text);
  if (normalized.length === 0) {
    return [];
  }
  return normalized.split("\n");
}

function fallbackPrefixSuffixDiff(before: string[], after: string[]): PatchDiffLine[] {
  // 兜底策略：当文本过大时避免 O(n*m) 的 LCS 内存与耗时开销。
  // 这里保留公共前后缀，中间块做 remove + add，保证审阅可读性和稳定性。
  let prefix = 0;
  while (
    prefix < before.length &&
    prefix < after.length &&
    before[prefix] === after[prefix]
  ) {
    prefix += 1;
  }

  let suffix = 0;
  while (
    suffix < before.length - prefix &&
    suffix < after.length - prefix &&
    before[before.length - 1 - suffix] === after[after.length - 1 - suffix]
  ) {
    suffix += 1;
  }

  const lines: PatchDiffLine[] = [];
  let oldCursor = 1;
  let newCursor = 1;

  for (let i = 0; i < prefix; i += 1) {
    lines.push({
      type: "context",
      text: before[i],
      oldLineNumber: oldCursor,
      newLineNumber: newCursor,
    });
    oldCursor += 1;
    newCursor += 1;
  }

  const beforeMiddleEnd = before.length - suffix;
  const afterMiddleEnd = after.length - suffix;
  for (let i = prefix; i < beforeMiddleEnd; i += 1) {
    lines.push({
      type: "remove",
      text: before[i],
      oldLineNumber: oldCursor,
    });
    oldCursor += 1;
  }
  for (let i = prefix; i < afterMiddleEnd; i += 1) {
    lines.push({
      type: "add",
      text: after[i],
      newLineNumber: newCursor,
    });
    newCursor += 1;
  }

  for (let i = suffix - 1; i >= 0; i -= 1) {
    lines.push({
      type: "context",
      text: before[before.length - 1 - i],
      oldLineNumber: oldCursor,
      newLineNumber: newCursor,
    });
    oldCursor += 1;
    newCursor += 1;
  }

  return lines;
}

/**
 * 将 before/after 转为行级 diff 结果，用于 Patch 审阅弹窗（红删绿增）。
 *
 * 设计说明：
 * - 小中型文本：使用 LCS 计算更精确的增删分块；
 * - 大文本：自动退化为 prefix/suffix 方案，避免浏览器卡顿。
 */
export function buildPatchLineDiff(
  beforeText: string,
  afterText: string,
): PatchDiffLine[] {
  const beforeLines = splitLines(beforeText);
  const afterLines = splitLines(afterText);
  const n = beforeLines.length;
  const m = afterLines.length;

  if (
    n > MAX_LCS_LINE_COUNT ||
    m > MAX_LCS_LINE_COUNT ||
    n * m > MAX_LCS_CELL_COUNT
  ) {
    return fallbackPrefixSuffixDiff(beforeLines, afterLines);
  }

  // dp[i][j] 表示 before[i..] 与 after[j..] 的 LCS 长度。
  const dp: Uint32Array[] = Array.from(
    { length: n + 1 },
    () => new Uint32Array(m + 1),
  );

  for (let i = n - 1; i >= 0; i -= 1) {
    for (let j = m - 1; j >= 0; j -= 1) {
      if (beforeLines[i] === afterLines[j]) {
        dp[i][j] = dp[i + 1][j + 1] + 1;
      } else {
        dp[i][j] = Math.max(dp[i + 1][j], dp[i][j + 1]);
      }
    }
  }

  const lines: PatchDiffLine[] = [];
  let i = 0;
  let j = 0;
  let oldCursor = 1;
  let newCursor = 1;

  while (i < n && j < m) {
    if (beforeLines[i] === afterLines[j]) {
      lines.push({
        type: "context",
        text: beforeLines[i],
        oldLineNumber: oldCursor,
        newLineNumber: newCursor,
      });
      i += 1;
      j += 1;
      oldCursor += 1;
      newCursor += 1;
      continue;
    }

    if (dp[i + 1][j] >= dp[i][j + 1]) {
      lines.push({
        type: "remove",
        text: beforeLines[i],
        oldLineNumber: oldCursor,
      });
      i += 1;
      oldCursor += 1;
    } else {
      lines.push({
        type: "add",
        text: afterLines[j],
        newLineNumber: newCursor,
      });
      j += 1;
      newCursor += 1;
    }
  }

  while (i < n) {
    lines.push({
      type: "remove",
      text: beforeLines[i],
      oldLineNumber: oldCursor,
    });
    i += 1;
    oldCursor += 1;
  }

  while (j < m) {
    lines.push({
      type: "add",
      text: afterLines[j],
      newLineNumber: newCursor,
    });
    j += 1;
    newCursor += 1;
  }

  return lines;
}
