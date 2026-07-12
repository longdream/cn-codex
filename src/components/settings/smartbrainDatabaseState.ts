import { invoke } from "@tauri-apps/api/core";

export type SmartbrainDbType = "postgresql" | "mysql" | "sqlite" | "sqlserver";

export interface SmartbrainDbPermissions {
  readSchema: boolean;
  readData: boolean;
  writeData: boolean;
}

export interface SmartbrainDbSource {
  id: string;
  name: string;
  dbType: SmartbrainDbType;
  connectionUri: string;
  password: string;
  enabled: boolean;
  host: string;
  port: number | null;
  databaseName: string;
  username: string;
  filePath: string;
  schema: string;
  queryParams: Record<string, string>;
  permissions: SmartbrainDbPermissions;
  updatedAt: number;
}

export interface SmartbrainDbSettings {
  defaultRowLimit: number;
  defaultTimeoutSec: number;
  requireReadonlyReminder: boolean;
  skipWhenNoPermission: boolean;
  denyDdl: boolean;
  denyDrop: boolean;
  denyDeleteWithoutWritePermission: boolean;
  rulesMarkdown: string;
}

interface SmartbrainDbParsePayload {
  host?: string | null;
  port?: number | null;
  databaseName?: string | null;
  username?: string | null;
  password?: string | null;
  filePath?: string | null;
  schema?: string | null;
  queryParams?: Record<string, string> | null;
}

const DB_SOURCES_KEY = "smartbrain.db.sources";
const DB_SETTINGS_KEY = "smartbrain.db.settings";

function nowSeconds(): number {
  return Math.floor(Date.now() / 1000);
}

function generateId(): string {
  if (typeof crypto !== "undefined" && typeof crypto.randomUUID === "function") {
    return crypto.randomUUID();
  }
  return `db-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
}

export function defaultSmartbrainDbRules(): string {
  return `# 数据库安全规则
- 优先使用只读账号连接数据库。
- 未勾选任何权限的数据库自动跳过，不参与 AI 查询和操作。
- 未开启写权限前，只允许读取库结构与数据。
- 禁止 DROP / TRUNCATE / ALTER / CREATE DATABASE / DROP DATABASE / DROP TABLE / DROP SCHEMA。
- 禁止删库、删表、清空表、修改高危权限、创建危险触发器。
- 即使开启写权限，也只允许经过用户明确授权的 INSERT / UPDATE / DELETE。
- 执行 SQL 前必须先检查该库的权限配置，越权请求应直接拒绝。`;
}

export function defaultSmartbrainDbSettings(): SmartbrainDbSettings {
  return {
    defaultRowLimit: 200,
    defaultTimeoutSec: 15,
    requireReadonlyReminder: true,
    skipWhenNoPermission: true,
    denyDdl: true,
    denyDrop: true,
    denyDeleteWithoutWritePermission: true,
    rulesMarkdown: defaultSmartbrainDbRules(),
  };
}

export function createEmptySmartbrainDbSource(): SmartbrainDbSource {
  return {
    id: generateId(),
    name: "",
    dbType: "postgresql",
    connectionUri: "",
    password: "",
    enabled: true,
    host: "",
    port: null,
    databaseName: "",
    username: "",
    filePath: "",
    schema: "",
    queryParams: {},
    permissions: {
      readSchema: true,
      readData: true,
      writeData: false,
    },
    updatedAt: nowSeconds(),
  };
}

export function sourceHasAnyPermission(source: SmartbrainDbSource): boolean {
  return source.permissions.readSchema || source.permissions.readData || source.permissions.writeData;
}

export function isSourceEffectivelyEnabled(
  source: SmartbrainDbSource,
  settings: SmartbrainDbSettings,
): boolean {
  if (!source.enabled) {
    return false;
  }
  if (settings.skipWhenNoPermission && !sourceHasAnyPermission(source)) {
    return false;
  }
  return true;
}

function inferDefaultPort(dbType: SmartbrainDbType): number | null {
  switch (dbType) {
    case "postgresql":
      return 5432;
    case "mysql":
      return 3306;
    case "sqlserver":
      return 1433;
    default:
      return null;
  }
}

function normalizeDbProtocol(dbType: SmartbrainDbType, uri: string): string {
  if (/^[a-zA-Z][a-zA-Z\d+\-.]*:/.test(uri)) {
    return uri;
  }
  switch (dbType) {
    case "postgresql":
      return `postgresql://${uri}`;
    case "mysql":
      return `mysql://${uri}`;
    case "sqlserver":
      return `sqlserver://${uri}`;
    default:
      return uri;
  }
}

function normalizeQueryParams(value: Record<string, string> | null | undefined): Record<string, string> {
  if (!value) {
    return {};
  }
  return Object.fromEntries(
    Object.entries(value)
      .map(([key, entryValue]) => [key.trim(), String(entryValue).trim()] as const)
      .filter(([key, entryValue]) => key.length > 0 && entryValue.length > 0),
  );
}

function parseSqlitePath(rawUri: string): { filePath: string; queryParams: Record<string, string> } {
  const trimmed = rawUri.trim();
  if (!trimmed) {
    return { filePath: "", queryParams: {} };
  }

  if (!trimmed.includes("://")) {
    return { filePath: trimmed, queryParams: {} };
  }

  const url = new URL(trimmed);
  const queryParams: Record<string, string> = {};
  url.searchParams.forEach((value, key) => {
    queryParams[key] = value;
  });
  return {
    filePath: decodeURIComponent(`${url.host}${url.pathname}`),
    queryParams,
  };
}

function decodeConnectionComponent(value: string): string {
  try {
    return decodeURIComponent(value);
  } catch {
    return value;
  }
}

function looksLikeKeyValueConnectionString(value: string): boolean {
  return value.includes("=") && value.includes(";") && !value.includes("://");
}

function normalizeConnectionKey(rawKey: string): string {
  return rawKey.trim().toLowerCase().replace(/[\s_-]+/g, "");
}

function parseKeyValueConnectionString(connectionUri: string): Record<string, string> {
  const result: Record<string, string> = {};
  for (const segment of connectionUri.split(";")) {
    const trimmed = segment.trim();
    if (!trimmed) {
      continue;
    }
    const separatorIndex = trimmed.indexOf("=");
    if (separatorIndex <= 0) {
      continue;
    }
    const key = normalizeConnectionKey(trimmed.slice(0, separatorIndex));
    const value = trimmed.slice(separatorIndex + 1).trim();
    if (!key || !value) {
      continue;
    }
    result[key] = value;
  }
  return result;
}

function pickConnectionValue(
  values: Record<string, string>,
  aliases: string[],
): { alias: string; value: string } | null {
  for (const alias of aliases) {
    const normalizedAlias = normalizeConnectionKey(alias);
    const value = values[normalizedAlias];
    if (value) {
      return { alias: normalizedAlias, value };
    }
  }
  return null;
}

function parseServerHostAndPort(
  dbType: SmartbrainDbType,
  rawValue: string,
): { host: string; port: number | null } {
  const trimmed = rawValue.trim();
  if (!trimmed) {
    return { host: "", port: null };
  }

  if (dbType === "sqlserver") {
    const sqlServerMatch = trimmed.match(/^([^,]+),(\d+)$/);
    if (sqlServerMatch) {
      return {
        host: sqlServerMatch[1].trim(),
        port: Number(sqlServerMatch[2]),
      };
    }
  }

  const colonMatch = trimmed.match(/^(.+?):(\d+)$/);
  if (colonMatch) {
    return {
      host: colonMatch[1].trim(),
      port: Number(colonMatch[2]),
    };
  }

  return {
    host: trimmed,
    port: null,
  };
}

function parseKeyValueStyleConnection(
  dbType: SmartbrainDbType,
  connectionUri: string,
): Partial<SmartbrainDbSource> | null {
  if (!looksLikeKeyValueConnectionString(connectionUri)) {
    return null;
  }

  const values = parseKeyValueConnectionString(connectionUri);
  if (Object.keys(values).length === 0) {
    return null;
  }

  const serverField = pickConnectionValue(values, [
    "server",
    "host",
    "hostname",
    "data source",
    "datasource",
    "address",
    "addr",
    "network address",
  ]);
  const portField = pickConnectionValue(values, ["port"]);
  const databaseField = pickConnectionValue(values, ["database", "initial catalog"]);
  const usernameField = pickConnectionValue(values, ["uid", "user id", "user", "username"]);
  const passwordField = pickConnectionValue(values, ["pwd", "password", "pass"]);
  const schemaField = pickConnectionValue(values, ["schema", "current schema", "currentschema"]);

  let host = "";
  let port: number | null = null;
  if (serverField) {
    const parsedServer = parseServerHostAndPort(dbType, serverField.value);
    host = parsedServer.host;
    port = parsedServer.port;
  }

  if (portField?.value) {
    const parsedPort = Number(portField.value);
    if (Number.isFinite(parsedPort) && parsedPort > 0) {
      port = parsedPort;
    }
  }

  if (dbType === "sqlite") {
    const sqliteDataSource = serverField?.value || databaseField?.value || "";
    return {
      filePath: sqliteDataSource,
      databaseName: sqliteDataSource.split(/[\\/]/).filter(Boolean).pop() ?? sqliteDataSource,
      host: "",
      port: null,
      username: usernameField?.value ?? "",
      password: passwordField?.value ?? "",
      schema: schemaField?.value ?? "",
      queryParams: normalizeQueryParams(
        Object.fromEntries(
          Object.entries(values).filter(([key]) =>
            ![
              serverField?.alias,
              databaseField?.alias,
              usernameField?.alias,
              schemaField?.alias,
              portField?.alias,
              "pwd",
              "password",
            ].includes(key),
          ),
        ),
      ),
    };
  }

  return {
    host,
    port: port ?? inferDefaultPort(dbType),
    databaseName: databaseField?.value ?? "",
    username: usernameField?.value ?? "",
    password: passwordField?.value ?? "",
    filePath: "",
    schema: schemaField?.value ?? "",
    queryParams: normalizeQueryParams(
      Object.fromEntries(
        Object.entries(values).filter(([key]) =>
          ![
            serverField?.alias,
            databaseField?.alias,
            usernameField?.alias,
            schemaField?.alias,
            portField?.alias,
            "pwd",
            "password",
          ].includes(key),
        ),
      ),
    ),
  };
}

function mergeParsedFields(
  base: Partial<SmartbrainDbSource>,
  override: Partial<SmartbrainDbSource>,
): Partial<SmartbrainDbSource> {
  return {
    ...base,
    ...override,
    host: override.host?.trim() || base.host || "",
    port: override.port ?? base.port ?? null,
    databaseName: override.databaseName?.trim() || base.databaseName || "",
    username: override.username?.trim() || base.username || "",
    password: override.password?.trim() || base.password || "",
    filePath: override.filePath?.trim() || base.filePath || "",
    schema: override.schema?.trim() || base.schema || "",
    queryParams: {
      ...(base.queryParams ?? {}),
      ...(override.queryParams ?? {}),
    },
  };
}

function normalizeParsedPayload(payload: SmartbrainDbParsePayload | null | undefined): Partial<SmartbrainDbSource> {
  if (!payload) {
    return {};
  }
  return {
    host: payload.host?.trim() ?? "",
    port: typeof payload.port === "number" && Number.isFinite(payload.port) ? payload.port : null,
    databaseName: payload.databaseName?.trim() ?? "",
    username: payload.username?.trim() ?? "",
    password: payload.password?.trim() ?? "",
    filePath: payload.filePath?.trim() ?? "",
    schema: payload.schema?.trim() ?? "",
    queryParams: normalizeQueryParams(payload.queryParams),
  };
}

function hasUsefulParsedFields(parsed: Partial<SmartbrainDbSource>, dbType: SmartbrainDbType): boolean {
  if (dbType === "sqlite") {
    return Boolean(parsed.filePath || parsed.databaseName);
  }
  return Boolean(parsed.host || parsed.databaseName || parsed.username);
}

function hasSufficientParsedFields(parsed: Partial<SmartbrainDbSource>, dbType: SmartbrainDbType): boolean {
  if (dbType === "sqlite") {
    return Boolean(parsed.filePath);
  }
  return Boolean(parsed.host && parsed.databaseName);
}

function shouldAttemptAiRefine(
  dbType: SmartbrainDbType,
  localParsed: Partial<SmartbrainDbSource>,
): boolean {
  if (dbType === "sqlite") {
    return false;
  }
  return !hasSufficientParsedFields(localParsed, dbType);
}

export function parseSmartbrainConnectionUriLocally(
  dbType: SmartbrainDbType,
  connectionUri: string,
): Partial<SmartbrainDbSource> {
  const trimmed = connectionUri.trim();
  if (!trimmed) {
    throw new Error("连接地址不能为空。");
  }

  if (dbType === "sqlite") {
    const parsed = parseSqlitePath(trimmed);
    return {
      filePath: parsed.filePath,
      databaseName: parsed.filePath.split(/[\\/]/).filter(Boolean).pop() ?? parsed.filePath,
      host: "",
      port: null,
      username: "",
      password: "",
      schema: "",
      queryParams: parsed.queryParams,
    };
  }

  const kvParsed = parseKeyValueStyleConnection(dbType, trimmed);
  if (kvParsed) {
    return kvParsed;
  }

  const normalizedUri = normalizeDbProtocol(dbType, trimmed);
  const url = new URL(normalizedUri);
  const queryParams: Record<string, string> = {};
  url.searchParams.forEach((value, key) => {
    queryParams[key] = value;
  });

  const databaseName = decodeURIComponent(url.pathname.replace(/^\/+/, ""));
  const schema = queryParams.schema ?? queryParams.currentSchema ?? "";

  return {
    host: url.hostname,
    port: url.port ? Number(url.port) : inferDefaultPort(dbType),
    databaseName,
    username: decodeConnectionComponent(url.username),
    password: decodeConnectionComponent(url.password),
    filePath: "",
    schema,
    queryParams,
  };
}

export async function parseSmartbrainConnectionUri(
  dbType: SmartbrainDbType,
  connectionUri: string,
): Promise<Partial<SmartbrainDbSource>> {
  let localParsed: Partial<SmartbrainDbSource> = {};
  let localError: unknown = null;

  try {
    localParsed = parseSmartbrainConnectionUriLocally(dbType, connectionUri);
    if (!shouldAttemptAiRefine(dbType, localParsed)) {
      return localParsed;
    }
  } catch (error) {
    localError = error;
  }

  try {
    const remoteParsed = await invoke<SmartbrainDbParsePayload>("smartbrain_parse_database_connection", {
      dbType,
      connectionUri,
    });
    return mergeParsedFields(localParsed, normalizeParsedPayload(remoteParsed));
  } catch {
    if (hasUsefulParsedFields(localParsed, dbType)) {
      return localParsed;
    }
    if (localError) {
      throw localError;
    }
    return localParsed;
  }
}

async function loadJsonState<T>(key: string, fallback: T): Promise<T> {
  try {
    const raw = await invoke<string | null>("app_state_get", { key });
    if (!raw) {
      return fallback;
    }
    return JSON.parse(raw) as T;
  } catch {
    return fallback;
  }
}

async function saveJsonState<T>(key: string, value: T): Promise<void> {
  await invoke("app_state_set", {
    key,
    value: JSON.stringify(value),
  });
}

export function mergeSmartbrainDbParsedFields(
  existing: Partial<SmartbrainDbSource>,
  parsed: Partial<SmartbrainDbSource>,
): Partial<SmartbrainDbSource> {
  return {
    host: existing.host?.trim() || parsed.host?.trim() || "",
    port:
      typeof existing.port === "number" && Number.isFinite(existing.port)
        ? existing.port
        : parsed.port ?? null,
    databaseName: existing.databaseName?.trim() || parsed.databaseName?.trim() || "",
    username: existing.username?.trim() || parsed.username?.trim() || "",
    password: existing.password?.trim() || parsed.password?.trim() || "",
    filePath: existing.filePath?.trim() || parsed.filePath?.trim() || "",
    schema: existing.schema?.trim() || parsed.schema?.trim() || "",
    queryParams:
      existing.queryParams && Object.keys(existing.queryParams).length > 0
        ? existing.queryParams
        : parsed.queryParams ?? {},
  };
}

function replaceKeyValueConnectionField(connectionUri: string, aliases: string[], nextValue: string): string {
  const segments = connectionUri.split(";");
  let replaced = false;
  const nextSegments = segments.map((segment) => {
    const trimmed = segment.trim();
    if (!trimmed) {
      return segment;
    }
    const separatorIndex = trimmed.indexOf("=");
    if (separatorIndex <= 0) {
      return segment;
    }
    const key = normalizeConnectionKey(trimmed.slice(0, separatorIndex));
    if (!aliases.map(normalizeConnectionKey).includes(key)) {
      return segment;
    }
    replaced = true;
    const leading = segment.match(/^\s*/)?.[0] ?? "";
    const originalKey = trimmed.slice(0, separatorIndex);
    return `${leading}${originalKey}=${nextValue}`;
  });
  if (!replaced) {
    const suffix = connectionUri.trim().endsWith(";") ? "" : ";";
    return `${connectionUri.trimEnd()}${suffix}Database=${nextValue};`;
  }
  return nextSegments.join(";");
}

export function applyDatabaseNameToConnectionUri(
  dbType: SmartbrainDbType,
  connectionUri: string,
  databaseName: string,
): string {
  const trimmedUri = connectionUri.trim();
  const trimmedName = databaseName.trim();
  if (!trimmedUri || !trimmedName || dbType === "sqlite") {
    return connectionUri;
  }

  if (looksLikeKeyValueConnectionString(trimmedUri)) {
    return replaceKeyValueConnectionField(trimmedUri, ["database", "initial catalog"], trimmedName);
  }

  try {
    const normalizedUri = normalizeDbProtocol(dbType, trimmedUri);
    const url = new URL(normalizedUri);
    url.pathname = `/${trimmedName}`;
    // Preserve original scheme style when user omitted protocol.
    if (!/^[a-zA-Z][a-zA-Z\d+\-.]*:/.test(trimmedUri)) {
      return `${url.host}${url.pathname}${url.search}${url.hash}`;
    }
    return url.toString();
  } catch {
    return connectionUri;
  }
}

export async function listSmartbrainDatabases(source: {
  dbType: SmartbrainDbType;
  host?: string;
  port?: number | null;
  username?: string;
  password?: string;
  connectionUri?: string;
  databaseName?: string;
  filePath?: string;
}): Promise<string[]> {
  const result = await invoke<{ databases?: string[] } | string[]>("smartbrain_list_databases", {
    dbType: source.dbType,
    host: source.host ?? "",
    port: source.port ?? null,
    username: source.username ?? "",
    password: source.password ?? "",
    connectionUri: source.connectionUri ?? "",
    databaseName: source.databaseName ?? "",
    filePath: source.filePath ?? "",
  });
  if (Array.isArray(result)) {
    return result.filter((item) => typeof item === "string" && item.trim().length > 0);
  }
  return (result.databases ?? []).filter((item) => typeof item === "string" && item.trim().length > 0);
}

export async function loadSmartbrainDbSources(): Promise<SmartbrainDbSource[]> {
  const loaded = await loadJsonState<SmartbrainDbSource[]>(DB_SOURCES_KEY, []);
  return loaded.map((item) => ({
    ...createEmptySmartbrainDbSource(),
    ...item,
    permissions: {
      ...createEmptySmartbrainDbSource().permissions,
      ...(item.permissions ?? {}),
    },
    queryParams: item.queryParams ?? {},
  }));
}

export async function saveSmartbrainDbSources(sources: SmartbrainDbSource[]): Promise<void> {
  await saveJsonState(DB_SOURCES_KEY, sources);
}

export async function loadSmartbrainDbSettings(): Promise<SmartbrainDbSettings> {
  const loaded = await loadJsonState<Partial<SmartbrainDbSettings>>(DB_SETTINGS_KEY, {});
  return {
    ...defaultSmartbrainDbSettings(),
    ...loaded,
  };
}

export async function saveSmartbrainDbSettings(settings: SmartbrainDbSettings): Promise<void> {
  await saveJsonState(DB_SETTINGS_KEY, settings);
}
