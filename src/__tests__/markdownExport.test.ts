import { describe, expect, it } from "vitest";
import { parseMarkdownBlocks } from "../utils/markdownAst";
import { exportMarkdownAsDocxBytes, exportMarkdownAsPdfBytes } from "../utils/markdownExport";

describe("markdown export", () => {
  it("parses markdown blocks into structured AST", () => {
    const blocks = parseMarkdownBlocks(`# Title

Paragraph with **bold** and [link](https://example.com).

- Item A
- Item B

| Name | Score |
| --- | --- |
| Alice | 100 |
`);

    expect(blocks.length).toBeGreaterThan(0);
    expect(blocks.some((block) => block.type === "heading")).toBe(true);
    expect(blocks.some((block) => block.type === "paragraph")).toBe(true);
    expect(blocks.some((block) => block.type === "list")).toBe(true);
    expect(blocks.some((block) => block.type === "table")).toBe(true);
  });

  it("exports editable DOCX bytes", async () => {
    const bytes = await exportMarkdownAsDocxBytes(`# Demo

This is a **docx** export test.

1. Step one
2. Step two
`);

    // DOCX 是 ZIP 容器，文件头固定为 PK。
    expect(bytes.length).toBeGreaterThan(128);
    expect(bytes[0]).toBe(0x50);
    expect(bytes[1]).toBe(0x4b);
  });

  it("exports text-based PDF bytes", async () => {
    const bytes = await exportMarkdownAsPdfBytes(`# Demo

PDF text export line one.
PDF text export line two.
`);

    const header = new TextDecoder().decode(bytes.slice(0, 8));
    expect(bytes.length).toBeGreaterThan(128);
    expect(header.startsWith("%PDF")).toBe(true);
  });
});
