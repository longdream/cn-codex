import { describe, expect, it } from "vitest";
import {
  normalizeReportedFileChanges,
  normalizeReportedFilePath,
} from "./reportedFilePath";

describe("normalizeReportedFilePath", () => {
  it("repairs legacy slash-separated Git octal escapes", () => {
    const path = "education/docs//346/225/231/350/202/262IDE_/351/234/200/346/261/202/346/226/207/346/241/243_/345/217/257/345/217/202/350/265/233/347/211/210.md";

    expect(normalizeReportedFilePath(path)).toBe(
      "education/docs/教育IDE_需求文档_可参赛版.md",
    );
  });

  it("does not reinterpret ordinary numeric path segments", () => {
    expect(normalizeReportedFilePath("reports/123/456/result.md")).toBe(
      "reports/123/456/result.md",
    );
  });

  it("normalizes paths while preserving change metadata", () => {
    expect(
      normalizeReportedFileChanges([
        { path: "docs//346/225/231.md", action: "modified" },
      ]),
    ).toEqual([{ path: "docs/教.md", action: "modified" }]);
  });
});
