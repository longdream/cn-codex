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
  role: ChatMessage["role"] = "system",
): ChatMessage {
  return {
    id: `m-${Math.random().toString(36).slice(2, 8)}`,
    role,
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

function emptyRuntime(partial: Partial<ThreadRuntimeState> = {}): ThreadRuntimeState {
  return {
    messages: [],
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
    ...partial,
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

  it("does not treat unrelated tools or path-only computer-use strings as control", () => {
    expect(isComputerUseToolCall("browser_run", '{"url":"https://example.com"}')).toBe(false);
    expect(isComputerUseToolCall("shell_command", '{"command":"Get-ChildItem"}')).toBe(false);
    expect(
      isComputerUseToolCall(
        "shell_command",
        JSON.stringify({ command: "codey/plugins/computer-use/scripts/foo.mjs" }),
      ),
    ).toBe(false);
    expect(
      isComputerUseToolCall(
        "read_file",
        JSON.stringify({ path: "codey/plugins/computer-use/.mcp.json" }),
      ),
    ).toBe(false);
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

  it("keeps active during streaming only for current-turn computer-use activity", () => {
    const currentTurnMessages = [
      { id: "u1", role: "user" as const, content: "open wechat", timestamp: 1 },
      messageWithTool(
        "mcp__node_repl__js",
        JSON.stringify({ code: "await sky.type_text({ text: 'hi', window: targetWindow })" }),
        "success",
      ),
    ];
    expect(
      resolveComputerUseActive({
        messages: currentTurnMessages,
        isStreaming: true,
        threadRuntimeStates: {},
      }),
    ).toBe(true);
    expect(
      resolveComputerUseActive({
        messages: currentTurnMessages,
        isStreaming: false,
        threadRuntimeStates: {},
      }),
    ).toBe(false);
  });

  it("does not stick after historical computer-use when a later turn is streaming", () => {
    const messages: ChatMessage[] = [
      { id: "u1", role: "user", content: "control desktop", timestamp: 1 },
      messageWithTool(
        "mcp__computer-use__click",
        "{}",
        "success",
      ),
      { id: "a1", role: "assistant", content: "done", timestamp: 2 },
      { id: "u2", role: "user", content: "just chat", timestamp: 3 },
      { id: "a2", role: "assistant", content: "sure", timestamp: 4 },
    ];

    expect(
      resolveComputerUseActive({
        messages,
        isStreaming: true,
        threadRuntimeStates: {},
      }),
    ).toBe(false);
  });

  it("activates from background thread runtime state when tool is running", () => {
    const runtime = emptyRuntime({
      messages: [messageWithTool("mcp__computer-use__drag", "{}", "running")],
      isStreaming: false,
    });

    expect(
      resolveComputerUseActive({
        messages: [],
        isStreaming: false,
        threadRuntimeStates: { "thread-bg": runtime },
      }),
    ).toBe(true);
  });

  it("does not activate from historical background computer-use during later streaming", () => {
    const runtime = emptyRuntime({
      messages: [
        { id: "u1", role: "user", content: "old cu", timestamp: 1 },
        messageWithTool("mcp__computer-use__click", "{}", "success"),
        { id: "u2", role: "user", content: "new chat", timestamp: 2 },
      ],
      isStreaming: true,
    });

    expect(
      resolveComputerUseActive({
        messages: [],
        isStreaming: false,
        threadRuntimeStates: { "thread-bg": runtime },
      }),
    ).toBe(false);
  });
});
