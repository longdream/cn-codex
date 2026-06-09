import type { SkillSummary } from "./skill";

export interface PluginInterfaceSummary {
  displayName?: string | null;
  shortDescription?: string | null;
  developerName?: string | null;
  category?: string | null;
  capabilities: string[];
  brandColor?: string | null;
}

export interface PluginSummary {
  id: string;
  name: string;
  displayName: string;
  version?: string | null;
  description?: string | null;
  keywords: string[];
  path: string;
  manifestPath: string;
  skillsDir?: string | null;
  skillsCount: number;
  skills: SkillSummary[];
  appsCount: number;
  apps: PluginAppSummary[];
  enabled: boolean;
  hasMcpServers: boolean;
  hasApps: boolean;
  hasHooks: boolean;
  interface?: PluginInterfaceSummary | null;
  error?: string | null;
  warnings: string[];
}

export interface PluginAppSummary {
  key: string;
  connectorId: string;
  path: string;
}

export interface PluginDetail extends PluginSummary {
  manifest?: unknown;
}

export interface PluginImportItem {
  name: string;
  version?: string | null;
  source: string;
  destination: string;
  updated: boolean;
}

export interface PluginImportError {
  source: string;
  error: string;
}

export interface PluginImportResult {
  sourceDir: string;
  destinationDir: string;
  imported: PluginImportItem[];
  errors: PluginImportError[];
}

export interface PluginUninstallResult {
  pluginId: string;
  removedPath: string;
}
