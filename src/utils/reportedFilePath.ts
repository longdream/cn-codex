const LEGACY_GIT_OCTAL_RUN = /(?:[\\/][0-7]{3}){2,}/g;

export function normalizeReportedFilePath(path: string): string {
  return path.replace(LEGACY_GIT_OCTAL_RUN, (run) => {
    const octets = run.match(/[0-7]{3}/g);
    if (!octets) return run;

    const bytes = Uint8Array.from(octets, (octet) => Number.parseInt(octet, 8));
    try {
      const decoded = new TextDecoder("utf-8", { fatal: true }).decode(bytes);
      return /[^\x00-\x7f]/.test(decoded) ? decoded : run;
    } catch {
      return run;
    }
  });
}

export function normalizeReportedFileChanges<T extends { path: string }>(
  changes?: T[] | null,
): T[] {
  if (!Array.isArray(changes)) return [];

  return changes
    .map((change) => ({
      ...change,
      path: normalizeReportedFilePath(String(change.path ?? "").trim()),
    }))
    .filter((change) => change.path.length > 0);
}
