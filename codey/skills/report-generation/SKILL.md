---
name: report-generation
description: Render a research, documentation, or analysis report as a designed HTML document using the project template, instead of emitting a raw `.md` file. Use whenever the user asks for a report, analysis, deep-dive, investigation, postmortem, summary, write-up, briefing, or any structured long-form (>500 words, multiple sections) output that would normally be a Markdown file. The user reads these in the browser as a designed document. Triggers: "report on X", "analyze X", "research X", "deep-dive on X", "investigate X", "summarise X for me", "write up X", "document X", "post-mortem", or any similar request where the natural output would be a multi-section Markdown document. Skip for: short conversational answers, single-file code edits, one-paragraph notes. Source: TheoRata/Report-Skill (MIT).
---

# Report Generation Skill

Convert agent research, documentation, code analysis, and technical write-ups into a designed HTML report instead of raw Markdown. The user reads these in the browser; the page handles light/dark theme, table of contents, footnotes, code blocks, callouts, framed figures with click-to-zoom, and one-click export back to Markdown.

## When this triggers

Any time the natural output would be a `.md` file with **headings, multiple sections, and more than ~500 words.** Specifically:

- "Write me a report / deep-dive / summary / analysis on X"
- "Investigate X and document what you find"
- "Research X and give me your findings"
- "Postmortem the Y incident"
- "Document the Z system / architecture / decision"
- "Write up the conclusions from our discussion"
- "Compare X and Y in detail"

**Skip this skill for:** quick conversational answers, single-file code edits, status updates, one-line confirmations, anything under ~300 words. The user wants this for *real* reports, not for every response.

## The workflow

1. **Write the report as Markdown with YAML frontmatter** (see [Frontmatter](#frontmatter) and [Markdown syntax](#markdown-syntax) below). Save it to the project's `reports/` directory.
2. **Run the renderer:**
   ```bash
   node codey/skills/report-generation/render.mjs reports/your-report.md
   ```
   This produces `reports/<slug>-<date>.html` in the same directory.
3. **Tell the user where to open it.** Provide the absolute file path; they open it in their browser.

The cost: the agent writes Markdown only. Token spend is identical to writing a plain `.md` file. The HTML is generated mechanically by the renderer.

## Editing an existing report

When the user asks to update, edit, modify, or expand an existing report — **do not re-write it from scratch.** Edit the source Markdown and re-render. This preserves footnotes, tags, structure, and keeps the diff small.

**Case A — the source `.md` still exists:**

1. Read `reports/<slug>-<date>.md`.
2. Apply the user's requested edit by editing the Markdown directly.
3. Re-render:
   ```bash
   node codey/skills/report-generation/render.mjs reports/<slug>-<date>.md
   ```
4. Tell the user the report has been updated and point at the same HTML path.

**Case B — only the `.html` exists:**

1. Recover the source Markdown from the embedded `<script type="text/markdown">` block:
   ```bash
   node codey/skills/report-generation/extract.mjs reports/the-report.html
   ```
   This writes `reports/the-report.md` next to the HTML. Pass `--force` to overwrite.
2. Edit the recovered `.md`.
3. Re-render with `render.mjs`.

## Frontmatter

Every report starts with a YAML frontmatter block. **Required fields are bold.**

```yaml
---
title: On the failure modes of LLM-generated frontend code   # required
summary: A pattern catalogue from two hundred reviewed PRs.  # required
generated_by: CN-Codex AI Assistant                          # required
date: 2026-06-19                                             # required (YYYY-MM-DD)
status: draft                                                # required — draft | in-review | reviewed | final
tags: [llm, frontend, design-systems]                        # optional
sources: 14                                                  # optional
version: 1                                                   # optional
eyebrow: Research report · Frontend tooling                  # optional
---
```

## Markdown syntax

The renderer supports standard Markdown plus extensions:

### Headings
- `##` for top-level sections, `###` for subsections, `####` for small headings
- Auto-generates IDs and anchor links

### Section ledes
First paragraph after `##`, if entirely italic, renders as a section lede.

### Inline formatting
```markdown
**bold**, *italic*, `inline code`, [link](url),
==highlighted text==, footnote reference[^1]
```

### Tables
Pipe syntax with alignment via `:` in separator.

### Code blocks
Fenced with optional language and filename:
````markdown
```ts src/agent/runner.ts
import { Agent } from './agent';
```
````

### Callouts

**Quiet (typographic):**
```markdown
> [NOTE] A soft mention or aside.
> [INSIGHT] A non-obvious finding.
> [CAUTION] A soft warning.
> [ASIDE] A tangent.
```

**Boxed (tinted panel + icon):**
```markdown
> [INFO] An important fact.
> [WARNING] A must-know caution.
> [TIP] An actionable suggestion.
> [DANGER] High-severity warning.
```

### Footnotes
```markdown
The defect rate was lower than expected.[^1]

[^1]: Specifically, one defect per ~300 lines.
```

### Images and figures
A bare image on its own line becomes a framed figure with click-to-zoom:
```markdown
![Caption text.](/path/to/image.png)
```

## Recommended structure

1. **Eyebrow** (optional, via frontmatter)
2. **Title** (from frontmatter)
3. **Summary/lede** (from frontmatter)
4. **Body sections** (`##`) — each with a section lede in italic
5. **Recommendations** (when investigative)
6. **Open questions** (when unresolved)
7. **Footnotes** (auto-collected)

## Output convention

- **Path:** `reports/<slug>-<date>.html`
- **Self-contained:** references Google Fonts via CDN, no other external deps
- **Embedded Markdown:** original source in `<script type="text/markdown">` for lossless round-trip
- **Theme:** supports light/dark toggle

## Files in this skill

| File | Purpose |
|------|---------|
| `SKILL.md` | This file — skill definition |
| `render.mjs` | Markdown → HTML renderer (Node.js, zero deps) |
| `template.html` | HTML document shell with styles and scripts |
| `extract.mjs` | Pull source Markdown from rendered HTML |

## Usage example

```bash
# Create a report
node codey/skills/report-generation/render.mjs reports/cache-analysis.md

# Extract source from HTML
node codey/skills/report-generation/extract.mjs reports/cache-analysis.html

# Re-render after edits
node codey/skills/report-generation/render.mjs reports/cache-analysis.md reports/cache-analysis.html
```
