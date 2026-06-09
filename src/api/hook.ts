import { invoke } from "@tauri-apps/api/core";
import type { HookListItem } from "../types/hook";

export async function hookList(): Promise<HookListItem[]> {
  return invoke("hook_list");
}
