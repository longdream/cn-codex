import { invoke } from "@tauri-apps/api/core";
import type {
  PluginDetail,
  PluginImportResult,
  PluginSummary,
  PluginUninstallResult,
} from "../types/plugin";

export async function pluginList(): Promise<PluginSummary[]> {
  return invoke("plugin_list");
}

export async function pluginRead(pluginId: string): Promise<PluginDetail> {
  return invoke("plugin_read", { pluginId });
}

export async function pluginImportCodexCache(sourceDir?: string): Promise<PluginImportResult> {
  const trimmed = sourceDir?.trim();
  return invoke("plugin_import_codex_cache", {
    sourceDir: trimmed ? trimmed : null,
  });
}

export async function pluginSetEnabled(
  pluginId: string,
  enabled: boolean,
): Promise<PluginSummary> {
  return invoke("plugin_set_enabled", { pluginId, enabled });
}

export async function pluginUninstall(pluginId: string): Promise<PluginUninstallResult> {
  return invoke("plugin_uninstall", { pluginId });
}
