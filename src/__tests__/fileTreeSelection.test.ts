import { describe, expect, it } from "vitest";
import {
  computeNextSelection,
  getBaseName,
  getParentPath,
  isPathInside,
  joinPath,
  pruneNestedPaths,
  resolveCreateParentPath,
  uniqueNameInDirectory,
} from "../utils/fileTreeSelection";

describe("fileTreeSelection helpers", () => {
  it("joins and splits paths with mixed separators", () => {
    expect(joinPath("E:\\work\\demo", "src")).toBe("E:\\work\\demo\\src");
    expect(joinPath("/tmp/demo", "src")).toBe("/tmp/demo/src");
    expect(getParentPath("E:\\work\\demo\\src\\a.ts")).toBe("E:\\work\\demo\\src");
    expect(getBaseName("E:\\work\\demo\\src\\a.ts")).toBe("a.ts");
  });

  it("supports ctrl/cmd toggle and shift range selection", () => {
    const visible = ["a", "b", "c", "d"];

    const toggled = computeNextSelection(visible, ["a"], "c", "toggle", "a");
    expect(toggled.selectedPaths).toEqual(["a", "c"]);
    expect(toggled.anchorPath).toBe("c");

    const ranged = computeNextSelection(visible, ["a"], "d", "range", "b");
    expect(ranged.selectedPaths).toEqual(["b", "c", "d"]);
    expect(ranged.anchorPath).toBe("b");

    const replaced = computeNextSelection(visible, ["a", "c"], "b", "replace", "a");
    expect(replaced.selectedPaths).toEqual(["b"]);
    expect(replaced.anchorPath).toBe("b");
  });

  it("prunes nested paths and resolves create parent", () => {
    expect(isPathInside("E:/work/demo/src", "E:/work/demo/src/a.ts")).toBe(true);
    expect(pruneNestedPaths([
      "E:/work/demo/src",
      "E:/work/demo/src/a.ts",
      "E:/work/demo/docs",
    ])).toEqual([
      "E:/work/demo/docs",
      "E:/work/demo/src",
    ]);

    expect(resolveCreateParentPath("E:/work/demo", [{ path: "E:/work/demo/src/a.ts", isDir: false }]))
      .toBe("E:/work/demo/src");
    expect(resolveCreateParentPath("E:/work/demo", [{ path: "E:/work/demo/src", isDir: true }]))
      .toBe("E:/work/demo/src");
  });

  it("generates unique names for paste/new entries", () => {
    expect(uniqueNameInDirectory(["a.ts", "b.ts"], "c.ts")).toBe("c.ts");
    expect(uniqueNameInDirectory(["a.ts"], "a.ts")).toBe("a copy.ts");
    expect(uniqueNameInDirectory(["a.ts", "a copy.ts"], "a.ts")).toBe("a copy 2.ts");
    expect(uniqueNameInDirectory(["folder"], "folder")).toBe("folder copy");
  });
});
