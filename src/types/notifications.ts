export interface AgentMessageDelta {
  threadId: string;
  turnId: string;
  itemId: string;
  delta: string;
}

export interface TurnStartedNotification {
  threadId: string;
  turn: { id: string };
}

export interface TurnCompletedNotification {
  threadId: string;
  turn: { id: string };
}

export interface ItemStartedNotification {
  threadId: string;
  turnId: string;
  itemId: string;
  type: string;
}

export interface ItemCompletedNotification {
  threadId: string;
  turnId: string;
  itemId: string;
  type: string;
}

export interface CommandOutputDelta {
  threadId: string;
  turnId: string;
  itemId: string;
  delta: string;
}

export interface FileChangeOutputDelta {
  threadId: string;
  turnId: string;
  itemId: string;
  delta: string;
}

export interface PatchProgressChange {
  path: string;
  action: string;
  moveTo?: string;
}

export interface FileChangePatchUpdated {
  threadId?: string;
  turnId?: string;
  itemId?: string;
  callId?: string;
  path?: string;
  patch?: string;
  changes?: PatchProgressChange[];
}

export interface FileReviewReady {
  threadId?: string;
  itemId?: string;
  callId?: string;
  fileCount?: number;
}

export interface FileReviewUpdated {
  threadId?: string;
  callId?: string;
  status?: "updated" | "applied" | "cancelled" | "failed";
  message?: string;
  changedFiles?: Array<{ path: string; action: string }>;
}

export interface DocumentDetailInsertSnippet {
  snippet?: string;
}

export interface FortuneDetailStartedNotification {
  requestId: string;
}

export interface FortuneDetailDeltaNotification {
  requestId: string;
  delta: string;
}

export interface FortuneDetailCompletedNotification {
  requestId: string;
  text: string;
  finishReason?: string;
}

export interface FortuneDetailErrorNotification {
  requestId: string;
  message: string;
}

export interface ThreadStartedNotification {
  thread: {
    id: string;
    name?: string | null;
    preview?: string | null;
    archived?: boolean | null;
    updatedAt?: number | null;
  };
}

export interface ThreadStatusChangedNotification {
  threadId: string;
  status: string;
}

export interface ThreadNameUpdatedNotification {
  threadId: string;
  threadName?: string;
}

export interface ThreadTokenUsageUpdatedNotification {
  threadId: string;
  usage?: {
    promptTokens: number;
    completionTokens: number;
    totalTokens: number;
    cachedTokens?: number;
    cacheCreationTokens?: number;
    reasoningTokens?: number;
    callCount?: number;
    lastSinglePromptTokens?: number;
    contextWindowTokens?: number;
  };
  inputTokens?: number;
  outputTokens?: number;
  totalTokens?: number;
  cachedTokens?: number;
  cacheCreationTokens?: number;
  reasoningTokens?: number;
  callCount?: number;
  lastSinglePromptTokens?: number;
  contextPromptTokens?: number;
  modelContextWindow?: number;
}

export interface AccountUpdatedNotification {
  account: import("./account").Account;
}

export interface ErrorNotification {
  message: string;
  code?: string;
}

export interface ServerRequestEvent {
  requestId: import("./approval").RequestId;
  method: string;
  params: Record<string, unknown>;
}

export type ServerEventName =
  | "agent-message-delta"
  | "turn-started"
  | "turn-completed"
  | "turn-diff-updated"
  | "turn-plan-updated"
  | "item-started"
  | "item-completed"
  | "command-output-delta"
  | "file-change-output-delta"
  | "file-change-patch-updated"
  | "file-review-ready"
  | "file-review-updated"
  | "document-detail-insert-snippet"
  | "fortune-detail-started"
  | "fortune-detail-delta"
  | "fortune-detail-completed"
  | "fortune-detail-error"
  | "reasoning-text-delta"
  | "reasoning-summary-delta"
  | "plan-delta"
  | "hook-started"
  | "hook-completed"
  | "thread-started"
  | "thread-status-changed"
  | "thread-name-updated"
  | "thread-settings-updated"
  | "thread-goal-updated"
  | "thread-goal-cleared"
  | "thread-token-usage-updated"
  | "thread-archived"
  | "thread-unarchived"
  | "thread-closed"
  | "context-compacted"
  | "guardian-review-started"
  | "guardian-review-completed"
  | "account-updated"
  | "account-rate-limits-updated"
  | "account-login-completed"
  | "model-rerouted"
  | "model-verification"
  | "mcp-tool-call-progress"
  | "mcp-server-status-updated"
  | "server-error"
  | "server-warning"
  | "config-warning"
  | "skills-changed"
  | "server-request-resolved"
  | "server-request"
  | "events-lagged"
  | "server-notification";
