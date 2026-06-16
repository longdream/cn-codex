import { invoke } from "@tauri-apps/api/core";

export interface WpsDocumentInfo {
  name: string;
  path?: string;
  saved?: boolean;
}

export interface WpsConnectionStatus {
  connected: boolean;
  addinName?: string;
  addinVersion?: string;
  wpsVersion?: string;
  activeDocument?: WpsDocumentInfo;
}

export interface WpsServerStatus {
  running: boolean;
  port: number;
  connections: WpsConnectionStatus[];
}

export async function wpsStartServer(port?: number): Promise<number> {
  return invoke<number>("wps_start_server", { port: port ?? null });
}

export async function wpsStopServer(): Promise<void> {
  return invoke<void>("wps_stop_server");
}

export async function wpsStatus(): Promise<WpsServerStatus> {
  return invoke<WpsServerStatus>("wps_status");
}

export async function wpsExecute(
  method: string,
  params?: Record<string, unknown>,
): Promise<unknown> {
  return invoke<unknown>("wps_execute", { method, params: params ?? null });
}
