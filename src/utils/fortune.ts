import { invoke } from "@tauri-apps/api/core";
import { appStateGet, appStateSet } from "../api/app_state";
import { useAppStore } from "../stores/appStore";
import type { BaziProfile } from "../stores/settingsStore";

export interface FortuneResult {
  date: string;
  overall: string;
  direction: string;
  bestAction: string;
  environment: string;
  summary: string;
  qimenDetail: string;
  ziweiDetail?: string;
  advice: string;
}

function localDateStr(d: Date = new Date()): string {
  const y = d.getFullYear();
  const m = String(d.getMonth() + 1).padStart(2, "0");
  const day = String(d.getDate()).padStart(2, "0");
  return `${y}-${m}-${day}`;
}

function todayKey(baziProfile?: BaziProfile | null): string {
  const date = localDateStr();
  if (baziProfile) {
    const parts = [baziProfile.birthDate, baziProfile.birthTime, baziProfile.gender];
    if (baziProfile.occupation) parts.push(baziProfile.occupation);
    if (baziProfile.industry) parts.push(baziProfile.industry);
    return `fortune_${date}_${parts.join("_")}`;
  }
  return `fortune_${date}`;
}

function buildPrompt(baziProfile?: BaziProfile | null): string {
  const now = new Date();
  const dateStr = now.toLocaleDateString("zh-CN", {
    year: "numeric",
    month: "long",
    day: "numeric",
    weekday: "long",
  });
  const timeStr = now.toLocaleTimeString("zh-CN", {
    hour: "2-digit",
    minute: "2-digit",
  });

  const hasOccupation = baziProfile?.occupation?.trim();
  const hasIndustry = baziProfile?.industry?.trim();

  let prompt = `你是一位精通奇门遁甲的玄学大师。请根据以下时间信息进行奇门遁甲时盘推演。

当前时间：${dateStr} ${timeStr}`;

  if (hasOccupation || hasIndustry) {
    prompt += `\n`;
    if (hasOccupation) prompt += `\n用户职业：${baziProfile!.occupation!.trim()}`;
    if (hasIndustry) prompt += `\n用户行业：${baziProfile!.industry!.trim()}`;
    prompt += `\n请结合用户的职业和行业特点，在奇门遁甲分析时给出针对性的建议。`;
  }

  prompt += `

请你根据当前时间排出奇门遁甲时盘，分析以下维度：

1. **今日整体**：判断当前时局是「顺势」「阻滞」还是「风险高」
2. **有利方位**：分析哪个方位（如东南、正北、西南等）最为有利
3. **最佳行为**：判断当前最容易成功的行为类型，从「沟通」「行动」「交易」「等待」中选择
4. **做事环境**：判断当前环境是否有利于推进事务，给出「有利」「中性」或「不利」`;

  if (baziProfile) {
    const calendarType = baziProfile.lunarCalendar ? "农历" : "公历";
    const genderStr = baziProfile.gender === "male" ? "男" : "女";
    prompt += `

此外，用户提供了个人生辰八字信息，请同时进行紫微斗数流日运势分析：
- 姓名：${baziProfile.name}
- 出生日期（${calendarType}）：${baziProfile.birthDate}
- 出生时辰：${baziProfile.birthTime}
- 性别：${genderStr}`;
    if (hasOccupation) prompt += `\n- 职业：${baziProfile.occupation!.trim()}`;
    if (hasIndustry) prompt += `\n- 行业：${baziProfile.industry!.trim()}`;
    prompt += `

请在奇门遁甲分析之后，追加紫微斗数的流日运势分析。`;
  }

  prompt += `

请严格按照以下 JSON 格式返回结果，不要包含任何其他文字、代码块标记或 markdown 格式，只返回纯 JSON：

{
  "date": "${localDateStr(now)}",
  "overall": "顺势 或 阻滞 或 风险高",
  "direction": "有利方位，如 东南",
  "bestAction": "沟通 或 行动 或 交易 或 等待",
  "environment": "有利 或 中性 或 不利",
  "summary": "一句话概括今日运势（15字以内）",
  "qimenDetail": "奇门遁甲详细分析（包含值符、值使、九星、八门等盘面解读，使用 Markdown 格式，200-400字）"${baziProfile ? ',\n  "ziweiDetail": "紫微斗数流日运势详细分析（包含命宫流日走势等，使用 Markdown 格式，200-300字）"' : ""},
  "advice": "综合建议，包含今日宜忌和注意事项（Markdown 格式，100-200字）"
}`;

  return prompt;
}

function parseFortuneResponse(text: string): FortuneResult | null {
  try {
    const jsonMatch = text.match(/\{[\s\S]*\}/);
    if (!jsonMatch) return null;
    const parsed = JSON.parse(jsonMatch[0]);
    if (!parsed.overall || !parsed.direction || !parsed.bestAction || !parsed.environment) {
      return null;
    }
    return parsed as FortuneResult;
  } catch {
    return null;
  }
}

async function callLlmDirect(prompt: string): Promise<string> {
  const store = useAppStore.getState();
  const provider = store.getActiveProvider();
  const model = store.getActiveModel();

  console.log("[fortune] provider:", provider?.name, "baseUrl:", provider?.baseUrl, "wireApi:", provider?.wireApi);
  console.log("[fortune] model id:", model?.id, "model name:", model?.model);

  if (!provider) throw new Error("No active provider configured");
  if (!model) throw new Error("No active model configured");

  const modelName = model.model || model.id;

  let baseUrl = provider.baseUrl;
  let apiKey = provider.apiKey;

  if (provider.type === "local-pool" && model) {
    const providerModel = provider.models.find(
      (m) => m.id === model.model,
    );
    const enabled = providerModel?.endpoints?.filter((ep) => ep.enabled) ?? [];
    const idx = store.activeEndpointIndex ?? 0;
    if (enabled.length > 0) {
      const ep = enabled[Math.min(idx, enabled.length - 1)];
      baseUrl = ep.url;
      if (ep.apiKey) apiKey = ep.apiKey;
    }
  }

  const params = {
    baseUrl,
    apiKey,
    model: modelName,
    wireApi: provider.wireApi || "chat",
    prompt,
  };
  console.log("[fortune] invoking fortune_llm_call with baseUrl:", params.baseUrl, "model:", params.model, "wireApi:", params.wireApi);

  try {
    const result = await invoke<string>("fortune_llm_call", params);
    console.log("[fortune] LLM response length:", result?.length, "preview:", result?.slice(0, 200));
    return result;
  } catch (err) {
    console.error("[fortune] invoke fortune_llm_call failed:", err);
    throw err;
  }
}

export async function fetchDailyFortune(
  baziProfile?: BaziProfile | null,
  forceRefresh = false,
): Promise<FortuneResult> {
  const cacheKey = todayKey(baziProfile);
  if (!forceRefresh) {
    try {
      const cached = await appStateGet(cacheKey);
      if (cached) {
        const parsed = JSON.parse(cached) as FortuneResult;
        if (parsed.overall && parsed.direction) return parsed;
      }
    } catch {
      // cache miss
    }
  }

  const prompt = buildPrompt(baziProfile);
  console.log("[fortune] prompt length:", prompt.length);
  const responseText = await callLlmDirect(prompt);
  console.log("[fortune] raw response:", responseText?.slice(0, 300));
  const result = parseFortuneResponse(responseText);

  if (!result) {
    console.error("[fortune] Failed to parse fortune response. Full text:", responseText);
    throw new Error("Failed to parse fortune response from LLM");
  }

  console.log("[fortune] parsed result:", JSON.stringify(result).slice(0, 200));
  await appStateSet(cacheKey, JSON.stringify(result)).catch(() => {});
  return result;
}
