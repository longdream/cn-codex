export interface UserInput {
  type: "text" | "image";
  text?: string;
  imageUrl?: string;
}

export interface TurnStartParams {
  threadId: string;
  input: UserInput[];
  model?: string;
  effort?: string;
}

export interface TurnStartResponse {
  turnId: string;
}

export interface TurnSteerParams {
  threadId: string;
  expectedTurnId?: string;
  input: UserInput[];
}

export interface TurnSteerResponse {}

export interface TurnInterruptParams {
  threadId: string;
  turnId: string;
}

export interface TurnInterruptResponse {}

export interface Turn {
  id: string;
  threadId: string;
  status: TurnStatus;
  createdAt: number;
  completedAt?: number;
}

export type TurnStatus = "in_progress" | "completed" | "interrupted" | "failed";
