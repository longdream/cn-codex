export interface Thread {
  id: string;
  sessionId: string;
  forkedFromId?: string;
  parentThreadId?: string;
  preview: string;
  name?: string;
  status: ThreadStatus;
  createdAt: number;
  updatedAt: number;
  archived: boolean;
}

export type ThreadStatus = "idle" | "active" | "loading" | "error";

export interface ThreadStartParams {
  model?: string;
  modelProvider?: string;
  cwd?: string;
  approvalPolicy?: ApprovalPolicy;
  instructions?: string;
}

export interface ThreadStartResponse {
  thread: Thread;
  model: string;
  modelProvider: string;
  serviceTier?: string;
  cwd: string;
}

export interface ThreadResumeParams {
  threadId: string;
}

export interface ThreadResumeResponse {
  thread: Thread;
}

export interface ThreadListParams {
  cursor?: string;
  limit?: number;
  archived?: boolean;
}

export interface ThreadListResponse {
  data: Thread[];
  nextCursor?: string;
}

export interface ThreadReadParams {
  threadId: string;
  includeTurns?: boolean;
}

export interface ThreadReadResponse {
  thread: Thread;
  turns?: Turn[];
}

export interface ThreadArchiveParams {
  threadId: string;
}

export interface ThreadArchiveResponse {}

export interface ThreadUnarchiveParams {
  threadId: string;
}

export interface ThreadUnarchiveResponse {
  thread: Thread;
}

export interface ThreadSetNameParams {
  threadId: string;
  name: string;
}

export interface ThreadSetNameResponse {}

export interface ThreadRollbackParams {
  threadId: string;
  dropCount: number;
}

export interface ThreadRollbackResponse {
  thread: Thread;
}

export interface ThreadUnsubscribeParams {
  threadId: string;
}

export interface ThreadUnsubscribeResponse {}

export type ApprovalPolicy =
  | "untrusted"
  | "on-failure"
  | "on-request"
  | "never";

import type { Turn } from "./turn";
