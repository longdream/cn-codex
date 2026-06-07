import { invoke } from "@tauri-apps/api/core";

export interface ServerStatus {
  initialized: boolean;
  currentThreadId: string | null;
  cwd: string;
  locale: string;
  configDir: string;
  configPath: string;
}

export async function getServerStatus(): Promise<ServerStatus> {
  return invoke("get_server_status");
}

export async function standaloneInit(): Promise<string> {
  return invoke("standalone_init");
}

export async function standaloneConfigRead(): Promise<{
  config: Record<string, unknown>;
  filePath: string;
}> {
  return invoke("standalone_config_read");
}

export async function standaloneConfigWrite(
  edits: Array<{ keyPath: string; value: unknown; mergeStrategy?: string }>,
): Promise<{ status: string; filePath: string }> {
  return invoke("standalone_config_write", { edits });
}

export async function standaloneThreadCreate(): Promise<{
  thread: { id: string };
}> {
  return invoke("standalone_thread_create");
}

export async function standaloneThreadList(): Promise<{
  data: Array<{
    id: string;
    name?: string;
    preview?: string;
    updatedAt?: number;
    archived?: boolean;
  }>;
}> {
  return invoke("standalone_thread_list");
}

export async function standaloneThreadRead(threadId: string): Promise<{
  thread: {
    id: string;
    name?: string;
    turns?: Array<{
      id: string;
      items?: Array<{
        type: string;
        id?: string;
        text?: string;
        content?: Array<{ type?: string; text?: string }>;
      }>;
      startedAt?: number;
      completedAt?: number;
    }>;
  };
}> {
  return invoke("standalone_thread_read", { threadId });
}

export async function standaloneChat(
  threadId: string,
  message: string,
  cwd?: string,
): Promise<{ status: string }> {
  return invoke("standalone_chat", { threadId, message, cwd });
}
