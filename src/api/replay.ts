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
  /** Whether a `<id>.input.json` sidecar exists (show the JSON badge). */
  hasInputDocument?: boolean;
  /** Number of fields inside the input document, when present. */
  inputFieldCount?: number | null;
  /** True for CSV-imported documents without a generated `.py` sibling. */
  importedOnly?: boolean;
}

/** One input slot inside a replay input document. */
export interface ReplayInputField {
  type: string;
  step: number;
  label: string;
  selector: string;
  selectorCandidates?: string[];
  value: string;
}

/** Input document: the script reads this JSON to know what to type. */
export interface ReplayInputDocument {
  id: string;
  name?: string;
  fields: ReplayInputField[];
  createdAt?: number;
  updatedAt?: number;
}

export interface ReplayCsvImportPreview {
  suggestedName?: string | null;
  fieldCount: number;
  fields: ReplayInputField[];
  headerDetected: boolean;
}

export interface ReplayCsvSaveResult {
  id: string;
  path: string;
  fieldCount: number;
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

export async function replayListScripts(): Promise<ReplayScriptMeta[]> {
  return invoke<ReplayScriptMeta[]>("replay_list_scripts");
}

export async function replayReadScript(id: string): Promise<ReplayReadResult> {
  return invoke<ReplayReadResult>("replay_read_script", { id });
}

export async function replayReadInputDocument(
  id: string,
): Promise<ReplayReadResult> {
  return invoke<ReplayReadResult>("replay_read_input_document", { id });
}

export async function replaySaveInputDocument(
  id: string,
  document: ReplayInputDocument,
): Promise<string> {
  return invoke<string>("replay_save_input_document", { id, document });
}

export async function replayParseCsv(
  csvContent: string,
): Promise<ReplayCsvImportPreview> {
  return invoke<ReplayCsvImportPreview>("replay_parse_csv", { csvContent });
}

export async function replaySaveCsv(
  stem: string,
  csvContent: string,
): Promise<ReplayCsvSaveResult> {
  return invoke<ReplayCsvSaveResult>("replay_save_csv", { stem, csvContent });
}

export async function replayDeleteInputDocument(id: string): Promise<void> {
  return invoke("replay_delete_input_document", { id });
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
