import { describe, expect, it } from "vitest";
import { shouldAcceptAgentMessageDelta } from "../utils/agentMessageDelta";

describe("agent message delta lifecycle", () => {
  it("keeps accepting a new turn after an earlier goal completed", () => {
    expect(shouldAcceptAgentMessageDelta(true, "sampling")).toBe(true);
    expect(shouldAcceptAgentMessageDelta(true, "toolRunning")).toBe(true);
  });

  it("rejects deltas after the current turn has completed", () => {
    expect(shouldAcceptAgentMessageDelta(false, "sampling")).toBe(false);
    expect(shouldAcceptAgentMessageDelta(true, "completed")).toBe(false);
  });
});
