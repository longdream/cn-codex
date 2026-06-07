export type TierLevel = "low" | "medium" | "high";

export interface LlmTierConfig {
  model: string;
  reasoningEffort: string;
  serviceTier: string;
  description: string;
}

export interface TokenBudget {
  dailyLimit: number;
  warningThreshold: number;
  autoDowngrade: boolean;
}

export interface TierUsageStats {
  lowTokens: number;
  mediumTokens: number;
  highTokens: number;
  totalTokens: number;
}

export interface LlmTiersConfig {
  tiers: Record<TierLevel, LlmTierConfig>;
  scenes: Record<string, TierLevel>;
  budget: TokenBudget;
}
