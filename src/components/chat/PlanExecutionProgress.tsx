import {
  IconChevronDown,
  IconChevronRight,
  IconCircle,
  IconCircleCheck,
  IconFiles,
  IconLoader2,
  IconNotes,
  IconRobot,
} from "@tabler/icons-react";
import { useEffect, useRef, useState } from "react";
import { useIntl } from "react-intl";
import type {
  PlanExecutionProgress as PlanExecutionProgressValue,
  PlanProgressStep,
} from "../../utils/planExecutionProgress";

interface PlanExecutionProgressProps {
  progress: PlanExecutionProgressValue;
}

function StepIcon({ step, active }: { step: PlanProgressStep; active: boolean }) {
  if (step.status === "completed") {
    return <IconCircleCheck size={16} stroke={2} className="text-[var(--accent-strong)]" />;
  }
  if (active) {
    return <IconLoader2 size={15} stroke={2} className="animate-spin text-[var(--accent-strong)]" />;
  }
  return <IconCircle size={14} stroke={1.6} className="text-[var(--chat-faint)]" />;
}

export function PlanExecutionProgress({ progress }: PlanExecutionProgressProps) {
  const intl = useIntl();
  const workflow = progress.robotWorkflow;
  const [open, setOpen] = useState(false);
  const [expandedSummaryIndex, setExpandedSummaryIndex] = useState<number | null>(null);
  const previousNodeIndexRef = useRef(workflow?.currentNodeIndex);
  const complete = workflow?.completed ?? progress.steps.every((step) => step.status === "completed");
  const displayedCurrentStep = workflow ? workflow.currentNodeIndex + 1 : progress.currentStep;
  const displayedTotalSteps = workflow ? workflow.nodes.length : progress.totalSteps;

  useEffect(() => {
    const nextNodeIndex = workflow?.currentNodeIndex;
    if (nextNodeIndex === undefined) return;
    if (previousNodeIndexRef.current !== nextNodeIndex) {
      setExpandedSummaryIndex(null);
    }
    previousNodeIndexRef.current = nextNodeIndex;
  }, [workflow?.currentNodeIndex]);

  useEffect(() => {
    if (!open) return;
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") setOpen(false);
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [open]);

  const renderPlanSteps = () => (
    <ol className="px-4 py-2.5">
      {progress.steps.map((step, index) => {
        const isCurrent = index + 1 === progress.currentStep;
        const isActive = progress.running && isCurrent && step.status !== "completed";
        return (
          <li key={`${index}-${step.step}`} className="flex min-w-0 items-start gap-2.5 py-1.5">
            <span className="mt-0.5 flex h-4 w-4 flex-shrink-0 items-center justify-center">
              <StepIcon step={step} active={isActive} />
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
  );

  return (
    <div className="mb-2 flex w-full flex-col items-center gap-2">
      {open && (
        <section
          aria-label={intl.formatMessage({
            id: workflow ? "chat.robotProgress.title" : "chat.planProgress.title",
          })}
          className="w-full max-w-[760px] overflow-hidden rounded-[var(--radius-md)] border border-[var(--chat-line)] bg-[var(--chat-card-solid)] shadow-[var(--shadow-soft)]"
        >
          <header className="flex items-center justify-between gap-3 border-b border-[var(--chat-line)] px-4 py-3">
            <div className="flex min-w-0 items-center gap-2.5">
              <span className="flex h-7 w-7 flex-shrink-0 items-center justify-center rounded-[var(--radius-sm)] bg-[var(--accent-soft)] text-[var(--accent-strong)]">
                {workflow ? <IconRobot size={16} stroke={1.9} /> : <IconNotes size={16} stroke={1.9} />}
              </span>
              <div className="min-w-0">
                <div className="flex min-w-0 items-center gap-2">
                  <h2 className="truncate text-[12px] font-semibold text-[var(--chat-prose)]">
                    {intl.formatMessage({
                      id: workflow ? "chat.robotProgress.title" : "chat.planProgress.title",
                    })}
                  </h2>
                  {workflow?.robotId && (
                    <span className="truncate font-mono text-[10px] text-[var(--chat-faint)]">
                      {workflow.robotId}
                    </span>
                  )}
                </div>
                <div className="mt-0.5 flex items-center gap-2 text-[10px] text-[var(--chat-faint)]">
                  <span>
                    {intl.formatMessage(
                      { id: "chat.planProgress.step" },
                      { current: displayedCurrentStep, total: displayedTotalSteps },
                    )}
                  </span>
                  {workflow && (
                    <span>
                      {intl.formatMessage(
                        { id: "chat.robotProgress.summaryCount" },
                        { count: workflow.summarizedCount, total: workflow.nodes.length },
                      )}
                    </span>
                  )}
                </div>
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
          </header>

          <div className="thin-scrollbar max-h-[min(42vh,430px)] overflow-y-auto">
            {workflow ? (
              <>
                {workflow.rootObjective && (
                  <div className="border-b border-[var(--chat-line)] px-4 py-2.5">
                    <div className="text-[10px] font-medium text-[var(--chat-faint)]">
                      {intl.formatMessage({ id: "chat.robotProgress.objective" })}
                    </div>
                    <p className="mt-1 text-[12px] leading-5 text-[var(--chat-muted)]">
                      {workflow.rootObjective}
                    </p>
                  </div>
                )}

                <ol className="px-2 py-2">
                  {workflow.nodes.map((node, index) => {
                    const current = index === workflow.currentNodeIndex && !workflow.completed;
                    const active = current && progress.running;
                    const summaryOpen = expandedSummaryIndex === index;
                    const statusId = node.status === "completed"
                      ? "chat.robotProgress.completed"
                      : current
                        ? "chat.robotProgress.current"
                        : "chat.robotProgress.pending";
                    return (
                      <li
                        key={`${index}-${node.step}`}
                        className={`rounded-[var(--radius-sm)] ${current ? "bg-[var(--accent-soft)]" : ""}`}
                      >
                        <button
                          type="button"
                          disabled={!node.deliverySummary}
                          onClick={() => setExpandedSummaryIndex(summaryOpen ? null : index)}
                          className="flex w-full items-start gap-2.5 px-2 py-2 text-left disabled:cursor-default"
                        >
                          <span className="mt-0.5 flex h-5 w-5 flex-shrink-0 items-center justify-center">
                            <StepIcon step={node} active={active} />
                          </span>
                          <span className="min-w-0 flex-1">
                            <span className="flex min-w-0 items-start gap-2">
                              <span className="mt-0.5 flex h-4 min-w-4 items-center justify-center rounded-[3px] bg-[var(--chat-chip)] px-1 font-mono text-[9px] text-[var(--chat-faint)]">
                                {index + 1}
                              </span>
                              <span className={`min-w-0 break-words text-[12px] leading-5 ${current ? "font-medium text-[var(--chat-prose)]" : "text-[var(--chat-muted)]"}`}>
                                {node.step}
                              </span>
                            </span>
                            <span className="mt-1 flex flex-wrap items-center gap-2 pl-6 text-[10px]">
                              <span className={current ? "text-[var(--accent-strong)]" : "text-[var(--chat-faint)]"}>
                                {intl.formatMessage({ id: statusId })}
                              </span>
                              {node.status === "completed" && (
                                <span className={node.deliverySummary ? "text-[var(--accent-strong)]" : "text-[var(--danger)]"}>
                                  {intl.formatMessage({
                                    id: node.deliverySummary
                                      ? "chat.robotProgress.summarized"
                                      : "chat.robotProgress.summaryMissing",
                                  })}
                                </span>
                              )}
                            </span>
                          </span>
                          {node.deliverySummary && (
                            summaryOpen
                              ? <IconChevronDown size={14} stroke={1.8} className="mt-1 flex-shrink-0 text-[var(--chat-faint)]" />
                              : <IconChevronRight size={14} stroke={1.8} className="mt-1 flex-shrink-0 text-[var(--chat-faint)]" />
                          )}
                        </button>
                        {summaryOpen && node.deliverySummary && (
                          <div className="mx-2 mb-2 border-l-2 border-[var(--accent-border)] bg-[var(--chat-chip)] px-3 py-2.5">
                            <div className="mb-1.5 flex items-center gap-1.5 text-[10px] font-medium text-[var(--accent-strong)]">
                              <IconNotes size={12} stroke={1.8} />
                              {intl.formatMessage({ id: "chat.robotProgress.summary" })}
                            </div>
                            <div className="whitespace-pre-wrap break-words font-mono text-[10px] leading-[1.65] text-[var(--chat-muted)]">
                              {node.deliverySummary}
                            </div>
                          </div>
                        )}
                      </li>
                    );
                  })}
                </ol>

                {progress.hasExplicitPlan && (
                  <div className="border-t border-[var(--chat-line)]">
                    <div className="flex items-center justify-between gap-3 px-4 pt-3 text-[10px] font-semibold text-[var(--chat-faint)]">
                      <span className="flex items-center gap-2">
                        <IconNotes size={13} stroke={1.8} />
                        {intl.formatMessage({ id: "chat.robotProgress.nodePlan" })}
                      </span>
                      <span className="font-normal">
                        {intl.formatMessage(
                          { id: "chat.planProgress.step" },
                          { current: progress.currentStep, total: progress.totalSteps },
                        )}
                      </span>
                    </div>
                    {renderPlanSteps()}
                  </div>
                )}
              </>
            ) : renderPlanSteps()}
          </div>
        </section>
      )}

      <button
        type="button"
        aria-expanded={open}
        onClick={() => setOpen((value) => !value)}
        title={intl.formatMessage({
          id: workflow ? "chat.robotProgress.open" : "chat.planProgress.open",
        })}
        className="flex max-w-full items-center gap-1.5 rounded-full border border-[var(--chat-line)] bg-[var(--chat-card-solid)] px-3 py-1.5 text-[11px] text-[var(--chat-muted)] shadow-[var(--shadow-soft)] transition-colors hover:border-[var(--accent-border)] hover:text-[var(--chat-prose)] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--accent-border)]"
      >
        {progress.running && !complete ? (
          <IconLoader2 size={13} stroke={2} className="flex-shrink-0 animate-spin text-[var(--accent-strong)]" />
        ) : (
          <IconCircleCheck size={13} stroke={2} className="flex-shrink-0 text-[var(--accent-strong)]" />
        )}
        {workflow && <IconRobot size={13} stroke={1.8} className="flex-shrink-0" />}
        <span className="whitespace-nowrap font-medium text-[var(--chat-prose)]">
          {intl.formatMessage(
            { id: "chat.planProgress.step" },
            { current: displayedCurrentStep, total: displayedTotalSteps },
          )}
        </span>
        {workflow && (
          <span className="whitespace-nowrap text-[var(--chat-faint)]">
            {intl.formatMessage(
              { id: "chat.robotProgress.summaryShort" },
              { count: workflow.summarizedCount },
            )}
          </span>
        )}
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
