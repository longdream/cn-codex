---
name: awesome-design-md
description: Use VoltAgent awesome-design-md style references to create and apply a project-level DESIGN.md with explicit brand tokens and implementation guardrails.
tags: ["design", "brand", "design-md", "ui", "frontend"]
---

# awesome-design-md

## Goal

Create or update a root-level `DESIGN.md` for the current project, then use that file as the design source of truth for implementation work.

This skill is optimized for "on-brand" requests where the user wants a recognizable style language instead of generic UI output.

## When to use this skill

Use this skill when at least one of these is true:

1. The user explicitly asks for `DESIGN.md` generation or update.
2. The user asks to follow a known brand style ("make it Stripe-like", "Linear style", "Notion style", etc.).
3. The user asks for consistent design tokens and rules before coding.
4. A redesign task needs visual consistency across multiple pages/components.

Do not use this skill as the only tool for implementation-heavy UI tasks. Pair with `taste-skill` after the design contract is defined.

## Input checklist

Before writing or changing `DESIGN.md`, collect:

1. Product type (SaaS marketing site, dashboard, portfolio, docs, etc.).
2. Target audience and tone (technical, enterprise, consumer, playful, formal).
3. Brand reference priority (exact brand, closest style, or mixed references).
4. Required constraints (a11y level, reduced motion, dark mode, localization, platform limits).

If these signals are ambiguous, ask one focused clarification question.

## Output contract

After using this skill, the result should include:

1. A one-line design read (what style was chosen and why).
2. A concrete token set (color, type, spacing, radius, shadow, motion).
3. A root `DESIGN.md` (or a precise diff if updating an existing one).
4. A short implementation note describing how coding tasks should consume these rules.

## Workflow

### Step 1 - Choose style source

Pick one source type:

- Exact brand reference from `references/` (preferred when available).
- Nearest in-repo reference when exact brand is missing.
- Custom synthesized style only when user intent is not tied to a known brand.

### Step 2 - Build `DESIGN.md` skeleton

Use this section order in `DESIGN.md`:

1. Design intent and audience.
2. Token system (colors, typography, spacing, radius, shadows).
3. Component rules (buttons, inputs, cards, navigation, feedback states).
4. Layout and responsive grid rules.
5. Motion and accessibility guardrails.
6. Do/Do-not examples.

Keep every rule explicit and testable. Avoid vague statements like "make it modern."

### Step 3 - Apply rules to implementation

When writing UI code after `DESIGN.md` is ready:

1. Use token names from `DESIGN.md` instead of hard-coded ad-hoc values.
2. Keep one visual language per page (single accent family, consistent radius system).
3. Enforce contrast checks on CTA text and form controls.
4. Respect reduced-motion settings for non-essential animation.

### Step 4 - Validate consistency

Run a final consistency pass:

- The same token names are reused across files.
- No conflicting color palettes are introduced in later sections.
- CTA semantics stay consistent ("Get started" is not mixed with "Start now" for the same intent).

## Collaboration rule with `taste-skill`

Use this boundary to avoid overlap:

- `awesome-design-md`: defines the brand contract and token policy.
- `taste-skill`: executes page-level composition and anti-slop visual decisions.

Priority order for conflicts:

1. Explicit user instruction.
2. Project root `DESIGN.md`.
3. `awesome-design-md` guidance.
4. `taste-skill` defaults.

## Safety and quality guardrails

1. Do not claim official affiliation with a brand unless provided by the user.
2. Do not copy trademark assets that are unavailable in the workspace.
3. Do not invent fake KPI numbers or pseudo-business metrics for presentation copy.
4. If source references are missing, state the gap and continue with the nearest explicit fallback.

## References

Skill-local references should live under:

- `codey/skills/awesome-design-md/references/`

Use `references/README.md` as the source map for what is bundled locally and how to refresh from upstream `VoltAgent/awesome-design-md`.
