import type {
  ChatMessage,
  FileChangeSnapshot,
  ThreadGoal,
  ToolCallItem,
} from "../stores/appStore";
import { buildPatchLineDiff } from "./lineDiff";

export type PlanProgressStepStatus = "pending" | "in_progress" | "completed";

export interface PlanProgressStep {
  step: string;
  status: PlanProgressStepStatus;
}

export interface RobotWorkflowNodeProgress extends PlanProgressStep {
  deliverySummary?: string;
}

export interface RobotWorkflowExecutionProgress {
  robotId?: string;
  rootObjective?: string;
  currentNodeIndex: number;
  completed: boolean;
  summarizedCount: number;
  nodes: RobotWorkflowNodeProgress[];
}

export interface PlanExecutionProgress {
  steps: PlanProgressStep[];
  hasExplicitPlan: boolean;
  robotWorkflow?: RobotWorkflowExecutionProgress;
  currentStep: number;
  totalSteps: number;
  changedFileCount: number;
  additions: number;
  deletions: number;
  running: boolean;
}

interface PatchArgumentStats {
  paths: string[];
  additions: number;
  deletions: number;
}

function parseArguments(value: string): Record<string, unknown> | null {
  try {
    const parsed = JSON.parse(value) as unknown;
    return parsed && typeof parsed === "object" && !Array.isArray(parsed)
      ? parsed as Record<string, unknown>
      : null;
  } catch {
    return null;
  }
}

function normalizePlanStatus(value: unknown): PlanProgressStepStatus {
  return value === "completed" || value === "in_progress" ? value : "pending";
}

function planFromToolCall(toolCall: ToolCallItem): PlanProgressStep[] | null {
  if (toolCall.name !== "update_plan" || toolCall.status === "failed") {
    return null;
  }
  const parsed = parseArguments(toolCall.arguments);
  if (!Array.isArray(parsed?.plan)) {
    return null;
  }
  const steps = parsed.plan
    .map((item) => {
      if (!item || typeof item !== "object") {
        return null;
      }
      const record = item as Record<string, unknown>;
      const step = typeof record.step === "string" ? record.step.trim() : "";
      return step ? { step, status: normalizePlanStatus(record.status) } : null;
    })
    .filter((item): item is PlanProgressStep => item !== null);
  return steps.length > 0 ? steps : null;
}

function extractPatchText(argumentsValue: string): string {
  const parsed = parseArguments(argumentsValue);
  const wrappedPatch = parsed?.patch ?? parsed?.command;
  return typeof wrappedPatch === "string" ? wrappedPatch : argumentsValue;
}

function normalizeDirectivePath(value: string): string {
  return value.replace(/\s+\*{3}\s*$/, "").trim();
}

export function parsePatchArgumentStats(argumentsValue: string): PatchArgumentStats {
  const lines = extractPatchText(argumentsValue)
    .replace(/\r\n/g, "\n")
    .replace(/\r/g, "\n")
    .split("\n");
  const paths = new Set<string>();
  let action: "add" | "update" | "delete" | null = null;
  let additions = 0;
  let deletions = 0;

  for (const line of lines) {
    if (line.startsWith("*** Add File: ")) {
      const path = normalizeDirectivePath(line.slice("*** Add File: ".length));
      if (path) paths.add(path);
      action = "add";
      continue;
    }
    if (line.startsWith("*** Update File: ")) {
      const path = normalizeDirectivePath(line.slice("*** Update File: ".length));
      if (path) paths.add(path);
      action = "update";
      continue;
    }
    if (line.startsWith("*** Delete File: ")) {
      const path = normalizeDirectivePath(line.slice("*** Delete File: ".length));
      if (path) paths.add(path);
      action = "delete";
      continue;
    }
    if (line === "*** End Patch") {
      action = null;
      continue;
    }
    if (action === "add" && line.startsWith("+")) {
      additions += 1;
      continue;
    }
    if (action === "update") {
      if (line.startsWith("+++ ") || line.startsWith("--- ")) {
        continue;
      }
      if (line.startsWith("+")) additions += 1;
      if (line.startsWith("-")) deletions += 1;
    }
  }

  return { paths: Array.from(paths), additions, deletions };
}

function stripTerminalLineBreak(text: string): string {
  const normalized = text.replace(/\r\n/g, "\n").replace(/\r/g, "\n");
  return normalized.endsWith("\n") ? normalized.slice(0, -1) : normalized;
}

function snapshotContents(snapshot: FileChangeSnapshot): [string, string] | null {
  if (snapshot.action === "created") {
    return ["", snapshot.afterContent ?? ""];
  }
  if (snapshot.action === "deleted") {
    return [snapshot.beforeContent ?? "", ""];
  }
  if (typeof snapshot.beforeContent !== "string" || typeof snapshot.afterContent !== "string") {
    return null;
  }
  return [snapshot.beforeContent, snapshot.afterContent];
}

function snapshotStats(snapshots: FileChangeSnapshot[]): PatchArgumentStats | null {
  const paths: string[] = [];
  let additions = 0;
  let deletions = 0;
  for (const snapshot of snapshots) {
    const contents = snapshotContents(snapshot);
    if (!contents) {
      return null;
    }
    paths.push(snapshot.path);
    for (const line of buildPatchLineDiff(
      stripTerminalLineBreak(contents[0]),
      stripTerminalLineBreak(contents[1]),
    )) {
      if (line.type === "add") additions += 1;
      if (line.type === "remove") deletions += 1;
    }
  }
  return { paths, additions, deletions };
}

function currentStepNumber(steps: PlanProgressStep[]): number {
  const activeIndex = steps.findIndex((step) => step.status === "in_progress");
  if (activeIndex >= 0) {
    return activeIndex + 1;
  }
  const completed = steps.filter((step) => step.status === "completed").length;
  return completed >= steps.length ? steps.length : Math.min(steps.length, completed + 1);
}

function shouldFinalizePlanSteps(
  running: boolean,
  goalStatus?: ThreadGoal["status"] | null,
): boolean {
  // Goal finished: always settle the plan UI even if transport still looks busy.
  if (goalStatus === "complete") {
    return true;
  }
  // Active/paused/blocked goals may still resume; keep the last update_plan snapshot.
  if (goalStatus != null) {
    return false;
  }
  // Chat / no-goal turns: settle once the turn is no longer running.
  return !running;
}

function finalizePlanSteps(steps: PlanProgressStep[]): PlanProgressStep[] {
  if (steps.every((step) => step.status === "completed")) {
    return steps;
  }
  return steps.map((step) => (
    step.status === "completed" ? step : { ...step, status: "completed" as const }
  ));
}

export function derivePlanExecutionProgress(
  messages: ChatMessage[],
  running: boolean,
  workflowProgress?: ThreadGoal["workflowProgress"],
  goalStatus?: ThreadGoal["status"] | null,
): PlanExecutionProgress | null {
  let lastUserIndex = -1;
  for (let index = messages.length - 1; index >= 0; index -= 1) {
    if (messages[index].role === "user") {
      lastUserIndex = index;
      break;
    }
  }
  const scope = messages.slice(Math.max(0, lastUserIndex));
  let steps: PlanProgressStep[] | null = null;
  let hasExplicitPlan = false;
  let latestSummary: ChatMessage["runSummary"];
  const changedPaths = new Set<string>();
  let additions = 0;
  let deletions = 0;

  for (const message of scope) {
    for (const change of message.fileChanges ?? []) {
      if (change.path.trim()) changedPaths.add(change.path.trim());
    }
    if (message.runSummary) {
      latestSummary = message.runSummary;
      for (const change of message.runSummary.changedFiles) {
        if (change.path.trim()) changedPaths.add(change.path.trim());
      }
    }
    for (const toolCall of message.toolCalls ?? []) {
      const parsedPlan = planFromToolCall(toolCall);
      if (parsedPlan) {
        steps = parsedPlan;
        hasExplicitPlan = true;
      }
      if (toolCall.name === "write_file" && toolCall.status === "success") {
        const parsed = parseArguments(toolCall.arguments);
        const path = typeof parsed?.path === "string" ? parsed.path.trim() : "";
        if (path) changedPaths.add(path);
      }
      if (toolCall.name !== "apply_patch" || toolCall.status === "failed") {
        continue;
      }

      const argumentStats = parsePatchArgumentStats(toolCall.arguments);
      const patchProgress = toolCall.patchProgress ?? [];
      if (toolCall.status !== "success" && patchProgress.length === 0) {
        continue;
      }
      for (const path of argumentStats.paths) changedPaths.add(path);
      for (const change of patchProgress) {
        if (change.path.trim()) changedPaths.add(change.path.trim());
      }
      const progressHasCounts = patchProgress.some(
        (change) => Number.isFinite(change.additions) || Number.isFinite(change.deletions),
      );
      if (progressHasCounts) {
        for (const change of patchProgress) {
          additions += Math.max(0, Number(change.additions ?? 0));
          deletions += Math.max(0, Number(change.deletions ?? 0));
        }
      } else {
        additions += argumentStats.additions;
        deletions += argumentStats.deletions;
      }
    }
  }

  let robotWorkflow: RobotWorkflowExecutionProgress | undefined;
  if (workflowProgress?.runtimeNodes.length) {
    const currentNodeIndex = Math.min(
      workflowProgress.runtimeNodes.length - 1,
      Math.max(0, workflowProgress.currentNodeIndex),
    );
    const completed = workflowProgress.completed === true;
    const deliveries = workflowProgress.nodeDeliveries ?? [];
    const nodes: RobotWorkflowNodeProgress[] = workflowProgress.runtimeNodes.map((step, index) => {
      const deliverySummary = deliveries[index]?.trim();
      return {
        step,
        status: index < currentNodeIndex || (completed && index === currentNodeIndex)
          ? "completed"
          : index === currentNodeIndex
            ? "in_progress"
            : "pending",
        ...(deliverySummary ? { deliverySummary } : {}),
      };
    });
    robotWorkflow = {
      ...(workflowProgress.robotId ? { robotId: workflowProgress.robotId } : {}),
      ...(workflowProgress.rootObjective ? { rootObjective: workflowProgress.rootObjective } : {}),
      currentNodeIndex,
      completed,
      summarizedCount: nodes.filter((node) => Boolean(node.deliverySummary)).length,
      nodes,
    };
    if (!steps) {
      steps = nodes.map(({ step, status }) => ({ step, status }));
    }
  }

  if (!steps) {
    return null;
  }

  const finalizeSteps = shouldFinalizePlanSteps(running, goalStatus);
  if (finalizeSteps) {
    steps = finalizePlanSteps(steps);
    if (robotWorkflow && !robotWorkflow.completed) {
      robotWorkflow = {
        ...robotWorkflow,
        completed: true,
        currentNodeIndex: Math.max(0, robotWorkflow.nodes.length - 1),
        nodes: robotWorkflow.nodes.map((node) => (
          node.status === "completed" ? node : { ...node, status: "completed" as const }
        )),
      };
    }
  }
  const effectiveRunning = finalizeSteps ? false : running;

  if (!running && latestSummary?.changedFileSnapshots?.length) {
    const finalStats = snapshotStats(latestSummary.changedFileSnapshots);
    if (finalStats) {
      additions = finalStats.additions;
      deletions = finalStats.deletions;
      for (const path of finalStats.paths) changedPaths.add(path);
    }
  }

  return {
    steps,
    hasExplicitPlan,
    ...(robotWorkflow ? { robotWorkflow } : {}),
    currentStep: currentStepNumber(steps),
    totalSteps: steps.length,
    changedFileCount: changedPaths.size,
    additions,
    deletions,
    running: effectiveRunning,
  };
}
