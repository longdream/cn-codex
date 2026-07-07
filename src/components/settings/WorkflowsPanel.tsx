import { useCallback, useEffect, useState } from "react";
import { useIntl } from "react-intl";
import { IconRoute, IconTrash, IconLoader2 } from "@tabler/icons-react";
import { workflowList, workflowDelete } from "../../api/workflow";
import type { WorkflowSummary } from "../../api/workflow";
import { SettingsPagination, usePagedItems } from "./SettingsPagination";

export function WorkflowsPanel() {
  const intl = useIntl();
  const [workflows, setWorkflows] = useState<WorkflowSummary[]>([]);
  const [loading, setLoading] = useState(true);
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

  const handleDelete = async (name: string) => {
    const confirmed = window.confirm(
      intl.formatMessage({ id: "settings.workflows.deleteConfirm" }),
    );
    if (!confirmed) return;
    try {
      await workflowDelete(name);
      setWorkflows((prev) => prev.filter((w) => w.name !== name));
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
      <div className="flex flex-col items-center gap-3 py-12 text-[var(--chat-muted)]">
        <IconRoute size={32} stroke={1.2} />
        <p className="text-sm">{intl.formatMessage({ id: "settings.workflows.empty" })}</p>
      </div>
    );
  }

  return (
    <div className="space-y-3">
      {pagedWorkflows.map((wf) => (
        <div
          key={wf.name}
          className="flex items-center gap-3 rounded-[var(--radius-md)] border border-[var(--chat-line)] bg-[var(--bg-secondary)] px-4 py-3"
        >
          <div className="flex h-9 w-9 flex-shrink-0 items-center justify-center rounded-[var(--radius-sm)] bg-[var(--chat-chip)] text-[var(--accent)]">
            <IconRoute size={18} stroke={1.6} />
          </div>
          <div className="min-w-0 flex-1">
            <div className="truncate text-sm font-medium text-[var(--chat-prose)]">
              {wf.title}
            </div>
            <div className="mt-0.5 flex items-center gap-2 text-xs text-[var(--chat-muted)]">
              <span>
                {intl.formatMessage(
                  { id: "settings.workflows.nodeCount" },
                  { count: wf.nodeCount },
                )}
              </span>
              <span className="opacity-50">·</span>
              <span>{wf.description}</span>
            </div>
          </div>
          <button
            type="button"
            onClick={() => void handleDelete(wf.name)}
            className="flex h-7 w-7 flex-shrink-0 items-center justify-center rounded-[var(--radius-sm)] text-[var(--chat-muted)] transition-colors hover:bg-[var(--chat-chip)] hover:text-red-500"
            title={intl.formatMessage({ id: "settings.workflows.delete" })}
          >
            <IconTrash size={14} stroke={1.8} />
          </button>
        </div>
      ))}
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
