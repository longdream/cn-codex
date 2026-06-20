import { describe, expect, it } from "vitest";

import { buildPastedImageName, extractClipboardImageFiles } from "../components/chat/ChatInput";

describe("clipboard image helpers", () => {
  it("builds deterministic pasted image names from mime type", () => {
    expect(buildPastedImageName("image/jpeg", 123, 1)).toBe("pasted-image-123-1.jpg");
    expect(buildPastedImageName("image/svg+xml", 123, 2)).toBe("pasted-image-123-2.svg");
    expect(buildPastedImageName("application/octet-stream", 123, 3)).toBe("pasted-image-123-3.png");
  });

  it("extracts clipboard images and generates fallback name for unnamed images", () => {
    const namedImage = new File(["named"], "capture.png", { type: "image/png" });
    const unnamedImage = new File(["unnamed"], "", { type: "image/png" });

    const items = [
      { type: "text/plain", getAsFile: () => null },
      { type: "image/png", getAsFile: () => namedImage },
      { type: "image/png", getAsFile: () => unnamedImage },
    ];

    const extracted = extractClipboardImageFiles(items, 456);
    expect(extracted).toHaveLength(2);
    expect(extracted[0]).toMatchObject({ file: namedImage, fallbackName: undefined });
    expect(extracted[1]).toMatchObject({
      file: unnamedImage,
      fallbackName: "pasted-image-456-2.png",
    });
  });

  it("ignores non-image clipboard items", () => {
    const items = [
      { type: "text/plain", getAsFile: () => null },
      { type: "application/json", getAsFile: () => null },
    ];

    expect(extractClipboardImageFiles(items, 789)).toEqual([]);
  });
});
