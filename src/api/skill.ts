import { invoke } from "@tauri-apps/api/core";
import type { SkillCategoryConfig, SkillSummary, SkillDetail } from "../types/skill";

export async function skillList(): Promise<SkillSummary[]> {
  return invoke("skill_list");
}

export async function skillRead(skillId: string): Promise<SkillDetail> {
  return invoke("skill_read", { skillId });
}

export async function skillCategoriesRead(): Promise<SkillCategoryConfig[]> {
  return invoke("skill_categories_read");
}
