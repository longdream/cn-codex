import type { PathRefAttachedFile } from "../types/provider";

export const PATH_REF_MIME = "application/x-cn-codex-path-ref";

// 详情窗与主窗通过事件通信时使用该前缀，避免把“行号引用”误当作普通文本插入输入框。
const PATH_REF_RANGE_SNIPPET_PREFIX = "__CN_CODEX_PATH_REF_RANGE__:";
const RANGE_REFERENCE_EXTENSIONS = new Set(["md", "mdx", "txt"]);

export interface PathRefRangeSnippetPayload {
  kind: "pathRefRange";
  sourcePath: string;
  name: string;
  lineStart: number;
  lineEnd: number;
}

function normalizeLineNumber(value: number): number {
  if (!Number.isFinite(value)) {
    return 1;
  }
  return Math.max(1, Math.floor(value));
}

export function supportsPathRefRangeByName(name: string): boolean {
  const ext = name.split(".").pop()?.toLowerCase() ?? "";
  return RANGE_REFERENCE_EXTENSIONS.has(ext);
}

export function formatPathRefLineRange(
  range: Pick<PathRefAttachedFile, "lineStart" | "lineEnd">,
): string | null {
  if (range.lineStart == null || range.lineEnd == null) {
    return null;
  }
  const lineStart = normalizeLineNumber(range.lineStart);
  const lineEnd = normalizeLineNumber(range.lineEnd);
  const safeEnd = Math.max(lineStart, lineEnd);
  return `L${lineStart}-L${safeEnd}`;
}

export function buildPathRefPromptValue(file: PathRefAttachedFile): string {
  const lineRange = formatPathRefLineRange(file);
  return lineRange ? `${file.sourcePath}#${lineRange}` : file.sourcePath;
}

export function encodePathRefRangeSnippet(payload: PathRefRangeSnippetPayload): string {
  const lineStart = normalizeLineNumber(payload.lineStart);
  const lineEnd = Math.max(lineStart, normalizeLineNumber(payload.lineEnd));
  const normalized: PathRefRangeSnippetPayload = {
    kind: "pathRefRange",
    sourcePath: payload.sourcePath.trim(),
    name: payload.name.trim(),
    lineStart,
    lineEnd,
  };
  return `${PATH_REF_RANGE_SNIPPET_PREFIX}${JSON.stringify(normalized)}`;
}

export function decodePathRefRangeSnippet(snippet: string): PathRefRangeSnippetPayload | null {
  if (!snippet.startsWith(PATH_REF_RANGE_SNIPPET_PREFIX)) {
    return null;
  }
  const payloadRaw = snippet.slice(PATH_REF_RANGE_SNIPPET_PREFIX.length);
  if (!payloadRaw.trim()) {
    return null;
  }
  try {
    const parsed = JSON.parse(payloadRaw) as Partial<PathRefRangeSnippetPayload>;
    if (
      parsed.kind !== "pathRefRange"
      || typeof parsed.sourcePath !== "string"
      || typeof parsed.name !== "string"
    ) {
      return null;
    }
    const sourcePath = parsed.sourcePath.trim();
    const name = parsed.name.trim();
    if (!sourcePath || !name) {
      return null;
    }
    const lineStart = normalizeLineNumber(parsed.lineStart ?? 1);
    const lineEnd = Math.max(lineStart, normalizeLineNumber(parsed.lineEnd ?? lineStart));
    return {
      kind: "pathRefRange",
      sourcePath,
      name,
      lineStart,
      lineEnd,
    };
  } catch {
    return null;
  }
}
