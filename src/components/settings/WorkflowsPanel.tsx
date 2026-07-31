import { useCallback, useEffect, useState } from "react";
import { useIntl } from "react-intl";
import {
  IconAlertTriangle,
  IconChevronDown,
  IconChevronRight,
  IconLoader2,
  IconRoute,
  IconTrash,
} from "@tabler/icons-react";
import {
  workflowDelete,
  workflowList,
  workflowRead,
  type WorkflowDef,
  type WorkflowSummary,
} from "../../api/workflow";
import { SettingsPagination, usePagedItems } from "./SettingsPagination";

export function WorkflowsPanel() {
  const intl = useIntl();
  const [workflows, setWorkflows] = useState<WorkflowSummary[]>([]);
  const [loading, setLoading] = useState(true);
  const [expandedName, setExpandedName] = useState<string | null>(null);
  const [detail, setDetail] = useState<WorkflowDef | null>(null);
  const [detailLoading, setDetailLoading] = useState(false);
  const [detailError, setDetailError] = useState<string>("");
  const {
    page,
    setPage,
    pageSize,
    totalItems,
    totalPages,
    pagedItems: pagedWorkflows,
  } = usePagedItems(workflows);

  const loadWorkflows = useCallback(async () => {
    setLoading(true);
    try {
      const list = await workflowList();
      setWorkflows(list);
    } catch (err) {
      console.error("Failed to load workflows:", err);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void loadWorkflows();
  }, [loadWorkflows]);

  const handleToggleExpand = async (name: string) => {
    if (expandedName === name) {
      setExpandedName(null);
      setDetail(null);
      setDetailError("");
      setDetailLoading(false);
      return;
    }

    setExpandedName(name);
    setDetail(null);
    setDetailError("");
    setDetailLoading(true);
    try {
      const nextDetail = await workflowRead(name);
      setDetail(nextDetail);
    } catch (err) {
      console.error("Failed to read workflow:", err);
      setDetailError(String(err));
    } finally {
      setDetailLoading(false);
    }
  };

  const handleDelete = async (name: string) => {
    const confirmed = window.confirm(
      intl.formatMessage({ id: "settings.workflows.deleteConfirm" }),
    );
    if (!confirmed) return;
    try {
      await workflowDelete(name);
      setWorkflows((prev) => prev.filter((w) => w.name !== name));
      if (expandedName === name) {
        setExpandedName(null);
        setDetail(null);
        setDetailError("");
      }
    } catch (err) {
      console.error("Failed to delete workflow:", err);
    }
  };

  if (loading) {
    return (
      <div className="flex items-center justify-center py-12">
        <IconLoader2 size={24} stroke={1.5} className="animate-spin text-[var(--chat-muted)]" />
      </div>
    );
  }

  if (workflows.length === 0) {
    return (
      <div className="space-y-4">
        <div className="flex flex-col items-center gap-3 py-12 text-[var(--chat-muted)]">
          <IconRoute size={32} stroke={1.2} />
          <p className="text-sm">{intl.formatMessage({ id: "settings.workflows.empty" })}</p>
        </div>
      </div>
    );
  }

  return (
    <div className="space-y-3">
      {pagedWorkflows.map((wf) => {
        const isExpanded = expandedName === wf.name;
        const isCurrentDetail = detail?.name === wf.name;

        return (
          <div
            key={wf.name}
            className="rounded-[var(--radius-md)] border border-[var(--chat-line)] bg-[var(--bg-secondary)]"
          >
            <div className="flex items-center gap-3 px-4 py-3">
              <button
                type="button"
                onClick={() => void handleToggleExpand(wf.name)}
                className="flex min-w-0 flex-1 items-center gap-3 text-left transition-colors"
              >
                <div className="flex h-9 w-9 flex-shrink-0 items-center justify-center rounded-[var(--radius-sm)] bg-[var(--chat-chip)] text-[var(--accent)]">
                  <IconRoute size={18} stroke={1.6} />
                </div>
                <div className="min-w-0 flex-1">
                  <div className="flex items-center gap-2">
                    {isExpanded ? (
                      <IconChevronDown size={14} stroke={1.8} className="shrink-0 text-[var(--chat-muted)]" />
                    ) : (
                      <IconChevronRight size={14} stroke={1.8} className="shrink-0 text-[var(--chat-muted)]" />
                    )}
                    <div className="min-w-0">
                      <div className="truncate text-sm font-medium text-[var(--chat-prose)]">
                        {wf.title || wf.name}
                      </div>
                      <div className="mt-0.5 flex flex-wrap items-center gap-2 text-xs text-[var(--chat-muted)]">
                        <span>
                          {intl.formatMessage(
                            { id: "settings.workflows.nodeCount" },
                            { count: wf.nodeCount },
                          )}
                        </span>
                        {wf.description && (
                          <>
                            <span className="opacity-50">·</span>
                            <span className="truncate">{wf.description}</span>
                          </>
                        )}
                      </div>
                    </div>
                  </div>
                </div>
              </button>
              <button
                type="button"
                onClick={() => void handleDelete(wf.name)}
                className="flex h-7 w-7 flex-shrink-0 items-center justify-center rounded-[var(--radius-sm)] text-[var(--chat-muted)] transition-colors hover:bg-[var(--chat-chip)] hover:text-red-500"
                title={intl.formatMessage({ id: "settings.workflows.delete" })}
              >
                <IconTrash size={14} stroke={1.8} />
              </button>
            </div>

            {isExpanded && (
              <div className="border-t border-[var(--chat-line)] px-4 py-4">
                {detailLoading && (
                  <div className="flex items-center gap-2 text-xs text-[var(--chat-muted)]">
                    <IconLoader2 size={14} stroke={1.8} className="animate-spin" />
                    <span>{intl.formatMessage({ id: "common.loading" })}</span>
                  </div>
                )}

                {!detailLoading && detailError && (
                  <p className="break-words text-xs text-red-400">{detailError}</p>
                )}

                {!detailLoading && !detailError && isCurrentDetail && detail && (
                  <div className="space-y-4">
                    <div className="space-y-1">
                      <p className="text-xs uppercase tracking-[0.14em] text-[var(--chat-faint)]">
                        {intl.formatMessage({ id: "settings.workflows.detailTitle" })}
                      </p>
                      <p className="text-sm font-medium text-[var(--chat-prose)]">
                        {detail.title || detail.name}
                      </p>
                      {detail.description && (
                        <p className="text-xs leading-relaxed text-[var(--chat-muted)]">
                          {detail.description}
                        </p>
                      )}
                      <div className="flex flex-wrap gap-2 pt-1 text-[11px] text-[var(--chat-faint)]">
                        <span className="rounded-full bg-[var(--chat-chip)] px-2 py-0.5 font-mono">
                          {detail.name}
                        </span>
                        {detail.createdAt && (
                          <span className="rounded-full bg-[var(--chat-chip)] px-2 py-0.5">
                            {intl.formatMessage(
                              { id: "settings.workflows.createdAt" },
                              { date: detail.createdAt },
                            )}
                          </span>
                        )}
                        {typeof detail.totalEstimatedTokens === "number" && (
                          <span className="rounded-full bg-[var(--chat-chip)] px-2 py-0.5">
                            {intl.formatMessage(
                              { id: "settings.workflows.estimatedTokens" },
                              { count: detail.totalEstimatedTokens },
                            )}
                          </span>
                        )}
                      </div>
                    </div>

                    {detail.triggerPhrases?.length > 0 && (
                      <div>
                        <p className="mb-1.5 text-[11px] font-semibold uppercase tracking-wider text-[var(--chat-faint)]">
                          {intl.formatMessage({ id: "settings.workflows.triggerPhrases" })}
                        </p>
                        <div className="flex flex-wrap gap-1.5">
                          {detail.triggerPhrases.map((phrase) => (
                            <span
                              key={phrase}
                              className="rounded-full bg-[var(--accent-soft)] px-2 py-0.5 text-[11px] text-[var(--accent-strong)]"
                            >
                              {phrase}
                            </span>
                          ))}
                        </div>
                      </div>
                    )}

                    {Object.keys(detail.variables ?? {}).length > 0 && (
                      <div>
                        <p className="mb-1.5 text-[11px] font-semibold uppercase tracking-wider text-[var(--chat-faint)]">
                          {intl.formatMessage({ id: "workflow.extract.variables" })}
                        </p>
                        <div className="space-y-1.5">
                          {Object.entries(detail.variables).map(([name, variable]) => (
                            <div
                              key={name}
                              className="rounded-[var(--radius-sm)] border border-[var(--chat-line)] bg-[var(--surface-contrast)] px-3 py-2 text-xs text-[var(--chat-muted)]"
                            >
                              <code className="text-[var(--accent)]">{`{{${name}}}`}</code>
                              <span className="ml-1 opacity-70">({variable.type})</span>
                              {" — "}
                              {variable.description}
                              {variable.default && (
                                <span className="opacity-60">
                                  {" "}
                                  (
                                  {intl.formatMessage(
                                    { id: "settings.workflows.defaultValue" },
                                    { value: variable.default },
                                  )}
                                  )
                                </span>
                              )}
                            </div>
                          ))}
                        </div>
                      </div>
                    )}

                    <div>
                      <p className="mb-2 text-[11px] font-semibold uppercase tracking-wider text-[var(--chat-faint)]">
                        {intl.formatMessage({ id: "workflow.extract.nodes" })} ({detail.nodes.length})
                      </p>
                      <div className="space-y-2">
                        {detail.nodes.map((node, index) => (
                          <div
                            key={node.nodeId || `${detail.name}-node-${index}`}
                            className="rounded-[var(--radius-sm)] border border-[var(--chat-line)] bg-[var(--surface-contrast)] px-3 py-2"
                          >
                            <div className="flex items-baseline gap-2">
                              <span className="flex h-5 w-5 flex-shrink-0 items-center justify-center rounded-full bg-[var(--accent)] text-[10px] font-bold text-white">
                                {index + 1}
                              </span>
                              <div className="min-w-0">
                                <p className="text-sm font-medium text-[var(--chat-prose)]">
                                  {node.objective}
                                </p>
                                {node.nodeId && (
                                  <p className="mt-0.5 font-mono text-[10px] text-[var(--chat-faint)]">
                                    {node.nodeId}
                                  </p>
                                )}
                              </div>
                            </div>

                            {node.tools?.length > 0 && (
                              <div className="mt-1.5 ml-7 flex flex-wrap gap-1">
                                {node.tools.map((tool) => (
                                  <span
                                    key={`${node.nodeId}-${tool}`}
                                    className="rounded-[var(--radius-xs)] bg-[var(--chat-chip)] px-1.5 py-0.5 text-[10px] text-[var(--chat-faint)]"
                                  >
                                    {tool}
                                  </span>
                                ))}
                              </div>
                            )}

                            {node.dependsOn?.length > 0 && (
                              <p className="mt-1.5 ml-7 text-[11px] text-[var(--chat-muted)]">
                                {intl.formatMessage(
                                  { id: "settings.workflows.dependsOn" },
                                  { nodes: node.dependsOn.join(", ") },
                                )}
                              </p>
                            )}

                            {node.expectedOutput && (
                              <p className="mt-1 ml-7 text-[11px] text-[var(--chat-muted)]">
                                {intl.formatMessage(
                                  { id: "settings.workflows.expectedOutput" },
                                  { output: node.expectedOutput },
                                )}
                              </p>
                            )}

                            {typeof node.tokenBudget === "number" && (
                              <p className="mt-1 ml-7 text-[11px] text-[var(--chat-faint)]">
                                {intl.formatMessage(
                                  { id: "settings.workflows.tokenBudget" },
                                  { count: node.tokenBudget },
                                )}
                              </p>
                            )}

                            {node.knownFailures && node.knownFailures.length > 0 && (
                              <div className="mt-2 ml-7 space-y-1.5">
                                {node.knownFailures.map((failure, failureIndex) => (
                                  <div
                                    key={`${node.nodeId}-failure-${failureIndex}`}
                                    className="rounded-[var(--radius-xs)] border border-[var(--warning-border)] bg-[var(--warning-soft)] px-2 py-1.5"
                                  >
                                    <div className="flex items-start gap-1.5">
                                      <IconAlertTriangle
                                        size={12}
                                        stroke={1.8}
                                        className="mt-0.5 flex-shrink-0 text-[var(--warning)]"
                                      />
                                      <div className="min-w-0">
                                        <p className="text-[11px] font-medium text-[var(--warning)]">
                                          {intl.formatMessage({ id: "workflow.extract.knownTrap" })}: {failure.error}
                                        </p>
                                        <p className="mt-0.5 text-[10px] text-[var(--chat-muted)]">
                                          {intl.formatMessage({ id: "workflow.extract.cause" })}: {failure.cause}
                                        </p>
                                        <p className="text-[10px] text-[var(--accent)]">
                                          {intl.formatMessage({ id: "workflow.extract.fix" })}: {failure.fix}
                                        </p>
                                      </div>
                                    </div>
                                  </div>
                                ))}
                              </div>
                            )}
                          </div>
                        ))}
                      </div>
                    </div>
                  </div>
                )}
              </div>
            )}
          </div>
        );
      })}
      <SettingsPagination
        page={page}
        onPageChange={setPage}
        pageSize={pageSize}
        totalItems={totalItems}
        totalPages={totalPages}
      />
    </div>
  );
}
