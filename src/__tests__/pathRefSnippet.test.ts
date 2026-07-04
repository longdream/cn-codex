import { describe, expect, it } from "vitest";
import {
  buildPathRefPromptValue,
  decodePathRefRangeSnippet,
  encodePathRefRangeSnippet,
  formatPathRefLineRange,
  supportsPathRefRangeByName,
} from "../utils/pathRefSnippet";

describe("pathRefSnippet", () => {
  it("supports txt/md extensions for range-tag insertion", () => {
    expect(supportsPathRefRangeByName("notes.md")).toBe(true);
    expect(supportsPathRefRangeByName("notes.mdx")).toBe(true);
    expect(supportsPathRefRangeByName("notes.txt")).toBe(true);
    expect(supportsPathRefRangeByName("main.rs")).toBe(false);
  });

  it("encodes and decodes range-tag payload safely", () => {
    const encoded = encodePathRefRangeSnippet({
      kind: "pathRefRange",
      sourcePath: "D:/workspace/readme.md",
      name: "readme.md",
      lineStart: 3,
      lineEnd: 15,
    });
    const decoded = decodePathRefRangeSnippet(encoded);
    expect(decoded).toEqual({
      kind: "pathRefRange",
      sourcePath: "D:/workspace/readme.md",
      name: "readme.md",
      lineStart: 3,
      lineEnd: 15,
    });
  });

  it("builds prompt value with line range when present", () => {
    const withRange = buildPathRefPromptValue({
      kind: "pathRef",
      name: "doc.md",
      type: "application/x-cn-codex-path-ref",
      size: 0,
      sourcePath: "D:/workspace/doc.md",
      lineStart: 10,
      lineEnd: 42,
    });
    const withoutRange = buildPathRefPromptValue({
      kind: "pathRef",
      name: "doc.md",
      type: "application/x-cn-codex-path-ref",
      size: 0,
      sourcePath: "D:/workspace/doc.md",
    });
    expect(withRange).toBe("D:/workspace/doc.md#L10-L42");
    expect(withoutRange).toBe("D:/workspace/doc.md");
    expect(formatPathRefLineRange({ lineStart: 10, lineEnd: 42 })).toBe("L10-L42");
    expect(formatPathRefLineRange({ lineStart: undefined, lineEnd: 42 })).toBeNull();
  });
});
