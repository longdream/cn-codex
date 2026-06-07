import {
  IconAlertTriangle,
  IconFileText,
  IconShieldCheck,
  IconShieldX,
  IconTerminal2,
} from "@tabler/icons-react";
import { useCallback, useEffect, useState } from "react";
import { useIntl } from "react-intl";
import { rejectApproval, resolveApproval } from "../../api";
import type { RequestId } from "../../types";

interface ApprovalRequest {
  requestId: RequestId;
  method: string;
  params: Record<string, unknown>;
}

export function ApprovalModal() {
  const intl = useIntl();
  const [requests, setRequests] = useState<ApprovalRequest[]>([]);

  useEffect(() => {
    const handler = (event: Event) => {
      const detail = ((event as CustomEvent).detail ?? {}) as Partial<ApprovalRequest> & {
        id?: RequestId;
      };
      const method = detail.method;
      const params = detail.params;

      if (typeof method !== "string" || !params || typeof params !== "object") {
        return;
      }

      const request: ApprovalRequest = {
        requestId: detail.requestId ?? detail.id ?? "",
        method,
        params: params as Record<string, unknown>,
      };

      setRequests((previous) => [...previous, request]);
    };
    window.addEventListener("cn-codex:server-request", handler);
    return () => window.removeEventListener("cn-codex:server-request", handler);
  }, []);

  const current = requests[0];

  const handleApprove = useCallback(async () => {
    if (!current) {
      return;
    }

    const decision = current.method.includes("commandExecution")
      ? { decision: "accept" }
      : current.method.includes("fileChange")
        ? { decision: "accept" }
        : { decision: "accept" };

    try {
      await resolveApproval(current.requestId, decision);
    } catch (err) {
      console.error("Approve failed:", err);
    }
    setRequests((previous) => previous.slice(1));
  }, [current]);

  const handleReject = useCallback(async () => {
    if (!current) {
      return;
    }
    try {
      await rejectApproval(current.requestId, -32000, "User declined");
    } catch (err) {
      console.error("Reject failed:", err);
    }
    setRequests((previous) => previous.slice(1));
  }, [current]);

  if (!current) {
    return null;
  }

  const isCommand =
    current.method.includes("commandExecution") || current.method.includes("execCommand");
  const isFile = current.method.includes("fileChange") || current.method.includes("applyPatch");
  const title = isCommand
    ? intl.formatMessage({ id: "approval.command" })
    : isFile
      ? intl.formatMessage({ id: "approval.fileChange" })
      : intl.formatMessage({ id: "approval.title" });

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 p-4 backdrop-blur-sm">
      <div className="app-shell-panel w-full max-w-2xl overflow-hidden">
        <div className="flex items-center justify-between gap-3 border-b border-[var(--border-subtle)] px-5 py-3">
          <div className="flex items-center gap-3">
            <div className="flex h-8 w-8 items-center justify-center rounded-[var(--radius-sm)] bg-[rgba(245,158,11,0.12)] text-[var(--warning)]">
              <IconAlertTriangle size={16} stroke={2} />
            </div>
            <div>
              <p className="text-[11px] font-medium uppercase tracking-wide text-[var(--text-faint)]">
                {intl.formatMessage({ id: "approval.queue" })}
              </p>
              <h3 className="text-sm font-semibold text-[var(--text-strong)]">{title}</h3>
            </div>
          </div>

          {requests.length > 1 && <span className="app-pill">1 / {requests.length}</span>}
        </div>

        <div className="thin-scrollbar max-h-[70vh] overflow-y-auto px-5 py-4">
          {isCommand && "command" in current.params && (
            <section className="rounded-[var(--radius-lg)] border border-[var(--border-subtle)] bg-[var(--surface-contrast)] p-4">
              <div className="mb-2 flex items-center gap-2 text-sm font-medium text-[var(--text-strong)]">
                <IconTerminal2 size={16} stroke={1.8} />
                {intl.formatMessage({ id: "approval.commandLabel" })}
              </div>
              <pre className="whitespace-pre-wrap break-all rounded-[var(--radius-md)] border border-[var(--border-subtle)] bg-[var(--surface-main)] p-3 font-mono text-sm text-[var(--text-base)]">
                {Array.isArray(current.params.command)
                  ? (current.params.command as string[]).join(" ")
                  : String(current.params.command ?? "")}
              </pre>
              {"cwd" in current.params && (
                <p className="mt-2 break-all font-mono text-xs text-[var(--text-faint)]">
                  {String(current.params.cwd)}
                </p>
              )}
            </section>
          )}

          {isFile && "path" in current.params && (
            <section className="rounded-[var(--radius-lg)] border border-[var(--border-subtle)] bg-[var(--surface-contrast)] p-4">
              <div className="mb-2 flex items-center gap-2 text-sm font-medium text-[var(--text-strong)]">
                <IconFileText size={16} stroke={1.8} />
                {intl.formatMessage({ id: "approval.fileLabel" })}
              </div>
              <div className="rounded-[var(--radius-md)] border border-[var(--border-subtle)] bg-[var(--surface-main)] p-3 font-mono text-sm text-[var(--text-base)]">
                {String(current.params.path)}
              </div>
              {"patch" in current.params && (
                <pre className="thin-scrollbar mt-2 max-h-52 overflow-y-auto whitespace-pre-wrap rounded-[var(--radius-md)] border border-[var(--border-subtle)] bg-[var(--surface-main)] p-3 text-xs text-[var(--text-muted)]">
                  {String(current.params.patch)}
                </pre>
              )}
            </section>
          )}

          {isFile && !("path" in current.params) && (
            <section className="rounded-[var(--radius-lg)] border border-[var(--border-subtle)] bg-[var(--surface-contrast)] p-4">
              <div className="mb-2 flex items-center gap-2 text-sm font-medium text-[var(--text-strong)]">
                <IconFileText size={16} stroke={1.8} />
                {intl.formatMessage({ id: "approval.patchLabel" })}
              </div>
              <pre className="whitespace-pre-wrap rounded-[var(--radius-md)] border border-[var(--border-subtle)] bg-[var(--surface-main)] p-3 text-xs text-[var(--text-muted)]">
                {JSON.stringify(current.params, null, 2)}
              </pre>
            </section>
          )}

          {!isCommand && !isFile && (
            <section className="rounded-[var(--radius-lg)] border border-[var(--border-subtle)] bg-[var(--surface-contrast)] p-4">
              <pre className="whitespace-pre-wrap text-xs text-[var(--text-muted)]">
                {JSON.stringify(current.params, null, 2)}
              </pre>
            </section>
          )}
        </div>

        <div className="flex items-center justify-end gap-3 border-t border-[var(--border-subtle)] px-5 py-3">
          <button onClick={handleReject} className="danger-button flex items-center gap-2 px-4">
            <IconShieldX size={16} stroke={1.8} />
            {intl.formatMessage({ id: "approval.reject" })}
          </button>
          <button onClick={handleApprove} className="primary-button flex items-center gap-2 px-4">
            <IconShieldCheck size={16} stroke={1.8} />
            {intl.formatMessage({ id: "approval.approve" })}
          </button>
        </div>
      </div>
    </div>
  );
}
