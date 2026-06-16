export interface ConfigReadParams {
  includeLayers?: boolean;
  cwd?: string;
}

export interface ConfigReadResponse {
  config: Record<string, unknown>;
  layers?: ConfigLayer[];
}

export interface ConfigLayer {
  source: string;
  values: Record<string, unknown>;
}

export interface ConfigValueWriteParams {
  keyPath: string;
  value: unknown;
  mergeStrategy?: "replace" | "merge";
  filePath?: string;
}

export interface ConfigWriteResponse {
  status: string;
  version?: number;
  filePath?: string;
}

export interface ConfigBatchWriteParams {
  edits: ConfigEdit[];
  filePath?: string;
}

export interface ConfigEdit {
  keyPath: string;
  value: unknown;
  mergeStrategy?: "replace" | "merge";
}
