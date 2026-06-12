import { invoke } from "@tauri-apps/api/core";

export interface NpmToolInfo {
  id: string;
  package: string;
  displayName: string;
  description: string;
  homepage: string;
  installed: boolean;
  installedVersion: string | null;
  nodeAvailable: boolean;
  nodeVersion: string | null;
  missingDeps: string[];
}

export interface NpmToolResult {
  success: boolean;
  output: string;
}

export async function npmToolList(): Promise<NpmToolInfo[]> {
  return invoke("npm_tool_list");
}

export async function npmToolCheck(toolId: string): Promise<NpmToolInfo> {
  return invoke("npm_tool_check", { toolId });
}

export async function npmToolInstall(toolId: string): Promise<NpmToolResult> {
  return invoke("npm_tool_install", { toolId });
}

export async function npmToolUninstall(toolId: string): Promise<NpmToolResult> {
  return invoke("npm_tool_uninstall", { toolId });
}
