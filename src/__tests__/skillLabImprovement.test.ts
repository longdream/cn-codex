import { describe, expect, it } from "vitest";

import {
  formatSkillLabImprovementRequest,
  needsSkillLabImprovement,
} from "../utils/skillLabImprovement";

describe("formatSkillLabImprovementRequest", () => {
  it("formats camelCase evaluation JSON into editable requirements", () => {
    expect(formatSkillLabImprovementRequest(JSON.stringify({
      criticalIssues: ["工具边界不清晰"],
      improveHints: ["补充失败降级"],
    }))).toBe("关键问题：\n- 工具边界不清晰\n\n改进建议：\n- 补充失败降级");
  });

  it("formats snake_case evaluation JSON", () => {
    expect(formatSkillLabImprovementRequest(JSON.stringify({
      critical_issues: ["缺少输入校验"],
      improve_hints: ["说明非法输入处理"],
    }))).toBe("关键问题：\n- 缺少输入校验\n\n改进建议：\n- 说明非法输入处理");
  });

  it("extracts evaluation JSON from a fenced Markdown block", () => {
    const raw = [
      "评估结果如下：",
      "```json",
      JSON.stringify({
        criticalIssues: ["步骤不完整"],
        improveHints: ["补充验证步骤"],
      }),
      "```",
    ].join("\n");

    expect(formatSkillLabImprovementRequest(raw)).toBe(
      "关键问题：\n- 步骤不完整\n\n改进建议：\n- 补充验证步骤",
    );
  });

  it("keeps natural-language evaluations unchanged", () => {
    const raw = "需要明确工具调用失败后的降级方案。";
    expect(formatSkillLabImprovementRequest(raw)).toBe(raw);
  });

  it("falls back to raw JSON text when issue arrays are empty", () => {
    const raw = JSON.stringify({ criticalIssues: [], improveHints: [] });
    expect(formatSkillLabImprovementRequest(raw)).toBe(raw);
  });

  it("returns an empty string for blank input", () => {
    expect(formatSkillLabImprovementRequest("  \n ")).toBe("");
    expect(formatSkillLabImprovementRequest(null)).toBe("");
  });
});

describe("needsSkillLabImprovement", () => {
  it("requires a failed status and a nonblank latest evaluation", () => {
    expect(needsSkillLabImprovement("failed", "需要补充降级策略")).toBe(true);
    expect(needsSkillLabImprovement("failed", "  ")).toBe(false);
    expect(needsSkillLabImprovement("passed", "仍有可选建议")).toBe(false);
    expect(needsSkillLabImprovement("idle", "需要改进")).toBe(false);
    expect(needsSkillLabImprovement("failed", null)).toBe(false);
  });
});
