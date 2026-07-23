import { convertFileSrc } from "@tauri-apps/api/core";
import { useEffect, useMemo, type CSSProperties } from "react";
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
  const backgroundBlur = useSettingsStore((state) => state.backgroundBlur);
  const backgroundBrightness = useSettingsStore((state) => state.backgroundBrightness);
  const backgroundOverlay = useSettingsStore((state) => state.backgroundOverlay);
  const backgroundScale = useSettingsStore((state) => state.backgroundScale);
  const workspaceCwd = useAppStore((state) => state.workspaceCwd);

  const imageSrc = useMemo(() => {
    if (!backgroundImagePath) return null;
    return localImagePreviewSrc(backgroundImagePath, workspaceCwd);
  }, [backgroundImagePath, workspaceCwd]);

  useEffect(() => {
    const root = document.documentElement;
    if (!imageSrc) {
      root.removeAttribute("data-has-background");
      return;
    }
    root.setAttribute("data-has-background", "true");
    return () => {
      root.removeAttribute("data-has-background");
    };
  }, [imageSrc]);

  if (!imageSrc) {
    return null;
  }

  const imageStyle = {
    ["--bg-blur" as string]: `${backgroundBlur}px`,
    ["--bg-brightness" as string]: String(backgroundBrightness),
    ["--bg-scale" as string]: String(backgroundScale),
  } as CSSProperties;

  const maskStyle = {
    ["--bg-overlay" as string]: String(backgroundOverlay),
  } as CSSProperties;

  return (
    <div className="app-background-layer" aria-hidden="true">
      <img
        className="app-background-image"
        src={imageSrc}
        alt=""
        draggable={false}
        style={imageStyle}
      />
      <div className="app-background-mask" style={maskStyle} />
    </div>
  );
}
