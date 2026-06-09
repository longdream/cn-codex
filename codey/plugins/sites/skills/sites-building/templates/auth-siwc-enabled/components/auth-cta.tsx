"use client";

import { useState } from "react";
import { authClient } from "@/lib/auth-client";

type AuthCtaProps = {
  callbackURL: string;
  isConfigured: boolean;
  label?: string;
  providerId: string;
};

export function AuthCta({
  callbackURL,
  isConfigured,
  label = "Sign in with ChatGPT",
  providerId,
}: AuthCtaProps) {
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function handleSignIn() {
    if (!isConfigured || pending) {
      return;
    }

    setPending(true);
    setError(null);

    const { error } = await authClient.signIn.oauth2({
      providerId,
      callbackURL,
    });

    if (error) {
      setError(error.message ?? "OAuth sign-in could not be started.");
      setPending(false);
    }
  }

  return (
    <div className="rounded-3xl border border-zinc-200 bg-white p-6 dark:border-zinc-800 dark:bg-zinc-900">
      <h2 className="text-lg font-semibold">Better Auth</h2>
      <p className="mt-3 text-sm leading-7 text-zinc-600 dark:text-zinc-300">
        {isConfigured
          ? "Start the generic OAuth flow with the provider configured in your environment."
          : "OAuth is wired up, but the provider is still using placeholder env values. Fill in .env.local before testing sign-in."}
      </p>
      <button
        className="mt-6 inline-flex rounded-full bg-zinc-950 px-5 py-3 text-sm font-medium text-white transition hover:bg-zinc-800 disabled:cursor-not-allowed disabled:bg-zinc-300 dark:bg-zinc-50 dark:text-zinc-950 dark:hover:bg-zinc-200 dark:disabled:bg-zinc-700 dark:disabled:text-zinc-400"
        disabled={!isConfigured || pending}
        onClick={handleSignIn}
        type="button"
      >
        {pending ? "Redirecting..." : label}
      </button>
      {error ? (
        <p className="mt-3 text-sm text-red-600 dark:text-red-400">{error}</p>
      ) : null}
    </div>
  );
}
