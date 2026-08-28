import { invoke } from "@tauri-apps/api/core";

export type ReplayLastStatus = "success" | "failed";

export interface ReplayScriptMeta {
  id: string;
  name: string;
  path: string;
  traceSessionId: string;
  createdAt: number;
  updatedAt: number;
  stepCount: number;
  steps?: string[];
  startUrl: string;
  lastStatus?: ReplayLastStatus | null;
  lastError?: string | null;
  lastRunAt?: number | null;
  reportCount?: number;
  lastReportAt?: number | null;
}

export interface ReplayRunResult {
  ok: boolean;
  exitCode?: number | null;
  stdout: string;
  stderr: string;
  durationMs: number;
  error?: string | null;
  /** When false, do not send this result to the main pipeline for auto-fix. */
  fixable?: boolean;
}

export interface ReplayReadResult {
  id: string;
  path: string;
  content: string;
}

export interface ReplayReportMeta {
  id: string;
  scriptId: string;
  path: string;
  createdAt: number;
  ok: boolean;
  summary: string;
}

export async function replayListScripts(): Promise<ReplayScriptMeta[]> {
  return invoke<ReplayScriptMeta[]>("replay_list_scripts");
}

export async function replayReadScript(id: string): Promise<ReplayReadResult> {
  return invoke<ReplayReadResult>("replay_read_script", { id });
}

export async function replayRunScript(id: string): Promise<ReplayRunResult> {
  return invoke<ReplayRunResult>("replay_run_script", { id });
}

export async function replayStopScript(id: string): Promise<boolean> {
  return invoke<boolean>("replay_stop_script", { id });
}

export async function replayDeleteScript(id: string): Promise<void> {
  return invoke("replay_delete_script", { id });
}

export async function replayRenameScript(id: string, name: string): Promise<string> {
  return invoke<string>("replay_rename_script", { id, name });
}

export async function replayGetDir(): Promise<string> {
  return invoke<string>("replay_get_dir");
}

export async function replayListReports(id: string): Promise<ReplayReportMeta[]> {
  return invoke<ReplayReportMeta[]>("replay_list_reports", { id });
}

export async function replayReadReport(id: string, reportId: string): Promise<ReplayReadResult> {
  return invoke<ReplayReadResult>("replay_read_report", { id, reportId });
}
