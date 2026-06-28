/**
 * 供应商管理类型定义
 * 实例化模型：用户可以从预设模板创建多个供应商实例（同类型可创建多个），
 * 每次只激活一个实例。类似 cc-switch 的设计。
 */

/** 供应商分类 */
export type ProviderCategory = "global" | "china" | "local" | "other";
/** 非视觉模型的图片后备策略 */
export type VisionFallbackKind = "multimodal" | "local_ocr";

/** 资源池模型的单个后端端点地址 */
export interface PoolModelEndpoint {
  id: string;
  /** 实际请求地址，如 http://10.0.0.1:8080/v1 */
  url: string;
  /** 该端点实际调用的模型名（API model 字段） */
  model: string;
  /** 可选标签，如 "节点1" */
  label: string;
  /** 是否启用 */
  enabled: boolean;
  /** 该端点的 API Key */
  apiKey?: string;
  /** 该端点的 API 协议（chat/responses/anthropic/gemini），留空则用供应商默认 */
  wireApi?: string;
}

/** 供应商下的单个模型 */
export interface ProviderModel {
  id: string;
  label: string;
  supportsVision: boolean;
  /** 当当前模型不支持视觉时，后补类型（多模态模型 / 本地 OCR） */
  visionFallbackKind?: VisionFallbackKind;
  /** 当当前模型不支持视觉时，可选的后补多模态供应商实例 ID */
  visionFallbackProviderId?: string;
  /** 当当前模型不支持视觉时，可选的后补多模态模型 ID */
  visionFallbackModelId?: string;
  /** 上下文窗口大小（token），默认 128000 */
  contextLength: number;
  /** 单次回复最大输出 token 数，默认 65535 */
  maxOutputTokens: number;
  /** 仅 local-pool 供应商使用：该模型的后端端点列表，按顺序切换容灾 */
  endpoints?: PoolModelEndpoint[];
}

/** 预设模型模板（允许省略 maxOutputTokens，实例化时补默认值） */
export type ProviderPresetModel = Omit<ProviderModel, "maxOutputTokens"> & {
  maxOutputTokens?: number;
};

/**
 * 供应商预设模板
 * 仅作为"创建实例"的模板，不直接存储用户配置
 */
export interface ProviderPreset {
  /** 预设类型标识，如 "openai", "deepseek", "custom" */
  type: string;
  /** 显示名称模板 */
  name: string;
  /** 分类 */
  category: ProviderCategory;
  /** 默认 API 基础地址 */
  defaultBaseUrl: string;
  /** 默认传输协议格式 */
  defaultWireApi: string;
  /** 是否需要 OpenAI 风格鉴权 */
  requiresOpenAIAuth: boolean;
  /** 默认可用模型列表 */
  defaultModels: ProviderPresetModel[];
  /** 注册/获取 API Key 的链接（用于引导用户） */
  signupUrl?: string;
}

/**
 * 供应商实例配置
 * 每个实例是用户从预设创建的独立记录，拥有唯一 id
 */
export interface ProviderConfig {
  /** UUID，每个实例唯一 */
  id: string;
  /** 所基于的预设类型（如 "openai"），便于识别同类实例 */
  type: string;
  /** 用户自定义名称（如 "OpenAI 官方", "中转站 A"） */
  name: string;
  /** 分类 */
  category: ProviderCategory;
  /** API 基础地址 */
  baseUrl: string;
  /** API 密钥 */
  apiKey: string;
  /** 传输协议格式 */
  wireApi: string;
  /** 是否需要 OpenAI 风格鉴权 */
  requiresOpenAIAuth: boolean;
  /** 该实例可用的模型列表 */
  models: ProviderModel[];
  /** 是否为用户完全自定义（非预设派生） */
  isCustom: boolean;
  /** 创建时间戳（用于排序） */
  createdAt: number;
}

/** 附件文件（输入框增强用） */
export interface AttachedFile {
  /** 文件名 */
  name: string;
  /** MIME 类型 */
  type: string;
  /** Base64 DataURL */
  dataUrl: string;
  /** 文件大小（字节） */
  size: number;
  /** 项目内文件的绝对路径（来自文件树拖拽时设置） */
  sourcePath?: string;
}
