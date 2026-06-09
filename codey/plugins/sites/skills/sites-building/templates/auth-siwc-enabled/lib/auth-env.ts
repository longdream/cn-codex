const DEV_DEFAULTS = {
  url: "http://localhost:3000",
  secrets: "1:dev-only-better-auth-secret-change-me-please-replace",
  clientId: "__FILL_ME__",
  callbackURL: "/",
  scope: "openid email profile",
} as const;

export const GENERIC_OAUTH_PROVIDER_ID =
  process.env.BETTER_AUTH_GENERIC_OAUTH_PROVIDER_ID?.trim() ||
  "sign-in-with-chatgpt";

function readEnv(name: string, fallback?: string) {
  const value = process.env[name];

  if (value) {
    return value;
  }

  if (process.env.NODE_ENV !== "production" && fallback !== undefined) {
    return fallback;
  }

  throw new Error(
    `Missing required environment variable ${name}. Add it to .env.local or your deployment environment.`,
  );
}

function readOptionalEnv(name: string) {
  return process.env[name]?.trim() || undefined;
}

function stripTrailingSlash(value: string) {
  return value.endsWith("/") ? value.slice(0, -1) : value;
}

function parseSecrets(value: string) {
  return value.split(",").map((entry) => {
    const [version, ...secretParts] = entry.trim().split(":");
    const secret = secretParts.join(":").trim();

    if (!version || !secret) {
      throw new Error(
        "BETTER_AUTH_SECRETS must use the format `<version>:<secret>` and can contain multiple comma-separated entries.",
      );
    }

    return {
      version: Number.parseInt(version, 10),
      value: secret,
    };
  });
}

const baseURL = stripTrailingSlash(readEnv("BETTER_AUTH_URL", DEV_DEFAULTS.url));
const clientId = readEnv(
  "BETTER_AUTH_GENERIC_OAUTH_CLIENT_ID",
  DEV_DEFAULTS.clientId,
);

export const authEnv = {
  baseURL,
  secrets: parseSecrets(
    readEnv("BETTER_AUTH_SECRETS", DEV_DEFAULTS.secrets),
  ),
  clientId,
  clientSecret: readOptionalEnv("BETTER_AUTH_GENERIC_OAUTH_CLIENT_SECRET"),
  redirectURI:
    readOptionalEnv("BETTER_AUTH_GENERIC_OAUTH_REDIRECT_URI") ||
    `${baseURL}/api/auth/callback/${GENERIC_OAUTH_PROVIDER_ID}`,
  postSignInCallbackURL: readEnv(
    "NEXT_PUBLIC_BETTER_AUTH_POST_SIGN_IN_CALLBACK_URL",
    DEV_DEFAULTS.callbackURL,
  ),
  discoveryUrl: readOptionalEnv("BETTER_AUTH_GENERIC_OAUTH_DISCOVERY_URL"),
  authorizationUrl: readOptionalEnv(
    "BETTER_AUTH_GENERIC_OAUTH_AUTHORIZATION_URL",
  ),
  tokenUrl: readOptionalEnv("BETTER_AUTH_GENERIC_OAUTH_TOKEN_URL"),
  userInfoUrl: readOptionalEnv("BETTER_AUTH_GENERIC_OAUTH_USER_INFO_URL"),
  audience: readOptionalEnv("BETTER_AUTH_GENERIC_OAUTH_AUDIENCE"),
  scope: readEnv("BETTER_AUTH_GENERIC_OAUTH_SCOPE", DEV_DEFAULTS.scope),
  deviceId: readOptionalEnv("BETTER_AUTH_GENERIC_OAUTH_DEVICE_ID"),
};

export const isOAuthConfigured =
  authEnv.clientId !== DEV_DEFAULTS.clientId &&
  (Boolean(authEnv.discoveryUrl) ||
    Boolean(
      authEnv.authorizationUrl &&
        authEnv.tokenUrl &&
        authEnv.userInfoUrl,
    ));
