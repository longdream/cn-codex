import { describe, expect, it } from "vitest";
import { parsePatchDiffEntries } from "../utils/parsePatchDiff";

describe("parsePatchDiffEntries", () => {
  it("parses update file hunks into before/after", () => {
    const patch = [
      "*** Begin Patch",
      "*** Update File: src/foo.ts",
      "@@",
      " context",
      "-old",
      "+new",
      " context",
      "*** End Patch",
    ].join("\n");

    const entries = parsePatchDiffEntries(patch);
    expect(entries).toHaveLength(1);
    expect(entries[0].paths).toEqual(["src/foo.ts"]);
    expect(entries[0].beforeContent).toBe("context\nold\ncontext");
    expect(entries[0].afterContent).toBe("context\nnew\ncontext");
  });

  it("parses add and delete files", () => {
    const patch = [
      "*** Begin Patch",
      "*** Add File: a.txt",
      "+hello",
      "*** Delete File: b.txt",
      "*** End Patch",
    ].join("\n");

    const entries = parsePatchDiffEntries(patch);
    expect(entries).toHaveLength(2);
    expect(entries[0]).toMatchObject({
      paths: ["a.txt"],
      beforeContent: "",
      afterContent: "hello",
    });
    expect(entries[1]).toMatchObject({
      paths: ["b.txt"],
      beforeContent: "[deleted file]",
      afterContent: "",
    });
  });
});
