import { headers } from "next/headers";
import { AuthCta } from "@/components/auth-cta";
import { getAuth } from "@/lib/auth";
import {
  authEnv,
  GENERIC_OAUTH_PROVIDER_ID,
  isOAuthConfigured,
} from "@/lib/auth-env";

export default async function Home() {
  const session = await getAuth().api.getSession({
    headers: await headers(),
  });

  return (
    <main className="min-h-screen bg-zinc-50 px-6 py-24 text-zinc-950 dark:bg-zinc-950 dark:text-zinc-50">
      <div className="mx-auto flex max-w-4xl flex-col gap-16">
        <section className="space-y-6">
          <p className="text-sm font-medium uppercase tracking-[0.2em] text-zinc-500 dark:text-zinc-400">
            Starter Project
          </p>
          <h1 className="max-w-2xl text-5xl font-semibold tracking-tight sm:text-6xl">
            Ship something real from a clean baseline.
          </h1>
          <p className="max-w-2xl text-lg leading-8 text-zinc-600 dark:text-zinc-300">
            Better Auth is mounted under <code>/api/auth</code> with a generic
            OAuth provider wired from environment variables and persisted in the
            local D1 database.
          </p>
        </section>

        <section className="grid gap-4 md:grid-cols-3">
          <article className="rounded-3xl border border-zinc-200 bg-white p-6 dark:border-zinc-800 dark:bg-zinc-900">
            <h2 className="text-lg font-semibold">Start with structure</h2>
            <p className="mt-3 text-sm leading-7 text-zinc-600 dark:text-zinc-300">
              Provider ID, client ID, redirect URI, and the post-sign-in return
              path are all configured through env vars.
            </p>
          </article>
          <article className="rounded-3xl border border-zinc-200 bg-white p-6 dark:border-zinc-800 dark:bg-zinc-900">
            <h2 className="text-lg font-semibold">Current session</h2>
            <p className="mt-3 text-sm leading-7 text-zinc-600 dark:text-zinc-300">
              {session
                ? `Signed in as ${session.user.email ?? session.user.name ?? "an authenticated user"}.`
                : "No active Better Auth session yet."}
            </p>
          </article>
          <article className="rounded-3xl border border-zinc-200 bg-white p-6 dark:border-zinc-800 dark:bg-zinc-900">
            <h2 className="text-lg font-semibold">OAuth callback</h2>
            <p className="mt-3 text-sm leading-7 text-zinc-600 dark:text-zinc-300">
              The provider redirects back to{" "}
              <code>{authEnv.redirectURI}</code>.
            </p>
          </article>
        </section>

        <section className="grid gap-4 md:grid-cols-2">
          <AuthCta
            callbackURL={authEnv.postSignInCallbackURL}
            isConfigured={isOAuthConfigured}
            providerId={GENERIC_OAUTH_PROVIDER_ID}
          />
          <article className="rounded-3xl border border-zinc-200 bg-white p-6 dark:border-zinc-800 dark:bg-zinc-900">
            <h2 className="text-lg font-semibold">Local development</h2>
            <p className="mt-3 text-sm leading-7 text-zinc-600 dark:text-zinc-300">
              Use <code>.env.local</code> for OAuth values and keep the D1
              binding in <code>.openai/hosting.json</code> set to{" "}
              <code>DB</code> so the auth tables resolve in local dev. Generate
              Drizzle SQL only after the final schema is ready.
            </p>
          </article>
        </section>
      </div>
    </main>
  );
}
