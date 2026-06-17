import { describe, expect, it } from "vitest";

import {
  buildSkillScopedPrompt,
  parseModifyRobotCommand,
  parseSkillCommand,
  parseSkillSelectionQuery,
  shouldShowSlashPanel,
} from "../components/chat/ChatInput";

describe("ChatInput slash helpers", () => {
  it("parses /skill with objective", () => {
    expect(parseSkillCommand("/skill code-review fix flaky test")).toEqual({
      skillId: "code-review",
      objective: "fix flaky test",
    });
  });

  it("parses /skill with skill only", () => {
    expect(parseSkillCommand("/skill browser")).toEqual({
      skillId: "browser",
      objective: "",
    });
  });

  it("returns null for invalid /skill command", () => {
    expect(parseSkillCommand("/skill")).toBeNull();
    expect(parseSkillCommand("skill browser")).toBeNull();
  });

  it("extracts skill picker query only before objective", () => {
    expect(parseSkillSelectionQuery("/skill")).toBe("");
    expect(parseSkillSelectionQuery("/skill browser")).toBe("browser");
    expect(parseSkillSelectionQuery("/skill browser fix login")).toBeNull();
    expect(parseSkillSelectionQuery("/goal something")).toBeNull();
  });

  it("parses /modifyrobot and legacy /modify", () => {
    expect(parseModifyRobotCommand("/modifyrobot")).toEqual({ objective: "" });
    expect(parseModifyRobotCommand("/modifyrobot tighten workflow")).toEqual({
      objective: "tighten workflow",
    });
    expect(parseModifyRobotCommand("/modify update system prompt")).toEqual({
      objective: "update system prompt",
    });
  });

  it("decides when slash panel should stay visible", () => {
    expect(shouldShowSlashPanel("/")).toBe(true);
    expect(shouldShowSlashPanel("/mo")).toBe(true);
    expect(shouldShowSlashPanel("/skill")).toBe(true);
    expect(shouldShowSlashPanel("/skill browser")).toBe(true);
    expect(shouldShowSlashPanel("/skill browser improve flows")).toBe(false);
    expect(shouldShowSlashPanel("plain text")).toBe(false);
  });

  it("wraps a skill-scoped prompt", () => {
    expect(buildSkillScopedPrompt("code-review", "fix tests")).toContain(
      'skill "code-review"',
    );
  });
});
