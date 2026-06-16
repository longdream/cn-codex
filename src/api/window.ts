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

export async function windowShowMain(): Promise<void> {
  await invoke("window_show_main");
}

export async function windowOpenBrowser(
  url?: string,
  rect?: { x: number; y: number; width: number; height: number },
): Promise<BrowserWindowInfo> {
  const trimmed = url?.trim();
  return invoke<BrowserWindowInfo>("window_open_browser", {
    url: trimmed ? trimmed : null,
    x: rect?.x ?? null,
    y: rect?.y ?? null,
    width: rect?.width ?? null,
    height: rect?.height ?? null,
  });
}

export async function windowResizeBrowser(
  x: number,
  y: number,
  width: number,
  height: number,
): Promise<void> {
  await invoke("window_resize_browser", { x, y, width, height });
}

export async function windowNavigateBrowser(url: string): Promise<void> {
  await invoke("window_navigate_browser", { url });
}

export async function windowCloseBrowser(): Promise<void> {
  await invoke("window_close_browser");
}

export async function revealInExplorer(path: string): Promise<void> {
  await invoke("reveal_in_explorer", { path });
}

export async function getUserHomeDir(): Promise<string> {
  return invoke<string>("get_user_home_dir");
}

export interface FileEntry {
  name: string;
  path: string;
  isDir: boolean;
  size: number;
}

export async function readDirectory(path: string): Promise<FileEntry[]> {
  return invoke<FileEntry[]>("read_directory", { path });
}

export interface FileAttachResult {
  name: string;
  mimeType: string;
  dataUrl: string;
  size: number;
  sourcePath: string;
  truncated: boolean;
}

export async function readFileForAttach(path: string): Promise<FileAttachResult> {
  return invoke<FileAttachResult>("read_file_for_attach", { path });
}

export interface TextFilePreviewResult {
  name: string;
  path: string;
  mimeType: string;
  content: string;
  size: number;
  truncated: boolean;
}

export async function readTextFilePreview(path: string): Promise<TextFilePreviewResult> {
  return invoke<TextFilePreviewResult>("read_text_file_preview", { path });
}
