import { invoke } from "@tauri-apps/api/core";
import type { LlmTiersConfig, LlmTierConfig, TierLevel, TierUsageStats } from "../types";

export async function llmGetTiers(): Promise<LlmTiersConfig> {
  return invoke("llm_get_tiers");
}

export async function llmSetTier(level: TierLevel, config: LlmTierConfig): Promise<void> {
  return invoke("llm_set_tier", { level, config });
}

export async function llmResolveTier(scene: string): Promise<LlmTierConfig> {
  return invoke("llm_resolve_tier", { scene });
}

export async function llmGetUsage(): Promise<TierUsageStats> {
  return invoke("llm_get_usage");
}

export async function llmRecordUsage(tokens: number): Promise<void> {
  return invoke("llm_record_usage", { tokens });
}

export async function llmBindScene(scene: string, level: TierLevel): Promise<void> {
  return invoke("llm_bind_scene", { scene, level });
}
