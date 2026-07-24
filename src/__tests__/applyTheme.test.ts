import { describe, expect, it } from "vitest";
import { applyDocumentTheme, resolveThemeMode } from "../utils/applyTheme";

describe("applyTheme", () => {
  it("resolves explicit theme preferences", () => {
    expect(resolveThemeMode("light")).toBe("light");
    expect(resolveThemeMode("dark")).toBe("dark");
  });

  it("resolves system theme from media preference", () => {
    expect(resolveThemeMode("system", true)).toBe("light");
    expect(resolveThemeMode("system", false)).toBe("dark");
  });

  it("applies light theme tokens to the document", () => {
    const root = document.createElement("html");
    const doc = {
      documentElement: root,
    } as unknown as Document;

    const resolved = applyDocumentTheme("light", doc);

    expect(resolved).toBe("light");
    expect(root.dataset.theme).toBe("light");
    expect(root.style.colorScheme).toBe("light");
  });
});
