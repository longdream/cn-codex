import { invoke } from "@tauri-apps/api/core";

export type SmartbrainSshAuthMethod = "password" | "privateKey";

export type SmartbrainSshValidationError =
  | "hostRequired"
  | "usernameRequired"
  | "passwordRequired"
  | "privateKeyRequired"
  | "portInvalid";

export interface SmartbrainSshSource {
  id: string;
  name: string;
  enabled: boolean;
  host: string;
  port: number | null;
  username: string;
  authMethod: SmartbrainSshAuthMethod;
  password: string;
  privateKey: string;
  privateKeyPath: string;
  passphrase: string;
  allowExec: boolean;
  updatedAt: number;
}

export const SSH_SOURCES_KEY = "smartbrain.ssh.sources";
export const DEFAULT_SSH_PORT = 22;

function nowSeconds(): number {
  return Math.floor(Date.now() / 1000);
}

function generateId(): string {
  if (typeof crypto !== "undefined" && typeof crypto.randomUUID === "function") {
    return crypto.randomUUID();
  }
  return `ssh-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
}

function asString(value: unknown): string {
  return typeof value === "string" ? value : "";
}

function asBoolean(value: unknown, fallback: boolean): boolean {
  return typeof value === "boolean" ? value : fallback;
}

function asPort(value: unknown): number | null {
  if (value == null || value === "") {
    return DEFAULT_SSH_PORT;
  }
  const parsed = typeof value === "number" ? value : Number(value);
  if (!Number.isInteger(parsed)) {
    return DEFAULT_SSH_PORT;
  }
  return parsed;
}

function asAuthMethod(value: unknown): SmartbrainSshAuthMethod {
  return value === "privateKey" ? "privateKey" : "password";
}

export function createEmptySmartbrainSshSource(): SmartbrainSshSource {
  return {
    id: generateId(),
    name: "",
    enabled: true,
    host: "",
    port: DEFAULT_SSH_PORT,
    username: "",
    authMethod: "password",
    password: "",
    privateKey: "",
    privateKeyPath: "",
    passphrase: "",
    allowExec: false,
    updatedAt: nowSeconds(),
  };
}

export function fallbackSshName(username: string, host: string): string {
  const user = username.trim();
  const server = host.trim();
  if (user && server) {
    return `${user}@${server}`;
  }
  return server || user || "";
}

export function normalizeSmartbrainSshSource(
  raw: Partial<SmartbrainSshSource> | Record<string, unknown> | null | undefined,
): SmartbrainSshSource {
  const empty = createEmptySmartbrainSshSource();
  const source = (raw ?? {}) as Record<string, unknown>;
  const host = asString(source.host).trim();
  const username = asString(source.username).trim();
  const name = asString(source.name).trim() || fallbackSshName(username, host);
  const id = asString(source.id).trim() || empty.id;
  return {
    id,
    name,
    enabled: asBoolean(source.enabled, true),
    host,
    port: asPort(source.port),
    username,
    authMethod: asAuthMethod(source.authMethod),
    password: asString(source.password),
    privateKey: asString(source.privateKey),
    privateKeyPath: asString(source.privateKeyPath).trim(),
    passphrase: asString(source.passphrase),
    allowExec: asBoolean(source.allowExec, false),
    updatedAt:
      typeof source.updatedAt === "number" && Number.isFinite(source.updatedAt)
        ? source.updatedAt
        : nowSeconds(),
  };
}

export function validateSmartbrainSshSource(
  source: Partial<SmartbrainSshSource>,
): SmartbrainSshValidationError | null {
  const host = (source.host ?? "").trim();
  const username = (source.username ?? "").trim();
  const authMethod = asAuthMethod(source.authMethod);
  const port = source.port;

  if (!host) {
    return "hostRequired";
  }
  if (!username) {
    return "usernameRequired";
  }
  if (port != null && (!Number.isInteger(port) || port < 1 || port > 65535)) {
    return "portInvalid";
  }
  if (authMethod === "password" && !(source.password ?? "").trim()) {
    return "passwordRequired";
  }
  if (
    authMethod === "privateKey" &&
    !(source.privateKey ?? "").trim() &&
    !(source.privateKeyPath ?? "").trim()
  ) {
    return "privateKeyRequired";
  }
  return null;
}

export function effectiveSshPort(port: number | null | undefined): number {
  if (port == null || !Number.isInteger(port) || port < 1 || port > 65535) {
    return DEFAULT_SSH_PORT;
  }
  return port;
}

export function formatSshTarget(source: Partial<SmartbrainSshSource>): string {
  const username = (source.username ?? "").trim() || "user";
  const host = (source.host ?? "").trim() || "host";
  return `${username}@${host}:${effectiveSshPort(source.port)}`;
}

export async function loadSmartbrainSshSources(): Promise<SmartbrainSshSource[]> {
  try {
    const raw = await invoke<string | null>("app_state_get", { key: SSH_SOURCES_KEY });
    if (!raw) {
      return [];
    }
    const parsed = JSON.parse(raw) as unknown;
    if (!Array.isArray(parsed)) {
      return [];
    }
    return parsed.map((item) => normalizeSmartbrainSshSource(item as Partial<SmartbrainSshSource>));
  } catch {
    return [];
  }
}

export async function saveSmartbrainSshSources(sources: SmartbrainSshSource[]): Promise<void> {
  await invoke("app_state_set", {
    key: SSH_SOURCES_KEY,
    value: JSON.stringify(sources),
  });
}

export async function testSmartbrainSshConnection(source: {
  host: string;
  port?: number | null;
  username: string;
  authMethod: SmartbrainSshAuthMethod;
  password?: string;
  privateKey?: string;
  privateKeyPath?: string;
  passphrase?: string;
  timeoutSec?: number;
}): Promise<{
  ok: boolean;
  message: string;
  host?: string;
  port?: number;
  username?: string;
  timeoutSec?: number;
}> {
  const result = await invoke<{
    ok?: boolean;
    message?: string;
    host?: string;
    port?: number;
    username?: string;
    timeoutSec?: number;
  }>("smartbrain_test_ssh_connection", {
    host: source.host,
    port: source.port ?? DEFAULT_SSH_PORT,
    username: source.username,
    authMethod: source.authMethod,
    password: source.password ?? "",
    privateKey: source.privateKey ?? "",
    privateKeyPath: source.privateKeyPath ?? "",
    passphrase: source.passphrase ?? "",
    timeoutSec: source.timeoutSec ?? 10,
  });
  return {
    ok: result.ok === true,
    message: result.message ?? (result.ok ? "Connection OK" : "Connection failed"),
    host: result.host,
    port: result.port,
    username: result.username,
    timeoutSec: result.timeoutSec,
  };
}
