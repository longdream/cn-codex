import { invoke } from "@tauri-apps/api/core";

export interface LaunchBrowserResult {
  cdpEndpoint: string;
}

export interface RecordingStartResult {
  sessionId: string;
}

export interface RecordingStatusResult {
  status: "idle" | "recording" | "processing";
  browserRunning: boolean;
}

export interface RecordingEvent {
  type: string;
  timestamp: number;
  url: string;
  selector: string;
  selectorCandidates: string[];
  tagName: string;
  value?: string;
  screenshot?: string;
  /** navigate: user | redirect | reload | link | form */
  cause?: string;
  navigationReason?: string;
  key?: string;
  inputType?: string;
  previousValue?: string;
  modifiers?: string;
  data?: string;
}

export interface TraceFile {
  sessionId: string;
  sessionName: string;
  startUrl: string;
  startedAt: string;
  stoppedAt: string;
  events: RecordingEvent[];
}

export interface TraceListEntry {
  sessionId: string;
  sessionName: string;
  startUrl: string;
  startedAt: string;
  eventCount: number;
  path: string;
}

export async function launchBrowser(
  browserPath?: string,
  cdpPort?: number,
): Promise<LaunchBrowserResult> {
  return invoke<LaunchBrowserResult>("launch_browser", {
    browserPath,
    cdpPort,
  });
}

export async function closeExternalBrowser(): Promise<void> {
  await invoke("close_external_browser");
}

export async function recordingStart(
  sessionName?: string,
): Promise<RecordingStartResult> {
  return invoke<RecordingStartResult>("recording_start", { sessionName });
}

export async function recordingStop(): Promise<TraceFile> {
  return invoke<TraceFile>("recording_stop");
}

export async function recordingStatus(): Promise<RecordingStatusResult> {
  return invoke<RecordingStatusResult>("recording_status");
}

export async function recordingShowToggle(visible: boolean): Promise<void> {
  await invoke("recording_show_toggle", { visible });
}

export async function recordingListTraces(): Promise<TraceListEntry[]> {
  return invoke<TraceListEntry[]>("recording_list_traces");
}

export async function recordingReadTrace(
  sessionId: string,
): Promise<TraceFile> {
  return invoke<TraceFile>("recording_read_trace", { sessionId });
}
