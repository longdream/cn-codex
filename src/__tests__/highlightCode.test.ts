import { describe, expect, it } from "vitest";
import { highlightCodeHtml } from "../utils/highlightCode";

function stripHtml(html: string): string {
  return html
    .replace(/<[^>]+>/g, "")
    .replace(/&lt;/g, "<")
    .replace(/&gt;/g, ">")
    .replace(/&amp;/g, "&")
    .replace(/&quot;/g, '"')
    .replace(/&#39;/g, "'")
    .replace(/&#x27;/gi, "'");
}

describe("highlightCodeHtml", () => {
  it("keeps source characters identical after highlighting", () => {
    const source = [
      "const value = 1;",
      "if (value > 0) {",
      "  console.log('ok');",
      "}",
      "",
    ].join("\n");

    const html = highlightCodeHtml(source, "typescript");
    expect(stripHtml(html)).toBe(source);
    expect(html).toContain("hljs-");
  });

  it("falls back to escaped plain text for unknown languages", () => {
    const source = "a < b && c > d";
    const html = highlightCodeHtml(source, "not-a-real-language");
    expect(html).toBe("a &lt; b &amp;&amp; c &gt; d");
    expect(stripHtml(html)).toBe(source);
  });
});
