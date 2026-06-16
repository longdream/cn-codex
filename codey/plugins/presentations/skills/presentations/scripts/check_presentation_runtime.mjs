#!/usr/bin/env node

import path from "node:path";
import { createRequire } from "node:module";

import {
  ensureArtifactToolWorkspace,
  importArtifactTool,
  parseArgs,
} from "./artifact_tool_utils.mjs";

function usage() {
  return [
    "Usage:",
    "  node scripts/check_presentation_runtime.mjs [--workspace <dir>]",
    "",
    "Sets up a Presentations workspace, resolves artifact-tool presentation JSX",
    "from that workspace, imports the presentation runtime, and prints a compact",
    "runtime report.",
  ].join("\n");
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  if (args.help) {
    console.log(usage());
    return;
  }

  const workspaceDir = path.resolve(args.workspace || process.cwd());
  await ensureArtifactToolWorkspace(workspaceDir);

  const requireFromWorkspace = createRequire(path.join(workspaceDir, "package.json"));
  const jsxRuntime = requireFromWorkspace.resolve("@oai/artifact-tool/presentation-jsx/jsx-runtime");
  const presentationJsx = requireFromWorkspace.resolve("@oai/artifact-tool/presentation-jsx");
  const artifact = await importArtifactTool(workspaceDir);

  const report = {
    runtime: process.env.PRESENTATIONS_RUNTIME || "artifact-tool",
    granolaRepo: process.env.GRANOLA_REPO || null,
    workspace: workspaceDir,
    presentationJsx,
    jsxRuntime,
    hasFileBlobLoad: typeof artifact.FileBlob?.load === "function",
    hasPresentationCreate: typeof artifact.Presentation?.create === "function",
    hasImportPptx: typeof artifact.PresentationFile?.importPptx === "function",
    hasExportPptx: typeof artifact.PresentationFile?.exportPptx === "function",
  };

  console.log(JSON.stringify(report, null, 2));
}

main().catch((error) => {
  console.error(error.stack || error.message || String(error));
  console.error(usage());
  process.exit(1);
});
