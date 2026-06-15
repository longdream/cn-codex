export interface PluginSkillRef {
  pluginId: string;
  skillId: string;
}

export interface RobotConfig {
  name: string;
  description: string;
  icon: string;
  skills: string[];
  pluginSkills: PluginSkillRef[];
  workflow: string[];
  systemPrompt: string;
  createdAt: number;
  updatedAt: number;
}

export interface RobotSummary {
  id: string;
  name: string;
  description: string;
  icon: string;
  skillsCount: number;
  pluginSkillsCount: number;
  workflowSteps: number;
  path: string;
}

export interface RobotDetail extends RobotConfig {
  id: string;
  path: string;
}
