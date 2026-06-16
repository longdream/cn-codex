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
  inputTokens: number;
  outputTokens: number;
  totalTokens: number;
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
