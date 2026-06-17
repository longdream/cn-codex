export interface FormatCodeSnippetArgs {
  path: string;
  startLine: number;
  endLine: number;
  language?: string;
  content: string;
  maxChars?: number;
}

export interface FormattedCodeSnippet {
  text: string;
  truncated: boolean;
  originalChars: number;
  emittedChars: number;
}

const DEFAULT_MAX_SNIPPET_CHARS = 6000;

/**
 * 把“文件选区”统一格式化成可投喂给对话的片段：
 * - 固定包含 path + 行范围 + 行数元信息
 * - 可按字符上限截断，避免一次注入过长影响输入体验
 */
export function formatCodeSnippet({
  path,
  startLine,
  endLine,
  language,
  content,
  maxChars = DEFAULT_MAX_SNIPPET_CHARS,
}: FormatCodeSnippetArgs): FormattedCodeSnippet {
  const safePath = path.trim();
  const safeStartLine = Math.max(1, Math.floor(startLine));
  const safeEndLine = Math.max(safeStartLine, Math.floor(endLine));
  const lineCount = safeEndLine - safeStartLine + 1;
  const original = content.replace(/\r\n/g, "\n");

  let snippet = original;
  let truncated = false;
  if (snippet.length > maxChars) {
    snippet = `${snippet.slice(0, maxChars)}\n... [snippet truncated] ...`;
    truncated = true;
  }

  const fenceLang = (language ?? "").trim();
  const header = [
    "[code-snippet]",
    `path: ${safePath}`,
    `range: L${safeStartLine}-L${safeEndLine}`,
    `lines: ${lineCount}`,
  ].join("\n");
  const fenced = fenceLang
    ? `\`\`\`${fenceLang}\n${snippet}\n\`\`\``
    : `\`\`\`\n${snippet}\n\`\`\``;
  const text = `${header}\n${fenced}`;

  return {
    text,
    truncated,
    originalChars: original.length,
    emittedChars: snippet.length,
  };
}
