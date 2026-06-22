import {
  IconX,
  IconCompass,
  IconSun,
  IconFlame,
  IconMessageCircle,
  IconCoin,
  IconClock,
  IconLoader2,
  IconRefresh,
} from "@tabler/icons-react";
import { useIntl } from "react-intl";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import type { BaziProfile } from "../../stores/settingsStore";
import { useFortuneDetailStream } from "../../hooks/useFortuneDetailStream";
import type { FortuneSummary } from "../../utils/fortune";

interface FortuneDetailModalProps {
  summary: FortuneSummary;
  baziProfile?: BaziProfile | null;
  onClose: () => void;
}

function overallColor(overall: string): string {
  if (overall.includes("顺")) return "text-green-400";
  if (overall.includes("阻")) return "text-yellow-400";
  return "text-red-400";
}

function overallBg(overall: string): string {
  if (overall.includes("顺")) return "bg-green-500/10 border-green-500/20";
  if (overall.includes("阻")) return "bg-yellow-500/10 border-yellow-500/20";
  return "bg-red-500/10 border-red-500/20";
}

const ACTION_ICONS: Record<string, typeof IconFlame> = {
  "沟通": IconMessageCircle,
  "行动": IconFlame,
  "交易": IconCoin,
  "等待": IconClock,
};

export function FortuneDetailModal({ summary, baziProfile, onClose }: FortuneDetailModalProps) {
  const intl = useIntl();
  const { status, detail, error, start } = useFortuneDetailStream({
    summary,
    baziProfile,
    autoStart: true,
  });
  const ActionIcon = ACTION_ICONS[summary.bestAction] ?? IconSun;
  const isDetailReady = status === "completed" && detail;
  const isLoading = status === "loading" || status === "streaming";

  return (
    <div className="fixed bottom-0 left-0 right-0 top-8 z-50 flex items-center justify-center bg-black/60 p-4 backdrop-blur-sm">
      <div className="fortune-detail-modal thin-scrollbar">
        <header className="sticky top-0 z-10 flex items-center justify-between border-b border-[var(--border-subtle)] bg-[var(--surface-raised)] px-6 py-4">
          <div className="flex items-center gap-3">
            <IconCompass size={20} className="text-[var(--accent)]" />
            <div>
              <h2 className="text-[14px] font-semibold text-[var(--text-strong)]">
                {intl.formatMessage({ id: "fortune.detail.title" })}
              </h2>
              <p className="text-[11px] text-[var(--text-faint)]">{summary.date}</p>
            </div>
          </div>
          <button
            onClick={onClose}
            className="icon-button"
            aria-label={intl.formatMessage({ id: "fortune.detail.close" })}
          >
            <IconX size={16} stroke={1.8} />
          </button>
        </header>

        <div className="space-y-6 p-6">
          <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-4">
            <div className={`fortune-indicator ${overallBg(summary.overall)}`}>
              <IconSun size={20} className={overallColor(summary.overall)} />
              <span className="text-[10px] uppercase tracking-wider text-[var(--text-faint)]">
                {intl.formatMessage({ id: "fortune.bubble.overall" })}
              </span>
              <span className={`text-[15px] font-bold ${overallColor(summary.overall)}`}>
                {summary.overall}
              </span>
            </div>

            <div className="fortune-indicator bg-blue-500/10 border-blue-500/20">
              <IconCompass size={20} className="text-blue-400" />
              <span className="text-[10px] uppercase tracking-wider text-[var(--text-faint)]">
                {intl.formatMessage({ id: "fortune.bubble.direction" })}
              </span>
              <span className="text-[15px] font-bold text-blue-400">
                {summary.direction}
              </span>
            </div>

            <div className="fortune-indicator bg-orange-500/10 border-orange-500/20">
              <ActionIcon size={20} className="text-orange-400" />
              <span className="text-[10px] uppercase tracking-wider text-[var(--text-faint)]">
                {intl.formatMessage({ id: "fortune.bubble.bestAction" })}
              </span>
              <span className="text-[15px] font-bold text-orange-400">
                {summary.bestAction}
              </span>
            </div>

            <div className="fortune-indicator bg-purple-500/10 border-purple-500/20">
              <IconFlame size={20} className="text-purple-400" />
              <span className="text-[10px] uppercase tracking-wider text-[var(--text-faint)]">
                {intl.formatMessage({ id: "fortune.bubble.environment" })}
              </span>
              <span className="text-[15px] font-bold text-purple-400">
                {summary.environment}
              </span>
            </div>
          </div>

          {!isDetailReady && (
            <section className="space-y-3">
              <h3 className="flex items-center gap-2 text-[13px] font-semibold text-[var(--text-strong)]">
                <IconCompass size={16} className="text-[var(--accent)]" />
                {intl.formatMessage({ id: "fortune.detail.streamingTitle" })}
              </h3>
              <div className="fortune-detail-content">
                {isLoading && (
                  <div className="mb-3 flex items-center gap-2 text-xs text-[var(--text-faint)]">
                    <IconLoader2 size={14} className="animate-spin text-[var(--accent)]" />
                    <span>
                      {intl.formatMessage({
                        id: status === "loading"
                          ? "fortune.detail.loading"
                          : "fortune.detail.streaming",
                      })}
                    </span>
                  </div>
                )}

                {status === "error" && (
                  <div className="space-y-3">
                    <p className="text-xs text-red-300">
                      {error || intl.formatMessage({ id: "fortune.detail.streamError" })}
                    </p>
                    <button
                      onClick={() => void start()}
                      className="flex items-center gap-1 rounded-lg bg-white/5 px-3 py-1 text-[11px] font-medium text-[var(--accent)] transition-colors hover:bg-white/10"
                    >
                      <IconRefresh size={12} stroke={2} />
                      {intl.formatMessage({ id: "fortune.detail.retry" })}
                    </button>
                  </div>
                )}

                {status !== "error" && !isLoading && (
                  <p className="text-xs text-[var(--text-faint)]">
                    {intl.formatMessage({ id: "fortune.detail.loading" })}
                  </p>
                )}
              </div>
            </section>
          )}

          {isDetailReady && detail && (
            <>
              <section className="space-y-3">
                <h3 className="flex items-center gap-2 text-[13px] font-semibold text-[var(--text-strong)]">
                  <IconCompass size={16} className="text-[var(--accent)]" />
                  {intl.formatMessage({ id: "fortune.detail.qimen" })}
                </h3>
                <div className="fortune-detail-content">
                  <ReactMarkdown remarkPlugins={[remarkGfm]}>
                    {detail.qimenDetail}
                  </ReactMarkdown>
                </div>
              </section>

              {detail.ziweiDetail && (
                <section className="space-y-3">
                  <h3 className="flex items-center gap-2 text-[13px] font-semibold text-[var(--text-strong)]">
                    <IconSun size={16} className="text-purple-400" />
                    {intl.formatMessage({ id: "fortune.detail.ziwei" })}
                  </h3>
                  <div className="fortune-detail-content">
                    <ReactMarkdown remarkPlugins={[remarkGfm]}>
                      {detail.ziweiDetail}
                    </ReactMarkdown>
                  </div>
                </section>
              )}

              <section className="space-y-3">
                <h3 className="flex items-center gap-2 text-[13px] font-semibold text-[var(--text-strong)]">
                  <IconFlame size={16} className="text-yellow-400" />
                  {intl.formatMessage({ id: "fortune.detail.advice" })}
                </h3>
                <div className="fortune-detail-content">
                  <ReactMarkdown remarkPlugins={[remarkGfm]}>
                    {detail.advice}
                  </ReactMarkdown>
                </div>
              </section>
            </>
          )}
        </div>
      </div>
    </div>
  );
}
