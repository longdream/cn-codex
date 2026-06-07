export type ApprovalType =
  | "commandExecution"
  | "fileChange"
  | "permissions"
  | "toolInput"
  | "mcpElicitation";

export interface CommandExecutionApprovalParams {
  threadId: string;
  turnId: string;
  itemId: string;
  command: string[];
  cwd: string;
}

export interface FileChangeApprovalParams {
  threadId: string;
  turnId: string;
  itemId: string;
  path: string;
  patch: string;
}

export type CommandApprovalDecision =
  | "accept"
  | "acceptForSession"
  | "decline"
  | "cancel";

export type FileChangeApprovalDecision =
  | "accept"
  | "acceptForSession"
  | "decline"
  | "cancel";

export type RequestId = string | number;

export interface ApprovalRequest {
  requestId: RequestId;
  type: ApprovalType;
  params: CommandExecutionApprovalParams | FileChangeApprovalParams | Record<string, unknown>;
}
