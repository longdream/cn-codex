import { describe, expect, it } from "vitest";
import type { ChatMessage, ThreadRuntimeState } from "../stores/appStore";
import {
  isComputerUseToolCall,
  resolveComputerUseActive,
} from "../utils/computerUse";

function messageWithTool(
  name: string,
  args: string,
  status: "running" | "success" | "failed" = "running",
): ChatMessage {
  return {
    id: "m1",
    role: "system",
    content: "",
    timestamp: Date.now(),
    toolCalls: [
      {
        id: "t1",
        name,
        arguments: args,
        status,
        displayLabel: name,
      },
    ],
  };
}

describe("computerUse helpers", () => {
  it("detects direct computer-use MCP names", () => {
    expect(isComputerUseToolCall("mcp__computer-use__click", "{}")).toBe(true);
    expect(isComputerUseToolCall("mcp_call_tool", '{"server":"computer-use","tool":"click"}')).toBe(
      true,
    );
  });

  it("detects sky bootstrap and input actions in node repl code", () => {
    expect(
      isComputerUseToolCall(
        "mcp__node_repl__js",
        JSON.stringify({
          code: "await sky.click({ window: targetWindow, x: 10, y: 20 })",
        }),
      ),
    ).toBe(true);

    expect(
      isComputerUseToolCall(
        "mcp__node_repl__js",
        JSON.stringify({
          code: "await setupComputerUseRuntime({ globals: globalThis })",
        }),
      ),
    ).toBe(true);
  });

  it("does not treat unrelated tools as computer use", () => {
    expect(isComputerUseToolCall("browser_run", '{"url":"https://example.com"}')).toBe(false);
    expect(isComputerUseToolCall("shell_command", '{"command":"Get-ChildItem"}')).toBe(false);
  });

  it("activates while a computer-use tool is running", () => {
    const messages = [
      messageWithTool(
        "mcp__node_repl__js",
        JSON.stringify({ code: "await sky.list_apps()" }),
        "running",
      ),
    ];
    expect(
      resolveComputerUseActive({
        messages,
        isStreaming: false,
        threadRuntimeStates: {},
      }),
    ).toBe(true);
  });

  it("keeps active during streaming after computer-use activity", () => {
    const messages = [
      messageWithTool(
        "mcp__node_repl__js",
        JSON.stringify({ code: "await sky.type_text({ text: 'hi', window: targetWindow })" }),
        "success",
      ),
    ];
    expect(
      resolveComputerUseActive({
        messages,
        isStreaming: true,
        threadRuntimeStates: {},
      }),
    ).toBe(true);
    expect(
      resolveComputerUseActive({
        messages,
        isStreaming: false,
        threadRuntimeStates: {},
      }),
    ).toBe(false);
  });

  it("activates from background thread runtime state", () => {
    const runtime: ThreadRuntimeState = {
      messages: [
        messageWithTool(
          "mcp__computer-use__drag",
          "{}",
          "running",
        ),
      ],
      streamingText: "",
      streamingLabel: "",
      isStreaming: false,
      liveTurnUsage: null,
      currentTurnId: null,
      pendingMessageQueue: [],
      pendingFileReviews: {},
      currentGoal: null,
      activePlan: null,
      latestPlanContent: null,
      chatMode: "chat",
      selectedRobotId: null,
      robotCreateMode: false,
      robotWaitCountdown: null,
      reasoningText: "",
      browserPanelUrl: null,
      browserPanelTitle: null,
      browserPanelStatus: "idle",
      browserCanGoBack: false,
      browserCanGoForward: false,
      browserActive: false,
      browserDetached: false,
      overrideProviderId: null,
      overrideModelId: null,
      smartbrainEnabled: false,
      updatedAt: Date.now(),
    };

    expect(
      resolveComputerUseActive({
        messages: [],
        isStreaming: false,
        threadRuntimeStates: { "thread-bg": runtime },
      }),
    ).toBe(true);
  });
});
