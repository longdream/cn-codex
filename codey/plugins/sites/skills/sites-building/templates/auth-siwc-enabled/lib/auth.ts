import { drizzleAdapter } from "better-auth/adapters/drizzle";
import { betterAuth } from "better-auth";
import { nextCookies } from "better-auth/next-js";
import { genericOAuth } from "better-auth/plugins";
import {
  createAuthorizationCodeRequest,
  getOAuth2Tokens,
} from "@better-auth/core/oauth2";
import { getDb } from "@/db";
import * as schema from "@/db/schema";
import { authEnv, GENERIC_OAUTH_PROVIDER_ID } from "./auth-env";

export function getAuth() {
  return betterAuth({
    baseURL: authEnv.baseURL,
    secrets: authEnv.secrets,
    database: drizzleAdapter(getDb(), {
      provider: "sqlite",
      schema,
    }),
    plugins: [
      genericOAuth({
        config: [
          {
            providerId: GENERIC_OAUTH_PROVIDER_ID,
            clientId: authEnv.clientId,
            clientSecret: authEnv.clientSecret,
            redirectURI: authEnv.redirectURI,
            discoveryUrl: authEnv.discoveryUrl,
            authorizationUrl: authEnv.authorizationUrl,
            tokenUrl: authEnv.tokenUrl,
            userInfoUrl: authEnv.userInfoUrl,
            authentication: "basic",
            pkce: true,
            scopes: authEnv.scope.split(/\s+/).filter(Boolean),
            authorizationUrlParams: {
              ...(authEnv.audience ? { audience: authEnv.audience } : {}),
              ...(authEnv.deviceId ? { device_id: authEnv.deviceId } : {}),
            },
            tokenUrlParams: undefined,
            ...(authEnv.tokenUrl
              ? {
                  getToken: async ({ code, codeVerifier, redirectURI }) => {
                    const { body, headers } = createAuthorizationCodeRequest({
                      authentication: "basic",
                      code,
                      codeVerifier,
                      headers: authEnv.deviceId
                        ? {
                            "OAI-Device-Id": authEnv.deviceId,
                          }
                        : undefined,
                      redirectURI,
                      options: {
                        clientId: authEnv.clientId,
                        clientSecret: authEnv.clientSecret,
                        redirectURI: authEnv.redirectURI,
                      },
                      tokenEndpoint: authEnv.tokenUrl,
                    });

                    const response = await fetch(authEnv.tokenUrl, {
                      body,
                      headers,
                      method: "POST",
                    });
                    const responseBody = (await response.json()) as Record<
                      string,
                      unknown
                    >;

                    if (!response.ok) {
                      throw new Error(
                        `OAuth token exchange failed with ${response.status} ${response.statusText}`,
                      );
                    }

                    return getOAuth2Tokens(responseBody);
                  },
                }
              : {}),
          },
        ],
      }),
      nextCookies(),
    ],
  });
}
