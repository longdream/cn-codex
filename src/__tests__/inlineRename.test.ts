import { describe, expect, it } from "vitest";
import { resolveInlineRename } from "../utils/inlineRename";

describe("resolveInlineRename", () => {
  it("trims and returns a changed non-empty name", () => {
    expect(resolveInlineRename("  renamed.ts  ", "old.ts")).toEqual({
      shouldRename: true,
      name: "renamed.ts",
    });
  });

  it.each(["", "   ", " old.ts "])("does not submit %j", (value) => {
    expect(resolveInlineRename(value, "old.ts")).toEqual({ shouldRename: false });
  });
});
