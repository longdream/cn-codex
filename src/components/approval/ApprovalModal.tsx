import {
  IconAlertTriangle,
  IconDatabase,
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
  recommended?: boolean;
}

interface UserInputQuestion {
  id: string;
  header: string;
  question: string;
  options?: UserInputQuestionOption[];
}

interface EntryFormField {
  name: string;
  label: string;
  dbType: string;
  inputType: string;
  required: boolean;
  readonly: boolean;
  autoIncrement: boolean;
  primaryKey: boolean;
  maxLength?: number | null;
  precision?: number | null;
  scale?: number | null;
  defaultValue?: string | null;
  comment?: string | null;
  options?: string[];
  value?: unknown;
  placeholder?: string | null;
}

interface EntryFormPayload {
  database: string;
  dbType: string;
  table: string;
  title: string;
  description: string;
  fields: EntryFormField[];
  knownValues?: Record<string, unknown>;
  matchedBy?: string;
}

export function ApprovalModal() {
  const intl = useIntl();
  const [requests, setRequests] = useState<ApprovalRequest[]>([]);
  const [answers, setAnswers] = useState<Record<string, string>>({});
  const [formValues, setFormValues] = useState<Record<string, unknown>>({});
  const [formError, setFormError] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);

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
  const isEntryForm = !!current && (
    current.method.includes("build_entry_form") ||
    current.method.includes("buildEntryForm")
  );
  const isRobotWait = current?.method === "robot_waiting_for_input";
  const userInputQuestions = useMemo(
    () => (isUserInput ? userInputQuestionsFromParams(current?.params ?? {}) : []),
    [current?.params, isUserInput],
  );
  const entryForm = useMemo(
    () => (isEntryForm ? entryFormFromParams(current?.params ?? {}) : null),
    [current?.params, isEntryForm],
  );

  useEffect(() => {
    if (!current || !isUserInput) {
      if (!isEntryForm) {
        setAnswers({});
      }
      return;
    }

    setAnswers(
      Object.fromEntries(
        userInputQuestions.map((question) => [
          question.id,
          preferredQuestionOptionLabel(question.options),
        ]),
      ),
    );
  }, [current?.requestId, isEntryForm, isUserInput, userInputQuestions]);

  useEffect(() => {
    if (!current || !isEntryForm || !entryForm) {
      setFormValues({});
      setFormError(null);
      return;
    }
    setFormValues(initialFormValues(entryForm.fields));
    setFormError(null);
  }, [current?.requestId, entryForm, isEntryForm]);

  const handleApprove = useCallback(async () => {
    if (!current || submitting) {
      return;
    }

    if (isEntryForm && entryForm) {
      const missing = entryForm.fields
        .filter((field) => field.required && !field.readonly && !field.autoIncrement)
        .filter((field) => isEmptyFormValue(formValues[field.name]));
      if (missing.length > 0) {
        setFormError(
          intl.formatMessage(
            { id: "approval.entryForm.requiredMissing" },
            { fields: missing.map((field) => field.label || field.name).join("、") },
          ),
        );
        return;
      }

      setSubmitting(true);
      setFormError(null);
      try {
        const values = Object.fromEntries(
          entryForm.fields
            .filter((field) => !field.autoIncrement)
            .map((field) => [field.name, normalizeFormValue(field, formValues[field.name])]),
        );
        await resolveApproval(current.requestId, {
          decision: "submit",
          database: entryForm.database,
          table: entryForm.table,
          values,
        });
        setRequests((previous) => previous.slice(1));
      } catch (err) {
        console.error("Approve failed:", err);
        setFormError(err instanceof Error ? err.message : String(err));
      } finally {
        setSubmitting(false);
      }
      return;
    }

    const decision = isRobotWait
      ? { userReply: answers.robotReply?.trim() ?? "" }
      : isUserInput
      ? {
          answers: Object.fromEntries(
            userInputQuestions.map((question) => [
              question.id,
              {
                answers: [
                  answers[question.id] ?? preferredQuestionOptionLabel(question.options),
                ],
              },
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
  }, [
    answers,
    current,
    entryForm,
    formValues,
    intl,
    isEntryForm,
    isPermissions,
    isRobotWait,
    isUserInput,
    submitting,
    userInputQuestions,
  ]);

  const handleReject = useCallback(async () => {
    if (!current || submitting) {
      return;
    }
    try {
      if (isEntryForm) {
        await resolveApproval(current.requestId, {
          decision: "cancel",
          cancelled: true,
        });
      } else {
        await rejectApproval(current.requestId, -32000, "User declined");
      }
    } catch (err) {
      console.error("Reject failed:", err);
    }
    setRequests((previous) => previous.slice(1));
  }, [current, isEntryForm, submitting]);

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
        ? intl.formatMessage({ id: "approval.permissionRequest" })
        : isEntryForm
          ? entryForm?.title || intl.formatMessage({ id: "approval.entryForm.title" })
          : isRobotWait
            ? "需要你的回复"
            : isUserInput
              ? intl.formatMessage({ id: "approval.title" })
              : intl.formatMessage({ id: "approval.title" });

  const approveLabel = isEntryForm
    ? intl.formatMessage({ id: "approval.entryForm.save" })
    : isUserInput || isRobotWait
      ? intl.formatMessage({ id: "approval.allow" })
      : intl.formatMessage({ id: "approval.allow" });

  const rejectLabel = isEntryForm
    ? intl.formatMessage({ id: "approval.entryForm.cancel" })
    : intl.formatMessage({ id: "approval.reject" });

  return (
    <div className="fixed bottom-0 left-0 right-0 top-8 z-50 flex items-center justify-center bg-black/50 p-4 backdrop-blur-sm">
      <div className={`app-shell-panel w-full overflow-hidden ${isEntryForm ? "max-w-3xl" : "max-w-2xl"}`}>
        <div className="flex items-center justify-between gap-3 border-b border-[var(--border-subtle)] px-5 py-3">
          <div className="flex items-center gap-3">
            <div className="flex h-8 w-8 items-center justify-center rounded-[var(--radius-sm)] bg-[rgba(245,158,11,0.12)] text-[var(--warning)]">
              {isEntryForm ? <IconDatabase size={16} stroke={2} /> : <IconAlertTriangle size={16} stroke={2} />}
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
          {isEntryForm && entryForm && (
            <section className="space-y-4">
              <div className="rounded-[var(--radius-lg)] border border-[var(--border-subtle)] bg-[var(--surface-contrast)] p-4">
                <div className="flex flex-wrap items-center gap-2 text-xs text-[var(--text-muted)]">
                  <span className="rounded-[var(--radius-sm)] bg-[var(--surface-main)] px-2 py-0.5 font-mono">
                    {entryForm.database}
                  </span>
                  <span>·</span>
                  <span className="rounded-[var(--radius-sm)] bg-[var(--surface-main)] px-2 py-0.5 font-mono">
                    {entryForm.table}
                  </span>
                  {entryForm.dbType ? (
                    <>
                      <span>·</span>
                      <span>{entryForm.dbType}</span>
                    </>
                  ) : null}
                </div>
                {entryForm.description ? (
                  <p className="mt-2 text-sm text-[var(--text-base)]">{entryForm.description}</p>
                ) : (
                  <p className="mt-2 text-sm text-[var(--text-muted)]">
                    {intl.formatMessage({ id: "approval.entryForm.description" })}
                  </p>
                )}
                {entryForm.matchedBy ? (
                  <p className="mt-1 text-xs text-[var(--text-faint)]">
                    {intl.formatMessage(
                      { id: "approval.entryForm.matchedBy" },
                      { reason: entryForm.matchedBy },
                    )}
                  </p>
                ) : null}
              </div>

              <div className="grid grid-cols-1 gap-3 md:grid-cols-2">
                {entryForm.fields.map((field) => (
                  <div
                    key={field.name}
                    className={`rounded-[var(--radius-lg)] border border-[var(--border-subtle)] bg-[var(--surface-contrast)] p-3 ${
                      field.inputType === "textarea" || field.inputType === "json"
                        ? "md:col-span-2"
                        : ""
                    }`}
                  >
                    <label className="mb-1.5 flex items-center gap-1 text-xs font-medium text-[var(--text-strong)]">
                      <span>{field.label || field.name}</span>
                      {field.required && !field.readonly ? (
                        <span className="text-red-400">*</span>
                      ) : null}
                      {field.autoIncrement ? (
                        <span className="rounded bg-[var(--surface-main)] px-1 py-0.5 text-[10px] text-[var(--text-faint)]">
                          AI
                        </span>
                      ) : null}
                    </label>
                    <p className="mb-2 font-mono text-[10px] text-[var(--text-faint)]">
                      {field.name}
                      {field.dbType ? ` · ${field.dbType}` : ""}
                    </p>
                    {renderEntryField(field, formValues[field.name], (next) => {
                      setFormValues((previous) => ({ ...previous, [field.name]: next }));
                      setFormError(null);
                    })}
                    {field.comment ? (
                      <p className="mt-1.5 text-[11px] text-[var(--text-faint)]">{field.comment}</p>
                    ) : null}
                  </div>
                ))}
              </div>

              {formError ? (
                <div className="rounded-[var(--radius-md)] border border-red-500/30 bg-red-500/10 px-3 py-2 text-xs text-red-300">
                  {formError}
                </div>
              ) : null}
            </section>
          )}

          {isRobotWait && (
            <section className="space-y-3 rounded-[var(--radius-lg)] border border-[var(--border-subtle)] bg-[var(--surface-contrast)] p-4">
              <p className="whitespace-pre-wrap text-sm text-[var(--text-base)]">
                {String(current.params.assistantText ?? "AI 正在等待你的补充信息。")}
              </p>
              <textarea
                autoFocus
                value={answers.robotReply ?? ""}
                onChange={(event) => setAnswers((previous) => ({ ...previous, robotReply: event.target.value }))}
                placeholder="输入你的回复"
                className="min-h-24 w-full resize-y rounded-[var(--radius-md)] border border-[var(--border-subtle)] bg-[var(--surface-main)] p-3 text-sm text-[var(--text-base)] outline-none focus:border-[var(--accent)]"
              />
            </section>
          )}
          {(isCommand || isFile) && (
            <p className="mb-4 text-sm text-[var(--text-muted)]">
              {intl.formatMessage({ id: "approval.description" })}
            </p>
          )}

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
                  {intl.formatMessage({ id: "approval.path" })}
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
                {intl.formatMessage({ id: "approval.path" })}
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
                        placeholder={intl.formatMessage({ id: "approval.other" })}
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
            <section className="rounded-[var(--radius-lg)] border border-[var(--border-subtle)] bg-[var(--surface-contrast)] p-4">
              <p className="text-sm text-[var(--text-base)]">
                {String(current.params.reason ?? intl.formatMessage({ id: "approval.permissionRequest" }))}
              </p>
              <div className="mt-3">
                <p className="text-xs font-medium text-[var(--text-muted)]">
                  {intl.formatMessage({ id: "approval.type" })}
                  permissions
                </p>
                <pre className="thin-scrollbar mt-2 max-h-52 overflow-y-auto whitespace-pre-wrap rounded-[var(--radius-md)] border border-[var(--border-subtle)] bg-[var(--surface-main)] p-3 text-xs text-[var(--text-muted)]">
                  {JSON.stringify(current.params.permissions ?? {}, null, 2)}
                </pre>
              </div>
            </section>
          )}

          {!isCommand && !isFile && !isUserInput && !isPermissions && !isEntryForm && !isRobotWait && (
            <section className="rounded-[var(--radius-lg)] border border-[var(--border-subtle)] bg-[var(--surface-contrast)] p-4">
              <pre className="whitespace-pre-wrap text-xs text-[var(--text-muted)]">
                {JSON.stringify(current.params, null, 2)}
              </pre>
            </section>
          )}
        </div>

        <div className="flex items-center justify-end gap-3 border-t border-[var(--border-subtle)] px-5 py-3">
          <button
            onClick={() => void handleReject()}
            disabled={submitting}
            className="danger-button flex items-center gap-2 px-4 disabled:opacity-50"
          >
            <IconShieldX size={16} stroke={1.8} />
            {rejectLabel}
          </button>
          <button
            onClick={() => void handleApprove()}
            disabled={submitting}
            className="primary-button flex items-center gap-2 px-4 disabled:opacity-50"
          >
            <IconShieldCheck size={16} stroke={1.8} />
            {submitting
              ? intl.formatMessage({ id: "approval.entryForm.saving" })
              : approveLabel}
          </button>
        </div>
      </div>
    </div>
  );
}

function entryFormFromParams(params: Record<string, unknown>): EntryFormPayload | null {
  const raw = (params.form ?? params) as Record<string, unknown>;
  if (!raw || typeof raw !== "object") {
    return null;
  }
  const database = typeof raw.database === "string" ? raw.database : "";
  const table = typeof raw.table === "string" ? raw.table : "";
  if (!database || !table || !Array.isArray(raw.fields)) {
    return null;
  }

  const fields = raw.fields
    .map((item): EntryFormField | null => {
      if (!item || typeof item !== "object") {
        return null;
      }
      const field = item as Record<string, unknown>;
      const name = typeof field.name === "string" ? field.name : "";
      if (!name) {
        return null;
      }
      return {
        name,
        label: typeof field.label === "string" ? field.label : name,
        dbType: typeof field.dbType === "string" ? field.dbType : String(field.db_type ?? ""),
        inputType:
          typeof field.inputType === "string"
            ? field.inputType
            : String(field.input_type ?? "text"),
        required: Boolean(field.required),
        readonly: Boolean(field.readonly),
        autoIncrement: Boolean(field.autoIncrement ?? field.auto_increment),
        primaryKey: Boolean(field.primaryKey ?? field.primary_key),
        maxLength:
          typeof field.maxLength === "number"
            ? field.maxLength
            : typeof field.max_length === "number"
              ? field.max_length
              : null,
        precision: typeof field.precision === "number" ? field.precision : null,
        scale: typeof field.scale === "number" ? field.scale : null,
        defaultValue:
          typeof field.defaultValue === "string"
            ? field.defaultValue
            : typeof field.default_value === "string"
              ? field.default_value
              : null,
        comment: typeof field.comment === "string" ? field.comment : null,
        options: Array.isArray(field.options)
          ? field.options.map((option) => String(option))
          : [],
        value: field.value,
        placeholder:
          typeof field.placeholder === "string" ? field.placeholder : null,
      };
    })
    .filter((field): field is EntryFormField => field !== null);

  return {
    database,
    dbType: typeof raw.dbType === "string" ? raw.dbType : String(raw.db_type ?? ""),
    table,
    title: typeof raw.title === "string" ? raw.title : `${database}.${table}`,
    description: typeof raw.description === "string" ? raw.description : "",
    fields,
    knownValues:
      raw.knownValues && typeof raw.knownValues === "object"
        ? (raw.knownValues as Record<string, unknown>)
        : raw.known_values && typeof raw.known_values === "object"
          ? (raw.known_values as Record<string, unknown>)
          : {},
    matchedBy:
      typeof raw.matchedBy === "string"
        ? raw.matchedBy
        : typeof raw.matched_by === "string"
          ? raw.matched_by
          : "",
  };
}

function initialFormValues(fields: EntryFormField[]): Record<string, unknown> {
  const values: Record<string, unknown> = {};
  for (const field of fields) {
    if (field.autoIncrement) {
      values[field.name] = null;
      continue;
    }
    if (field.value !== undefined && field.value !== null) {
      values[field.name] = field.inputType === "boolean"
        ? coerceBoolean(field.value)
        : field.value;
      continue;
    }
    if (field.inputType === "boolean") {
      values[field.name] = false;
      continue;
    }
    values[field.name] = "";
  }
  return values;
}

function renderEntryField(
  field: EntryFormField,
  value: unknown,
  onChange: (next: unknown) => void,
) {
  const disabled = field.readonly || field.autoIncrement;
  const commonClass =
    "w-full rounded-[var(--radius-sm)] border border-[var(--border-subtle)] bg-[var(--surface-main)] px-3 py-2 text-sm text-[var(--text-base)] outline-none focus:border-[var(--accent-border)] disabled:opacity-60";

  if (field.inputType === "boolean") {
    return (
      <label className="inline-flex items-center gap-2 text-sm text-[var(--text-base)]">
        <input
          type="checkbox"
          checked={Boolean(value)}
          disabled={disabled}
          onChange={(event) => onChange(event.target.checked)}
          className="accent-[var(--accent)]"
        />
        <span>{Boolean(value) ? "true" : "false"}</span>
      </label>
    );
  }

  if (field.inputType === "select" && field.options && field.options.length > 0) {
    return (
      <select
        value={value == null ? "" : String(value)}
        disabled={disabled}
        onChange={(event) => onChange(event.target.value)}
        className={`app-select ${commonClass}`}
      >
        <option value="">--</option>
        {field.options.map((option) => (
          <option key={option} value={option}>
            {option}
          </option>
        ))}
      </select>
    );
  }

  if (field.inputType === "textarea" || field.inputType === "json") {
    return (
      <textarea
        value={value == null ? "" : typeof value === "string" ? value : JSON.stringify(value, null, 2)}
        disabled={disabled}
        rows={field.inputType === "json" ? 5 : 3}
        maxLength={field.maxLength ?? undefined}
        placeholder={field.placeholder ?? undefined}
        onChange={(event) => onChange(event.target.value)}
        className={`${commonClass} min-h-20 resize-y font-mono`}
      />
    );
  }

  const inputType =
    field.inputType === "number"
      ? "number"
      : field.inputType === "date"
        ? "date"
        : field.inputType === "time"
          ? "time"
          : field.inputType === "datetime"
            ? "datetime-local"
            : "text";

  return (
    <input
      type={inputType}
      value={formatInputValue(field.inputType, value)}
      disabled={disabled}
      maxLength={field.maxLength ?? undefined}
      step={field.inputType === "number" ? (field.scale && field.scale > 0 ? "0.01" : "1") : undefined}
      placeholder={field.placeholder ?? undefined}
      onChange={(event) => {
        if (field.inputType === "number") {
          const text = event.target.value;
          if (text === "") {
            onChange("");
            return;
          }
          const num = Number(text);
          onChange(Number.isFinite(num) ? num : text);
          return;
        }
        onChange(event.target.value);
      }}
      className={commonClass}
    />
  );
}

function formatInputValue(inputType: string, value: unknown): string {
  if (value == null) {
    return "";
  }
  if (inputType === "datetime") {
    const text = String(value).trim();
    if (!text) {
      return "";
    }
    // Accept "YYYY-MM-DD HH:mm:ss" or ISO and map to datetime-local.
    const normalized = text.replace(" ", "T").slice(0, 16);
    return normalized;
  }
  if (typeof value === "object") {
    return JSON.stringify(value);
  }
  return String(value);
}

function normalizeFormValue(field: EntryFormField, value: unknown): unknown {
  if (field.autoIncrement) {
    return null;
  }
  if (value === undefined || value === null) {
    return null;
  }
  if (field.inputType === "boolean") {
    return coerceBoolean(value);
  }
  if (field.inputType === "number") {
    if (typeof value === "number") {
      return value;
    }
    const text = String(value).trim();
    if (!text) {
      return null;
    }
    const num = Number(text);
    return Number.isFinite(num) ? num : text;
  }
  if (field.inputType === "json") {
    const text = typeof value === "string" ? value.trim() : JSON.stringify(value);
    if (!text) {
      return null;
    }
    try {
      return JSON.parse(text);
    } catch {
      return text;
    }
  }
  if (field.inputType === "datetime") {
    const text = String(value).trim();
    if (!text) {
      return null;
    }
    return text.includes("T") ? text.replace("T", " ") : text;
  }
  if (typeof value === "string") {
    const text = value.trim();
    return text === "" ? null : text;
  }
  return value;
}

function coerceBoolean(value: unknown): boolean {
  if (typeof value === "boolean") {
    return value;
  }
  if (typeof value === "number") {
    return value !== 0;
  }
  const text = String(value).trim().toLowerCase();
  return text === "1" || text === "true" || text === "yes" || text === "y" || text === "on";
}

function isEmptyFormValue(value: unknown): boolean {
  if (value === undefined || value === null) {
    return true;
  }
  if (typeof value === "string") {
    return value.trim() === "";
  }
  return false;
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
              const recommended = raw.recommended === true
                || raw.isRecommended === true
                || label.toLowerCase().includes("recommended")
                || label.includes("推荐")
                || description.toLowerCase().includes("recommended")
                || description.includes("推荐");
              return label ? { label, description, recommended } : null;
            })
            .filter((option): option is UserInputQuestionOption => option !== null)
            .sort((left, right) => Number(Boolean(right.recommended)) - Number(Boolean(left.recommended)))
        : undefined;

      return { id, header, question: prompt, options };
    })
    .filter((question): question is UserInputQuestion => question !== null);
}

function preferredQuestionOptionLabel(options?: UserInputQuestionOption[]): string {
  if (!options || options.length === 0) {
    return "";
  }
  const recommended = options.find((option) => option.recommended && option.label.trim());
  if (recommended) {
    return recommended.label;
  }
  return options[0]?.label ?? "";
}
