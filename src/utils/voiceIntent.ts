/**
 * 语音模式 LLM 意图识别与输出生成。
 *
 * 设计约束（需求第 5 节）：
 * - 意图判定完全由 LLM 完成，禁止正则/关键词规则；
 * - 一次调用同时完成意图分类与内容生成（chat → reply；solve → summary + ack）；
 * - 所有面向用户的语音文本由 LLM 生成，代码中不出现写死的播报模板；
 * - JSON 解析失败重试一次，再失败退化为 solve（summary = ASR 原文，ack 为空）。
 */
import { useAppStore } from "../stores/appStore";
import { invoke } from "@tauri-apps/api/core";
import {
  resolveFortuneLlmConfig,
  type FortuneLlmResolvedConfig,
} from "./fortune";
import type { VoiceIntentResult, VoicePersonaSettings } from "../stores/voiceStore";

/** 个性预设 → 描述（拼进 system prompt，仅闲聊分支使用） */
const PERSONA_DESCRIPTIONS: Record<string, string> = {
  friendly: "友好随和，亲切自然，像老朋友一样交流",
  professional: "专业严谨，用词准确，条理清晰",
  humorous: "幽默风趣，适度玩梗，让对话轻松愉快",
  concise: "简洁直接，不绕弯子，直接给重点",
  custom: "",
};

const LANGUAGE_STYLE_DESCRIPTIONS: Record<string, string> = {
  casual: "口语化表达，短句为主",
  formal: "正式书面化的表达",
  playful: "活泼网络化的语气，可使用流行表达",
  brief: "尽量简短，一两句话回应",
};

export function buildVoiceIntentSystemPrompt(persona: VoicePersonaSettings): string {
  return `你是桌面编程助手 CN-Codex 的语音交互大脑。用户通过语音说出一段话，你需要完成两件事：
1. 判断用户意图（intent）：
   - "chat"：问候、寒暄、情绪表达、与项目无关的日常闲聊；
   - "solve"：涉及编程、代码、项目、文件、命令、技术问题，或任何需要助手执行任务的请求。
2. 根据意图生成对应输出：
   - intent 为 "chat" 时：输出 reply 字段，内容是你的闲聊回复。回复风格要求：${PERSONA_DESCRIPTIONS[persona.personaPreset] ?? PERSONA_DESCRIPTIONS.friendly}；语言风格：${LANGUAGE_STYLE_DESCRIPTIONS[persona.languageStyle] ?? LANGUAGE_STYLE_DESCRIPTIONS.casual}${persona.customPrompt ? `；用户附加的性格要求：${persona.customPrompt}` : ""}。
   - intent 为 "solve" 时：输出 summary 字段（把用户口语化的问题提炼成一句清晰、可执行的任务描述，将交给编程助手执行）和 ack 字段（用自然口语说一句准备开始处理该任务的话，让用户知道请求已被理解并即将执行；不要复述任务全文，不要使用固定模板，每次换一种自然的说法）。

注意：
- 只输出一个 JSON 对象，不要输出任何其他文字或代码块标记。
- JSON 格式：{"intent":"chat","reply":"..."} 或 {"intent":"solve","summary":"...","ack":"..."}
- 用户说的话可能包含语音识别错误，尽量按语义理解。`;
}

export function buildVoiceIntentUserMessage(
  text: string,
  chatHistory: Array<{ role: "user" | "assistant"; content: string }>,
): string {
  const historyLines = chatHistory
    .slice(-10)
    .map((m) => `${m.role === "user" ? "用户" : "你"}: ${m.content}`)
    .join("\n");
  if (historyLines) {
    return `近期闲聊记录（供参考，不代表当前意图）：
${historyLines}

本次语音输入：
${text}`;
  }
  return `本次语音输入：
${text}`;
}

function resolveLlmConfig(): FortuneLlmResolvedConfig {
  const store = useAppStore.getState();
  const provider = store.getActiveProvider();
  const activeModel = store.getActiveModel();
  const currentModel = store.currentModel;
  return resolveFortuneLlmConfig({
    provider,
    activeModel,
    currentModel,
    activeEndpointIndex: store.activeEndpointIndex,
  });
}

async function callLlm(prompt: string, system: string): Promise<string> {
  const resolved = resolveLlmConfig();
  return invoke<string>("fortune_llm_call", {
    baseUrl: resolved.baseUrl,
    apiKey: resolved.apiKey,
    model: resolved.modelName,
    wireApi: resolved.wireApi,
    prompt,
    systemPrompt: system,
  });
}

/** 从 LLM 返回文本中提取 JSON 对象（容忍 markdown 代码块包裹）。 */
export function extractVoiceIntentJson(raw: string): VoiceIntentResult | null {
  if (!raw) return null;
  let text = raw.trim();
  // 剥离 ```json ... ``` 包裹。
  const fenceMatch = text.match(/```(?:json)?\s*([\s\S]*?)```/i);
  if (fenceMatch) {
    text = fenceMatch[1].trim();
  }
  // 找第一个 { 到最后一个 }。
  const start = text.indexOf("{");
  const end = text.lastIndexOf("}");
  if (start === -1 || end === -1 || end <= start) return null;
  try {
    const obj = JSON.parse(text.slice(start, end + 1));
    const intent = obj?.intent;
    if (intent === "chat" && typeof obj.reply === "string" && obj.reply.trim()) {
      return { intent: "chat", reply: obj.reply.trim() };
    }
    if (intent === "solve" && typeof obj.summary === "string" && obj.summary.trim()) {
      return {
        intent: "solve",
        summary: obj.summary.trim(),
        ack: typeof obj.ack === "string" ? obj.ack.trim() : "",
      };
    }
    return null;
  } catch {
    return null;
  }
}

/**
 * 语音转写文本 → 意图识别 + 内容生成。
 * 失败重试一次；再失败返回退化 solve（summary=原文，ack 为空）。
 */
export async function classifyVoiceIntent(
  text: string,
  persona: VoicePersonaSettings,
  chatHistory: Array<{ role: "user" | "assistant"; content: string }>,
): Promise<VoiceIntentResult> {
  const system = buildVoiceIntentSystemPrompt(persona);
  const user = buildVoiceIntentUserMessage(text, chatHistory);
  for (let attempt = 0; attempt < 2; attempt++) {
    try {
      const raw = await callLlm(user, system);
      const parsed = extractVoiceIntentJson(raw);
      if (parsed) return parsed;
      console.warn("[voice] intent parse failed, attempt", attempt, raw?.slice(0, 200));
    } catch (err) {
      console.error("[voice] intent LLM call failed:", err);
    }
  }
  // 退化：按 solve 处理，summary 用原文。
  return { intent: "solve", summary: text, ack: "" };
}
