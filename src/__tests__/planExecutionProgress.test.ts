import { describe, expect, it } from "vitest";
import type { ChatMessage, ToolCallItem } from "../stores/appStore";
import {
  derivePlanExecutionProgress,
  parsePatchArgumentStats,
} from "../utils/planExecutionProgress";

function userMessage(id = "user-1"): ChatMessage {
  return { id, role: "user", content: "implement", timestamp: 1 };
}

function toolMessage(toolCalls: ToolCallItem[]): ChatMessage {
  return { id: `tools-${toolCalls[0]?.id}`, role: "system", content: "", timestamp: 2, toolCalls };
}

function planCall(statuses: Array<"pending" | "in_progress" | "completed">): ToolCallItem {
  return {
    id: "plan-call",
    name: "update_plan",
    arguments: JSON.stringify({
      plan: statuses.map((status, index) => ({ step: `Step ${index + 1}`, status })),
    }),
    status: "success",
    displayLabel: `${statuses.length} steps`,
  };
}

describe("plan execution progress", () => {
  it("derives the current step and live patch totals", () => {
    const patch = `*** Begin Patch
*** Update File: src/app.ts
@@
-old
+new
*** Add File: src/new.ts
+first
*** End Patch`;
    const messages = [
      userMessage(),
      toolMessage([
        planCall(["completed", "in_progress", "pending"]),
        {
          id: "patch-call",
          name: "apply_patch",
          arguments: JSON.stringify({ patch }),
          status: "success",
          displayLabel: "src/app.ts",
        },
      ]),
    ];

    expect(derivePlanExecutionProgress(messages, true)).toMatchObject({
      currentStep: 2,
      totalSteps: 3,
      changedFileCount: 2,
      additions: 2,
      deletions: 1,
      running: true,
    });
  });

  it("prefers persisted progress counts when available", () => {
    const messages = [
      userMessage(),
      toolMessage([
        planCall(["in_progress"]),
        {
          id: "patch-call",
          name: "apply_patch",
          arguments: "*** Begin Patch\n*** Update File: src/app.ts\n-old\n+new\n*** End Patch",
          status: "success",
          displayLabel: "src/app.ts",
          patchProgress: [{
            path: "src/app.ts",
            action: "modified",
            additions: 7,
            deletions: 3,
          }],
        },
      ]),
    ];

    expect(derivePlanExecutionProgress(messages, true)).toMatchObject({
      changedFileCount: 1,
      additions: 7,
      deletions: 3,
    });
  });

  it("uses final snapshots after a run completes", () => {
    const messages: ChatMessage[] = [
      userMessage(),
      toolMessage([planCall(["completed", "completed"])]),
      {
        id: "summary",
        role: "system",
        content: "",
        timestamp: 3,
        runSummary: {
          turnId: "turn-1",
          mode: "goal",
          changedFiles: [
            { path: "src/app.ts", action: "modified" },
            { path: "src/new.ts", action: "created" },
          ],
          changedFileSnapshots: [
            {
              path: "src/app.ts",
              action: "modified",
              beforeContent: "alpha\nbeta\n",
              afterContent: "alpha\ngamma\ndelta\n",
            },
            {
              path: "src/new.ts",
              action: "created",
              afterContent: "one\ntwo\n",
            },
          ],
        },
      },
    ];

    expect(derivePlanExecutionProgress(messages, false)).toMatchObject({
      currentStep: 2,
      changedFileCount: 2,
      additions: 4,
      deletions: 1,
      running: false,
    });
  });

  it("does not leak a previous plan into the next user turn", () => {
    const messages = [
      userMessage(),
      toolMessage([planCall(["completed"])]),
      userMessage("user-2"),
    ];
    expect(derivePlanExecutionProgress(messages, false)).toBeNull();
  });

  it("uses robot workflow nodes when the model did not call update_plan", () => {
    const messages = [userMessage()];

    expect(derivePlanExecutionProgress(messages, true, {
      robotId: "full-stack",
      currentNodeIndex: 1,
      rootObjective: "Ship the feature",
      runtimeNodes: ["Analyze", "Implement", "Verify"],
      nodeDeliveries: [
        "Artifacts: analysis.md\nDecisions: use the existing API\nValidation: reviewed\nOpen items: none",
      ],
    })).toMatchObject({
      currentStep: 2,
      totalSteps: 3,
      changedFileCount: 0,
      running: true,
      hasExplicitPlan: false,
      steps: [
        { step: "Analyze", status: "completed" },
        { step: "Implement", status: "in_progress" },
        { step: "Verify", status: "pending" },
      ],
      robotWorkflow: {
        robotId: "full-stack",
        rootObjective: "Ship the feature",
        currentNodeIndex: 1,
        summarizedCount: 1,
        nodes: [
          { step: "Analyze", status: "completed", deliverySummary: expect.stringContaining("Artifacts:") },
          { step: "Implement", status: "in_progress" },
          { step: "Verify", status: "pending" },
        ],
      },
    });
  });

  it("keeps robot workflow nodes alongside an explicit update_plan", () => {
    const messages = [
      userMessage(),
      toolMessage([planCall(["completed", "in_progress"])]),
    ];

    expect(derivePlanExecutionProgress(messages, true, {
      currentNodeIndex: 0,
      runtimeNodes: ["Robot step"],
    })).toMatchObject({
      currentStep: 2,
      totalSteps: 2,
      hasExplicitPlan: true,
      steps: [
        { step: "Step 1", status: "completed" },
        { step: "Step 2", status: "in_progress" },
      ],
      robotWorkflow: {
        currentNodeIndex: 0,
        nodes: [{ step: "Robot step", status: "in_progress" }],
      },
    });
  });

  it("marks the final robot node completed when the workflow snapshot is complete", () => {
    expect(derivePlanExecutionProgress([userMessage()], false, {
      currentNodeIndex: 1,
      runtimeNodes: ["Implement", "Verify"],
      nodeDeliveries: ["Artifacts: code", "Validation: tests passed"],
      completed: true,
    })).toMatchObject({
      currentStep: 2,
      robotWorkflow: {
        completed: true,
        summarizedCount: 2,
        nodes: [
          { status: "completed" },
          { status: "completed" },
        ],
      },
    });
  });

  it("does not report a patch before the backend confirms it was applied", () => {
    const messages = [
      userMessage(),
      toolMessage([
        planCall(["in_progress"]),
        {
          id: "patch-call",
          name: "apply_patch",
          arguments: "*** Begin Patch\n*** Add File: src/new.ts\n+new\n*** End Patch",
          status: "running" as const,
          displayLabel: "src/new.ts",
        },
      ]),
    ];

    expect(derivePlanExecutionProgress(messages, true)).toMatchObject({
      changedFileCount: 0,
      additions: 0,
      deletions: 0,
    });
  });

  it("parses wrapped patches without counting unified diff headers", () => {
    const stats = parsePatchArgumentStats(JSON.stringify({
      command: `*** Begin Patch
*** Update File: src/app.ts
--- a/src/app.ts
+++ b/src/app.ts
@@
 context
-old
+new
*** End Patch`,
    }));

    expect(stats).toEqual({ paths: ["src/app.ts"], additions: 1, deletions: 1 });
  });
});
