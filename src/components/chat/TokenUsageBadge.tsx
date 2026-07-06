import { useMemo, useState } from "react";
import { useIntl } from "react-intl";
import { IconBolt } from "@tabler/icons-react";
import { useAppStore } from "../../stores/appStore";
import { formatDuration } from "../../utils/formatDuration";

function formatCount(value: number): string {
  const safe = Number.isFinite(value) ? Math.max(0, Math.round(value)) : 0;
  return new Intl.NumberFormat(undefined, { maximumFractionDigits: 0 }).format(safe);
}

function formatPercent(value: number): string {
  const safe = Number.isFinite(value) ? Math.min(1, Math.max(0, value)) : 0;
  return `${(safe * 100).toFixed(1)}%`;
}

interface Aggregate {
  prompt: number;
  completion: number;
  total: number;
  cached: number;
  cacheWrite: number;
  reasoning: number;
  durationMs: number;
}

/**
 * 顶部状态栏中的 Token 用量徽章：展示本次对话的 Token 总和（带 ⚡ 图标），
 * 鼠标悬停或聚焦时弹出完整消耗明细。明细为本次对话所有轮次的累加总和，
 * 仅依赖已持久化的 RunSummary 与实时用量，因此关闭对话再打开仍可查看。
 */
export function TokenUsageBadge() {
  const intl = useIntl();
  const messages = useAppStore((s) => s.messages);
  const liveUsage = useAppStore((s) => s.liveTurnUsage);
  const [open, setOpen] = useState(false);

  const agg = useMemo<Aggregate>(() => {
    const acc: Aggregate = {
      prompt: 0,
      completion: 0,
      total: 0,
      cached: 0,
      cacheWrite: 0,
      reasoning: 0,
      durationMs: 0,
    };
    for (const m of messages) {
      const u = m.runSummary?.usage;
      if (u) {
        acc.prompt += u.promptTokens ?? 0;
        acc.completion += u.completionTokens ?? 0;
        acc.total += u.totalTokens ?? u.promptTokens + u.completionTokens;
        acc.cached += u.cachedTokens ?? 0;
        acc.cacheWrite += u.cacheCreationTokens ?? 0;
        acc.reasoning += u.reasoningTokens ?? 0;
      }
      if (m.runSummary?.durationMs) {
        acc.durationMs += m.runSummary.durationMs;
      }
    }
    if (liveUsage) {
      acc.prompt += liveUsage.promptTokens ?? 0;
      acc.completion += liveUsage.completionTokens ?? 0;
      acc.total += liveUsage.totalTokens ?? liveUsage.promptTokens + liveUsage.completionTokens;
      acc.cached += liveUsage.cachedTokens ?? 0;
      acc.cacheWrite += liveUsage.cacheCreationTokens ?? 0;
      acc.reasoning += liveUsage.reasoningTokens ?? 0;
    }
    return acc;
  }, [messages, liveUsage]);

  if (agg.total <= 0) {
    return null;
  }

  const cacheMiss = Math.max(0, agg.prompt - agg.cached);
  const response = Math.max(0, agg.completion - agg.reasoning);
  const cacheHitRate = agg.prompt > 0 ? agg.cached / agg.prompt : 0;

  const t = (id: string) => intl.formatMessage({ id });

  return (
    <div
      className="relative"
      onMouseEnter={() => setOpen(true)}
      onMouseLeave={() => setOpen(false)}
    >
      <button
        type="button"
        className="flex items-center gap-1 rounded-[var(--radius-sm)] border border-[var(--chat-line)] bg-[var(--chat-chip)] px-2 h-6 font-mono text-[11px] text-[var(--chat-muted)] transition-colors hover:border-[var(--accent-border)] hover:text-[var(--accent)]"
        title={t("chat.tokenDetail.title")}
        onClick={() => setOpen((v) => !v)}
        onFocus={() => setOpen(true)}
        onBlur={() => setOpen(false)}
      >
        <IconBolt size={13} stroke={1.8} className="text-[var(--accent)]" />
        {formatCount(agg.total)}
      </button>

      {open && (
        <div className="token-detail-popover absolute right-0 top-full z-50 mt-2 w-[280px] rounded-[var(--radius-lg)] border border-[var(--chat-line)] bg-[var(--surface-panel)] p-3 shadow-[0_8px_28px_rgba(0,0,0,0.28)]">
          <div className="mb-2 text-[12px] font-semibold text-[var(--text-strong)]">
            {t("chat.tokenDetail.title")}
          </div>

          <div className="space-y-1 font-mono text-[12px]">
            <Row label={t("chat.tokenDetail.total")} value={formatCount(agg.total)} emphasize />

            <div className="mt-1 border-t border-[var(--chat-line)] pt-1">
              <Row label={t("chat.tokenDetail.input")} value={formatCount(agg.prompt)} />
              <SubRow label={t("chat.tokenDetail.cached")} value={formatCount(agg.cached)} />
              <SubRow label={t("chat.tokenDetail.cacheMiss")} value={formatCount(cacheMiss)} />
              <SubRow label={t("chat.tokenDetail.cacheWrite")} value={formatCount(agg.cacheWrite)} />
            </div>

            <div className="border-t border-[var(--chat-line)] pt-1">
              <Row label={t("chat.tokenDetail.output")} value={formatCount(agg.completion)} />
              <SubRow label={t("chat.tokenDetail.reasoning")} value={formatCount(agg.reasoning)} />
              <SubRow label={t("chat.tokenDetail.response")} value={formatCount(response)} />
            </div>

            <div className="border-t border-[var(--chat-line)] pt-1">
              <Row label={t("chat.tokenDetail.cacheHitRate")} value={formatPercent(cacheHitRate)} />
              <Row label={t("chat.tokenDetail.duration")} value={formatDuration(agg.durationMs)} />
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

function Row({ label, value, emphasize }: { label: string; value: string; emphasize?: boolean }) {
  return (
    <div className="flex items-center justify-between gap-3">
      <span className={emphasize ? "font-semibold text-[var(--text-strong)]" : "text-[var(--chat-muted)]"}>
        {label}
      </span>
      <span className={emphasize ? "font-semibold text-[var(--accent)]" : "text-[var(--chat-prose)]"}>
        {value}
      </span>
    </div>
  );
}

function SubRow({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex items-center justify-between gap-3 pl-3">
      <span className="text-[var(--chat-faint)]">{label}</span>
      <span className="text-[var(--chat-muted)]">{value}</span>
    </div>
  );
}
