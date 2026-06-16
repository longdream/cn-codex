import {
  IconAlertTriangle,
  IconFileText,
  IconShieldCheck,
  IconShieldX,
  IconTerminal2,
} from "@tabler/icons-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useIntl } from "react-intl";
import { rejectApproval, resolveApproval } from "../../api";
import type { RequestId } from "../../types";

interface ApprovalRequest {
  requestId: RequestId;
  method: string;
  params: Record<string, unknown>;
}

interface UserInputQuestionOption {
  label: string;
  description: string;
}

interface UserInputQuestion {
  id: string;
  header: string;
  question: string;
  options?: UserInputQuestionOption[];
}

export function ApprovalModal() {
  const intl = useIntl();
  const [requests, setRequests] = useState<ApprovalRequest[]>([]);
  const [answers, setAnswers] = useState<Record<string, string>>({});

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
  const isUserInput = !!current && (
    current.method.includes("request_user_input") ||
    current.method.includes("requestUserInput")
  );
  const isPermissions = !!current && (
    current.method.includes("request_permissions") ||
    current.method.includes("requestPermissions")
  );
  const userInputQuestions = useMemo(
    () => (isUserInput ? userInputQuestionsFromParams(current?.params ?? {}) : []),
    [current?.params, isUserInput],
  );

  useEffect(() => {
    if (!current || !isUserInput) {
      setAnswers({});
      return;
    }

    setAnswers(
      Object.fromEntries(
        userInputQuestions.map((question) => [
          question.id,
          question.options?.[0]?.label ?? "",
        ]),
      ),
    );
  }, [current?.requestId, isUserInput]);

  const handleApprove = useCallback(async () => {
    if (!current) {
      return;
    }

    const decision = isUserInput
      ? {
          answers: Object.fromEntries(
            userInputQuestions.map((question) => [
              question.id,
              { answers: [answers[question.id] ?? question.options?.[0]?.label ?? ""] },
            ]),
          ),
        }
      : isPermissions
        ? {
            permissions: current.params.permissions ?? {},
            scope: "turn",
            strict_auto_review: false,
          }
      : current.method.includes("commandExecution")
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
  }, [answers, current, isPermissions, isUserInput, userInputQuestions]);

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
      : isPermissions
        ? "Permission Request"
        : intl.formatMessage({ id: "approval.title" });

  return (
    <div className="fixed bottom-0 left-0 right-0 top-8 z-50 flex items-center justify-center bg-black/50 p-4 backdrop-blur-sm">
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

          {isUserInput && userInputQuestions.length > 0 && (
            <section className="space-y-3">
              {userInputQuestions.map((question) => (
                <div
                  key={question.id}
                  className="rounded-[var(--radius-lg)] border border-[var(--border-subtle)] bg-[var(--surface-contrast)] p-4"
                >
                  <div className="mb-3">
                    <p className="text-[11px] font-medium uppercase tracking-wide text-[var(--text-faint)]">
                      {question.header}
                    </p>
                    <h4 className="mt-1 text-sm font-semibold text-[var(--text-strong)]">
                      {question.question}
                    </h4>
                  </div>

                  {question.options?.length ? (
                    <div className="space-y-2">
                      {question.options.map((option) => {
                        const selected = answers[question.id] === option.label;
                        return (
                          <button
                            key={option.label}
                            type="button"
                            onClick={() =>
                              setAnswers((previous) => ({
                                ...previous,
                                [question.id]: option.label,
                              }))
                            }
                            className={`w-full rounded-[var(--radius-sm)] border px-3 py-2 text-left transition-colors ${
                              selected
                                ? "border-[var(--accent-border)] bg-[var(--surface-elevated)]"
                                : "border-[var(--border-subtle)] bg-[var(--surface-main)] hover:border-[var(--border-strong)]"
                            }`}
                          >
                            <div className="text-sm font-medium text-[var(--text-strong)]">
                              {option.label}
                            </div>
                            <div className="mt-0.5 text-xs text-[var(--text-muted)]">
                              {option.description}
                            </div>
                          </button>
                        );
                      })}
                      <textarea
                        value={
                          question.options.some((option) => option.label === answers[question.id])
                            ? ""
                            : answers[question.id] ?? ""
                        }
                        onChange={(event) =>
                          setAnswers((previous) => ({
                            ...previous,
                            [question.id]: event.target.value,
                          }))
                        }
                        placeholder="Other"
                        rows={2}
                        className="w-full resize-none rounded-[var(--radius-sm)] border border-[var(--border-subtle)] bg-[var(--surface-main)] px-3 py-2 text-sm text-[var(--text-base)] outline-none focus:border-[var(--accent-border)]"
                      />
                    </div>
                  ) : (
                    <textarea
                      value={answers[question.id] ?? ""}
                      onChange={(event) =>
                        setAnswers((previous) => ({
                          ...previous,
                          [question.id]: event.target.value,
                        }))
                      }
                      rows={3}
                      className="w-full resize-none rounded-[var(--radius-sm)] border border-[var(--border-subtle)] bg-[var(--surface-main)] px-3 py-2 text-sm text-[var(--text-base)] outline-none focus:border-[var(--accent-border)]"
                    />
                  )}
                </div>
              ))}
            </section>
          )}

          {isPermissions && (
            <section className="space-y-3 rounded-[var(--radius-lg)] border border-[var(--border-subtle)] bg-[var(--surface-contrast)] p-4">
              {typeof current.params.reason === "string" && current.params.reason.trim() && (
                <div>
                  <p className="text-[11px] font-medium uppercase tracking-wide text-[var(--text-faint)]">
                    Reason
                  </p>
                  <p className="mt-1 text-sm text-[var(--text-strong)]">
                    {current.params.reason}
                  </p>
                </div>
              )}
              {"cwd" in current.params && (
                <div>
                  <p className="text-[11px] font-medium uppercase tracking-wide text-[var(--text-faint)]">
                    Workspace
                  </p>
                  <p className="mt-1 break-all font-mono text-xs text-[var(--text-muted)]">
                    {String(current.params.cwd)}
                  </p>
                </div>
              )}
              <div>
                <p className="text-[11px] font-medium uppercase tracking-wide text-[var(--text-faint)]">
                  Permissions
                </p>
                <pre className="thin-scrollbar mt-2 max-h-52 overflow-y-auto whitespace-pre-wrap rounded-[var(--radius-md)] border border-[var(--border-subtle)] bg-[var(--surface-main)] p-3 text-xs text-[var(--text-muted)]">
                  {JSON.stringify(current.params.permissions ?? {}, null, 2)}
                </pre>
              </div>
            </section>
          )}

          {!isCommand && !isFile && !isUserInput && !isPermissions && (
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

function userInputQuestionsFromParams(params: Record<string, unknown>): UserInputQuestion[] {
  if (!Array.isArray(params.questions)) {
    return [];
  }

  return params.questions
    .map((question): UserInputQuestion | null => {
      if (!question || typeof question !== "object") {
        return null;
      }
      const value = question as Record<string, unknown>;
      const id = typeof value.id === "string" ? value.id : "";
      const header = typeof value.header === "string" ? value.header : id;
      const prompt = typeof value.question === "string" ? value.question : "";
      if (!id || !prompt) {
        return null;
      }

      const options = Array.isArray(value.options)
        ? value.options
            .map((option): UserInputQuestionOption | null => {
              if (!option || typeof option !== "object") {
                return null;
              }
              const raw = option as Record<string, unknown>;
              const label = typeof raw.label === "string" ? raw.label : "";
              const description = typeof raw.description === "string" ? raw.description : "";
              return label ? { label, description } : null;
            })
            .filter((option): option is UserInputQuestionOption => option !== null)
        : undefined;

      return { id, header, question: prompt, options };
    })
    .filter((question): question is UserInputQuestion => question !== null);
}
