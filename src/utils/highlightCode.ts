import hljs from "highlight.js";

function escapeHtml(text: string): string {
  return text
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");
}

/**
 * 生成与原始代码字符一一对应的高亮 HTML。
 * 详情页编辑器依赖“透明 textarea + 高亮层”叠层，必须保证文本内容不被改写。
 */
export function highlightCodeHtml(code: string, language: string): string {
  const normalizedLanguage = language.trim().toLowerCase();
  if (!code) {
    return "";
  }

  try {
    if (normalizedLanguage && hljs.getLanguage(normalizedLanguage)) {
      return hljs.highlight(code, {
        language: normalizedLanguage,
        ignoreIllegals: true,
      }).value;
    }
  } catch {
    // 回退到纯文本转义，避免高亮失败时影响编辑。
  }

  return escapeHtml(code);
}
