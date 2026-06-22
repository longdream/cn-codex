/**
 * 用量追踪仪表盘
 * 显示 token 使用统计、费用趋势、按模型分组等信息
 */
import { useCallback, useEffect, useState } from "react";
import { useIntl } from "react-intl";
import type { UsageStats, DailyUsage, ModelUsage } from "../../types/usage";
import { usageGetStats, usageGetDaily, usageGetByModel } from "../../api/usage";

/** 时间范围选项 */
type TimeRange = "7d" | "30d" | "all";

function rangeToDays(range: TimeRange): number {
  return range === "7d" ? 7 : range === "30d" ? 30 : 365;
}

/** 生成完整日期序列，将后端稀疏数据填充为连续每天 */
function fillDailyData(data: DailyUsage[], days: number): DailyUsage[] {
  const map = new Map(data.map((d) => [d.date, d]));
  const result: DailyUsage[] = [];
  const now = new Date();
  for (let i = days - 1; i >= 0; i--) {
    const d = new Date(now);
    d.setDate(d.getDate() - i);
    const dateStr = d.toISOString().slice(0, 10);
    result.push(
      map.get(dateStr) ?? {
        date: dateStr,
        requests: 0,
        promptTokens: 0,
        completionTokens: 0,
        totalTokens: 0,
        costUsd: 0,
      },
    );
  }
  return result;
}

/** 根据天数范围决定 X 轴标签间隔 */
function labelInterval(days: number): number {
  if (days <= 7) return 1;
  if (days <= 30) return 5;
  return 30;
}

/** 格式化日期为短标签 MM/DD */
function formatDateLabel(dateStr: string): string {
  const [, m, d] = dateStr.split("-");
  return `${m}/${d}`;
}

export function UsageDashboard() {
  const intl = useIntl();
  const [timeRange, setTimeRange] = useState<TimeRange>("30d");
  const [stats, setStats] = useState<UsageStats | null>(null);
  const [dailyUsage, setDailyUsage] = useState<DailyUsage[]>([]);
  const [modelUsage, setModelUsage] = useState<ModelUsage[]>([]);
  const [loading, setLoading] = useState(true);

  /** 根据时间范围计算 since timestamp */
  const getSinceTimestamp = useCallback((range: TimeRange): number | undefined => {
    if (range === "all") return undefined;
    const days = range === "7d" ? 7 : 30;
    return Math.floor(Date.now() / 1000) - days * 86400;
  }, []);

  /** 加载数据 */
  const loadData = useCallback(async () => {
    setLoading(true);
    try {
      const since = getSinceTimestamp(timeRange);
      const days = rangeToDays(timeRange);
      const [statsData, dailyData, modelData] = await Promise.all([
        usageGetStats(since),
        usageGetDaily(days),
        usageGetByModel(since),
      ]);
      setStats(statsData);
      setDailyUsage(fillDailyData(dailyData, days));
      setModelUsage(modelData);
    } catch (e) {
      console.error("Failed to load usage data:", e);
    } finally {
      setLoading(false);
    }
  }, [timeRange, getSinceTimestamp]);

  useEffect(() => {
    loadData();
  }, [loadData]);

  /** 格式化数字 */
  const formatNumber = (n: number | undefined | null) => {
    if (n == null) return "0";
    if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(2)}M`;
    if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`;
    return n.toString();
  };

  /** 格式化费用 */
  const formatCost = (cost: number | undefined | null) => {
    if (cost == null) return "$0.00";
    if (cost < 0.01) return `$${cost.toFixed(4)}`;
    return `$${cost.toFixed(2)}`;
  };

  if (loading && !stats) {
    return (
      <div className="flex items-center justify-center py-12">
        <div className="animate-pulse text-sm text-[var(--text-muted)]">
          {intl.formatMessage({ id: "usage.loading" })}
        </div>
      </div>
    );
  }

  return (
    <div className="space-y-5">
      {/* 时间范围选择器 */}
      <div className="flex items-center justify-between">
        <h4 className="text-sm font-semibold text-[var(--text-strong)]">
          {intl.formatMessage({ id: "usage.title" })}
        </h4>
        <div className="flex gap-1 rounded-lg bg-[var(--surface-soft)] p-0.5">
          {(["7d", "30d", "all"] as TimeRange[]).map((range) => (
            <button
              key={range}
              onClick={() => setTimeRange(range)}
              className={`rounded-md px-3 py-1 text-xs font-medium transition-colors ${
                timeRange === range
                  ? "bg-[var(--accent)] text-white"
                  : "text-[var(--text-muted)] hover:text-[var(--text-strong)]"
              }`}
            >
              {range === "7d"
                ? intl.formatMessage({ id: "usage.range.7d" })
                : range === "30d"
                  ? intl.formatMessage({ id: "usage.range.30d" })
                  : intl.formatMessage({ id: "usage.range.all" })}
            </button>
          ))}
        </div>
      </div>

      {/* 汇总卡片 */}
      {stats && (
        <div className="grid grid-cols-2 gap-3 md:grid-cols-4">
          <StatCard
            label={intl.formatMessage({ id: "usage.totalRequests" })}
            value={formatNumber(stats.totalRequests)}
          />
          <StatCard
            label={intl.formatMessage({ id: "usage.totalTokens" })}
            value={formatNumber(stats.totalTokens)}
          />
          <StatCard
            label={intl.formatMessage({ id: "usage.inputTokens" })}
            value={formatNumber(stats.totalPromptTokens)}
          />
          <StatCard
            label={intl.formatMessage({ id: "usage.totalCost" })}
            value={formatCost(stats.totalCostUsd)}
            highlight
          />
        </div>
      )}

      {/* 每日趋势 */}
      {dailyUsage.length > 0 && (
        <section className="settings-card space-y-3 overflow-visible">
          <h4 className="text-xs font-semibold uppercase tracking-wider text-[var(--text-muted)]">
            {intl.formatMessage({ id: "usage.dailyTrend" })}
          </h4>
          {(() => {
            const maxTokens = Math.max(...dailyUsage.map((d) => d.totalTokens), 1);
            const barHeight = 80;
            const days = rangeToDays(timeRange);
            const interval = labelInterval(days);
            return (
              <div className="relative pt-10">
                <div
                  className="flex items-end"
                  style={{ height: `${barHeight}px`, gap: days <= 7 ? "4px" : "1px" }}
                >
                  {dailyUsage.map((day) => {
                    const hasData = day.totalTokens > 0;
                    const h = hasData
                      ? Math.max((day.totalTokens / maxTokens) * barHeight, 4)
                      : 2;
                    return (
                      <div
                        key={day.date}
                        className="group relative flex flex-col items-center"
                        style={{ flex: "1 1 0", minWidth: 0 }}
                      >
                        {/* tooltip */}
                        <div className="pointer-events-none absolute bottom-full left-1/2 z-50 mb-2 hidden -translate-x-1/2 whitespace-nowrap rounded-md bg-[var(--surface-elevated)] px-2 py-1 text-[11px] shadow-lg group-hover:block">
                          <div className="font-medium text-[var(--text-strong)]">
                            {day.date}
                          </div>
                          <div className="text-[var(--text-muted)]">
                            {formatNumber(day.totalTokens)} tokens
                          </div>
                          <div className="text-[var(--text-muted)]">
                            {formatCost(day.costUsd)}
                          </div>
                        </div>
                        {/* bar */}
                        <div
                          className={`w-full rounded-t transition-opacity ${
                            hasData
                              ? "bg-[var(--accent)] opacity-70 hover:opacity-100"
                              : "bg-[var(--text-faint)] opacity-20"
                          }`}
                          style={{ height: `${h}px`, minWidth: "2px" }}
                        />
                      </div>
                    );
                  })}
                </div>
                {/* X 轴线 */}
                <div className="h-px w-full bg-[var(--border)]" />
                {/* X 轴日期标签 */}
                <div className="flex" style={{ gap: days <= 7 ? "4px" : "1px" }}>
                  {dailyUsage.map((day, idx) => {
                    const show = idx % interval === 0 || idx === dailyUsage.length - 1;
                    return (
                      <div
                        key={day.date}
                        className="overflow-hidden text-center"
                        style={{ flex: "1 1 0", minWidth: 0 }}
                      >
                        {show && (
                          <span className="mt-1 block truncate text-[10px] text-[var(--text-faint)]">
                            {formatDateLabel(day.date)}
                          </span>
                        )}
                      </div>
                    );
                  })}
                </div>
              </div>
            );
          })()}
        </section>
      )}

      {/* 按模型分组 */}
      {modelUsage.length > 0 && (
        <section className="settings-card space-y-3">
          <h4 className="text-xs font-semibold uppercase tracking-wider text-[var(--text-muted)]">
            {intl.formatMessage({ id: "usage.byModel" })}
          </h4>
          <div className="space-y-2">
            {modelUsage.map((item) => (
              <div
                key={`${item.provider}-${item.model}`}
                className="flex items-center justify-between rounded-md bg-[var(--surface-soft)] px-3 py-2"
              >
                <div className="flex items-center gap-2">
                  <span className="inline-block h-2 w-2 rounded-full bg-[var(--accent)]" />
                  <span className="text-xs font-medium text-[var(--text-strong)]">
                    {item.model}
                  </span>
                  <span className="text-[11px] text-[var(--text-faint)]">({item.provider})</span>
                </div>
                <div className="flex items-center gap-4 text-xs text-[var(--text-muted)]">
                  <span>{intl.formatMessage({ id: "usage.requestCount" }, { count: item.requests })}</span>
                  <span>{formatNumber(item.totalTokens)} tokens</span>
                  <span className="font-medium text-[var(--text-strong)]">
                    {formatCost(item.costUsd)}
                  </span>
                </div>
              </div>
            ))}
          </div>
        </section>
      )}

      {/* 空状态 */}
      {stats && stats.totalRequests === 0 && (
        <div className="flex flex-col items-center justify-center py-12 text-center">
          <div className="mb-2 text-3xl opacity-30">📊</div>
          <p className="text-sm text-[var(--text-muted)]">
            {intl.formatMessage({ id: "usage.empty" })}
          </p>
        </div>
      )}
    </div>
  );
}

/** 统计数字卡片 */
function StatCard({
  label,
  value,
  highlight,
}: {
  label: string;
  value: string;
  highlight?: boolean;
}) {
  return (
    <div className="rounded-lg bg-[var(--surface-soft)] p-3">
      <p className="text-[11px] font-medium uppercase tracking-wider text-[var(--text-faint)]">
        {label}
      </p>
      <p
        className={`mt-1 text-lg font-bold ${
          highlight ? "text-[var(--accent)]" : "text-[var(--text-strong)]"
        }`}
      >
        {value}
      </p>
    </div>
  );
}
