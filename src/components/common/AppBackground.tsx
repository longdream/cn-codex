import { convertFileSrc } from "@tauri-apps/api/core";
import { useMemo } from "react";
import { useAppStore } from "../../stores/appStore";
import { useSettingsStore } from "../../stores/settingsStore";

function normalizeLocalImagePath(raw: string): string {
  const trimmed = raw.trim();
  if (!trimmed) return "";
  if (trimmed.startsWith("\\\\?\\UNC\\")) {
    return `\\\\${trimmed.slice("\\\\?\\UNC\\".length)}`;
  }
  if (trimmed.startsWith("\\\\?\\")) {
    return trimmed.slice("\\\\?\\".length);
  }
  return trimmed;
}

function localImagePreviewSrc(path: string, workspaceCwd: string | null): string | null {
  const normalizedPath = normalizeLocalImagePath(path);
  if (!normalizedPath) return null;

  const absolute =
    /^[A-Za-z]:[\\/]/.test(normalizedPath) ||
    normalizedPath.startsWith("\\\\") ||
    normalizedPath.startsWith("/");
  const resolvedPath = absolute
    ? normalizedPath
    : workspaceCwd
      ? `${workspaceCwd.replace(/[\\/]+$/, "")}\\${normalizedPath.replace(/^[\\/]+/, "")}`
      : null;

  return resolvedPath ? convertFileSrc(resolvedPath) : null;
}

export function AppBackground() {
  const backgroundImagePath = useSettingsStore((state) => state.backgroundImagePath);
  const workspaceCwd = useAppStore((state) => state.workspaceCwd);

  const imageSrc = useMemo(() => {
    if (!backgroundImagePath) return null;
    return localImagePreviewSrc(backgroundImagePath, workspaceCwd);
  }, [backgroundImagePath, workspaceCwd]);

  if (!imageSrc) {
    return null;
  }

  return (
    <div className="app-background-layer" aria-hidden="true">
      <img className="app-background-image" src={imageSrc} alt="" draggable={false} />
      <div className="app-background-mask" />
    </div>
  );
}
