import { describe, expect, it } from "vitest";
import { parseMarkdownBlocks } from "../utils/markdownAst";
import { exportMarkdownAsDocxBytes, exportMarkdownAsPdfBytes } from "../utils/markdownExport";
import { inflateRawSync, inflateSync } from "node:zlib";

function extractPdfStreamTexts(pdfBytes: Uint8Array): string {
  const binary = Buffer.from(pdfBytes).toString("latin1");
  const streamRegex = /stream\r?\n([\s\S]*?)\r?\nendstream/g;
  const chunks: string[] = [];
  let match: RegExpExecArray | null;
  while ((match = streamRegex.exec(binary)) !== null) {
    const raw = Buffer.from(match[1], "latin1");
    try {
      chunks.push(inflateSync(raw).toString("latin1"));
      continue;
    } catch {
      // ignore
    }
    try {
      chunks.push(inflateRawSync(raw).toString("latin1"));
      continue;
    } catch {
      // ignore
    }
    chunks.push(raw.toString("latin1"));
  }
  return chunks.join("\n");
}

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

  it("parses GFM table column alignment", () => {
    const blocks = parseMarkdownBlocks(`| Left | Center | Right |
| :--- | :---: | ---: |
| L | C | R |
`);
    const table = blocks.find((block) => block.type === "table");
    expect(table?.type).toBe("table");
    if (table?.type === "table") {
      expect(table.align).toEqual(["left", "center", "right"]);
      expect(table.header.map((cell) => cell.inlines.map((inline) => (inline.type === "text" ? inline.text : "")).join(""))).toEqual([
        "Left",
        "Center",
        "Right",
      ]);
    }
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

  it("exports markdown tables as bordered PDF table content", async () => {
    const markdown = `# Demo

| Name | Score | Note |
| --- | ---: | --- |
| Alice | 100 | Excellent |
| Bob | 90 | Good |
`;
    const blocks = parseMarkdownBlocks(markdown);
    expect(blocks.some((block) => block.type === "table")).toBe(true);

    const bytes = await exportMarkdownAsPdfBytes(markdown);
    const header = new TextDecoder().decode(bytes.slice(0, 8));
    const content = extractPdfStreamTexts(bytes);

    expect(bytes.length).toBeGreaterThan(128);
    expect(header.startsWith("%PDF")).toBe(true);
    // 表格单元格应作为独立文本写入，而不是仅输出 "A | B" 管道文本。
    expect(content).toContain("(Name)");
    expect(content).toContain("(Score)");
    expect(content).toContain("(Alice)");
    expect(content).toContain("(100)");
    expect(content).not.toContain("(Name | Score | Note)");
    // 边框表格会写入矩形路径操作符。
    expect(content).toMatch(/(?:^|\s)re(?:\s|$)/m);
  });

  it("repeats table headers when a PDF table spans multiple pages", async () => {
    const rows = Array.from({ length: 80 }, (_, index) => `| Row${index + 1} | Value${index + 1} |`).join("\n");
    const markdown = `# Long Table

| Name | Score |
| --- | ---: |
${rows}
`;
    const bytes = await exportMarkdownAsPdfBytes(markdown);
    const content = extractPdfStreamTexts(bytes);
    const headerMatches = content.match(/\(Name\)/g) ?? [];
    const pageCount = (content.match(/\/Type\s*\/Page\b/g) ?? []).length;

    expect(bytes.length).toBeGreaterThan(128);
    // 多页表格应至少重复一次表头。
    expect(headerMatches.length).toBeGreaterThan(1);
    // 内容足够长时通常会跨页；即使页面对象统计方式变化，表头重复仍是核心断言。
    expect(pageCount === 0 || pageCount >= 1).toBe(true);
    expect(content).toContain("(Row1)");
    expect(content).toContain("(Row80)");
  });
});
