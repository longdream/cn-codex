export interface SkillSummary {
  id: string;
  name: string;
  description: string;
  tags: string[];
  path: string;
}

export interface SkillDetail {
  id: string;
  name: string;
  description: string;
  tags: string[];
  path: string;
  content: string;
}
