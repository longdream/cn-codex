export type ThemePreference = "dark" | "light" | "system";
export type ResolvedTheme = "dark" | "light";

export function resolveThemeMode(
  theme: ThemePreference,
  prefersLight = false,
): ResolvedTheme {
  if (theme === "system") {
    return prefersLight ? "light" : "dark";
  }
  return theme;
}

export function applyDocumentTheme(
  theme: ThemePreference,
  target: Document = document,
  mediaQuery?: MediaQueryList,
): ResolvedTheme {
  const media =
    mediaQuery ??
    (typeof window !== "undefined"
      ? typeof window.matchMedia === "function"
        ? window.matchMedia("(prefers-color-scheme: light)")
        : undefined
      : undefined);
  const resolvedTheme = resolveThemeMode(theme, Boolean(media?.matches));
  target.documentElement.dataset.theme = resolvedTheme;
  target.documentElement.style.colorScheme = resolvedTheme;
  return resolvedTheme;
}
