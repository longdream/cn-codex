/**
 * 用量追踪 API 调用层
 */
import { invoke } from "@tauri-apps/api/core";
import type { UsageStats, DailyUsage, ModelUsage, UsageRecord, ModelPricing } from "../types/usage";

/** 获取全局汇总统计 */
export async function usageGetStats(sinceTimestamp?: number): Promise<UsageStats> {
  return invoke("usage_get_stats", { sinceTimestamp: sinceTimestamp ?? null });
}

/** 获取按天分组的用量（最近 N 天） */
export async function usageGetDaily(days?: number): Promise<DailyUsage[]> {
  return invoke("usage_get_daily", { days: days ?? null });
}

/** 获取按模型分组的用量统计 */
export async function usageGetByModel(sinceTimestamp?: number): Promise<ModelUsage[]> {
  return invoke("usage_get_by_model", { sinceTimestamp: sinceTimestamp ?? null });
}

/** 获取最近 N 条用量记录 */
export async function usageGetRecent(limit?: number): Promise<UsageRecord[]> {
  return invoke("usage_get_recent", { limit: limit ?? null });
}

/** 设置/更新模型价格 */
export async function usageSetPricing(
  modelPattern: string,
  promptPricePer1m: number,
  completionPricePer1m: number,
): Promise<void> {
  return invoke("usage_set_pricing", { modelPattern, promptPricePer1m, completionPricePer1m });
}

/** 获取所有定价信息 */
export async function usageGetPricing(): Promise<ModelPricing[]> {
  return invoke("usage_get_pricing");
}
