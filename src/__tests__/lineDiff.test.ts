import { describe, expect, it } from "vitest";
import { buildPatchLineDiff } from "../utils/lineDiff";

describe("buildPatchLineDiff", () => {
  it("marks removed and added lines", () => {
    const lines = buildPatchLineDiff(
      "alpha\nbeta\ngamma",
      "alpha\nbeta2\ngamma\ndelta",
    );

    expect(lines.some((line) => line.type === "remove" && line.text === "beta")).toBe(
      true,
    );
    expect(lines.some((line) => line.type === "add" && line.text === "beta2")).toBe(
      true,
    );
    expect(lines.some((line) => line.type === "add" && line.text === "delta")).toBe(
      true,
    );
  });

  it("treats empty text as zero lines", () => {
    const addLines = buildPatchLineDiff("", "hello");
    expect(addLines).toEqual([
      {
        type: "add",
        text: "hello",
        newLineNumber: 1,
      },
    ]);

    const removeLines = buildPatchLineDiff("hello", "");
    expect(removeLines).toEqual([
      {
        type: "remove",
        text: "hello",
        oldLineNumber: 1,
      },
    ]);
  });
});
