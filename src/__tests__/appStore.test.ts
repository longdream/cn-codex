import { describe, it, expect, beforeEach, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

import { invoke } from "@tauri-apps/api/core";
const mockInvoke = vi.mocked(invoke);

import { useAppStore } from "../stores/appStore";

describe("appStore", () => {
  beforeEach(() => {
    useAppStore.setState({
      initialized: false,
      currentThreadId: null,
      currentTurnId: null,
      currentModel: null,
      threads: [],
      messages: [],
      streamingText: "",
      isStreaming: false,
      chatMode: "chat",
      currentGoal: null,
      showSettings: false,
      pendingComposerInsert: null,
      pendingFileReviews: {},
    });
    vi.clearAllMocks();
  });

  describe("synchronous setters", () => {
    it("setInitialized updates initialized state", () => {
      useAppStore.getState().setInitialized(true);
      expect(useAppStore.getState().initialized).toBe(true);
    });

    it("setCurrentThread sets thread id and clears messages/streaming", () => {
      useAppStore.setState({
        messages: [{ id: "1", role: "user", content: "hello", timestamp: 1 }],
        streamingText: "partial",
      });
      useAppStore.getState().setCurrentThread("thread-123");
      const state = useAppStore.getState();
      expect(state.currentThreadId).toBe("thread-123");
      expect(state.messages).toEqual([]);
      expect(state.streamingText).toBe("");
    });

    it("setCurrentModel updates model", () => {
      useAppStore.getState().setCurrentModel("gpt-4.1");
      expect(useAppStore.getState().currentModel).toBe("gpt-4.1");
    });

    it("setStreaming updates isStreaming", () => {
      useAppStore.getState().setStreaming(true);
      expect(useAppStore.getState().isStreaming).toBe(true);
    });

    it("setShowSettings updates showSettings", () => {
      useAppStore.getState().setShowSettings(true);
      expect(useAppStore.getState().showSettings).toBe(true);
    });

    it("setShowSettings(true) hides right panel and does not auto-restore", () => {
      useAppStore.setState({ rightPanelVisible: true });

      useAppStore.getState().setShowSettings(true);
      expect(useAppStore.getState().showSettings).toBe(true);
      expect(useAppStore.getState().rightPanelVisible).toBe(false);

      useAppStore.getState().setShowSettings(false);
      expect(useAppStore.getState().showSettings).toBe(false);
      expect(useAppStore.getState().rightPanelVisible).toBe(false);
    });

    it("setChatMode switches between chat and goal mode", () => {
      useAppStore.getState().setChatMode("goal");
      expect(useAppStore.getState().chatMode).toBe("goal");
      useAppStore.getState().setChatMode("chat");
      expect(useAppStore.getState().chatMode).toBe("chat");
    });

    it("queues and consumes composer inserts for cross-panel snippets", () => {
      const store = useAppStore.getState();
      store.queueComposerInsert("first block");
      store.queueComposerInsert("second block");
      expect(useAppStore.getState().pendingComposerInsert).toContain("first block");
      expect(useAppStore.getState().pendingComposerInsert).toContain("second block");

      const consumed = useAppStore.getState().consumeComposerInsert();
      expect(consumed).toContain("first block");
      expect(consumed).toContain("second block");
      expect(useAppStore.getState().pendingComposerInsert).toBeNull();
    });

    it("tracks pending file review keep/edit state", () => {
      const store = useAppStore.getState();
      store.upsertPendingFileReview({
        threadId: "thread-1",
        callId: "call-1",
        rawPatch: "*** Begin Patch\n*** End Patch",
        createdAtMs: 1,
        updatedAtMs: 1,
        files: [
          {
            path: "src/app.ts",
            action: "modified",
            candidateContent: "old",
            keep: true,
          },
          {
            path: "src/new.ts",
            action: "created",
            candidateContent: "new",
            keep: true,
          },
        ],
        selectedPath: null,
        keepAll: true,
        status: "pending",
      });

      store.setPendingFileReviewKeepAll("call-1", false);
      store.setPendingFileReviewFileKeep("call-1", "src/app.ts", true);
      store.setPendingFileReviewEditedContent("call-1", "src/app.ts", "edited");
      store.setPendingFileReviewSelectedPath("call-1", "src/app.ts");

      const review = useAppStore.getState().pendingFileReviews["call-1"];
      expect(review.keepAll).toBe(false);
      expect(review.selectedPath).toBe("src/app.ts");
      expect(review.files.find((file) => file.path === "src/app.ts")?.editedContent).toBe("edited");
      expect(review.files.find((file) => file.path === "src/new.ts")?.keep).toBe(false);
    });
  });

  describe("addMessage", () => {
    it("appends message to list", () => {
      useAppStore.getState().addMessage({ id: "m1", role: "user", content: "hi", timestamp: 1 });
      useAppStore.getState().addMessage({ id: "m2", role: "assistant", content: "hello", timestamp: 2 });
      expect(useAppStore.getState().messages).toHaveLength(2);
      expect(useAppStore.getState().messages[1].content).toBe("hello");
    });
  });

  describe("tool call progress", () => {
    it("attaches apply_patch progress to the matching tool call", () => {
      useAppStore.getState().addMessage({
        id: "tools",
        role: "system",
        content: "",
        timestamp: 1,
        toolCalls: [
          {
            id: "patch-call",
            name: "apply_patch",
            arguments: "{}",
            status: "running",
            displayLabel: "apply_patch",
          },
        ],
      });

      useAppStore.getState().updateToolCallPatchProgress("patch-call", [
        { path: "src/app.ts", action: "modified" },
        { path: "src/old.ts", action: "renamed", moveTo: "src/new.ts" },
      ]);

      const toolCall = useAppStore.getState().messages[0].toolCalls?.[0];
      expect(toolCall?.patchProgress).toEqual([
        { path: "src/app.ts", action: "modified" },
        { path: "src/old.ts", action: "renamed", moveTo: "src/new.ts" },
      ]);
    });
  });

  describe("appendStreamingText", () => {
    it("concatenates delta text", () => {
      useAppStore.getState().appendStreamingText("Hello ");
      useAppStore.getState().appendStreamingText("world");
      expect(useAppStore.getState().streamingText).toBe("Hello world");
    });
  });

  describe("addThread", () => {
    it("prepends new thread to list", () => {
      useAppStore.setState({
        threads: [{ id: "old", preview: "", updatedAt: 1 }],
      });
      useAppStore.getState().addThread({ id: "new", preview: "", updatedAt: 2 });
      const threads = useAppStore.getState().threads;
      expect(threads[0].id).toBe("new");
      expect(threads[1].id).toBe("old");
    });
  });

  describe("createThread", () => {
    it("sets currentThreadId and adds thread on success", async () => {
      mockInvoke.mockResolvedValueOnce({ thread: { id: "t-abc" } });
      const id = await useAppStore.getState().createThread();
      expect(id).toBe("t-abc");
      expect(useAppStore.getState().currentThreadId).toBe("t-abc");
      expect(useAppStore.getState().threads.some((t) => t.id === "t-abc")).toBe(true);
    });

    it("resets state on failure", async () => {
      mockInvoke.mockRejectedValueOnce(new Error("fail"));
      const id = await useAppStore.getState().createThread();
      expect(id).toBeNull();
      expect(useAppStore.getState().currentThreadId).toBeNull();
    });

    it("does not duplicate existing thread", async () => {
      useAppStore.setState({
        threads: [{ id: "t-abc", preview: "", updatedAt: 1 }],
      });
      mockInvoke.mockResolvedValueOnce({ thread: { id: "t-abc" } });
      await useAppStore.getState().createThread();
      expect(useAppStore.getState().threads.filter((t) => t.id === "t-abc")).toHaveLength(1);
    });
  });

  describe("loadThreads", () => {
    it("filters threads not in threadProjectMap and deletes them", async () => {
      useAppStore.setState({
        threadProjectMap: { "t1": "proj1", "t3": "proj1" },
      });
      mockInvoke.mockResolvedValueOnce({
        data: [
          { id: "t1", name: "Active", updatedAt: 1 },
          { id: "t2", name: "Orphaned", updatedAt: 2 },
          { id: "t3", name: "Also Active", updatedAt: 3 },
        ],
      });
      await useAppStore.getState().loadThreads();
      const threads = useAppStore.getState().threads;
      expect(threads).toHaveLength(2);
      expect(threads.map((t) => t.id).sort()).toEqual(["t1", "t3"]);
    });
  });

  describe("loadThread", () => {
    it("maps persisted thread goal into currentGoal", async () => {
      mockInvoke.mockImplementation(async (command) => {
        if (command === "standalone_thread_read") {
          return {
            thread: {
              id: "t-goal",
              goal: {
                objective: "ship browser skill parity",
                status: "paused",
                tokenBudget: 2000,
                tokensUsed: 750,
                createdAt: 100,
                updatedAt: 200,
              },
              turns: [],
            },
          };
        }
        return {};
      });

      await useAppStore.getState().loadThread("t-goal");

      expect(useAppStore.getState().currentGoal).toMatchObject({
        objective: "ship browser skill parity",
        status: "paused",
        tokenBudget: 2000,
        tokensUsed: 750,
      });
    });

    it("maps persisted turn metadata into a run summary message", async () => {
      mockInvoke.mockImplementation(async (command) => {
        if (command === "standalone_thread_read") {
          return {
            thread: {
              id: "t-run",
              turns: [
                {
                  id: "turn-1",
                  startedAt: 100,
                  completedAt: 103,
                  mode: "goal",
                  durationMs: 2345,
                  usage: {
                    promptTokens: 1200,
                    completionTokens: 345,
                    totalTokens: 1545,
                    callCount: 4,
                  },
                  goalBudgetTokens: 1500,
                  budgetLimited: true,
                  changedFiles: [
                    { path: "src/components/Figure.tsx", action: "modified" },
                  ],
                  items: [
                    {
                      type: "userMessage",
                      id: "u1",
                      content: [{ type: "text", text: "优化第二章图的样式" }],
                    },
                    {
                      type: "agentMessage",
                      id: "a1",
                      text: "已完成。",
                    },
                  ],
                },
              ],
            },
          };
        }
        return {};
      });

      await useAppStore.getState().loadThread("t-run");

      const summary = useAppStore
        .getState()
        .messages.find((message) => message.runSummary)?.runSummary;
      expect(summary).toMatchObject({
        turnId: "turn-1",
        mode: "goal",
        durationMs: 2345,
        usage: {
          promptTokens: 1200,
          completionTokens: 345,
          totalTokens: 1545,
          callCount: 4,
        },
        goalBudgetTokens: 1500,
        budgetLimited: true,
        changedFiles: [
          { path: "src/components/Figure.tsx", action: "modified" },
        ],
      });
    });

    it("keeps zero millisecond durations as valid run summary data", async () => {
      mockInvoke.mockImplementation(async (command) => {
        if (command === "standalone_thread_read") {
          return {
            thread: {
              id: "t-zero",
              turns: [
                {
                  id: "turn-zero",
                  startedAt: 100,
                  completedAt: 100,
                  mode: "chat",
                  durationMs: 0,
                  changedFiles: [],
                  items: [
                    {
                      type: "userMessage",
                      id: "u-zero",
                      content: [{ type: "text", text: "ping" }],
                    },
                  ],
                },
              ],
            },
          };
        }
        return {};
      });

      await useAppStore.getState().loadThread("t-zero");

      const summary = useAppStore
        .getState()
        .messages.find((message) => message.runSummary)?.runSummary;
      expect(summary).toMatchObject({
        turnId: "turn-zero",
        mode: "chat",
        durationMs: 0,
        changedFiles: [],
      });
    });

    it("labels restored browser_run tool calls with url and action count", async () => {
      mockInvoke.mockImplementation(async (command) => {
        if (command === "standalone_thread_read") {
          return {
            thread: {
              id: "t-browser",
              turns: [
                {
                  id: "turn-browser",
                  startedAt: 100,
                  items: [
                    {
                      type: "toolUse",
                      id: "browser-tools",
                      calls: [
                        {
                          id: "browser-call",
                          name: "browser_run",
                          arguments: JSON.stringify({
                            url: "http://localhost:1420",
                            actions: [
                              { type: "screenshot" },
                              { type: "text", selector: "body" },
                            ],
                          }),
                        },
                      ],
                    },
                  ],
                },
              ],
            },
          };
        }
        return {};
      });

      await useAppStore.getState().loadThread("t-browser");

      const toolCall = useAppStore
        .getState()
        .messages.find((message) => message.toolCalls)?.toolCalls?.[0];
      expect(toolCall?.displayLabel).toBe("http://localhost:1420 (2 actions)");
    });

    it("labels restored close_agent tool calls with target id", async () => {
      mockInvoke.mockImplementation(async (command) => {
        if (command === "standalone_thread_read") {
          return {
            thread: {
              id: "t-close-agent",
              turns: [
                {
                  id: "turn-close-agent",
                  startedAt: 100,
                  items: [
                    {
                      type: "toolUse",
                      id: "close-agent-tools",
                      calls: [
                        {
                          id: "close-agent-call",
                          name: "close_agent",
                          arguments: JSON.stringify({ target: "agent-123" }),
                        },
                      ],
                    },
                  ],
                },
              ],
            },
          };
        }
        return {};
      });

      await useAppStore.getState().loadThread("t-close-agent");

      const toolCall = useAppStore
        .getState()
        .messages.find((message) => message.toolCalls)?.toolCalls?.[0];
      expect(toolCall?.displayLabel).toBe("agent-123");
    });

    it("labels restored send_input tool calls with target id", async () => {
      mockInvoke.mockImplementation(async (command) => {
        if (command === "standalone_thread_read") {
          return {
            thread: {
              id: "t-send-input",
              turns: [
                {
                  id: "turn-send-input",
                  startedAt: 100,
                  items: [
                    {
                      type: "toolUse",
                      id: "send-input-tools",
                      calls: [
                        {
                          id: "send-input-call",
                          name: "send_input",
                          arguments: JSON.stringify({
                            target: "agent-456",
                            message: "please continue",
                          }),
                        },
                      ],
                    },
                  ],
                },
              ],
            },
          };
        }
        return {};
      });

      await useAppStore.getState().loadThread("t-send-input");

      const toolCall = useAppStore
        .getState()
        .messages.find((message) => message.toolCalls)?.toolCalls?.[0];
      expect(toolCall?.displayLabel).toBe("agent-456");
    });

    it("labels restored resume_agent tool calls with id", async () => {
      mockInvoke.mockImplementation(async (command) => {
        if (command === "standalone_thread_read") {
          return {
            thread: {
              id: "t-resume-agent",
              turns: [
                {
                  id: "turn-resume-agent",
                  startedAt: 100,
                  items: [
                    {
                      type: "toolUse",
                      id: "resume-agent-tools",
                      calls: [
                        {
                          id: "resume-agent-call",
                          name: "resume_agent",
                          arguments: JSON.stringify({ id: "agent-789" }),
                        },
                      ],
                    },
                  ],
                },
              ],
            },
          };
        }
        return {};
      });

      await useAppStore.getState().loadThread("t-resume-agent");

      const toolCall = useAppStore
        .getState()
        .messages.find((message) => message.toolCalls)?.toolCalls?.[0];
      expect(toolCall?.displayLabel).toBe("agent-789");
    });

    it("labels restored shell_command tool calls with command text", async () => {
      mockInvoke.mockImplementation(async (command) => {
        if (command === "standalone_thread_read") {
          return {
            thread: {
              id: "t-shell-command",
              turns: [
                {
                  id: "turn-shell-command",
                  startedAt: 100,
                  items: [
                    {
                      type: "toolUse",
                      id: "shell-command-tools",
                      calls: [
                        {
                          id: "shell-command-call",
                          name: "shell_command",
                          arguments: JSON.stringify({
                            command: "Get-ChildItem -Force",
                            workdir: "D:/workspace/app",
                            timeout_ms: 5000,
                          }),
                        },
                      ],
                    },
                  ],
                },
              ],
            },
          };
        }
        return {};
      });

      await useAppStore.getState().loadThread("t-shell-command");

      const toolCall = useAppStore
        .getState()
        .messages.find((message) => message.toolCalls)?.toolCalls?.[0];
      expect(toolCall?.displayLabel).toBe("Get-ChildItem -Force");
    });

    it("labels restored exec session tool calls", async () => {
      mockInvoke.mockImplementation(async (command) => {
        if (command === "standalone_thread_read") {
          return {
            thread: {
              id: "t-exec-session",
              turns: [
                {
                  id: "turn-exec-session",
                  startedAt: 100,
                  items: [
                    {
                      type: "toolUse",
                      id: "exec-session-tools",
                      calls: [
                        {
                          id: "exec-call",
                          name: "exec_command",
                          arguments: JSON.stringify({
                            cmd: "pnpm dev",
                            workdir: "D:/workspace/app",
                            yield_time_ms: 1000,
                          }),
                        },
                        {
                          id: "stdin-call",
                          name: "write_stdin",
                          arguments: JSON.stringify({
                            session_id: 3,
                            chars: "q",
                          }),
                        },
                        {
                          id: "close-exec-call",
                          name: "close_exec_session",
                          arguments: JSON.stringify({
                            session_id: 3,
                          }),
                        },
                      ],
                    },
                  ],
                },
              ],
            },
          };
        }
        return {};
      });

      await useAppStore.getState().loadThread("t-exec-session");

      const toolCalls = useAppStore
        .getState()
        .messages.find((message) => message.toolCalls)?.toolCalls;
      expect(toolCalls?.[0].displayLabel).toBe("pnpm dev");
      expect(toolCalls?.[1].displayLabel).toBe("session 3");
      expect(toolCalls?.[2].displayLabel).toBe("session 3");
    });

    it("labels restored raw apply_patch calls with the first changed path", async () => {
      mockInvoke.mockImplementation(async (command) => {
        if (command === "standalone_thread_read") {
          return {
            thread: {
              id: "t-raw-patch",
              turns: [
                {
                  id: "turn-raw-patch",
                  startedAt: 100,
                  items: [
                    {
                      type: "toolUse",
                      id: "raw-patch-tools",
                      calls: [
                        {
                          id: "raw-patch-call",
                          name: "apply_patch",
                          arguments: "*** Begin Patch\n*** Update File: src/raw.ts\n@@\n-old\n+new\n*** End Patch",
                        },
                      ],
                    },
                  ],
                },
              ],
            },
          };
        }
        return {};
      });

      await useAppStore.getState().loadThread("t-raw-patch");

      const toolCall = useAppStore
        .getState()
        .messages.find((message) => message.toolCalls)?.toolCalls?.[0];
      expect(toolCall?.displayLabel).toBe("src/raw.ts");
    });

    it("labels restored request_user_input calls with question count", async () => {
      mockInvoke.mockImplementation(async (command) => {
        if (command === "standalone_thread_read") {
          return {
            thread: {
              id: "t-user-input",
              turns: [
                {
                  id: "turn-user-input",
                  startedAt: 100,
                  items: [
                    {
                      type: "toolUse",
                      id: "user-input-tools",
                      calls: [
                        {
                          id: "user-input-call",
                          name: "request_user_input",
                          arguments: JSON.stringify({
                            questions: [
                              {
                                id: "direction",
                                header: "Direction",
                                question: "Which direction should I take?",
                                options: [
                                  { label: "Fast", description: "Move quickly." },
                                  { label: "Careful", description: "Verify more." },
                                ],
                              },
                            ],
                          }),
                        },
                      ],
                    },
                  ],
                },
              ],
            },
          };
        }
        return {};
      });

      await useAppStore.getState().loadThread("t-user-input");

      const toolCall = useAppStore
        .getState()
        .messages.find((message) => message.toolCalls)?.toolCalls?.[0];
      expect(toolCall?.displayLabel).toBe("1 question(s)");
    });

    it("labels restored request_permissions calls with reason", async () => {
      mockInvoke.mockImplementation(async (command) => {
        if (command === "standalone_thread_read") {
          return {
            thread: {
              id: "t-permissions",
              turns: [
                {
                  id: "turn-permissions",
                  startedAt: 100,
                  items: [
                    {
                      type: "toolUse",
                      id: "permissions-tools",
                      calls: [
                        {
                          id: "permissions-call",
                          name: "request_permissions",
                          arguments: JSON.stringify({
                            reason: "Need network for dependency metadata",
                            permissions: {
                              network: { enabled: true },
                            },
                          }),
                        },
                      ],
                    },
                  ],
                },
              ],
            },
          };
        }
        return {};
      });

      await useAppStore.getState().loadThread("t-permissions");

      const toolCall = useAppStore
        .getState()
        .messages.find((message) => message.toolCalls)?.toolCalls?.[0];
      expect(toolCall?.displayLabel).toBe("Need network for dependency metadata");
    });

    it("labels restored mcp_status tool calls with server name", async () => {
      mockInvoke.mockImplementation(async (command) => {
        if (command === "standalone_thread_read") {
          return {
            thread: {
              id: "t-mcp-status",
              turns: [
                {
                  id: "turn-mcp-status",
                  startedAt: 100,
                  items: [
                    {
                      type: "toolUse",
                      id: "mcp-status-tools",
                      calls: [
                        {
                          id: "mcp-status-call",
                          name: "mcp_status",
                          arguments: JSON.stringify({ server: "browser", probe: true }),
                        },
                      ],
                    },
                  ],
                },
              ],
            },
          };
        }
        return {};
      });

      await useAppStore.getState().loadThread("t-mcp-status");

      const toolCall = useAppStore
        .getState()
        .messages.find((message) => message.toolCalls)?.toolCalls?.[0];
      expect(toolCall?.displayLabel).toBe("browser");
    });

    it("labels restored apps_list tool calls with connector id", async () => {
      mockInvoke.mockImplementation(async (command) => {
        if (command === "standalone_thread_read") {
          return {
            thread: {
              id: "t-apps-list",
              turns: [
                {
                  id: "turn-apps-list",
                  startedAt: 100,
                  items: [
                    {
                      type: "toolUse",
                      id: "apps-list-tools",
                      calls: [
                        {
                          id: "apps-list-call",
                          name: "apps_list",
                          arguments: JSON.stringify({ connector_id: "connector_sites" }),
                        },
                      ],
                    },
                  ],
                },
              ],
            },
          };
        }
        return {};
      });

      await useAppStore.getState().loadThread("t-apps-list");

      const toolCall = useAppStore
        .getState()
        .messages.find((message) => message.toolCalls)?.toolCalls?.[0];
      expect(toolCall?.displayLabel).toBe("connector_sites");
    });

    it("labels restored plugin install tool calls", async () => {
      mockInvoke.mockImplementation(async (command) => {
        if (command === "standalone_thread_read") {
          return {
            thread: {
              id: "t-plugin-install",
              turns: [
                {
                  id: "turn-plugin-install",
                  startedAt: 100,
                  items: [
                    {
                      type: "toolUse",
                      id: "plugin-install-tools",
                      calls: [
                        {
                          id: "plugin-list-call",
                          name: "list_available_plugins_to_install",
                          arguments: JSON.stringify({ query: "sites" }),
                        },
                        {
                          id: "plugin-install-call",
                          name: "request_plugin_install",
                          arguments: JSON.stringify({ tool_id: "local-cache:sites/1.0.0" }),
                        },
                      ],
                    },
                  ],
                },
              ],
            },
          };
        }
        return {};
      });

      await useAppStore.getState().loadThread("t-plugin-install");

      const toolCalls = useAppStore
        .getState()
        .messages.find((message) => message.toolCalls)?.toolCalls;
      expect(toolCalls?.[0].displayLabel).toBe("sites");
      expect(toolCalls?.[1].displayLabel).toBe("local-cache:sites/1.0.0");
    });

    it("labels restored image_generate tool calls with output path", async () => {
      mockInvoke.mockImplementation(async (command) => {
        if (command === "standalone_thread_read") {
          return {
            thread: {
              id: "t-image",
              turns: [
                {
                  id: "turn-image",
                  startedAt: 100,
                  items: [
                    {
                      type: "toolUse",
                      id: "image-tools",
                      calls: [
                        {
                          id: "image-call",
                          name: "image_generate",
                          arguments: JSON.stringify({
                            prompt: "draw a compact built-in browser UI",
                            output_path: "codey/images/generated/browser.png",
                          }),
                        },
                      ],
                    },
                  ],
                },
              ],
            },
          };
        }
        return {};
      });

      await useAppStore.getState().loadThread("t-image");

      const toolCall = useAppStore
        .getState()
        .messages.find((message) => message.toolCalls)?.toolCalls?.[0];
      expect(toolCall?.displayLabel).toBe("codey/images/generated/browser.png");
    });

    it("labels restored direct MCP tool calls with server and tool", async () => {
      mockInvoke.mockImplementation(async (command) => {
        if (command === "standalone_thread_read") {
          return {
            thread: {
              id: "t-mcp",
              turns: [
                {
                  id: "turn-mcp",
                  startedAt: 100,
                  items: [
                    {
                      type: "toolUse",
                      id: "mcp-tools",
                      calls: [
                        {
                          id: "mcp-call",
                          name: "mcp__docs__search",
                          arguments: JSON.stringify({ query: "browser" }),
                        },
                      ],
                    },
                  ],
                },
              ],
            },
          };
        }
        return {};
      });

      await useAppStore.getState().loadThread("t-mcp");

      const toolCall = useAppStore
        .getState()
        .messages.find((message) => message.toolCalls)?.toolCalls?.[0];
      expect(toolCall?.displayLabel).toBe("docs:search");
    });

    it("labels restored tool_search calls with the query", async () => {
      mockInvoke.mockImplementation(async (command) => {
        if (command === "standalone_thread_read") {
          return {
            thread: {
              id: "t-tool-search",
              turns: [
                {
                  id: "turn-tool-search",
                  startedAt: 100,
                  items: [
                    {
                      type: "toolUse",
                      id: "tool-search-tools",
                      calls: [
                        {
                          id: "tool-search-call",
                          name: "tool_search",
                          arguments: JSON.stringify({ query: "browser automation" }),
                        },
                      ],
                    },
                  ],
                },
              ],
            },
          };
        }
        return {};
      });

      await useAppStore.getState().loadThread("t-tool-search");

      const toolCall = useAppStore
        .getState()
        .messages.find((message) => message.toolCalls)?.toolCalls?.[0];
      expect(toolCall?.displayLabel).toBe("browser automation");
    });

    it("labels restored code_review tool calls with base ref", async () => {
      mockInvoke.mockImplementation(async (command) => {
        if (command === "standalone_thread_read") {
          return {
            thread: {
              id: "t-code-review",
              turns: [
                {
                  id: "turn-code-review",
                  startedAt: 100,
                  items: [
                    {
                      type: "toolUse",
                      id: "code-review-tools",
                      calls: [
                        {
                          id: "code-review-call",
                          name: "code_review",
                          arguments: JSON.stringify({
                            base_ref: "origin/main",
                            paths: ["src"],
                          }),
                        },
                      ],
                    },
                  ],
                },
              ],
            },
          };
        }
        return {};
      });

      await useAppStore.getState().loadThread("t-code-review");

      const toolCall = useAppStore
        .getState()
        .messages.find((message) => message.toolCalls)?.toolCalls?.[0];
      expect(toolCall?.displayLabel).toBe("vs origin/main");
    });

    it("labels restored memory update and forget tool calls with paths", async () => {
      mockInvoke.mockImplementation(async (command) => {
        if (command === "standalone_thread_read") {
          return {
            thread: {
              id: "t-memory-update",
              turns: [
                {
                  id: "turn-memory-update",
                  startedAt: 100,
                  items: [
                    {
                      type: "toolUse",
                      id: "memory-tools",
                      calls: [
                        {
                          id: "memory-update-call",
                          name: "memory_update",
                          arguments: JSON.stringify({
                            path: "project/preferences.md",
                            old_text: "old",
                            new_text: "new",
                          }),
                        },
                        {
                          id: "memory-forget-call",
                          name: "memory_forget",
                          arguments: JSON.stringify({
                            path: "project/obsolete.md",
                          }),
                        },
                      ],
                    },
                  ],
                },
              ],
            },
          };
        }
        return {};
      });

      await useAppStore.getState().loadThread("t-memory-update");

      const toolCalls = useAppStore
        .getState()
        .messages.find((message) => message.toolCalls)?.toolCalls;
      expect(toolCalls?.[0].displayLabel).toBe("project/preferences.md");
      expect(toolCalls?.[1].displayLabel).toBe("project/obsolete.md");
    });
  });

  describe("vision fallback sync", () => {
    it("writes local OCR fallback kind when active model switches", async () => {
      useAppStore.setState({
        providers: [
          {
            id: "provider-main",
            type: "custom-main",
            name: "Main",
            category: "other",
            baseUrl: "http://localhost:9999/v1",
            apiKey: "sk-main",
            wireApi: "chat",
            requiresOpenAIAuth: false,
            models: [
              {
                id: "text-model",
                label: "Text Model",
                supportsVision: false,
                visionFallbackKind: "local_ocr",
                contextLength: 128000,
                maxOutputTokens: 65535,
              },
            ],
            isCustom: true,
            createdAt: Date.now(),
          },
        ],
        configuredModels: [
          {
            id: "provider-main:text-model",
            provider: "provider-main",
            model: "text-model",
            label: "Text Model",
            supportsVision: false,
          },
        ],
      });
      mockInvoke.mockResolvedValue({ status: "ok" });

      useAppStore.getState().setActiveModelId("provider-main:text-model");
      await Promise.resolve();

      const standaloneCalls = mockInvoke.mock.calls.filter(([command]) => command === "standalone_config_write");
      expect(standaloneCalls.length).toBeGreaterThan(0);
      const latestCall = standaloneCalls[standaloneCalls.length - 1];
      const edits = (latestCall?.[1] as { edits: Array<{ keyPath: string; value: unknown }> }).edits;
      const values = Object.fromEntries(edits.map((edit) => [edit.keyPath, edit.value]));
      expect(values.model_supports_vision).toBe(false);
      expect(values.vision_fallback_kind).toBe("local_ocr");
      expect(values.vision_fallback_provider).toBeNull();
      expect(values.vision_fallback_model).toBeNull();
    });

    it("keeps legacy multimodal fallback compatible and writes fallback kind", async () => {
      useAppStore.setState({
        providers: [
          {
            id: "provider-main",
            type: "custom-main",
            name: "Main",
            category: "other",
            baseUrl: "http://localhost:9999/v1",
            apiKey: "sk-main",
            wireApi: "chat",
            requiresOpenAIAuth: false,
            models: [
              {
                id: "text-model",
                label: "Text Model",
                supportsVision: false,
                visionFallbackProviderId: "provider-vision",
                visionFallbackModelId: "qwen-vl-max",
                contextLength: 128000,
                maxOutputTokens: 65535,
              },
            ],
            isCustom: true,
            createdAt: Date.now(),
          },
          {
            id: "provider-vision",
            type: "qwen",
            name: "Qwen Vision",
            category: "china",
            baseUrl: "https://dashscope.aliyuncs.com/compatible-mode/v1",
            apiKey: "sk-vision",
            wireApi: "chat",
            requiresOpenAIAuth: false,
            models: [
              {
                id: "qwen-vl-max",
                label: "Qwen VL Max",
                supportsVision: true,
                contextLength: 128000,
                maxOutputTokens: 65535,
              },
            ],
            isCustom: false,
            createdAt: Date.now(),
          },
        ],
        configuredModels: [
          {
            id: "provider-main:text-model",
            provider: "provider-main",
            model: "text-model",
            label: "Text Model",
            supportsVision: false,
          },
        ],
      });
      mockInvoke.mockResolvedValue({ status: "ok" });

      useAppStore.getState().setActiveModelId("provider-main:text-model");
      await Promise.resolve();

      const standaloneCalls = mockInvoke.mock.calls.filter(([command]) => command === "standalone_config_write");
      expect(standaloneCalls.length).toBeGreaterThan(0);
      const latestCall = standaloneCalls[standaloneCalls.length - 1];
      const edits = (latestCall?.[1] as { edits: Array<{ keyPath: string; value: unknown }> }).edits;
      const values = Object.fromEntries(edits.map((edit) => [edit.keyPath, edit.value]));
      expect(values.vision_fallback_kind).toBe("multimodal");
      expect(values.vision_fallback_provider).toBe("qwen");
      expect(values.vision_fallback_model).toBe("qwen-vl-max");
    });

    it("writes local OCR fallback kind when provider is activated", async () => {
      useAppStore.setState({
        activeProviderId: null,
        activeModelId: null,
        providers: [
          {
            id: "provider-main",
            type: "custom-main",
            name: "Main",
            category: "other",
            baseUrl: "http://localhost:9999/v1",
            apiKey: "sk-main",
            wireApi: "chat",
            requiresOpenAIAuth: false,
            models: [
              {
                id: "text-model",
                label: "Text Model",
                supportsVision: false,
                visionFallbackKind: "local_ocr",
                contextLength: 128000,
                maxOutputTokens: 65535,
              },
            ],
            isCustom: true,
            createdAt: Date.now(),
          },
        ],
      });
      mockInvoke.mockResolvedValue({ status: "ok" });

      useAppStore.getState().activateProvider("provider-main");
      await Promise.resolve();

      const standaloneCalls = mockInvoke.mock.calls.filter(([command]) => command === "standalone_config_write");
      expect(standaloneCalls.length).toBeGreaterThan(0);
      const latestCall = standaloneCalls[standaloneCalls.length - 1];
      const edits = (latestCall?.[1] as { edits: Array<{ keyPath: string; value: unknown }> }).edits;
      const values = Object.fromEntries(edits.map((edit) => [edit.keyPath, edit.value]));
      expect(values.model_supports_vision).toBe(false);
      expect(values.vision_fallback_kind).toBe("local_ocr");
      expect(values.vision_fallback_provider).toBeNull();
      expect(values.vision_fallback_model).toBeNull();
    });

    it("writes multimodal fallback kind when provider is activated", async () => {
      useAppStore.setState({
        activeProviderId: null,
        activeModelId: null,
        providers: [
          {
            id: "provider-main",
            type: "custom-main",
            name: "Main",
            category: "other",
            baseUrl: "http://localhost:9999/v1",
            apiKey: "sk-main",
            wireApi: "chat",
            requiresOpenAIAuth: false,
            models: [
              {
                id: "text-model",
                label: "Text Model",
                supportsVision: false,
                visionFallbackProviderId: "provider-vision",
                visionFallbackModelId: "qwen-vl-max",
                contextLength: 128000,
                maxOutputTokens: 65535,
              },
            ],
            isCustom: true,
            createdAt: Date.now(),
          },
          {
            id: "provider-vision",
            type: "qwen",
            name: "Qwen Vision",
            category: "china",
            baseUrl: "https://dashscope.aliyuncs.com/compatible-mode/v1",
            apiKey: "sk-vision",
            wireApi: "chat",
            requiresOpenAIAuth: false,
            models: [
              {
                id: "qwen-vl-max",
                label: "Qwen VL Max",
                supportsVision: true,
                contextLength: 128000,
                maxOutputTokens: 65535,
              },
            ],
            isCustom: false,
            createdAt: Date.now(),
          },
        ],
      });
      mockInvoke.mockResolvedValue({ status: "ok" });

      useAppStore.getState().activateProvider("provider-main");
      await Promise.resolve();

      const standaloneCalls = mockInvoke.mock.calls.filter(([command]) => command === "standalone_config_write");
      expect(standaloneCalls.length).toBeGreaterThan(0);
      const latestCall = standaloneCalls[standaloneCalls.length - 1];
      const edits = (latestCall?.[1] as { edits: Array<{ keyPath: string; value: unknown }> }).edits;
      const values = Object.fromEntries(edits.map((edit) => [edit.keyPath, edit.value]));
      expect(values.model_supports_vision).toBe(false);
      expect(values.vision_fallback_kind).toBe("multimodal");
      expect(values.vision_fallback_provider).toBe("qwen");
      expect(values.vision_fallback_model).toBe("qwen-vl-max");
    });
  });
});
