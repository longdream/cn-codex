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
  startUrl: string;
  lastStatus?: ReplayLastStatus | null;
  lastError?: string | null;
  lastRunAt?: number | null;
}

export interface ReplayRunResult {
  ok: boolean;
  exitCode?: number | null;
  stdout: string;
  stderr: string;
  durationMs: number;
  error?: string | null;
}

export interface ReplayReadResult {
  id: string;
  path: string;
  content: string;
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

export async function replayGetDir(): Promise<string> {
  return invoke<string>("replay_get_dir");
}
