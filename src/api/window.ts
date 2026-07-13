import { invoke } from "@tauri-apps/api/core";

export interface BrowserWindowInfo {
  label: string;
  url: string;
  created: boolean;
  debugPort: number;
  cdpEndpoint: string;
}

export interface BrowserEditContext {
  editable: boolean;
  currentUrl: string;
  sourcePath?: string | null;
  reason?: string | null;
  livePreviewMode: string;
}

export interface BrowserPickedElement {
  selector: string;
  selectorCandidates: string[];
  tagName: string;
  text: string;
  url: string;
  x: number;
  y: number;
  width: number;
  height: number;
  pickedAt: number;
  sourcePath?: string | null;
}

export interface BrowserDomEditRequest {
  selector: string;
  text?: string | null;
  color?: string | null;
  backgroundColor?: string | null;
  fontSize?: string | null;
  fontWeight?: string | null;
  lineHeight?: string | null;
  margin?: string | null;
  padding?: string | null;
}

export interface BrowserDomEditResult {
  selector: string;
  currentUrl: string;
  sourcePath?: string | null;
  previewHtml: string;
  livePreview: boolean;
}

export interface BrowserNavigationState {
  url: string;
  title: string;
  canGoBack: boolean;
  canGoForward: boolean;
}

export interface DocumentDetailWindowInfo {
  label: string;
  path: string;
  created: boolean;
}

export interface RunSummaryDiffPayload {
  path: string;
  beforeContent: string;
  afterContent: string;
  fileAction: string;
  diffSource: "snapshot" | "patch" | "empty";
  canPersist: boolean;
  persistHint?: string;
  emptyHint?: string;
}

export interface RunSummaryDiffWindowInfo {
  label: string;
  path: string;
  created: boolean;
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
  workspaceRoot?: string,
): Promise<BrowserWindowInfo> {
  const trimmed = url?.trim();
  const payload: Record<string, unknown> = {
    url: trimmed ? trimmed : null,
    x: rect?.x ?? null,
    y: rect?.y ?? null,
    width: rect?.width ?? null,
    height: rect?.height ?? null,
  };
  if (workspaceRoot !== undefined) {
    const trimmedRoot = workspaceRoot.trim();
    payload.workspaceRoot = trimmedRoot ? trimmedRoot : null;
  }
  return invoke<BrowserWindowInfo>("window_open_browser", payload);
}

export async function windowResizeBrowser(
  x: number,
  y: number,
  width: number,
  height: number,
): Promise<void> {
  await invoke("window_resize_browser", { x, y, width, height });
}

export async function windowNavigateBrowser(
  url: string,
  workspaceRoot?: string,
): Promise<void> {
  const payload: Record<string, unknown> = { url };
  if (workspaceRoot !== undefined) {
    const trimmedRoot = workspaceRoot.trim();
    payload.workspaceRoot = trimmedRoot ? trimmedRoot : null;
  }
  await invoke("window_navigate_browser", payload);
}

export async function windowCloseBrowser(): Promise<void> {
  await invoke("window_close_browser");
}

export async function windowDetachBrowser(
  size?: { width: number; height: number },
): Promise<BrowserWindowInfo> {
  return invoke<BrowserWindowInfo>("window_detach_browser", {
    width: size?.width ?? null,
    height: size?.height ?? null,
  });
}

export async function windowAttachBrowser(
  url?: string,
  rect?: { x: number; y: number; width: number; height: number },
  workspaceRoot?: string,
): Promise<BrowserWindowInfo> {
  const trimmed = url?.trim();
  const payload: Record<string, unknown> = {
    url: trimmed ? trimmed : null,
    x: rect?.x ?? null,
    y: rect?.y ?? null,
    width: rect?.width ?? null,
    height: rect?.height ?? null,
  };
  if (workspaceRoot !== undefined) {
    const trimmedRoot = workspaceRoot.trim();
    payload.workspaceRoot = trimmedRoot ? trimmedRoot : null;
  }
  return invoke<BrowserWindowInfo>("window_attach_browser", payload);
}

export async function browserGetEditContext(): Promise<BrowserEditContext> {
  return invoke<BrowserEditContext>("browser_get_edit_context");
}

export async function browserStartPickMode(): Promise<BrowserEditContext> {
  return invoke<BrowserEditContext>("browser_start_pick_mode");
}

export async function browserStopPickMode(): Promise<void> {
  await invoke("browser_stop_pick_mode");
}

export async function browserPollPickedElement(): Promise<BrowserPickedElement | null> {
  return invoke<BrowserPickedElement | null>("browser_poll_picked_element");
}

export async function browserApplyDomEdit(
  request: BrowserDomEditRequest,
): Promise<BrowserDomEditResult> {
  return invoke<BrowserDomEditResult>("browser_apply_dom_edit", { request });
}

export async function browserRefreshPreview(): Promise<string> {
  return invoke<string>("browser_refresh_preview");
}

export async function browserGoBack(): Promise<BrowserNavigationState> {
  return invoke<BrowserNavigationState>("browser_go_back");
}

export async function browserGoForward(): Promise<BrowserNavigationState> {
  return invoke<BrowserNavigationState>("browser_go_forward");
}

export async function browserNavigateHome(): Promise<BrowserNavigationState> {
  return invoke<BrowserNavigationState>("browser_navigate_home");
}

export async function browserGetNavigationState(): Promise<BrowserNavigationState> {
  return invoke<BrowserNavigationState>("browser_get_navigation_state");
}

export async function windowOpenDocumentDetail(
  path: string,
  workspaceRoot?: string,
  line?: number | null,
): Promise<DocumentDetailWindowInfo> {
  return invoke<DocumentDetailWindowInfo>("window_open_document_detail", {
    path,
    workspaceRoot: workspaceRoot ?? null,
    line: typeof line === "number" && Number.isFinite(line) && line > 0
      ? Math.floor(line)
      : null,
  });
}

export async function windowCloseDocumentDetail(): Promise<void> {
  await invoke("window_close_document_detail");
}

export async function windowGetDocumentDetailPath(): Promise<string | null> {
  return invoke<string | null>("window_get_document_detail_path");
}

export async function windowGetDocumentDetailLine(): Promise<number | null> {
  return invoke<number | null>("window_get_document_detail_line");
}

export async function windowOpenRunSummaryDiff(
  payload: RunSummaryDiffPayload,
): Promise<RunSummaryDiffWindowInfo> {
  return invoke<RunSummaryDiffWindowInfo>("window_open_runsummary_diff", { payload });
}

export async function windowCloseRunSummaryDiff(): Promise<void> {
  await invoke("window_close_runsummary_diff");
}

export async function windowGetRunSummaryDiffPayload(): Promise<RunSummaryDiffPayload | null> {
  return invoke<RunSummaryDiffPayload | null>("window_get_runsummary_diff_payload");
}

export async function documentDetailInsertSnippet(snippet: string): Promise<void> {
  await invoke("document_detail_insert_snippet", { snippet });
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

export interface WorkspaceSearchMatch {
  path: string;
  name: string;
  relativePath: string;
  kind: "name" | "content" | string;
  line?: number | null;
  preview?: string | null;
  isDir: boolean;
}

export interface WorkspaceSearchResult {
  query: string;
  matches: WorkspaceSearchMatch[];
  truncated: boolean;
  searchedFiles: number;
}

export async function searchWorkspaceFiles(
  root: string,
  query: string,
  maxResults?: number,
  options?: {
    caseSensitive?: boolean;
    include?: string;
  },
): Promise<WorkspaceSearchResult> {
  return invoke<WorkspaceSearchResult>("search_workspace_files", {
    root,
    query,
    maxResults: maxResults ?? null,
    caseSensitive: options?.caseSensitive ?? false,
    include: options?.include?.trim() ? options.include.trim() : null,
  });
}

export async function deletePath(path: string, recursive = false): Promise<void> {
  await invoke("delete_path", { path, recursive });
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

export interface TextFileWriteResult {
  path: string;
  size: number;
}

export async function writeTextFilePreview(
  path: string,
  content: string,
): Promise<TextFileWriteResult> {
  return invoke<TextFileWriteResult>("write_text_file_preview", { path, content });
}
