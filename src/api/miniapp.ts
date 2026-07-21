import { invoke } from "@tauri-apps/api/core";

export type MiniAppStatus = "draft" | "generated" | "running" | "stopped" | "error";

export interface MiniAppPage {
  id: string;
  title: string;
  path: string;
  description?: string;
}

export interface MiniAppToolMeta {
  name: string;
  description?: string;
}

export interface MiniAppRecord {
  id: string;
  name: string;
  slug: string;
  description: string;
  databaseId: string;
  databaseName: string;
  status: MiniAppStatus;
  port?: number | null;
  rootPath: string;
  pages: MiniAppPage[];
  tools: MiniAppToolMeta[];
  lastError?: string;
  createdAt: number;
  updatedAt: number;
}

export interface MiniAppCreateArgs {
  name: string;
  slug: string;
  description?: string;
  databaseId: string;
  databaseName?: string;
}

export interface MiniAppOpenPageResult {
  ok: boolean;
  url: string;
  slug: string;
  name: string;
  port?: number | null;
  pageId?: string | null;
}

export async function miniappList(): Promise<MiniAppRecord[]> {
  return invoke<MiniAppRecord[]>("miniapp_list");
}

export async function miniappCreate(args: MiniAppCreateArgs): Promise<MiniAppRecord> {
  return invoke<MiniAppRecord>("miniapp_create", { args });
}

export async function miniappStart(idOrSlug: string): Promise<MiniAppRecord> {
  return invoke<MiniAppRecord>("miniapp_start", { idOrSlug });
}

export async function miniappStop(idOrSlug: string): Promise<MiniAppRecord> {
  return invoke<MiniAppRecord>("miniapp_stop", { idOrSlug });
}

export async function miniappDelete(idOrSlug: string): Promise<void> {
  return invoke("miniapp_delete", { idOrSlug });
}

export async function miniappOpenPage(
  idOrSlug: string,
  pageId?: string,
): Promise<MiniAppOpenPageResult> {
  return invoke<MiniAppOpenPageResult>("miniapp_open_page", { idOrSlug, pageId });
}
