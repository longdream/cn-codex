import { describe, expect, it } from "vitest";
import type { ChatMessage } from "../stores/appStore";
import {
  countRunningSubagents,
  deriveActiveSubagents,
  isTerminalSubagentStatus,
  liveSubagentFromPayload,
  parseSubagentToolOutput,
} from "../utils/subagentStatus";

function msg(
  toolCalls: ChatMessage["toolCalls"],
  timestamp = 1_000,
): ChatMessage {
  return {
    id: "m1",
    role: "assistant",
    content: "",
    timestamp,
    toolCalls,
  };
}

describe("subagentStatus helpers", () => {
  it("parses subagent tool output json", () => {
    const parsed = parseSubagentToolOutput(JSON.stringify({
      completed: true,
      agents: [
        {
          id: "a1",
          role: "reviewer",
          status: "completed",
          prompt: "review changes",
          durationMs: 1200,
          output: "looks good",
        },
      ],
    }));
    expect(parsed?.completed).toBe(true);
    expect(parsed?.agents[0]?.id).toBe("a1");
    expect(parsed?.agents[0]?.durationMs).toBe(1200);
  });

  it("derives running spawn agents and keeps recent completed ones briefly", () => {
    const messages: ChatMessage[] = [
      msg([
        {
          id: "t1",
          name: "spawn_agent",
          arguments: JSON.stringify({ role: "explorer", prompt: "scan files" }),
          status: "running",
          displayLabel: "explorer",
        },
        {
          id: "t2",
          name: "spawn_agent",
          arguments: JSON.stringify({ role: "tester", prompt: "run tests" }),
          status: "success",
          displayLabel: "tester",
          output: JSON.stringify({
            completed: false,
            agents: [
              {
                id: "agent-2",
                role: "tester",
                status: "completed",
                prompt: "run tests",
                durationMs: 800,
                output: "all green",
              },
            ],
          }),
        },
        {
          id: "t3",
          name: "close_agent",
          arguments: JSON.stringify({ target: "agent-old" }),
          status: "success",
          displayLabel: "agent-old",
          output: JSON.stringify({
            target: "agent-old",
            closed: true,
            previousStatus: "running",
            message: "closed",
          }),
        },
      ], 10_000),
    ];

    const agents = deriveActiveSubagents(messages, {
      includeTerminalMs: 120_000,
      now: 15_000,
    });

    expect(agents.some((a) => a.status === "running")).toBe(true);
    expect(agents.find((a) => a.role === "explorer")?.status).toBe("running");
    expect(agents.find((a) => a.id === "agent-2")?.status).toBe("completed");
    expect(agents.find((a) => a.id === "agent-old")?.status).toBe("closed");
    expect(countRunningSubagents(agents)).toBe(1);
  });

  it("hides old terminal agents outside the retention window", () => {
    const messages: ChatMessage[] = [
      msg([
        {
          id: "t1",
          name: "spawn_agent",
          arguments: JSON.stringify({ role: "done", prompt: "done" }),
          status: "success",
          displayLabel: "done",
          output: JSON.stringify({
            agents: [{ id: "old", role: "done", status: "completed" }],
          }),
        },
      ], 1_000),
    ];
    const agents = deriveActiveSubagents(messages, {
      includeTerminalMs: 5_000,
      now: 20_000,
    });
    expect(agents).toHaveLength(0);
    expect(isTerminalSubagentStatus("completed")).toBe(true);
  });

  it("lets newer live events override tool-card derived status", () => {
    const messages: ChatMessage[] = [
      msg([
        {
          id: "t1",
          name: "spawn_agent",
          arguments: JSON.stringify({ role: "worker", prompt: "work" }),
          status: "success",
          displayLabel: "worker",
          output: JSON.stringify({
            agents: [{ id: "live-1", role: "worker", status: "running", prompt: "work" }],
          }),
        },
      ], 10_000),
    ];
    const live = liveSubagentFromPayload({
      id: "live-1",
      role: "worker",
      status: "completed",
      prompt: "work",
      durationMs: 1500,
      output: "done via live event",
      updatedAt: 20_000,
    });
    expect(live).not.toBeNull();
    const agents = deriveActiveSubagents(messages, {
      includeTerminalMs: 120_000,
      now: 21_000,
      liveAgents: live ? [live] : [],
    });
    expect(agents.find((a) => a.id === "live-1")?.status).toBe("completed");
    expect(agents.find((a) => a.id === "live-1")?.source).toBe("live");
    expect(countRunningSubagents(agents)).toBe(0);
  });

  it("hides dismissed agent ids from the active list", () => {
    const messages: ChatMessage[] = [
      msg([
        {
          id: "t1",
          name: "spawn_agent",
          arguments: JSON.stringify({ role: "worker", prompt: "work" }),
          status: "success",
          displayLabel: "worker",
          output: JSON.stringify({
            agents: [
              { id: "keep", role: "worker", status: "completed" },
              { id: "drop", role: "worker", status: "completed" },
            ],
          }),
        },
      ], 10_000),
    ];
    const agents = deriveActiveSubagents(messages, {
      includeTerminalMs: 120_000,
      now: 11_000,
      dismissedIds: ["drop"],
    });
    expect(agents.map((a) => a.id)).toEqual(["keep"]);
  });
});
