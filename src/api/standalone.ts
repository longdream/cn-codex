import { invoke } from "@tauri-apps/api/core";
import type { AttachedFile } from "../types/provider";

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
  }>;
}> {
  return invoke("standalone_thread_list");
}

export async function threadArchive(threadId: string): Promise<{ status: string }> {
  return invoke("thread_archive", { params: { threadId } });
}

export async function standaloneThreadRead(threadId: string): Promise<{
  thread: {
    id: string;
    name?: string;
    goal?: ThreadGoal | null;
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
      mode?: "chat" | "goal";
      durationMs?: number;
      changedFiles?: Array<{ path: string; action: string }>;
      usage?: {
        promptTokens: number;
        completionTokens: number;
        totalTokens: number;
        callCount?: number;
      };
      goalBudgetTokens?: number;
      budgetLimited?: boolean;
    }>;
  };
}> {
  return invoke("standalone_thread_read", { threadId });
}

export type ThreadGoalStatus =
  | "active"
  | "paused"
  | "blocked"
  | "usageLimited"
  | "budgetLimited"
  | "complete";

export interface ThreadGoal {
  objective: string;
  status: ThreadGoalStatus;
  tokenBudget?: number | null;
  tokensUsed?: number;
  createdAt?: number;
  updatedAt?: number;
}

export async function standaloneThreadGoalSet(
  threadId: string,
  objective: string,
  status?: ThreadGoalStatus,
  goalBudgetTokens?: number,
): Promise<{ goal: ThreadGoal }> {
  return invoke("standalone_thread_goal_set", {
    threadId,
    objective,
    status,
    goalBudgetTokens,
  });
}

export async function standaloneThreadGoalStatus(
  threadId: string,
  status: ThreadGoalStatus,
): Promise<{ goal: ThreadGoal }> {
  return invoke("standalone_thread_goal_status", {
    threadId,
    status,
  });
}

export async function standaloneThreadGoalEdit(
  threadId: string,
  objective: string,
  goalBudgetTokens?: number,
): Promise<{ goal: ThreadGoal }> {
  return invoke("standalone_thread_goal_edit", {
    threadId,
    objective,
    goalBudgetTokens,
  });
}

export async function standaloneThreadGoalClear(
  threadId: string,
): Promise<{ goal: null }> {
  return invoke("standalone_thread_goal_clear", { threadId });
}

export async function standaloneChat(
  threadId: string,
  message: string,
  cwd?: string,
  mode?: "chat" | "goal" | "robot-create" | "robot-modify",
  attachments?: AttachedFile[],
  goalBudgetTokens?: number,
  robotId?: string,
): Promise<{ status: string }> {
  return invoke("standalone_chat", {
    threadId,
    message,
    cwd,
    mode,
    attachments,
    goalBudgetTokens,
    robotId,
  });
}

export async function standaloneTurnInterrupt(): Promise<{ status: string }> {
  return invoke("standalone_turn_interrupt");
}
