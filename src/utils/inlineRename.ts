export type InlineRenameResolution =
  | { shouldRename: false }
  | { shouldRename: true; name: string };

export function resolveInlineRename(
  value: string,
  originalName: string,
): InlineRenameResolution {
  const name = value.trim();
  if (!name || name === originalName) {
    return { shouldRename: false };
  }
  return { shouldRename: true, name };
}
