#!/usr/bin/env node

import fsSync from "node:fs";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const DEFAULT_GRANOLA_REPO = "/Users/esun/code/openai/lib/js/oai_js_granola";

function usage() {
  return [
    "Usage:",
    "  node scripts/run_with_local_granola.mjs <script.mjs> [script args...]",
    "",
    "Runs a Presentations helper through local Granola using:",
    "  PRESENTATIONS_RUNTIME=local-granola",
    "  GRANOLA_REPO=<local lib/js/oai_js_granola>",
    "",
    "Examples:",
    "  node scripts/run_with_local_granola.mjs inspect_template_deck.mjs --workspace /tmp/pres --pptx source.pptx",
    "  GRANOLA_REPO=/path/to/lib/js/oai_js_granola node scripts/run_with_local_granola.mjs build_artifact_deck.mjs ...",
  ].join("\n");
}

function scriptDir() {
  return path.dirname(fileURLToPath(import.meta.url));
}

function resolveGranolaRepo() {
  const candidates = [
    process.env.GRANOLA_REPO,
    path.resolve(process.cwd(), "lib/js/oai_js_granola"),
    DEFAULT_GRANOLA_REPO,
  ].filter(Boolean);

  for (const candidate of candidates) {
    const resolved = path.resolve(candidate);
    if (fsSync.existsSync(path.join(resolved, "package.json"))) {
      return resolved;
    }
  }

  throw new Error(
    [
      "Could not find a local Granola repo.",
      "Set GRANOLA_REPO=/absolute/path/to/lib/js/oai_js_granola.",
    ].join("\n"),
  );
}

function resolveTargetScript(value) {
  if (!value || value === "--help" || value === "-h") {
    console.log(usage());
    process.exit(value ? 0 : 1);
  }

  const cwdPath = path.resolve(value);
  if (fsSync.existsSync(cwdPath)) return cwdPath;

  const localScriptPath = path.join(scriptDir(), value);
  if (fsSync.existsSync(localScriptPath)) return localScriptPath;

  throw new Error(`Could not find target script: ${value}`);
}

function resolveTsxCommand(granolaRepo) {
  const localTsx = path.join(granolaRepo, "node_modules", ".bin", process.platform === "win32" ? "tsx.cmd" : "tsx");
  if (fsSync.existsSync(localTsx)) {
    return { command: localTsx, argsPrefix: [] };
  }
  return { command: "pnpm", argsPrefix: ["--dir", granolaRepo, "exec", "tsx"] };
}

const [scriptArg, ...scriptArgs] = process.argv.slice(2);
const granolaRepo = resolveGranolaRepo();
const targetScript = resolveTargetScript(scriptArg);
const { command, argsPrefix } = resolveTsxCommand(granolaRepo);

const result = spawnSync(command, [...argsPrefix, targetScript, ...scriptArgs], {
  cwd: process.cwd(),
  env: {
    ...process.env,
    PRESENTATIONS_RUNTIME: "local-granola",
    PRESENTATIONS_LOCAL_GRANOLA_TSX: "1",
    GRANOLA_REPO: granolaRepo,
  },
  stdio: "inherit",
});

if (result.error) {
  throw result.error;
}

process.exit(result.status ?? 1);
