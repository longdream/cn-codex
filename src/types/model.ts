export interface Model {
  id: string;
  name: string;
  provider: string;
  supportedReasoningEfforts?: string[];
  supportedServiceTiers?: string[];
}

export interface ModelListParams {
  cursor?: string;
}

export interface ModelListResponse {
  data: Model[];
  nextCursor?: string;
}
