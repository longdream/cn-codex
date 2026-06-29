export interface FormatWebSnippetArgs {
  url: string;
  selector: string;
  selectorCandidates?: string[];
  sourcePath?: string | null;
  tagName?: string;
  text?: string;
  rect?: {
    x: number;
    y: number;
    width: number;
    height: number;
  };
  maxTextChars?: number;
}

export interface FormattedWebSnippet {
  text: string;
  truncated: boolean;
}

const DEFAULT_MAX_TEXT_CHARS = 320;

export function formatWebSnippet({
  url,
  selector,
  selectorCandidates = [],
  sourcePath,
  tagName,
  text,
  rect,
  maxTextChars = DEFAULT_MAX_TEXT_CHARS,
}: FormatWebSnippetArgs): FormattedWebSnippet {
  const trimmedText = (text ?? "").trim();
  const truncated = trimmedText.length > maxTextChars;
  const previewText = truncated
    ? `${trimmedText.slice(0, maxTextChars)}...`
    : trimmedText;
  const safeSelector = selector.trim() || selectorCandidates[0] || "(empty)";
  const safeCandidates = selectorCandidates
    .map((item) => item.trim())
    .filter((item) => item.length > 0)
    .slice(0, 6);
  const lines = [
    "[web-snippet]",
    `url: ${url.trim() || "about:blank"}`,
    `selector: ${safeSelector}`,
  ];

  if (sourcePath?.trim()) {
    lines.push(`sourcePath: ${sourcePath.trim()}`);
  }
  if (tagName?.trim()) {
    lines.push(`tag: ${tagName.trim()}`);
  }
  if (rect) {
    lines.push(
      `rect: x=${Math.round(rect.x)}, y=${Math.round(rect.y)}, w=${Math.round(rect.width)}, h=${Math.round(rect.height)}`,
    );
  }
  if (safeCandidates.length > 1) {
    lines.push(`selectorCandidates: ${safeCandidates.join(" | ")}`);
  }
  if (previewText) {
    lines.push(`text: ${previewText}`);
  }

  return {
    text: lines.join("\n"),
    truncated,
  };
}
