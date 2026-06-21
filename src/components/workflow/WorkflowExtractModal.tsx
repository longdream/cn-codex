import { useState } from "react";
import { useIntl } from "react-intl";
import {
  IconRoute,
  IconLoader2,
  IconCheck,
  IconX,
  IconAlertTriangle,
} from "@tabler/icons-react";
import { useAppStore } from "../../stores/appStore";
import { workflowExtract, workflowSave } from "../../api/workflow";
import type { WorkflowDef } from "../../api/workflow";

type ExtractState = "idle" | "extracting" | "preview" | "saving" | "done" | "error";

export function WorkflowExtractModal() {
  const intl = useIntl();
  const threadId = useAppStore((s) => s.workflowExtractThreadId);
  const setThreadId = useAppStore((s) => s.setWorkflowExtractThreadId);

  const [state, setState] = useState<ExtractState>("idle");
  const [workflow, setWorkflow] = useState<WorkflowDef | null>(null);
  const [error, setError] = useState<string>("");
  const [editTitle, setEditTitle] = useState("");
  const [editDescription, setEditDescription] = useState("");

  if (!threadId) return null;

  const handleExtract = async () => {
    setState("extracting");
    setError("");
    try {
      const result = await workflowExtract(threadId);
      setWorkflow(result);
      setEditTitle(result.title);
      setEditDescription(result.description);
      setState("preview");
    } catch (err) {
      setError(String(err));
      setState("error");
    }
  };

  const handleSave = async () => {
    if (!workflow) return;
    setState("saving");
    try {
      const updated = { ...workflow, title: editTitle, description: editDescription };
      await workflowSave(updated);
      setState("done");
      setTimeout(() => {
        setThreadId(null);
        setState("idle");
        setWorkflow(null);
      }, 1500);
    } catch (err) {
      setError(String(err));
      setState("error");
    }
  };

  const handleClose = () => {
    setThreadId(null);
    setState("idle");
    setWorkflow(null);
    setError("");
  };

  // Auto-start extraction on open
  if (state === "idle") {
    void handleExtract();
  }

  return (
    <div className="fixed inset-0 z-[200] flex items-center justify-center bg-black/50">
      <div className="relative mx-4 flex max-h-[80vh] w-full max-w-[600px] flex-col rounded-[var(--radius-lg)] border border-[var(--chat-line)] bg-[var(--bg-primary)] shadow-2xl">
        {/* Header */}
        <div className="flex items-center gap-2 border-b border-[var(--chat-line)] px-5 py-4">
          <IconRoute size={20} stroke={1.8} className="text-[var(--accent)]" />
          <h2 className="text-base font-semibold text-[var(--chat-prose)]">
            {intl.formatMessage({ id: "workflow.extract.title" })}
          </h2>
          <button
            type="button"
            onClick={handleClose}
            className="ml-auto flex h-7 w-7 items-center justify-center rounded-[var(--radius-sm)] text-[var(--chat-muted)] hover:bg-[var(--chat-chip)]"
          >
            <IconX size={16} stroke={2} />
          </button>
        </div>

        {/* Body */}
        <div className="flex-1 overflow-y-auto px-5 py-4">
          {state === "extracting" && (
            <div className="flex flex-col items-center gap-3 py-8">
              <IconLoader2 size={32} stroke={1.5} className="animate-spin text-[var(--accent)]" />
              <p className="text-sm text-[var(--chat-muted)]">
                {intl.formatMessage({ id: "workflow.extract.analyzing" })}
              </p>
            </div>
          )}

          {state === "error" && (
            <div className="flex flex-col items-center gap-3 py-8">
              <IconAlertTriangle size={32} stroke={1.5} className="text-[var(--warning)]" />
              <p className="text-sm text-[var(--chat-muted)]">{error}</p>
              <button
                type="button"
                onClick={handleExtract}
                className="rounded-[var(--radius-sm)] border border-[var(--chat-line)] px-3 py-1.5 text-xs hover:border-[var(--accent-border)]"
              >
                {intl.formatMessage({ id: "workflow.extract.retry" })}
              </button>
            </div>
          )}

          {state === "done" && (
            <div className="flex flex-col items-center gap-3 py-8">
              <IconCheck size={32} stroke={1.5} className="text-[var(--accent)]" />
              <p className="text-sm text-[var(--chat-muted)]">
                {intl.formatMessage({ id: "workflow.extract.saved" })}
              </p>
            </div>
          )}

          {(state === "preview" || state === "saving") && workflow && (
            <div className="space-y-4">
              {/* Editable title */}
              <div>
                <label className="mb-1 block text-xs font-medium text-[var(--chat-muted)]">
                  {intl.formatMessage({ id: "workflow.extract.fieldTitle" })}
                </label>
                <input
                  type="text"
                  value={editTitle}
                  onChange={(e) => setEditTitle(e.target.value)}
                  className="w-full rounded-[var(--radius-sm)] border border-[var(--chat-line)] bg-[var(--bg-secondary)] px-3 py-2 text-sm text-[var(--chat-prose)] outline-none focus:border-[var(--accent-border)]"
                />
              </div>

              {/* Editable description */}
              <div>
                <label className="mb-1 block text-xs font-medium text-[var(--chat-muted)]">
                  {intl.formatMessage({ id: "workflow.extract.fieldDescription" })}
                </label>
                <textarea
                  value={editDescription}
                  onChange={(e) => setEditDescription(e.target.value)}
                  rows={2}
                  className="w-full resize-none rounded-[var(--radius-sm)] border border-[var(--chat-line)] bg-[var(--bg-secondary)] px-3 py-2 text-sm text-[var(--chat-prose)] outline-none focus:border-[var(--accent-border)]"
                />
              </div>

              {/* Nodes preview */}
              <div>
                <label className="mb-2 block text-xs font-medium text-[var(--chat-muted)]">
                  {intl.formatMessage({ id: "workflow.extract.nodes" })} ({workflow.nodes.length})
                </label>
                <div className="space-y-2">
                  {workflow.nodes.map((node, i) => (
                    <div
                      key={node.nodeId}
                      className="rounded-[var(--radius-sm)] border border-[var(--chat-line)] bg-[var(--bg-secondary)] px-3 py-2"
                    >
                      <div className="flex items-baseline gap-2">
                        <span className="flex h-5 w-5 flex-shrink-0 items-center justify-center rounded-full bg-[var(--accent)] text-[10px] font-bold text-white">
                          {i + 1}
                        </span>
                        <span className="text-sm font-medium text-[var(--chat-prose)]">
                          {node.objective}
                        </span>
                      </div>
                      <div className="mt-1 ml-7 flex flex-wrap gap-1">
                        {node.tools.map((tool) => (
                          <span
                            key={tool}
                            className="rounded-[var(--radius-xs)] bg-[var(--chat-chip)] px-1.5 py-0.5 text-[10px] text-[var(--chat-muted)]"
                          >
                            {tool}
                          </span>
                        ))}
                      </div>
                    </div>
                  ))}
                </div>
              </div>

              {/* Variables */}
              {Object.keys(workflow.variables).length > 0 && (
                <div>
                  <label className="mb-1 block text-xs font-medium text-[var(--chat-muted)]">
                    {intl.formatMessage({ id: "workflow.extract.variables" })}
                  </label>
                  <div className="space-y-1">
                    {Object.entries(workflow.variables).map(([name, v]) => (
                      <div key={name} className="text-xs text-[var(--chat-muted)]">
                        <code className="text-[var(--accent)]">{`{{${name}}}`}</code>
                        {" — "}
                        {v.description}
                        {v.default && <span className="opacity-60"> (默认: {v.default})</span>}
                      </div>
                    ))}
                  </div>
                </div>
              )}
            </div>
          )}
        </div>

        {/* Footer */}
        {(state === "preview" || state === "saving") && (
          <div className="flex items-center justify-end gap-2 border-t border-[var(--chat-line)] px-5 py-3">
            <button
              type="button"
              onClick={handleClose}
              className="rounded-[var(--radius-sm)] border border-[var(--chat-line)] px-4 py-1.5 text-xs text-[var(--chat-muted)] hover:border-[var(--chat-prose)]"
            >
              {intl.formatMessage({ id: "workflow.extract.cancel" })}
            </button>
            <button
              type="button"
              onClick={handleSave}
              disabled={state === "saving" || !editTitle.trim()}
              className="flex items-center gap-1.5 rounded-[var(--radius-sm)] bg-[var(--accent)] px-4 py-1.5 text-xs font-medium text-white disabled:opacity-50"
            >
              {state === "saving" && <IconLoader2 size={12} className="animate-spin" />}
              {intl.formatMessage({ id: "workflow.extract.save" })}
            </button>
          </div>
        )}
      </div>
    </div>
  );
}
