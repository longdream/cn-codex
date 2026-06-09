import { invoke } from "@tauri-apps/api/core";

export interface BrowserWindowInfo {
  label: string;
  url: string;
  created: boolean;
  debugPort: number;
  cdpEndpoint: string;
}

export async function windowStartDragging(): Promise<void> {
  await invoke("window_start_dragging");
}

export async function windowMinimize(): Promise<void> {
  await invoke("window_minimize");
}

export async function windowToggleMaximize(): Promise<void> {
  await invoke("window_toggle_maximize");
}

export async function windowClose(): Promise<void> {
  await invoke("window_close");
}

export async function windowOpenBrowser(url?: string): Promise<BrowserWindowInfo> {
  const trimmed = url?.trim();
  return invoke<BrowserWindowInfo>("window_open_browser", {
    url: trimmed ? trimmed : null,
  });
}
