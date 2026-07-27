interface SkillLabEvaluationShape {
  criticalIssues?: unknown;
  critical_issues?: unknown;
  improveHints?: unknown;
  improve_hints?: unknown;
}

function stringItems(value: unknown): string[] {
  if (!Array.isArray(value)) return [];
  return value
    .filter((item): item is string => typeof item === "string")
    .map((item) => item.trim())
    .filter(Boolean);
}

function parseEvaluation(raw: string): SkillLabEvaluationShape | null {
  const candidates = [raw];
  const fencedJson = raw.match(/```(?:json)?\s*([\s\S]*?)```/i)?.[1]?.trim();
  if (fencedJson) candidates.unshift(fencedJson);

  for (const candidate of candidates) {
    try {
      const parsed = JSON.parse(candidate);
      if (parsed && typeof parsed === "object" && !Array.isArray(parsed)) {
        return parsed as SkillLabEvaluationShape;
      }
    } catch {
      // 非 JSON 评估文本应原样保留，供用户继续编辑。
    }
  }
  return null;
}

export function formatSkillLabImprovementRequest(
  raw: string | null | undefined,
): string {
  const trimmed = raw?.trim() ?? "";
  if (!trimmed) return "";

  const evaluation = parseEvaluation(trimmed);
  if (!evaluation) return trimmed;

  const criticalIssues = stringItems(
    evaluation.criticalIssues ?? evaluation.critical_issues,
  );
  const improveHints = stringItems(
    evaluation.improveHints ?? evaluation.improve_hints,
  );
  if (criticalIssues.length === 0 && improveHints.length === 0) {
    return trimmed;
  }

  const sections: string[] = [];
  if (criticalIssues.length > 0) {
    sections.push(`关键问题：\n${criticalIssues.map((item) => `- ${item}`).join("\n")}`);
  }
  if (improveHints.length > 0) {
    sections.push(`改进建议：\n${improveHints.map((item) => `- ${item}`).join("\n")}`);
  }
  return sections.join("\n\n");
}

export function needsSkillLabImprovement(
  status: string,
  lastEvaluation: string | null | undefined,
): boolean {
  return status === "failed" && Boolean(lastEvaluation?.trim());
}
