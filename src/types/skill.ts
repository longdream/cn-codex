export interface SkillSummary {
  id: string;
  name: string;
  description: string;
  tags: string[];
  path: string;
}

export interface SkillCategoryConfig {
  labelId: string;
  ids: string[];
}

export interface SkillDetail {
  id: string;
  name: string;
  description: string;
  tags: string[];
  path: string;
  content: string;
}
