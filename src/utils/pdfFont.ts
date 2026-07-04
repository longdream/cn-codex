import { readFile } from "@tauri-apps/plugin-fs";
import type { jsPDF } from "jspdf";

const CJK_FONT_NAME = "CnCodexCjk";
const CJK_FONT_FILE_NAME = "cn-codex-cjk.ttf";
const WINDOWS_CJK_FONT_CANDIDATES = [
  "C:/Windows/Fonts/simhei.ttf",
  "C:/Windows/Fonts/simfang.ttf",
  "C:/Windows/Fonts/simkai.ttf",
  "C:/Windows/Fonts/simsun.ttc",
  "C:/Windows/Fonts/msyh.ttc",
  "C:/Windows/Fonts/msyhbd.ttc",
  "C:/Windows/Fonts/msyhui.ttf",
];

let cachedFontBinaryString: string | null = null;
let loadingPromise: Promise<string> | null = null;

function containsCjk(text: string): boolean {
  return /[\u3400-\u9fff\uF900-\uFAFF]/.test(text);
}

function uint8ArrayToBinaryString(bytes: Uint8Array): string {
  const chunkSize = 0x8000;
  let result = "";
  for (let index = 0; index < bytes.length; index += chunkSize) {
    const chunk = bytes.subarray(index, index + chunkSize);
    result += String.fromCharCode(...chunk);
  }
  return result;
}

async function loadWindowsCjkFontBinaryString(): Promise<string> {
  const errors: string[] = [];
  for (const candidatePath of WINDOWS_CJK_FONT_CANDIDATES) {
    try {
      const bytes = await readFile(candidatePath);
      if (!bytes || bytes.length === 0) {
        errors.push(`${candidatePath}: empty file`);
        continue;
      }
      return uint8ArrayToBinaryString(bytes);
    } catch (err) {
      errors.push(`${candidatePath}: ${String(err)}`);
    }
  }
  throw new Error(
    `无法读取可用中文字体，请确认系统字体可访问。尝试路径：${errors.join(" | ")}`,
  );
}

async function getCjkFontBinaryString(): Promise<string> {
  if (cachedFontBinaryString) {
    return cachedFontBinaryString;
  }
  if (!loadingPromise) {
    loadingPromise = loadWindowsCjkFontBinaryString();
  }
  cachedFontBinaryString = await loadingPromise;
  return cachedFontBinaryString;
}

function hasRegisteredFont(doc: jsPDF, fontName: string): boolean {
  const fonts = doc.getFontList();
  return Object.prototype.hasOwnProperty.call(fonts, fontName);
}

/**
 * 为 PDF 文本导出准备可显示中文的字体。
 * 返回 true 代表当前文档已切换到 CJK 字体。
 */
export async function ensurePdfTextFont(doc: jsPDF, markdown: string): Promise<boolean> {
  if (!containsCjk(markdown)) {
    doc.setFont("helvetica", "normal");
    return false;
  }

  const binaryString = await getCjkFontBinaryString();
  if (!hasRegisteredFont(doc, CJK_FONT_NAME)) {
    doc.addFileToVFS(CJK_FONT_FILE_NAME, binaryString);
    doc.addFont(CJK_FONT_FILE_NAME, CJK_FONT_NAME, "normal");
  }
  doc.setFont(CJK_FONT_NAME, "normal");
  return true;
}
