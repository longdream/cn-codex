import {
  IconX,
  IconCompass,
  IconFlame,
  IconMessageCircle,
  IconCoin,
  IconClock,
  IconAlertCircle,
  IconArrowRight,
  IconLoader2,
  IconRefresh,
  IconSun,
} from "@tabler/icons-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { useIntl } from "react-intl";
import { useAppStore } from "../../stores/appStore";
import { useSettingsStore } from "../../stores/settingsStore";
import { standaloneThreadPeekGoal } from "../../api/standalone";
import { fetchDailyFortune, type FortuneResult } from "../../utils/fortune";
import { FortuneDetailModal } from "./FortuneDetailModal";

type BubbleMode = "task" | "fortune" | "loading" | "error";

interface UnfinishedTask {
  threadId: string;
  objective: string;
}

const ACTION_ICONS: Record<string, typeof IconFlame> = {
  "沟通": IconMessageCircle,
  "行动": IconFlame,
  "交易": IconCoin,
  "等待": IconClock,
};

function overallColor(overall: string): string {
  if (overall.includes("顺")) return "text-green-400";
  if (overall.includes("阻")) return "text-yellow-400";
  return "text-red-400";
}

export function FortuneBubble() {
  const intl = useIntl();
  const fortuneEnabled = useSettingsStore((s) => s.fortuneEnabled);
  const baziProfile = useSettingsStore((s) => s.baziProfile);
  const threads = useAppStore((s) => s.threads);
  const initialized = useAppStore((s) => s.initialized);
  const loadThread = useAppStore((s) => s.loadThread);

  const fortuneRefreshTrigger = useSettingsStore((s) => s.fortuneRefreshTrigger);

  const [dismissed, setDismissed] = useState(false);
  const [visible, setVisible] = useState(false);
  const [mode, setMode] = useState<BubbleMode>("loading");
  const [unfinishedTask, setUnfinishedTask] = useState<UnfinishedTask | null>(null);
  const [fortune, setFortune] = useState<FortuneResult | null>(null);
  const [showDetail, setShowDetail] = useState(false);
  const checkedRef = useRef(false);
  const lastRefreshTrigger = useRef(0);

  const checkAndLoad = useCallback(async () => {
    if (checkedRef.current) return;
    checkedRef.current = true;

    if (!fortuneEnabled) return;

    try {
      const sortedThreads = [...threads].sort((a, b) => b.updatedAt - a.updatedAt);
      for (const t of sortedThreads.slice(0, 5)) {
        try {
          const result = await standaloneThreadPeekGoal(t.id);
          const goal = result?.goal;
          if (goal && goal.status !== "complete") {
            setUnfinishedTask({ threadId: t.id, objective: goal.objective });
            setMode("task");
            setVisible(true);
            return;
          }
        } catch {
          // skip unreadable threads
        }
      }
    } catch {
      // skip goal check on error
    }

    setMode("loading");
    setVisible(true);

    try {
      const result = await fetchDailyFortune(baziProfile);
      setFortune(result);
      setMode("fortune");
    } catch (err) {
      console.error("[FortuneBubble] fetchDailyFortune failed:", err);
      setMode("error");
    }
  }, [fortuneEnabled, threads, baziProfile]);

  useEffect(() => {
    if (!initialized || dismissed || !fortuneEnabled) return;

    const timer = setTimeout(() => {
      checkAndLoad();
    }, 1500);

    return () => clearTimeout(timer);
  }, [initialized, dismissed, fortuneEnabled, checkAndLoad]);

  useEffect(() => {
    if (fortuneRefreshTrigger === 0 || fortuneRefreshTrigger === lastRefreshTrigger.current) return;
    lastRefreshTrigger.current = fortuneRefreshTrigger;

    setDismissed(false);
    setVisible(true);
    setMode("loading");

    (async () => {
      try {
        const result = await fetchDailyFortune(baziProfile, /*forceRefresh*/ true);
        setFortune(result);
        setMode("fortune");
      } catch (err) {
        console.error("[FortuneBubble] refresh fetchDailyFortune failed:", err);
        setMode("error");
      }
    })();
  }, [fortuneRefreshTrigger, baziProfile]);

  const handleRetry = useCallback(async () => {
    console.log("[FortuneBubble] retry clicked");
    setMode("loading");
    try {
      const result = await fetchDailyFortune(baziProfile);
      setFortune(result);
      setMode("fortune");
    } catch (err) {
      console.error("[FortuneBubble] retry fetchDailyFortune failed:", err);
      setMode("error");
    }
  }, [baziProfile]);

  const handleContinueTask = useCallback(() => {
    if (unfinishedTask) {
      loadThread(unfinishedTask.threadId);
      setDismissed(true);
    }
  }, [unfinishedTask, loadThread]);

  if (dismissed || !visible || !fortuneEnabled) return null;

  const ActionIcon = fortune?.bestAction ? (ACTION_ICONS[fortune.bestAction] ?? IconSun) : IconSun;

  return (
    <>
      <div className="fortune-bubble">
        <button
          onClick={() => setDismissed(true)}
          className="absolute right-2 top-2 rounded-full p-0.5 text-[var(--text-faint)] transition-colors hover:bg-white/10 hover:text-[var(--text-muted)]"
          aria-label={intl.formatMessage({ id: "common.close" })}
        >
          <IconX size={14} stroke={2} />
        </button>

        {mode === "loading" && (
          <div className="flex items-center gap-3 py-2">
            <IconLoader2 size={18} className="animate-spin text-[var(--accent)]" />
            <span className="text-xs text-[var(--text-muted)]">
              {intl.formatMessage({ id: "fortune.bubble.loading" })}
            </span>
          </div>
        )}

        {mode === "error" && (
          <button
            onClick={handleRetry}
            className="flex w-full items-center gap-3 py-2 text-left"
          >
            <IconRefresh size={18} className="text-red-400" />
            <span className="text-xs text-[var(--text-muted)]">
              {intl.formatMessage({ id: "fortune.bubble.error" })}
            </span>
          </button>
        )}

        {mode === "task" && unfinishedTask && (
          <div className="space-y-2.5">
            <div className="flex items-center gap-2">
              <IconAlertCircle size={16} className="shrink-0 text-yellow-400" />
              <span className="text-[12px] font-semibold text-[var(--text-strong)]">
                {intl.formatMessage({ id: "fortune.bubble.taskTitle" })}
              </span>
            </div>
            <p className="line-clamp-2 text-xs leading-relaxed text-[var(--text-muted)]">
              {unfinishedTask.objective}
            </p>
            <button
              onClick={handleContinueTask}
              className="flex items-center gap-1 rounded-lg bg-[var(--accent-strong)] px-3 py-1 text-[11px] font-medium text-white transition-opacity hover:opacity-90"
            >
              {intl.formatMessage({ id: "fortune.bubble.taskContinue" })}
              <IconArrowRight size={12} stroke={2} />
            </button>
          </div>
        )}

        {mode === "fortune" && fortune && (
          <div className="space-y-2.5">
            <div className="flex items-center gap-2">
              <IconCompass size={16} className="shrink-0 text-[var(--accent)]" />
              <span className="text-[12px] font-semibold text-[var(--text-strong)]">
                {intl.formatMessage({ id: "fortune.bubble.title" })}
              </span>
            </div>

            <div className="grid grid-cols-2 gap-x-4 gap-y-1.5">
              <div className="flex items-center gap-1.5">
                <IconSun size={13} className={`shrink-0 ${overallColor(fortune.overall)}`} />
                <span className="text-[11px] text-[var(--text-faint)]">
                  {intl.formatMessage({ id: "fortune.bubble.overall" })}
                </span>
                <span className={`text-[11px] font-medium ${overallColor(fortune.overall)}`}>
                  {fortune.overall}
                </span>
              </div>

              <div className="flex items-center gap-1.5">
                <IconCompass size={13} className="shrink-0 text-blue-400" />
                <span className="text-[11px] text-[var(--text-faint)]">
                  {intl.formatMessage({ id: "fortune.bubble.direction" })}
                </span>
                <span className="text-[11px] font-medium text-[var(--text-strong)]">
                  {fortune.direction}
                </span>
              </div>

              <div className="flex items-center gap-1.5">
                <ActionIcon size={13} className="shrink-0 text-orange-400" />
                <span className="text-[11px] text-[var(--text-faint)]">
                  {intl.formatMessage({ id: "fortune.bubble.bestAction" })}
                </span>
                <span className="text-[11px] font-medium text-[var(--text-strong)]">
                  {fortune.bestAction}
                </span>
              </div>

              <div className="flex items-center gap-1.5">
                <IconFlame size={13} className="shrink-0 text-purple-400" />
                <span className="text-[11px] text-[var(--text-faint)]">
                  {intl.formatMessage({ id: "fortune.bubble.environment" })}
                </span>
                <span className="text-[11px] font-medium text-[var(--text-strong)]">
                  {fortune.environment}
                </span>
              </div>
            </div>

            {fortune.ziweiDetail && (
              <div className="flex items-center gap-1.5 border-t border-white/5 pt-1.5">
                <span className="text-[10px] text-[var(--accent)]">
                  {intl.formatMessage({ id: "fortune.bubble.ziwei" })}
                </span>
                <span className="text-[10px] text-[var(--text-muted)]">
                  {fortune.summary}
                </span>
              </div>
            )}

            {!fortune.ziweiDetail && fortune.summary && (
              <p className="border-t border-white/5 pt-1.5 text-[10px] text-[var(--text-muted)]">
                {fortune.summary}
              </p>
            )}

            <button
              onClick={() => setShowDetail(true)}
              className="flex items-center gap-1 rounded-lg bg-white/5 px-3 py-1 text-[11px] font-medium text-[var(--accent)] transition-colors hover:bg-white/10"
            >
              {intl.formatMessage({ id: "fortune.bubble.detail" })}
              <IconArrowRight size={12} stroke={2} />
            </button>
          </div>
        )}
      </div>

      {showDetail && fortune && (
        <FortuneDetailModal
          fortune={fortune}
          onClose={() => setShowDetail(false)}
        />
      )}
    </>
  );
}
