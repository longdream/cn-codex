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

  it("strips provider-added trailing stars from directive paths", () => {
    const patch = [
      "*** Begin Patch ***",
      "*** Update File: D:\\work\\old.ts ***",
      "*** Move to: D:\\work\\new.ts ***",
      "@@",
      "-old",
      "+new",
      "*** End Patch ***",
    ].join("\n");

    const entries = parsePatchDiffEntries(patch);
    expect(entries).toHaveLength(1);
    expect(entries[0].paths).toEqual(["D:\\work\\old.ts", "D:\\work\\new.ts"]);
  });

  it("rejects a patch without an outer Begin Patch marker", () => {
    const patch = [
      "*** Update File: src-tauri/src/tool_executor.rs",
      "--- src-tauri/src/tool_executor.rs\t2025-04-05 10:00:00",
      "***************",
      "*** 963,976 ****",
      " old",
      "+new",
      "*** End of Patch ***",
    ].join("\n");

    expect(parsePatchDiffEntries(patch)).toEqual([]);
  });

  it("rejects nested patch wrappers instead of merging files", () => {
    const patch = [
      "*** Begin Patch",
      "*** Update File: src-tauri/src/tool_executor.rs",
      "@@",
      " unchanged",
      "*** Begin Patch",
      "*** Update File: src-tauri/src/tool_executor_tests.rs",
      "@@",
      "-old",
      "+new",
      "*** End Patch",
    ].join("\n");

    expect(parsePatchDiffEntries(patch)).toEqual([]);
  });

  it("rejects update sections that contain no text changes", () => {
    const patch = [
      "*** Begin Patch",
      "*** Update File: src/app.ts",
      "@@",
      " unchanged",
      "*** End Patch",
    ].join("\n");

    expect(parsePatchDiffEntries(patch)).toEqual([]);
  });

  it("parses multiple files inside one patch wrapper", () => {
    const patch = [
      "*** Begin Patch",
      "*** Update File: src/first.ts",
      "@@",
      "-old first",
      "+new first",
      "*** Update File: src/second.ts",
      "@@",
      "-old second",
      "+new second",
      "*** End Patch",
    ].join("\n");

    const entries = parsePatchDiffEntries(patch);
    expect(entries.map((entry) => entry.paths)).toEqual([
      ["src/first.ts"],
      ["src/second.ts"],
    ]);
  });
});
