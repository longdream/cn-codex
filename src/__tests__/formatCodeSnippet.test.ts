import { describe, expect, it } from "vitest";

import { formatCodeSnippet } from "../utils/formatCodeSnippet";

describe("formatCodeSnippet", () => {
  it("formats snippet with path and line metadata", () => {
    const result = formatCodeSnippet({
      path: "src/main.ts",
      startLine: 12,
      endLine: 15,
      language: "typescript",
      content: "const a = 1;\nconst b = 2;",
    });

    expect(result.text).toContain("path: src/main.ts");
    expect(result.text).toContain("range: L12-L15");
    expect(result.text).toContain("lines: 4");
    expect(result.text).toContain("```typescript");
    expect(result.truncated).toBe(false);
  });

  it("truncates overly long content", () => {
    const result = formatCodeSnippet({
      path: "src/huge.ts",
      startLine: 1,
      endLine: 400,
      language: "typescript",
      content: "x".repeat(30),
      maxChars: 10,
    });

    expect(result.truncated).toBe(true);
    expect(result.text).toContain("[snippet truncated]");
    expect(result.originalChars).toBe(30);
    expect(result.emittedChars).toBeGreaterThan(10);
  });
});
