export interface Account {
  id?: string;
  email?: string;
  name?: string;
  plan?: string;
}

export interface GetAccountResponse {
  account?: Account;
  requiresOpenaiAuth: boolean;
}

export interface LoginAccountParams {
  type: "apiKey" | "chatgpt" | "chatgptDeviceCode";
  apiKey?: string;
}

export interface LoginAccountResponse {
  loginUrl?: string;
  userCode?: string;
}

export interface CancelLoginAccountParams {}

export interface CancelLoginAccountResponse {}

export interface LogoutAccountResponse {}

export interface GetAccountRateLimitsResponse {
  limits?: RateLimit[];
}

export interface RateLimit {
  name: string;
  limit: number;
  remaining: number;
  resetAt?: number;
}
