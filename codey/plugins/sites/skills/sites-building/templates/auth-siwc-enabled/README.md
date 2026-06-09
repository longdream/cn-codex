# auth-siwc-enabled

A full-stack starter running on
[vinext](https://github.com/cloudflare/vinext), with Better Auth generic OAuth
on top of Cloudflare D1 and Drizzle, intended for SIWC-enabled sites.

## Prerequisites

- Node.js `>=22.13.0`

## Quick Start

```bash
npm install
cp .env.example .env.local
npm run db:generate
npm run dev
npm run build
```

For local SIWC testing, run the local SIWC or identity sidecar separately, then
set `.env.local` to the client id, client secret, redirect URI, and provider
endpoints exposed by that sidecar. The app itself should still start with
`npm run dev`.

`.openai/hosting.json` is the single deployment resource contract. The local
Vite worker derives its simulated `DB` binding from that declaration, and the
build artifact packages generated Drizzle migrations for hosted D1 setup.

## Included Shape

- edit site code under `app/`
- `app/api/auth/[...all]/route.ts` mounts Better Auth at `/api/auth`
- `lib/auth.ts` and `lib/auth-env.ts` wire generic OAuth from env vars
- `.openai/hosting.json` declares the `DB` binding used by the auth tables
- `db/schema.ts` contains only the Better Auth Drizzle schema required for
  sign-in sessions. Product-specific tables should be added later when the app
  needs persistence.
- `drizzle/` starts with the same empty Drizzle journal shape as
  `vinext-starter`. Agents should run `npm run db:generate` to generate auth
  migrations for the concrete generated site.
- `vite.config.ts` simulates declared bindings for local development
- `drizzle.config.ts` supports local migration generation when needed

## Better Auth Environment

Provide these environment variables for local development and deployment:

- `BETTER_AUTH_URL`
- `BETTER_AUTH_SECRETS`
- `BETTER_AUTH_GENERIC_OAUTH_PROVIDER_ID`
- `BETTER_AUTH_GENERIC_OAUTH_CLIENT_ID`
- `BETTER_AUTH_GENERIC_OAUTH_CLIENT_SECRET`
- `BETTER_AUTH_GENERIC_OAUTH_REDIRECT_URI`
- `BETTER_AUTH_GENERIC_OAUTH_AUDIENCE`
- `BETTER_AUTH_GENERIC_OAUTH_SCOPE`
- `BETTER_AUTH_GENERIC_OAUTH_DEVICE_ID`
- `NEXT_PUBLIC_BETTER_AUTH_POST_SIGN_IN_CALLBACK_URL`

To complete a real provider setup, also configure either:

- `BETTER_AUTH_GENERIC_OAUTH_DISCOVERY_URL`

or:

- `BETTER_AUTH_GENERIC_OAUTH_AUTHORIZATION_URL`
- `BETTER_AUTH_GENERIC_OAUTH_TOKEN_URL`
- `BETTER_AUTH_GENERIC_OAUTH_USER_INFO_URL`

## Useful Commands

- `npm run dev`: start local development
- `npm run build`: verify the vinext build output
- `npm run db:generate`: generate Drizzle migrations after schema changes

## Learn More

- [vinext Documentation](https://github.com/cloudflare/vinext)
- [Drizzle D1 Guide](https://orm.drizzle.team/docs/get-started/d1-new)
