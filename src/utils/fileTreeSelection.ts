export interface FileTreeSelectable {
  path: string;
  isDir: boolean;
}

export type FileTreeSelectionMode = "replace" | "toggle" | "range";

export function normalizePathKey(path: string): string {
  return path.replace(/[\\/]+$/, "").replace(/\\/g, "/").toLowerCase();
}

export function joinPath(parent: string, name: string): string {
  const safeParent = parent.replace(/[\\/]+$/, "");
  const separator = parent.includes("\\") ? "\\" : "/";
  return `${safeParent}${separator}${name}`;
}

export function getParentPath(path: string): string {
  const normalized = path.replace(/[\\/]+$/, "");
  const idx = Math.max(normalized.lastIndexOf("/"), normalized.lastIndexOf("\\"));
  if (idx <= 0) {
    return normalized;
  }
  return normalized.slice(0, idx);
}

export function getBaseName(path: string): string {
  const normalized = path.replace(/[\\/]+$/, "");
  const parts = normalized.split(/[\\/]/).filter(Boolean);
  return parts[parts.length - 1] ?? normalized;
}

export function pathsEqual(a: string, b: string): boolean {
  return normalizePathKey(a) === normalizePathKey(b);
}

export function isPathInside(parent: string, child: string): boolean {
  const parentKey = normalizePathKey(parent);
  const childKey = normalizePathKey(child);
  if (parentKey === childKey) {
    return true;
  }
  return childKey.startsWith(`${parentKey}/`);
}

/** Prefer deleting parents instead of both parent and nested children. */
export function pruneNestedPaths(paths: string[]): string[] {
  const unique = Array.from(new Set(paths.filter(Boolean)));
  unique.sort((a, b) => normalizePathKey(a).localeCompare(normalizePathKey(b)));
  const result: string[] = [];
  for (const path of unique) {
    const covered = result.some((existing) => isPathInside(existing, path));
    if (!covered) {
      result.push(path);
    }
  }
  return result;
}

export function uniqueNameInDirectory(
  existingNames: Iterable<string>,
  desiredName: string,
): string {
  const desired = desiredName.trim() || "untitled";
  const used = new Set(
    Array.from(existingNames)
      .map((name) => name.trim().toLowerCase())
      .filter(Boolean),
  );
  if (!used.has(desired.toLowerCase())) {
    return desired;
  }

  const dot = desired.lastIndexOf(".");
  const hasExt = dot > 0 && dot < desired.length - 1 && !desired.slice(dot + 1).includes(" ");
  const stem = hasExt ? desired.slice(0, dot) : desired;
  const ext = hasExt ? desired.slice(dot) : "";

  let index = 1;
  while (index < 10_000) {
    const candidate = `${stem} copy${index > 1 ? ` ${index}` : ""}${ext}`;
    if (!used.has(candidate.toLowerCase())) {
      return candidate;
    }
    index += 1;
  }
  return `${stem}-${Date.now()}${ext}`;
}

export function computeNextSelection(
  visiblePaths: string[],
  currentSelected: string[],
  targetPath: string,
  mode: FileTreeSelectionMode,
  anchorPath: string | null,
): { selectedPaths: string[]; anchorPath: string } {
  const visibleSet = new Set(visiblePaths);
  const normalizedCurrent = currentSelected.filter((path) => visibleSet.has(path) || path === targetPath);

  if (mode === "toggle") {
    const exists = normalizedCurrent.includes(targetPath);
    const selectedPaths = exists
      ? normalizedCurrent.filter((path) => path !== targetPath)
      : [...normalizedCurrent, targetPath];
    return {
      selectedPaths: selectedPaths.length > 0 ? selectedPaths : [targetPath],
      anchorPath: targetPath,
    };
  }

  if (mode === "range") {
    const startPath = anchorPath && visibleSet.has(anchorPath) ? anchorPath : targetPath;
    const startIndex = visiblePaths.indexOf(startPath);
    const endIndex = visiblePaths.indexOf(targetPath);
    if (startIndex < 0 || endIndex < 0) {
      return { selectedPaths: [targetPath], anchorPath: targetPath };
    }
    const [from, to] = startIndex <= endIndex ? [startIndex, endIndex] : [endIndex, startIndex];
    return {
      selectedPaths: visiblePaths.slice(from, to + 1),
      anchorPath: startPath,
    };
  }

  return {
    selectedPaths: [targetPath],
    anchorPath: targetPath,
  };
}

export function resolveCreateParentPath(
  rootPath: string,
  selectedNodes: FileTreeSelectable[],
): string {
  if (selectedNodes.length === 0) {
    return rootPath;
  }
  const primary = selectedNodes[selectedNodes.length - 1];
  return primary.isDir ? primary.path : getParentPath(primary.path);
}

export function buildClipboardPayload(paths: string[], mode: "copy" | "cut") {
  return {
    mode,
    paths: Array.from(new Set(paths.filter(Boolean))),
  };
}
