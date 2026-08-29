import { invoke } from "@tauri-apps/api/core";
import { jsonrepair } from "jsonrepair";
import { appStateGet, appStateSet } from "../api/app_state";
import { useAppStore } from "../stores/appStore";
import type { BaziProfile } from "../stores/settingsStore";
import type { ModelEntry } from "../stores/appStore";
import type { ProviderConfig, ProviderModel } from "../types/provider";

export interface FortuneSummary {
  date: string;
  overall: string;
  direction: string;
  bestAction: string;
  environment: string;
  summary: string;
}

export interface FortuneDetail {
  qimenDetail: string;
  ziweiDetail?: string;
  advice: string;
}

export type FortuneResult = FortuneSummary & FortuneDetail;
export const FORTUNE_DETAIL_CACHE_TTL_MS = 60 * 60 * 1000;

export interface FortuneLlmResolvedConfig {
  baseUrl: string;
  apiKey: string;
  modelName: string;
  wireApi: string;
}

export interface ResolveFortuneLlmConfigInput {
  provider: ProviderConfig | null;
  activeModel: ModelEntry | null;
  currentModel: string | null;
  activeEndpointIndex: number | null;
}

interface FortuneDetailStreamStartResult {
  requestId: string;
}

interface StartFortuneDetailStreamOptions {
  requestId?: string;
}

interface FortuneDetailCachePayload {
  cachedAt: number;
  detail: FortuneDetail;
}

const REQUIRED_SUMMARY_FIELDS = [
  "overall",
  "direction",
  "bestAction",
  "environment",
  "summary",
];

const REQUIRED_DETAIL_FIELDS = [
  "qimenDetail",
  "advice",
] as const;

function localDateStr(d: Date = new Date()): string {
  const y = d.getFullYear();
  const m = String(d.getMonth() + 1).padStart(2, "0");
  const day = String(d.getDate()).padStart(2, "0");
  return `${y}-${m}-${day}`;
}

function summaryCacheKey(baziProfile?: BaziProfile | null): string {
  const date = localDateStr();
  if (baziProfile) {
    const parts = [baziProfile.birthDate, baziProfile.birthTime, baziProfile.gender];
    if (baziProfile.occupation) parts.push(baziProfile.occupation);
    if (baziProfile.industry) parts.push(baziProfile.industry);
    return `fortune_summary_${date}_${parts.join("_")}`;
  }
  return `fortune_summary_${date}`;
}

function simpleHash(input: string): string {
  let hash = 2166136261;
  for (let i = 0; i < input.length; i += 1) {
    hash ^= input.charCodeAt(i);
    hash = Math.imul(hash, 16777619);
  }
  return (hash >>> 0).toString(16);
}

function profileSignature(baziProfile?: BaziProfile | null): string {
  if (!baziProfile) {
    return "none";
  }
  return [
    baziProfile.name ?? "",
    baziProfile.birthDate ?? "",
    baziProfile.birthTime ?? "",
    baziProfile.gender ?? "",
    baziProfile.lunarCalendar ? "lunar" : "solar",
    baziProfile.occupation ?? "",
    baziProfile.industry ?? "",
  ].join("|");
}

function detailCacheKey(
  summary: FortuneSummary,
  baziProfile?: BaziProfile | null,
): string {
  const signature = [
    summary.date,
    summary.overall,
    summary.direction,
    summary.bestAction,
    summary.environment,
    summary.summary,
    profileSignature(baziProfile),
  ].join("|");
  return `fortune_detail_${summary.date}_${simpleHash(signature)}`;
}

function buildSummaryPrompt(
  baziProfile?: BaziProfile | null,
): string {
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

  let prompt = `你是一位精通奇门遁甲的玄学大师。请根据以下时间信息做“今日简版运势”推演。

当前时间：${dateStr} ${timeStr}`;

  if (hasOccupation || hasIndustry) {
    prompt += `\n`;
    if (hasOccupation) prompt += `\n用户职业：${baziProfile!.occupation!.trim()}`;
    if (hasIndustry) prompt += `\n用户行业：${baziProfile!.industry!.trim()}`;
    prompt += `\n请结合职业和行业特征给出更贴合的简版结论。`;
  }

  prompt += `

请只给简版结果，不要生成长文详解。请分析以下维度：
- 今日整体：顺势 / 阻滞 / 风险高（三选一）
- 有利方位：如 东南、正北、西南
- 最佳行为：沟通 / 行动 / 交易 / 等待（四选一）
- 做事环境：有利 / 中性 / 不利（三选一）
- 一句话摘要：15字以内`;

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

请严格返回纯 JSON 对象，不要返回代码块和解释：
- 仅返回一个 JSON 对象
- 不要输出推理过程
- 字符串值内部不要出现英文双引号

{
  "date": "${localDateStr(now)}",
  "overall": "顺势 或 阻滞 或 风险高",
  "direction": "有利方位，如 东南",
  "bestAction": "沟通 或 行动 或 交易 或 等待",
  "environment": "有利 或 中性 或 不利",
  "summary": "一句话概括今日运势（15字以内）"
}`;

  return prompt;
}

export function buildFortuneDetailPrompt(
  summary: FortuneSummary,
  baziProfile?: BaziProfile | null,
): string {
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

  let prompt = `你是一位精通奇门遁甲与紫微斗数的玄学大师。请基于已给出的简版结果，生成“详情长文”。

当前时间：${dateStr} ${timeStr}
简版结论：
- 今日整体：${summary.overall}
- 有利方位：${summary.direction}
- 最佳行为：${summary.bestAction}
- 做事环境：${summary.environment}
- 摘要：${summary.summary}`;

  if (baziProfile) {
    const calendarType = baziProfile.lunarCalendar ? "农历" : "公历";
    const genderStr = baziProfile.gender === "male" ? "男" : "女";
    prompt += `

用户画像：
- 姓名：${baziProfile.name}
- 出生日期（${calendarType}）：${baziProfile.birthDate}
- 出生时辰：${baziProfile.birthTime}
- 性别：${genderStr}`;
    if (hasOccupation) prompt += `\n- 职业：${baziProfile.occupation!.trim()}`;
    if (hasIndustry) prompt += `\n- 行业：${baziProfile.industry!.trim()}`;
  }

  prompt += `

请严格返回纯 JSON 对象（不要输出代码块/解释/推理过程）：
- 输出内容不要使用 Markdown 表格
- 字符串值内部不要出现英文双引号
- 若无紫微内容可省略 ziweiDetail 字段

{
  "qimenDetail": "奇门遁甲详解，180-320字，短段落",
  "ziweiDetail": "紫微斗数详解，120-260字，短段落（可选）",
  "advice": "综合建议与宜忌，80-180字"
}`;

  return prompt;
}

function normalizeFortuneRawText(text: string): string {
  const trimmed = text.replace(/^\uFEFF/, "").trim();
  const wholeFenced = trimmed.match(/^```(?:json)?\s*([\s\S]*?)\s*```$/i);
  if (wholeFenced?.[1]) {
    return wholeFenced[1].trim();
  }
  const fencedSegment = trimmed.match(/```(?:json)?\s*([\s\S]*?)\s*```/i);
  if (fencedSegment?.[1]) {
    return fencedSegment[1].trim();
  }
  return trimmed;
}

function extractJsonLikeSegment(text: string): string | null {
  const start = text.indexOf("{");
  if (start < 0) {
    return null;
  }
  const end = text.lastIndexOf("}");
  if (end < start) {
    return text.slice(start).trim();
  }
  return text.slice(start, end + 1).trim();
}

function toRecord(value: unknown): Record<string, unknown> | null {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    return null;
  }
  return value as Record<string, unknown>;
}

function parseJsonObjectFromRawText(text: string): Record<string, unknown> | null {
  try {
    const normalized = normalizeFortuneRawText(text);
    const jsonLike = extractJsonLikeSegment(normalized);
    if (!jsonLike) {
      return null;
    }
    const repaired = jsonrepair(jsonLike);
    const parsed = JSON.parse(repaired) as unknown;
    return toRecord(parsed);
  } catch {
    return null;
  }
}

function hasRequiredStringFields(value: Record<string, unknown>, fields: readonly string[]): boolean {
  return fields.every((field) => {
    const candidate = value[field];
    return typeof candidate === "string" && candidate.trim().length > 0;
  });
}

function normalizeFortuneSummary(value: Record<string, unknown>): FortuneSummary {
  return {
    date: typeof value.date === "string" && value.date.trim()
      ? value.date.trim()
      : localDateStr(),
    overall: String(value.overall ?? "").trim(),
    direction: String(value.direction ?? "").trim(),
    bestAction: String(value.bestAction ?? "").trim(),
    environment: String(value.environment ?? "").trim(),
    summary: String(value.summary ?? "").trim(),
  };
}

function normalizeFortuneDetail(value: Record<string, unknown>): FortuneDetail {
  return {
    qimenDetail: String(value.qimenDetail ?? "").trim(),
    ziweiDetail: typeof value.ziweiDetail === "string" && value.ziweiDetail.trim()
      ? value.ziweiDetail.trim()
      : undefined,
    advice: String(value.advice ?? "").trim(),
  };
}

function parseCachedFortuneSummary(raw: string): FortuneSummary | null {
  try {
    const parsed = toRecord(JSON.parse(raw));
    if (!parsed || !hasRequiredStringFields(parsed, REQUIRED_SUMMARY_FIELDS)) {
      return null;
    }
    return normalizeFortuneSummary(parsed);
  } catch {
    return null;
  }
}

function parseCachedFortuneDetail(raw: string, now = Date.now()): FortuneDetail | null {
  try {
    const payload = toRecord(JSON.parse(raw));
    if (!payload) {
      return null;
    }
    const cachedAt = Number(payload.cachedAt);
    if (!Number.isFinite(cachedAt) || cachedAt <= 0) {
      return null;
    }
    if (now - cachedAt > FORTUNE_DETAIL_CACHE_TTL_MS) {
      return null;
    }

    const detailObj = toRecord(payload.detail);
    if (!detailObj || !hasRequiredStringFields(detailObj, REQUIRED_DETAIL_FIELDS)) {
      return null;
    }
    return normalizeFortuneDetail(detailObj);
  } catch {
    return null;
  }
}

function parseFortuneSummaryResponse(text: string): FortuneSummary | null {
  const parsed = parseJsonObjectFromRawText(text);
  if (!parsed || !hasRequiredStringFields(parsed, REQUIRED_SUMMARY_FIELDS)) {
    return null;
  }
  return normalizeFortuneSummary(parsed);
}

export function parseFortuneDetailResponse(text: string): FortuneDetail | null {
  const parsed = parseJsonObjectFromRawText(text);
  if (!parsed || !hasRequiredStringFields(parsed, REQUIRED_DETAIL_FIELDS)) {
    return null;
  }
  return normalizeFortuneDetail(parsed);
}

export function parseFortuneSummaryResponseForTest(text: string): FortuneSummary | null {
  return parseFortuneSummaryResponse(text);
}

export function parseFortuneDetailResponseForTest(text: string): FortuneDetail | null {
  return parseFortuneDetailResponse(text);
}

export async function getCachedFortuneDetail(
  summary: FortuneSummary,
  baziProfile?: BaziProfile | null,
  now = Date.now(),
): Promise<FortuneDetail | null> {
  const cacheKey = detailCacheKey(summary, baziProfile);
  try {
    const cached = await appStateGet(cacheKey);
    if (!cached) {
      return null;
    }
    return parseCachedFortuneDetail(cached, now);
  } catch {
    return null;
  }
}

export async function setCachedFortuneDetail(
  summary: FortuneSummary,
  detail: FortuneDetail,
  baziProfile?: BaziProfile | null,
  cachedAt = Date.now(),
): Promise<void> {
  const cacheKey = detailCacheKey(summary, baziProfile);
  const payload: FortuneDetailCachePayload = {
    cachedAt,
    detail,
  };
  await appStateSet(cacheKey, JSON.stringify(payload));
}

function resolveProviderModel(
  provider: ProviderConfig,
  activeModel: ModelEntry | null,
  currentModel: string | null,
): ProviderModel | null {
  const preferredModelId = activeModel
    && (activeModel.provider === provider.id || activeModel.provider === provider.type)
    ? activeModel.model
    : null;
  if (preferredModelId) {
    const matched = provider.models.find((model) => model.id === preferredModelId);
    if (matched) {
      return matched;
    }
  }

  if (currentModel) {
    const matchedCurrent = provider.models.find((model) => model.id === currentModel);
    if (matchedCurrent) {
      return matchedCurrent;
    }
  }

  return provider.models[0] ?? null;
}

export function resolveFortuneLlmConfig(input: ResolveFortuneLlmConfigInput): FortuneLlmResolvedConfig {
  const {
    provider,
    activeModel,
    currentModel,
    activeEndpointIndex,
  } = input;

  if (!provider) {
    throw new Error("No active provider configured");
  }

  const selectedModel = resolveProviderModel(provider, activeModel, currentModel);
  if (!selectedModel) {
    throw new Error("No model configured for active provider");
  }

  if (provider.type === "local-pool") {
    const enabledEndpoints = selectedModel.endpoints?.filter((ep) => ep.enabled) ?? [];
    if (enabledEndpoints.length === 0) {
      throw new Error(`No enabled endpoint configured for local-pool model: ${selectedModel.id}`);
    }

    const endpointIndex = Math.max(0, Math.floor(activeEndpointIndex ?? 0));
    const endpoint = enabledEndpoints[Math.min(endpointIndex, enabledEndpoints.length - 1)];
    const endpointUrl = endpoint.url.trim();
    if (!endpointUrl) {
      throw new Error(`Endpoint URL is empty for local-pool model: ${selectedModel.id}`);
    }

    return {
      baseUrl: endpointUrl,
      apiKey: endpoint.apiKey ?? "",
      modelName: endpoint.model.trim() || selectedModel.id,
      wireApi: endpoint.wireApi ?? provider.wireApi ?? "chat",
    };
  }

  const providerUrl = provider.baseUrl.trim();
  if (!providerUrl) {
    throw new Error(`No base URL configured for provider: ${provider.id}`);
  }

  return {
    baseUrl: providerUrl,
    apiKey: provider.apiKey ?? "",
    modelName: selectedModel.id,
    wireApi: provider.wireApi || "chat",
  };
}

async function callLlmDirect(prompt: string): Promise<string> {
  const store = useAppStore.getState();
  const provider = store.getActiveProvider();
  const activeModel = store.getActiveModel();
  const currentModel = store.currentModel;

  console.log("[fortune] provider:", provider?.name, "baseUrl:", provider?.baseUrl, "wireApi:", provider?.wireApi);
  console.log("[fortune] active model id:", activeModel?.id, "model name:", activeModel?.model, "currentModel:", currentModel);

  const resolved = resolveFortuneLlmConfig({
    provider,
    activeModel,
    currentModel,
    activeEndpointIndex: store.activeEndpointIndex,
  });

  const params = {
    baseUrl: resolved.baseUrl,
    apiKey: resolved.apiKey,
    model: resolved.modelName,
    wireApi: resolved.wireApi,
    prompt,
    systemPrompt: null,
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

export async function fetchDailyFortuneSummary(
  baziProfile?: BaziProfile | null,
  forceRefresh = false,
): Promise<FortuneSummary> {
  const cacheKey = summaryCacheKey(baziProfile);
  if (!forceRefresh) {
    try {
      const cached = await appStateGet(cacheKey);
      if (cached) {
        const parsed = parseCachedFortuneSummary(cached);
        if (parsed) return parsed;
      }
    } catch {
      // cache miss
    }
  }

  const prompt = buildSummaryPrompt(baziProfile);
  console.log("[fortune] prompt length:", prompt.length);
  const responseText = await callLlmDirect(prompt);
  console.log("[fortune] raw response:", responseText?.slice(0, 300));
  const result = parseFortuneSummaryResponse(responseText);
  if (!result) {
    console.error("[fortune] Failed to parse fortune summary response. Full text:", responseText);
    throw new Error("Failed to parse fortune summary response from LLM");
  }

  console.log("[fortune] parsed summary:", JSON.stringify(result).slice(0, 200));
  await appStateSet(cacheKey, JSON.stringify(result)).catch(() => {});
  return result;
}

export async function startFortuneDetailStream(
  summary: FortuneSummary,
  baziProfile?: BaziProfile | null,
  options?: StartFortuneDetailStreamOptions,
): Promise<FortuneDetailStreamStartResult> {
  const store = useAppStore.getState();
  const provider = store.getActiveProvider();
  const activeModel = store.getActiveModel();
  const currentModel = store.currentModel;

  const resolved = resolveFortuneLlmConfig({
    provider,
    activeModel,
    currentModel,
    activeEndpointIndex: store.activeEndpointIndex,
  });

  const prompt = buildFortuneDetailPrompt(summary, baziProfile);
  return invoke<FortuneDetailStreamStartResult>("fortune_detail_stream_start", {
    baseUrl: resolved.baseUrl,
    apiKey: resolved.apiKey,
    model: resolved.modelName,
    wireApi: resolved.wireApi,
    prompt,
    requestId: options?.requestId,
  });
}
