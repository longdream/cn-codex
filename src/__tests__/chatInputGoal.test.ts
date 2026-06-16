import { describe, expect, it } from "vitest";

import { buildGoalEditCommand, parseGoalCommand } from "../components/chat/ChatInput";
import { formatGoalSummaryMessage } from "../components/chat/ChatPage";

describe("parseGoalCommand", () => {
  it("parses token budgets with compact suffixes", () => {
    expect(parseGoalCommand("/goal --tokens 98.5K improve benchmark coverage")).toEqual({
      action: "set",
      objective: "improve benchmark coverage",
      goalBudgetTokens: 98_500,
    });
    expect(parseGoalCommand("/goal --tokens=2M migrate plugins")).toEqual({
      action: "set",
      objective: "migrate plugins",
      goalBudgetTokens: 2_000_000,
    });
  });

  it("keeps plain goal text unchanged", () => {
    expect(parseGoalCommand("/goal fix the window controls")).toEqual({
      action: "set",
      objective: "fix the window controls",
    });
  });

  it("parses lifecycle controls", () => {
    expect(parseGoalCommand("/goal")).toEqual({ action: "show", objective: "" });
    expect(parseGoalCommand("/goal pause")).toEqual({ action: "pause", objective: "" });
    expect(parseGoalCommand("/goal resume")).toEqual({ action: "resume", objective: "" });
    expect(parseGoalCommand("/goal clear")).toEqual({ action: "clear", objective: "" });
    expect(parseGoalCommand("/goal edit")).toEqual({ action: "edit", objective: "" });
    expect(parseGoalCommand("/goal edit refine plugin parity")).toEqual({
      action: "edit",
      objective: "refine plugin parity",
    });
    expect(parseGoalCommand("/goal edit --tokens 2M refine plugin parity")).toEqual({
      action: "edit",
      objective: "refine plugin parity",
      goalBudgetTokens: 2_000_000,
    });
  });

  it("returns null for non-goal input", () => {
    expect(parseGoalCommand("regular chat")).toBeNull();
  });

  it("builds a bare edit prefill command from the current goal", () => {
    expect(buildGoalEditCommand(null)).toBeNull();
    expect(buildGoalEditCommand({ objective: "  refine browser parity  " })).toBe(
      "/goal edit refine browser parity",
    );
  });

  it("formats current goal summary text", () => {
    const summary = formatGoalSummaryMessage(
      {
        objective: "ship browser skill parity",
        status: "active",
        tokenBudget: 2_500,
        tokensUsed: 1_200,
      },
      {
        title: "Goal",
        status: "Status",
        objective: "Objective",
        tokens: "Tokens",
        commands: "Commands",
        statusLabel: "Goal active",
      },
    );

    expect(summary).toContain("Goal");
    expect(summary).toContain("Status: Goal active");
    expect(summary).toContain("Objective: ship browser skill parity");
    expect(summary).toContain("Tokens: 1,200 / 2,500");
    expect(summary).toContain("Commands: /goal edit, /goal pause, /goal clear");
  });
});
