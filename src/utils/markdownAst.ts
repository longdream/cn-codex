import type {
  BlockContent,
  DefinitionContent,
  Heading,
  List,
  ListItem,
  Paragraph,
  PhrasingContent,
  Root,
  RootContent,
  Table,
  TableCell,
  TableRow,
} from "mdast";
import remarkGfm from "remark-gfm";
import remarkParse from "remark-parse";
import { unified } from "unified";

export interface MarkdownTextInline {
  type: "text";
  text: string;
}

export interface MarkdownStrongInline {
  type: "strong";
  children: MarkdownInline[];
}

export interface MarkdownEmphasisInline {
  type: "emphasis";
  children: MarkdownInline[];
}

export interface MarkdownDeleteInline {
  type: "delete";
  children: MarkdownInline[];
}

export interface MarkdownInlineCodeInline {
  type: "inlineCode";
  text: string;
}

export interface MarkdownLinkInline {
  type: "link";
  url: string;
  title?: string;
  children: MarkdownInline[];
}

export interface MarkdownLineBreakInline {
  type: "lineBreak";
}

export type MarkdownInline =
  | MarkdownTextInline
  | MarkdownStrongInline
  | MarkdownEmphasisInline
  | MarkdownDeleteInline
  | MarkdownInlineCodeInline
  | MarkdownLinkInline
  | MarkdownLineBreakInline;

export interface MarkdownParagraphBlock {
  type: "paragraph";
  inlines: MarkdownInline[];
}

export interface MarkdownHeadingBlock {
  type: "heading";
  depth: 1 | 2 | 3 | 4 | 5 | 6;
  inlines: MarkdownInline[];
}

export interface MarkdownCodeBlock {
  type: "code";
  lang?: string;
  value: string;
}

export interface MarkdownListItemBlock {
  blocks: MarkdownBlock[];
  checked?: boolean | null;
}

export interface MarkdownListBlock {
  type: "list";
  ordered: boolean;
  start: number;
  items: MarkdownListItemBlock[];
}

export interface MarkdownTableCellBlock {
  inlines: MarkdownInline[];
}

export interface MarkdownTableBlock {
  type: "table";
  header: MarkdownTableCellBlock[];
  rows: MarkdownTableCellBlock[][];
  /** GFM 列对齐：left / center / right，缺省按 left 处理。 */
  align: Array<"left" | "center" | "right" | null>;
}

export interface MarkdownBlockQuoteBlock {
  type: "blockquote";
  blocks: MarkdownBlock[];
}

export interface MarkdownThematicBreakBlock {
  type: "thematicBreak";
}

export type MarkdownBlock =
  | MarkdownParagraphBlock
  | MarkdownHeadingBlock
  | MarkdownCodeBlock
  | MarkdownListBlock
  | MarkdownTableBlock
  | MarkdownBlockQuoteBlock
  | MarkdownThematicBreakBlock;

function textFromUnknown(value: unknown): string {
  return typeof value === "string" ? value : "";
}

function inlinesFromPhrasing(children: PhrasingContent[]): MarkdownInline[] {
  return children.flatMap((child) => inlineFromPhrasing(child));
}

function inlineFromPhrasing(node: PhrasingContent): MarkdownInline[] {
  switch (node.type) {
    case "text":
      return [{ type: "text", text: node.value }];
    case "strong":
      return [{ type: "strong", children: inlinesFromPhrasing(node.children) }];
    case "emphasis":
      return [{ type: "emphasis", children: inlinesFromPhrasing(node.children) }];
    case "delete":
      return [{ type: "delete", children: inlinesFromPhrasing(node.children) }];
    case "inlineCode":
      return [{ type: "inlineCode", text: node.value }];
    case "break":
      return [{ type: "lineBreak" }];
    case "link":
      return [
        {
          type: "link",
          url: textFromUnknown(node.url),
          title: textFromUnknown(node.title) || undefined,
          children: inlinesFromPhrasing(node.children),
        },
      ];
    case "image": {
      const altText = textFromUnknown(node.alt).trim();
      return [{ type: "text", text: altText ? `[图片: ${altText}]` : "[图片]" }];
    }
    case "linkReference":
    case "imageReference":
    case "footnoteReference":
      return [{ type: "text", text: flattenPhrasingToText(node) }];
    case "html":
      // markdown 内嵌 HTML 初版降级为可读文本，优先保证可编辑导出稳定性。
      return [{ type: "text", text: textFromUnknown(node.value) }];
    default:
      return [{ type: "text", text: flattenPhrasingToText(node) }];
  }
}

function rowToCells(row: TableRow | undefined): MarkdownTableCellBlock[] {
  if (!row) {
    return [];
  }
  return row.children.map((cell: TableCell) => ({
    inlines: inlinesFromPhrasing(cell.children as PhrasingContent[]),
  }));
}

function blockToInlineFallback(block: DefinitionContent | BlockContent): MarkdownInline[] {
  switch (block.type) {
    case "paragraph":
      return inlinesFromPhrasing(block.children);
    case "heading":
      return inlinesFromPhrasing(block.children);
    case "code":
      return [{ type: "inlineCode", text: block.value }];
    case "list":
      return [{ type: "text", text: block.children.map(listItemToPlainText).join(" ") }];
    case "thematicBreak":
      return [{ type: "text", text: "---" }];
    case "blockquote":
      return [{ type: "text", text: block.children.map(flattenBlockToText).join(" ") }];
    case "table":
      return [{ type: "text", text: block.children.map((row) => rowToCells(row).map((cell) => flattenInlineText(cell.inlines)).join(" | ")).join("\n") }];
    case "definition":
      return [{ type: "text", text: textFromUnknown(block.url) }];
    case "html":
      return [{ type: "text", text: textFromUnknown(block.value) }];
    default:
      return [{ type: "text", text: "" }];
  }
}

function listItemToPlainText(item: ListItem): string {
  return item.children.map(flattenBlockToText).join(" ").trim();
}

function blockFromNode(node: DefinitionContent | BlockContent): MarkdownBlock[] {
  switch (node.type) {
    case "paragraph":
      return [{ type: "paragraph", inlines: inlinesFromPhrasing((node as Paragraph).children) }];
    case "heading":
      return [
        {
          type: "heading",
          depth: Math.max(1, Math.min(6, (node as Heading).depth)) as 1 | 2 | 3 | 4 | 5 | 6,
          inlines: inlinesFromPhrasing((node as Heading).children),
        },
      ];
    case "code":
      return [
        {
          type: "code",
          lang: textFromUnknown(node.lang) || undefined,
          value: node.value,
        },
      ];
    case "blockquote":
      return [
        {
          type: "blockquote",
          blocks: node.children.flatMap((child) => blockFromNode(child)),
        },
      ];
    case "list": {
      const listNode = node as List;
      return [
        {
          type: "list",
          ordered: Boolean(listNode.ordered),
          start: Number.isFinite(listNode.start ?? NaN) ? Math.max(1, Number(listNode.start)) : 1,
          items: listNode.children.map((item) => ({
            checked: item.checked ?? undefined,
            blocks: item.children.flatMap((child) => blockFromNode(child)),
          })),
        },
      ];
    }
    case "table": {
      const tableNode = node as Table;
      const [headerRow, ...bodyRows] = tableNode.children;
      const align = (tableNode.align ?? []).map((value) => {
        if (value === "left" || value === "center" || value === "right") {
          return value;
        }
        return null;
      });
      return [
        {
          type: "table",
          header: rowToCells(headerRow),
          rows: bodyRows.map((row) => rowToCells(row)),
          align,
        },
      ];
    }
    case "thematicBreak":
      return [{ type: "thematicBreak" }];
    case "definition":
      return [];
    case "html": {
      const trimmed = textFromUnknown(node.value).trim();
      return trimmed ? [{ type: "paragraph", inlines: [{ type: "text", text: trimmed }] }] : [];
    }
    default:
      return [];
  }
}

export function parseMarkdownBlocks(markdown: string): MarkdownBlock[] {
  const processor = unified().use(remarkParse).use(remarkGfm);
  const tree = processor.parse(markdown) as Root;
  return tree.children
    .filter((child): child is DefinitionContent | BlockContent => isDefinitionOrBlockNode(child))
    .flatMap((child) => blockFromNode(child));
}

export function flattenInlineText(inlines: MarkdownInline[]): string {
  return inlines
    .map((inline) => {
      switch (inline.type) {
        case "text":
          return inline.text;
        case "inlineCode":
          return inline.text;
        case "lineBreak":
          return "\n";
        case "link":
          return inline.children.length > 0
            ? `${flattenInlineText(inline.children)} (${inline.url})`
            : inline.url;
        case "strong":
        case "emphasis":
        case "delete":
          return flattenInlineText(inline.children);
        default:
          return "";
      }
    })
    .join("");
}

export function flattenBlockToText(block: DefinitionContent | BlockContent): string {
  return blockToInlineFallback(block).map((inline) => flattenInlineText([inline])).join(" ").trim();
}

function flattenPhrasingToText(node: PhrasingContent): string {
  switch (node.type) {
    case "text":
      return node.value;
    case "inlineCode":
      return node.value;
    case "break":
      return "\n";
    case "link":
      return node.children.length > 0
        ? `${node.children.map((child) => flattenPhrasingToText(child)).join("")} (${node.url})`
        : node.url;
    case "strong":
    case "emphasis":
    case "delete":
      return node.children.map((child) => flattenPhrasingToText(child)).join("");
    case "image":
      return textFromUnknown(node.alt);
    case "html":
      return textFromUnknown(node.value);
    case "imageReference":
    case "linkReference":
    case "footnoteReference":
      return textFromUnknown(node.label);
    default:
      return "";
  }
}

function isDefinitionOrBlockNode(node: RootContent): boolean {
  switch (node.type) {
    case "paragraph":
    case "heading":
    case "code":
    case "blockquote":
    case "list":
    case "table":
    case "thematicBreak":
    case "definition":
    case "html":
      return true;
    default:
      return false;
  }
}
