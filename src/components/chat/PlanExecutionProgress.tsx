import {
  IconChevronDown,
  IconCircle,
  IconCircleCheck,
  IconFiles,
  IconLoader2,
} from "@tabler/icons-react";
import { useEffect, useRef, useState } from "react";
import { useIntl } from "react-intl";
import type { PlanExecutionProgress as PlanExecutionProgressValue } from "../../utils/planExecutionProgress";

interface PlanExecutionProgressProps {
  progress: PlanExecutionProgressValue;
}

export function PlanExecutionProgress({ progress }: PlanExecutionProgressProps) {
  const intl = useIntl();
  const [open, setOpen] = useState(false);
  const containerRef = useRef<HTMLDivElement>(null);
  const complete = progress.steps.every((step) => step.status === "completed");

  useEffect(() => {
    if (!open) return;
    const handlePointerDown = (event: MouseEvent) => {
      const target = event.target as Node | null;
      if (target && !containerRef.current?.contains(target)) {
        setOpen(false);
      }
    };
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") setOpen(false);
    };
    window.addEventListener("mousedown", handlePointerDown);
    window.addEventListener("keydown", handleKeyDown);
    return () => {
      window.removeEventListener("mousedown", handlePointerDown);
      window.removeEventListener("keydown", handleKeyDown);
    };
  }, [open]);

  return (
    <div ref={containerRef} className="relative mb-2 flex justify-center">
      {open && (
        <div
          role="dialog"
          aria-label={intl.formatMessage({ id: "chat.planProgress.title" })}
          className="absolute bottom-full left-1/2 z-30 mb-2 w-[min(420px,calc(100vw-2rem))] -translate-x-1/2 overflow-hidden rounded-[var(--radius-md)] border border-[var(--chat-line)] bg-[var(--chat-card-solid)] shadow-[var(--shadow-strong)]"
        >
          <div className="flex items-center justify-between gap-3 border-b border-[var(--chat-line)] px-3.5 py-2.5">
            <div className="min-w-0">
              <div className="text-[12px] font-semibold text-[var(--chat-prose)]">
                {intl.formatMessage({ id: "chat.planProgress.title" })}
              </div>
              <div className="mt-0.5 text-[10px] text-[var(--chat-faint)]">
                {intl.formatMessage(
                  { id: "chat.planProgress.step" },
                  { current: progress.currentStep, total: progress.totalSteps },
                )}
              </div>
            </div>
            <div className="flex flex-shrink-0 items-center gap-2 text-[11px]">
              <span className="inline-flex items-center gap-1 text-[var(--chat-muted)]">
                <IconFiles size={13} stroke={1.8} />
                {progress.changedFileCount}
              </span>
              <span className="font-medium text-[var(--accent-strong)]">+{progress.additions}</span>
              <span className="font-medium text-[var(--danger)]">-{progress.deletions}</span>
            </div>
          </div>
          <ol className="max-h-64 overflow-y-auto px-3.5 py-2.5">
            {progress.steps.map((step, index) => {
              const isCurrent = index + 1 === progress.currentStep;
              const isActive = progress.running && isCurrent && step.status !== "completed";
              return (
                <li key={`${index}-${step.step}`} className="flex min-w-0 items-start gap-2.5 py-1.5">
                  <span className="mt-0.5 flex h-4 w-4 flex-shrink-0 items-center justify-center">
                    {step.status === "completed" ? (
                      <IconCircleCheck size={16} stroke={2} className="text-[var(--accent-strong)]" />
                    ) : isActive ? (
                      <IconLoader2 size={15} stroke={2} className="animate-spin text-[var(--accent-strong)]" />
                    ) : (
                      <IconCircle size={14} stroke={1.6} className="text-[var(--chat-faint)]" />
                    )}
                  </span>
                  <span
                    className={`min-w-0 break-words text-[12px] leading-5 ${
                      step.status === "completed"
                        ? "text-[var(--chat-muted)] line-through"
                        : isCurrent
                          ? "font-medium text-[var(--chat-prose)]"
                          : "text-[var(--chat-muted)]"
                    }`}
                  >
                    {step.step}
                  </span>
                </li>
              );
            })}
          </ol>
        </div>
      )}

      <button
        type="button"
        aria-expanded={open}
        aria-haspopup="dialog"
        onClick={() => setOpen((value) => !value)}
        title={intl.formatMessage({ id: "chat.planProgress.open" })}
        className="flex max-w-full items-center gap-1.5 rounded-full border border-[var(--chat-line)] bg-[var(--chat-card-solid)] px-3 py-1.5 text-[11px] text-[var(--chat-muted)] shadow-[var(--shadow-soft)] transition-colors hover:border-[var(--accent-border)] hover:text-[var(--chat-prose)]"
      >
        {progress.running && !complete ? (
          <IconLoader2 size={13} stroke={2} className="flex-shrink-0 animate-spin text-[var(--accent-strong)]" />
        ) : (
          <IconCircleCheck size={13} stroke={2} className="flex-shrink-0 text-[var(--accent-strong)]" />
        )}
        <span className="whitespace-nowrap font-medium text-[var(--chat-prose)]">
          {intl.formatMessage(
            { id: "chat.planProgress.step" },
            { current: progress.currentStep, total: progress.totalSteps },
          )}
        </span>
        <span aria-hidden="true" className="text-[var(--chat-faint)]">·</span>
        <span className="inline-flex min-w-0 items-center gap-1 whitespace-nowrap">
          <IconFiles size={12} stroke={1.8} className="flex-shrink-0" />
          {intl.formatMessage(
            { id: "chat.planProgress.filesChanged" },
            { count: progress.changedFileCount },
          )}
        </span>
        <span className="whitespace-nowrap font-medium text-[var(--accent-strong)]">+{progress.additions}</span>
        <span className="whitespace-nowrap font-medium text-[var(--danger)]">-{progress.deletions}</span>
        <IconChevronDown
          size={12}
          stroke={2}
          className={`flex-shrink-0 transition-transform ${open ? "rotate-180" : ""}`}
        />
      </button>
    </div>
  );
}
