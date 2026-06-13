import { invoke } from "@tauri-apps/api/core";
import type { SkillSummary, SkillDetail } from "../types/skill";

export async function skillList(): Promise<SkillSummary[]> {
  return invoke("skill_list");
}

export async function skillRead(skillId: string): Promise<SkillDetail> {
  return invoke("skill_read", { skillId });
}

export async function skillSetEnabled(skillId: string, enabled: boolean): Promise<void> {
  return invoke("skill_set_enabled", { skillId, enabled });
}
