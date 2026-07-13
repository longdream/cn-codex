import { invoke } from "@tauri-apps/api/core";

export interface UpdateCheckResult {
  updateAvailable: boolean;
  currentVersion: string;
  latestVersion: string;
  url: string;
  sha256: string;
  notes: string;
  force: boolean;
  publishedAt?: string;
  message?: string;
}

export interface UpdateStartResult {
  started: boolean;
  message: string;
}

export async function updateCheck(): Promise<UpdateCheckResult> {
  return invoke<UpdateCheckResult>("update_check");
}

export async function updateStart(
  url: string,
  sha256?: string,
): Promise<UpdateStartResult> {
  return invoke<UpdateStartResult>("update_start", {
    url,
    sha256: sha256?.trim() ? sha256 : null,
  });
}
