import {
  BorderStyle,
  Document,
  HeadingLevel,
  Packer,
  Paragraph,
  Table,
  TableCell,
  TableRow,
  TextRun,
  UnderlineType,
  WidthType,
} from "docx";
import { jsPDF } from "jspdf";
import {
  flattenInlineText,
  parseMarkdownBlocks,
  type MarkdownBlock,
  type MarkdownInline,
  type MarkdownListBlock,
} from "./markdownAst";
import { ensurePdfTextFont } from "./pdfFont";

const DOCX_LIST_INDENT_STEP = 360;
const PDF_MARGIN_X = 40;
const PDF_MARGIN_Y = 42;
const PDF_DEFAULT_FONT_SIZE = 11;
const PDF_DEFAULT_LINE_HEIGHT = 18;
const PDF_PARAGRAPH_SPACING = 8;

interface DocxInlineStyle {
  bold?: boolean;
  italics?: boolean;
  strike?: boolean;
  code?: boolean;
}

interface PdfCursorState {
  doc: jsPDF;
  cursorY: number;
  pageHeight: number;
  contentWidth: number;
}

type DocxHeadingLevel = (typeof HeadingLevel)[keyof typeof HeadingLevel];
type TableAlign = "left" | "center" | "right" | null;

function headingLevelByDepth(depth: number): DocxHeadingLevel {
  switch (depth) {
    case 1:
      return HeadingLevel.HEADING_1;
    case 2:
      return HeadingLevel.HEADING_2;
    case 3:
      return HeadingLevel.HEADING_3;
    case 4:
      return HeadingLevel.HEADING_4;
    case 5:
      return HeadingLevel.HEADING_5;
    default:
      return HeadingLevel.HEADING_6;
  }
}

function textRunsFromInline(inline: MarkdownInline, style: DocxInlineStyle = {}): TextRun[] {
  switch (inline.type) {
    case "text":
      return [
        new TextRun({
          text: inline.text,
          bold: style.bold,
          italics: style.italics,
          strike: style.strike,
          font: style.code ? "Consolas" : undefined,
        }),
      ];
    case "inlineCode":
      return [
        new TextRun({
          text: inline.text,
          bold: style.bold,
          italics: style.italics,
          strike: style.strike,
          font: "Consolas",
          color: "334155",
        }),
      ];
    case "lineBreak":
      return [new TextRun({ text: "", break: 1 })];
    case "strong":
      return inline.children.flatMap((child) =>
        textRunsFromInline(child, { ...style, bold: true })
      );
    case "emphasis":
      return inline.children.flatMap((child) =>
        textRunsFromInline(child, { ...style, italics: true })
      );
    case "delete":
      return inline.children.flatMap((child) =>
        textRunsFromInline(child, { ...style, strike: true })
      );
    case "link": {
      const label = flattenInlineText(inline.children).trim() || inline.url;
      return [
        new TextRun({
          text: `${label} (${inline.url})`,
          color: "1D4ED8",
          underline: { type: UnderlineType.SINGLE },
          bold: style.bold,
          italics: style.italics,
          strike: style.strike,
        }),
      ];
    }
    default:
      return [];
  }
}

function paragraphFromInlines(
  inlines: MarkdownInline[],
  options?: {
    heading?: DocxHeadingLevel;
    indentLeft?: number;
    prefix?: string;
    spacingBefore?: number;
    spacingAfter?: number;
    align?: TableAlign;
  },
): Paragraph {
  const runs = inlines.flatMap((inline) => textRunsFromInline(inline));
  if (options?.prefix) {
    runs.unshift(new TextRun({ text: options.prefix }));
  }
  const alignment =
    options?.align === "center"
      ? "center"
      : options?.align === "right"
        ? "right"
        : options?.align === "left"
          ? "left"
          : undefined;
  return new Paragraph({
    heading: options?.heading,
    alignment,
    children: runs.length > 0 ? runs : [new TextRun({ text: "" })],
    indent: options?.indentLeft ? { left: options.indentLeft } : undefined,
    spacing: {
      before: options?.spacingBefore,
      after: options?.spacingAfter ?? 140,
    },
  });
}

function codeParagraph(value: string, indentLeft = 0): Paragraph {
  const lines = value.replace(/\r\n?/g, "\n").split("\n");
  const runs: TextRun[] = [];
  lines.forEach((line, index) => {
    runs.push(
      new TextRun({
        text: line,
        font: "Consolas",
        color: "1E293B",
      }),
    );
    if (index < lines.length - 1) {
      runs.push(new TextRun({ text: "", break: 1, font: "Consolas" }));
    }
  });
  return new Paragraph({
    children: runs.length > 0 ? runs : [new TextRun({ text: "" })],
    indent: { left: indentLeft + 180 },
    spacing: { before: 120, after: 180 },
  });
}

function listItemToPlainText(itemBlocks: MarkdownBlock[]): string {
  return itemBlocks
    .map((block) => {
      switch (block.type) {
        case "paragraph":
        case "heading":
          return flattenInlineText(block.inlines).trim();
        case "code":
          return block.value.trim();
        case "blockquote":
          return listItemToPlainText(block.blocks);
        case "list":
          return block.items.map((item) => listItemToPlainText(item.blocks)).join(" ");
        case "table":
          return [
            block.header.map((cell) => flattenInlineText(cell.inlines).trim()).join(" | "),
            ...block.rows.map((row) => row.map((cell) => flattenInlineText(cell.inlines).trim()).join(" | ")),
          ].join(" ");
        case "thematicBreak":
          return "---";
        default:
          return "";
      }
    })
    .filter(Boolean)
    .join(" ")
    .trim();
}

function docxChildrenFromList(block: MarkdownListBlock, depth: number): Array<Paragraph | Table> {
  const children: Array<Paragraph | Table> = [];
  block.items.forEach((item, index) => {
    const marker = block.ordered ? `${block.start + index}. ` : "• ";
    const checkedPrefix = item.checked == null ? "" : item.checked ? "[x] " : "[ ] ";
    const itemText = listItemToPlainText(item.blocks);
    children.push(
      new Paragraph({
        children: [new TextRun(`${marker}${checkedPrefix}${itemText || " "}`)],
        indent: { left: depth * DOCX_LIST_INDENT_STEP },
        spacing: { after: 90 },
      }),
    );
    item.blocks.forEach((childBlock) => {
      if (childBlock.type === "list") {
        children.push(...docxChildrenFromList(childBlock, depth + 1));
      }
    });
  });
  return children;
}

function docxChildrenFromBlock(block: MarkdownBlock, depth = 0): Array<Paragraph | Table> {
  switch (block.type) {
    case "heading":
      return [
        paragraphFromInlines(block.inlines, {
          heading: headingLevelByDepth(block.depth),
          spacingBefore: block.depth <= 2 ? 220 : 160,
          spacingAfter: 120,
        }),
      ];
    case "paragraph":
      return [paragraphFromInlines(block.inlines)];
    case "code":
      return [codeParagraph(block.value, depth * DOCX_LIST_INDENT_STEP)];
    case "blockquote":
      return block.blocks.flatMap((child) => {
        if (child.type === "paragraph" || child.type === "heading") {
          return [
            paragraphFromInlines(child.inlines, {
              prefix: "│ ",
              indentLeft: 260 + depth * DOCX_LIST_INDENT_STEP,
              spacingAfter: 100,
            }),
          ];
        }
        if (child.type === "code") {
          return [codeParagraph(child.value, 260 + depth * DOCX_LIST_INDENT_STEP)];
        }
        return docxChildrenFromBlock(child, depth + 1);
      });
    case "list":
      return docxChildrenFromList(block, depth);
    case "table": {
      const maxColumns = Math.max(
        1,
        block.header.length,
        ...block.rows.map((row) => row.length),
        block.align.length,
      );
      const aligns: TableAlign[] = Array.from({ length: maxColumns }, (_, index) => block.align[index] ?? null);
      const normalizedRows = [block.header, ...block.rows].map((row) => {
        const next = [...row];
        while (next.length < maxColumns) {
          next.push({ inlines: [{ type: "text", text: "" }] });
        }
        return next;
      });
      const rows = normalizedRows.map((row, rowIndex) =>
        new TableRow({
          children: row.map((cell, columnIndex) =>
            new TableCell({
              width: { size: 100 / maxColumns, type: WidthType.PERCENTAGE },
              children: [
                paragraphFromInlines(cell.inlines, {
                  spacingAfter: 60,
                  align: aligns[columnIndex] ?? null,
                }),
              ],
              shading: rowIndex === 0
                ? {
                    fill: "F8FAFC",
                    color: "auto",
                  }
                : undefined,
            })
          ),
        })
      );
      return [
        new Table({
          rows,
          width: { size: 100, type: WidthType.PERCENTAGE },
          borders: {
            top: { style: BorderStyle.SINGLE, color: "CBD5E1", size: 1 },
            bottom: { style: BorderStyle.SINGLE, color: "CBD5E1", size: 1 },
            left: { style: BorderStyle.SINGLE, color: "CBD5E1", size: 1 },
            right: { style: BorderStyle.SINGLE, color: "CBD5E1", size: 1 },
            insideHorizontal: { style: BorderStyle.SINGLE, color: "E2E8F0", size: 1 },
            insideVertical: { style: BorderStyle.SINGLE, color: "E2E8F0", size: 1 },
          },
        }),
      ];
    }
    case "thematicBreak":
      return [new Paragraph({ children: [new TextRun("────────────────────")] })];
    default:
      return [];
  }
}

function createPdfState(doc: jsPDF): PdfCursorState {
  const pageHeight = doc.internal.pageSize.getHeight();
  const contentWidth = doc.internal.pageSize.getWidth() - PDF_MARGIN_X * 2;
  return {
    doc,
    cursorY: PDF_MARGIN_Y,
    pageHeight,
    contentWidth,
  };
}

function ensurePdfPageSpace(state: PdfCursorState, neededHeight: number): void {
  const bottomLimit = state.pageHeight - PDF_MARGIN_Y;
  if (state.cursorY + neededHeight <= bottomLimit) {
    return;
  }
  state.doc.addPage();
  state.cursorY = PDF_MARGIN_Y;
}

function writePdfWrappedText(
  state: PdfCursorState,
  text: string,
  options?: {
    fontSize?: number;
    lineHeight?: number;
    indentX?: number;
    color?: [number, number, number];
    spacingAfter?: number;
  },
): void {
  const fontSize = options?.fontSize ?? PDF_DEFAULT_FONT_SIZE;
  const lineHeight = options?.lineHeight ?? PDF_DEFAULT_LINE_HEIGHT;
  const indentX = options?.indentX ?? 0;
  const contentWidth = Math.max(1, state.contentWidth - indentX);
  const normalizedText = text.replace(/\r\n?/g, "\n");
  state.doc.setFontSize(fontSize);
  if (options?.color) {
    state.doc.setTextColor(options.color[0], options.color[1], options.color[2]);
  } else {
    state.doc.setTextColor(17, 24, 39);
  }
  const splitLines = state.doc.splitTextToSize(normalizedText || " ", contentWidth) as string[];
  splitLines.forEach((line) => {
    ensurePdfPageSpace(state, lineHeight);
    state.doc.text(line, PDF_MARGIN_X + indentX, state.cursorY);
    state.cursorY += lineHeight;
  });
  state.cursorY += options?.spacingAfter ?? PDF_PARAGRAPH_SPACING;
}

function writePdfCodeBlock(state: PdfCursorState, code: string): void {
  const codeLines = state.doc.splitTextToSize(code.replace(/\r\n?/g, "\n") || " ", state.contentWidth - 16) as string[];
  const lineHeight = 14;
  const blockHeight = codeLines.length * lineHeight + 14;
  ensurePdfPageSpace(state, blockHeight + PDF_PARAGRAPH_SPACING);
  state.doc.setFillColor(248, 250, 252);
  state.doc.rect(PDF_MARGIN_X, state.cursorY - 10, state.contentWidth, blockHeight, "F");
  state.doc.setTextColor(30, 41, 59);
  state.doc.setFontSize(10);
  codeLines.forEach((line, index) => {
    state.doc.text(line, PDF_MARGIN_X + 8, state.cursorY + index * lineHeight);
  });
  state.cursorY += blockHeight + PDF_PARAGRAPH_SPACING;
}

function writePdfList(state: PdfCursorState, block: MarkdownListBlock, depth = 0): void {
  block.items.forEach((item, index) => {
    const itemText = listItemToPlainText(item.blocks);
    const marker = block.ordered ? `${block.start + index}.` : "•";
    const checkedPrefix = item.checked == null ? "" : item.checked ? "[x] " : "[ ] ";
    writePdfWrappedText(state, `${marker} ${checkedPrefix}${itemText || " "}`, {
      indentX: depth * 18,
      spacingAfter: 4,
    });
    item.blocks.forEach((childBlock) => {
      if (childBlock.type === "list") {
        writePdfList(state, childBlock, depth + 1);
      }
    });
  });
  state.cursorY += 4;
}

function measurePdfTextWidth(state: PdfCursorState, text: string, fontSize: number): number {
  state.doc.setFontSize(fontSize);
  return state.doc.getTextWidth(text || " ");
}

function wrapPdfCellText(
  state: PdfCursorState,
  text: string,
  maxWidth: number,
  fontSize: number,
): string[] {
  state.doc.setFontSize(fontSize);
  const lines = state.doc.splitTextToSize((text || " ").replace(/\r\n?/g, "\n"), Math.max(1, maxWidth)) as string[];
  return lines.length > 0 ? lines : [" "];
}

function writePdfTable(state: PdfCursorState, block: Extract<MarkdownBlock, { type: "table" }>): void {
  const fontSize = 10;
  const lineHeight = 13;
  const cellPaddingX = 6;
  const cellPaddingY = 5;
  const borderColor: [number, number, number] = [203, 213, 225];
  const headerFill: [number, number, number] = [248, 250, 252];
  const headerTextColor: [number, number, number] = [15, 23, 42];
  const bodyTextColor: [number, number, number] = [30, 41, 59];

  const matrix = [
    block.header.map((cell) => flattenInlineText(cell.inlines).trim() || " "),
    ...block.rows.map((row) => row.map((cell) => flattenInlineText(cell.inlines).trim() || " ")),
  ];
  if (matrix.length === 0) {
    return;
  }

  const columnCount = Math.max(1, ...matrix.map((row) => row.length));
  const normalized = matrix.map((row) => {
    const next = [...row];
    while (next.length < columnCount) {
      next.push(" ");
    }
    return next.slice(0, columnCount);
  });
  const aligns: TableAlign[] = Array.from(
    { length: columnCount },
    (_, index) => block.align[index] ?? null,
  );

  // 按内容估算列宽，再按可用宽度等比缩放，保证表格边框与列对齐。
  const rawWidths = Array.from({ length: columnCount }, (_, columnIndex) => {
    let maxContentWidth = 28;
    normalized.forEach((row) => {
      const cellText = row[columnIndex] ?? " ";
      // 长文本允许换行，但至少按前 24 个字符估算基础列宽。
      const sample = cellText.length > 24 ? `${cellText.slice(0, 24)}…` : cellText;
      maxContentWidth = Math.max(maxContentWidth, measurePdfTextWidth(state, sample, fontSize));
    });
    return maxContentWidth + cellPaddingX * 2;
  });
  const rawTotal = rawWidths.reduce((sum, width) => sum + width, 0);
  const scale = rawTotal > state.contentWidth ? state.contentWidth / rawTotal : 1;
  const columnWidths = rawWidths.map((width) => Math.max(36, width * scale));
  // 缩放后可能仍有浮点误差，最后一列补齐到内容宽度，避免右边框错位。
  const widthSum = columnWidths.reduce((sum, width) => sum + width, 0);
  if (columnWidths.length > 0) {
    columnWidths[columnWidths.length - 1] += Math.max(0, state.contentWidth - widthSum);
  }

  const measureRowHeight = (row: string[]): number => {
    let maxLines = 1;
    row.forEach((cell, columnIndex) => {
      const lines = wrapPdfCellText(
        state,
        cell,
        Math.max(1, columnWidths[columnIndex] - cellPaddingX * 2),
        fontSize,
      );
      maxLines = Math.max(maxLines, lines.length);
    });
    return maxLines * lineHeight + cellPaddingY * 2;
  };

  const drawRow = (row: string[], isHeader: boolean, y: number, rowHeight: number): void => {
    let x = PDF_MARGIN_X;
    row.forEach((cell, columnIndex) => {
      const width = columnWidths[columnIndex];
      if (isHeader) {
        state.doc.setFillColor(headerFill[0], headerFill[1], headerFill[2]);
        state.doc.rect(x, y, width, rowHeight, "F");
      }
      state.doc.setDrawColor(borderColor[0], borderColor[1], borderColor[2]);
      state.doc.setLineWidth(0.6);
      state.doc.rect(x, y, width, rowHeight, "S");

      const lines = wrapPdfCellText(
        state,
        cell,
        Math.max(1, width - cellPaddingX * 2),
        fontSize,
      );
      const textColor = isHeader ? headerTextColor : bodyTextColor;
      const align = aligns[columnIndex] ?? "left";
      state.doc.setTextColor(textColor[0], textColor[1], textColor[2]);
      state.doc.setFontSize(fontSize);
      lines.forEach((line, lineIndex) => {
        // jsPDF text baseline 约在字高 80%，加上顶部内边距后视觉居中更稳。
        const textY = y + cellPaddingY + lineHeight * 0.8 + lineIndex * lineHeight;
        const textWidth = measurePdfTextWidth(state, line, fontSize);
        const contentWidth = Math.max(1, width - cellPaddingX * 2);
        let textX = x + cellPaddingX;
        if (align === "center") {
          textX = x + cellPaddingX + Math.max(0, (contentWidth - textWidth) / 2);
        } else if (align === "right") {
          textX = x + width - cellPaddingX - textWidth;
        }
        state.doc.text(line, textX, textY);
      });
      x += width;
    });
  };

  const headerRow = normalized[0] ?? Array.from({ length: columnCount }, () => " ");
  const bodyRows = normalized.slice(1);
  const headerHeight = measureRowHeight(headerRow);
  const pageBottom = state.pageHeight - PDF_MARGIN_Y;

  const drawHeaderRow = (): void => {
    ensurePdfPageSpace(state, headerHeight);
    drawRow(headerRow, true, state.cursorY, headerHeight);
    state.cursorY += headerHeight;
  };

  // 首页先画表头；后续分页时重复表头，方便阅读。
  drawHeaderRow();

  bodyRows.forEach((row) => {
    const rowHeight = measureRowHeight(row);
    // 当前页放不下时：换页并重复表头。
    if (state.cursorY + rowHeight > pageBottom) {
      state.doc.addPage();
      state.cursorY = PDF_MARGIN_Y;
      drawHeaderRow();
    }
    ensurePdfPageSpace(state, rowHeight);
    drawRow(row, false, state.cursorY, rowHeight);
    state.cursorY += rowHeight;
  });
  state.cursorY += 8;
}

export async function exportMarkdownAsDocxBytes(markdown: string): Promise<Uint8Array> {
  const blocks = parseMarkdownBlocks(markdown);
  const children = blocks.flatMap((block) => docxChildrenFromBlock(block));
  const doc = new Document({
    sections: [
      {
        children: children.length > 0 ? children : [new Paragraph("")],
      },
    ],
  });
  const blob = await Packer.toBlob(doc);
  return new Uint8Array(await blob.arrayBuffer());
}

export async function exportMarkdownAsPdfBytes(markdown: string): Promise<Uint8Array> {
  const doc = new jsPDF({
    orientation: "portrait",
    unit: "pt",
    format: "a4",
    compress: true,
  });
  const usingCjkFont = await ensurePdfTextFont(doc, markdown);
  const blocks = parseMarkdownBlocks(markdown);
  const state = createPdfState(doc);

  blocks.forEach((block) => {
    switch (block.type) {
      case "heading": {
        const headingText = flattenInlineText(block.inlines).trim() || " ";
        const fontSizeByDepth: Record<number, number> = { 1: 22, 2: 18, 3: 16, 4: 14, 5: 13, 6: 12 };
        writePdfWrappedText(state, headingText, {
          fontSize: fontSizeByDepth[block.depth] ?? 12,
          lineHeight: (fontSizeByDepth[block.depth] ?? 12) + 6,
          spacingAfter: 8,
          color: [15, 23, 42],
        });
        break;
      }
      case "paragraph":
        writePdfWrappedText(state, flattenInlineText(block.inlines), {
          fontSize: PDF_DEFAULT_FONT_SIZE,
          lineHeight: PDF_DEFAULT_LINE_HEIGHT,
        });
        break;
      case "code":
        if (usingCjkFont) {
          doc.setFontSize(10.5);
        } else {
          doc.setFont("courier", "normal");
        }
        writePdfCodeBlock(state, block.value);
        doc.setFontSize(PDF_DEFAULT_FONT_SIZE);
        if (!usingCjkFont) {
          doc.setFont("helvetica", "normal");
        }
        break;
      case "blockquote":
        block.blocks.forEach((child) => {
          if (child.type === "paragraph" || child.type === "heading") {
            writePdfWrappedText(state, `│ ${flattenInlineText(child.inlines)}`, {
              indentX: 14,
              color: [71, 85, 105],
              spacingAfter: 5,
            });
          } else if (child.type === "code") {
            writePdfCodeBlock(state, child.value);
          } else if (child.type === "list") {
            writePdfList(state, child, 1);
          }
        });
        state.cursorY += 4;
        break;
      case "list":
        writePdfList(state, block);
        break;
      case "table":
        writePdfTable(state, block);
        break;
      case "thematicBreak":
        ensurePdfPageSpace(state, 20);
        doc.setDrawColor(203, 213, 225);
        doc.line(PDF_MARGIN_X, state.cursorY, PDF_MARGIN_X + state.contentWidth, state.cursorY);
        state.cursorY += 14;
        break;
      default:
        break;
    }
  });

  return new Uint8Array(doc.output("arraybuffer"));
}
