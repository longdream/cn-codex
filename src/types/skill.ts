export interface SkillSummary {
  id: string;
  name: string;
  description: string;
  tags: string[];
  path: string;
  enabled: boolean;
}

export interface SkillDetail {
  id: string;
  name: string;
  description: string;
  tags: string[];
  path: string;
  content: string;
  enabled: boolean;
}
