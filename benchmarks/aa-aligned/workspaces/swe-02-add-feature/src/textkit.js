export function slugify(input) {
  return String(input)
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
}

export function truncate(str, maxLen) {
  if (str.length <= maxLen) return str;
  if (maxLen <= 0) return "";
  return str.slice(0, maxLen - 1) + "\u2026";
}

export function countWords(str) {
  const trimmed = str.trim();
  if (trimmed === "") return 0;
  return trimmed.split(/\s+/).length;
}