/**
 * 用量追踪相关类型定义
 */

/** 单条用量记录 */
export interface UsageRecord {
  id: number;
  /** 供应商标识 */
  provider: string;
  /** 模型名称 */
  model: string;
  /** 线程 ID */
  threadId: string;
  /** 输入 token 数 */
  promptTokens: number;
  /** 输出 token 数 */
  completionTokens: number;
  /** 总 token 数 */
  totalTokens: number;
  /** 费用（美元） */
  costUsd: number;
  /** 时间戳（Unix 秒） */
  timestamp: number;
}

/** 全局汇总统计 */
export interface UsageStats {
  totalRequests: number;
  totalPromptTokens: number;
  totalCompletionTokens: number;
  totalTokens: number;
  totalCostUsd: number;
}

/** 按天分组的用量 */
export interface DailyUsage {
  date: string;
  requests: number;
  promptTokens: number;
  completionTokens: number;
  totalTokens: number;
  costUsd: number;
}

/** 按模型分组的用量 */
export interface ModelUsage {
  provider: string;
  model: string;
  requests: number;
  promptTokens: number;
  completionTokens: number;
  totalTokens: number;
  costUsd: number;
}

/** 模型定价 */
export interface ModelPricing {
  modelPattern: string;
  promptPricePer1m: number;
  completionPricePer1m: number;
}
