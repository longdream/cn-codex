import { invoke } from "@tauri-apps/api/core";

export async function appStateGet(key: string): Promise<string | null> {
  return invoke("app_state_get", { key });
}

export async function appStateSet(key: string, value: string): Promise<void> {
  return invoke("app_state_set", { key, value });
}

export async function appStateDelete(key: string): Promise<void> {
  return invoke("app_state_delete", { key });
}

export async function appStateGetAll(): Promise<Record<string, string>> {
  return invoke("app_state_get_all");
}
