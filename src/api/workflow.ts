import { invoke } from "@tauri-apps/api/core";

export interface WorkflowVariable {
  type: string;
  description: string;
  default?: string;
}

export interface WorkflowNode {
  nodeId: string;
  objective: string;
  tools: string[];
  argsHints?: Record<string, unknown>;
  dependsOn: string[];
  expectedOutput?: string;
  tokenBudget?: number;
}

export interface WorkflowDef {
  name: string;
  title: string;
  description: string;
  triggerPhrases: string[];
  sourceThreadId?: string;
  createdAt: string;
  variables: Record<string, WorkflowVariable>;
  nodes: WorkflowNode[];
  totalEstimatedTokens?: number;
}

export interface WorkflowSummary {
  name: string;
  title: string;
  description: string;
  nodeCount: number;
  createdAt: string;
  path: string;
}

export async function workflowExtract(threadId: string): Promise<WorkflowDef> {
  return invoke("workflow_extract", { threadId });
}

export async function workflowSave(workflow: WorkflowDef): Promise<string> {
  return invoke("workflow_save", { workflow });
}

export async function workflowList(): Promise<WorkflowSummary[]> {
  return invoke("workflow_list");
}

export async function workflowRead(name: string): Promise<WorkflowDef> {
  return invoke("workflow_read", { name });
}

export async function workflowDelete(name: string): Promise<void> {
  return invoke("workflow_delete", { name });
}
