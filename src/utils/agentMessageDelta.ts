export type AgentTurnPhase = "created" | "sampling" | "toolRunning" | "completed";

export function shouldAcceptAgentMessageDelta(
  isStreaming: boolean,
  turnPhase?: AgentTurnPhase,
): boolean {
  return isStreaming && turnPhase !== "completed";
}
