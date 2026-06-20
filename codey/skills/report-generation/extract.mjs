#!/usr/bin/env node
/*
 * extract.mjs — pull the embedded source Markdown out of a rendered HTML report.
 *
 * Every report rendered by render.mjs embeds its original Markdown source inside
 *   <script id="source-md" type="text/markdown"> ... </script>
 *
 * This script reads that block and writes it to a .md file, so the report can
 * be edited and re-rendered.
 *
 * Usage:
 *   node extract.mjs <input.html> [output.md]
 *
 * If [output.md] is omitted, writes to a sibling file with the same basename
 * but .md extension (e.g. report.html → report.md).
 *
 * Refuses to overwrite an existing .md unless --force is passed.
 */

import fs from 'node:fs';
import path from 'node:path';

const args = process.argv.slice(2);
const force = args.includes('--force');
const positional = args.filter(a => !a.startsWith('--'));

if (positional.length < 1) {
  console.error('Usage: node extract.mjs <input.html> [output.md] [--force]');
  process.exit(1);
}

const inputPath = path.resolve(positional[0]);
if (!fs.existsSync(inputPath)) {
  console.error(`error: input not found: ${inputPath}`);
  process.exit(1);
}

const html = fs.readFileSync(inputPath, 'utf8');
const m = html.match(/<script id="source-md"[^>]*>\s*([\s\S]*?)\s*<\/script>/);
if (!m) {
  console.error('error: no <script id="source-md"> block found in input HTML.');
  console.error('hint: this report may have been rendered without an embedded source, or hand-edited.');
  process.exit(2);
}

const md = m[1].trim() + '\n';

const outputPath = positional[1]
  ? path.resolve(positional[1])
  : path.join(path.dirname(inputPath), path.basename(inputPath, path.extname(inputPath)) + '.md');

if (fs.existsSync(outputPath) && !force) {
  console.error(`error: output already exists: ${outputPath}`);
  console.error('hint: pass --force to overwrite, or specify a different output path.');
  process.exit(3);
}

fs.writeFileSync(outputPath, md, 'utf8');
console.log(`✓ extracted: ${outputPath}`);
