#!/usr/bin/env node

import { existsSync, readdirSync, readFileSync } from "node:fs";
import path from "node:path";
import { spawn } from "node:child_process";
import process from "node:process";

const [, , packageSpec, ...args] = process.argv;

if (!packageSpec) {
  console.error("Usage: node scripts/run-package-bin.mjs <package-name[:bin-name]> [...args]");
  process.exit(1);
}

const [packageName, explicitBinName] = packageSpec.split(":");

const projectRoot = process.cwd();
const pnpmRoot = path.join(projectRoot, "node_modules", ".pnpm");

function normalizePackageForPnpmDir(name) {
  return name.replace(/\//g, "+");
}

function findPackageDir(name) {
  const directPath = path.join(projectRoot, "node_modules", name);
  if (existsSync(path.join(directPath, "package.json"))) {
    return directPath;
  }

  if (!existsSync(pnpmRoot)) {
    return null;
  }

  const prefix = `${normalizePackageForPnpmDir(name)}@`;
  const candidates = readdirSync(pnpmRoot)
    .filter((entry) => entry.startsWith(prefix))
    .sort()
    .reverse();

  for (const candidate of candidates) {
    const candidatePath = path.join(pnpmRoot, candidate, "node_modules", ...name.split("/"));
    if (existsSync(path.join(candidatePath, "package.json"))) {
      return candidatePath;
    }
  }

  return null;
}

function resolveBinEntry(packageDir, name, binName) {
  const pkg = JSON.parse(readFileSync(path.join(packageDir, "package.json"), "utf8"));
  if (typeof pkg.bin === "string") {
    return path.resolve(packageDir, pkg.bin);
  }
  if (pkg.bin && typeof pkg.bin === "object") {
    const preferredNames = [binName, name].filter(Boolean);
    for (const preferredName of preferredNames) {
      if (typeof pkg.bin[preferredName] === "string") {
        return path.resolve(packageDir, pkg.bin[preferredName]);
      }
    }
    const first = Object.values(pkg.bin).find((value) => typeof value === "string");
    if (typeof first === "string") {
      return path.resolve(packageDir, first);
    }
  }
  return null;
}

const packageDir = findPackageDir(packageName);
if (!packageDir) {
  console.error(`Package not found: ${packageName}`);
  process.exit(1);
}

const binPath = resolveBinEntry(packageDir, packageName, explicitBinName);
if (!binPath) {
  console.error(`No executable entry found for package: ${packageName}`);
  process.exit(1);
}

const child = spawn(process.execPath, [binPath, ...args], {
  cwd: projectRoot,
  stdio: "inherit",
  env: process.env,
});

child.on("exit", (code, signal) => {
  if (signal) {
    process.kill(process.pid, signal);
    return;
  }
  process.exit(code ?? 1);
});

child.on("error", (error) => {
  console.error(`Failed to launch ${packageName}:`, error);
  process.exit(1);
});
