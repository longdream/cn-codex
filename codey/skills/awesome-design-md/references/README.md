# awesome-design-md references

This directory stores local reference templates used by the `awesome-design-md` skill.

Upstream inspiration source:

- https://github.com/VoltAgent/awesome-design-md

## Purpose

Keep a local, versioned snapshot of the brand/style reference docs that are safe to use in this workspace.

This avoids runtime network dependency and keeps design output reproducible.

## Suggested file naming

Use lowercase kebab-case file names:

- `stripe.md`
- `linear.md`
- `notion.md`
- `vercel.md`

If a source has multiple variants, suffix by context:

- `stripe-marketing.md`
- `stripe-dashboard.md`

## Suggested file template

Each reference file should include:

1. Source link and retrieval date.
2. Scope note (what this style is good for).
3. Token hints (color, type, spacing, radius, shadows).
4. Component patterns (buttons, nav, cards, forms).
5. "Do" and "Avoid" checklist.

## Sync process from upstream

1. Review upstream changes in `VoltAgent/awesome-design-md`.
2. Copy only the sections needed for local usage.
3. Normalize wording to deterministic rules (avoid vague language).
4. Add/update the local reference markdown file.
5. Keep this README aligned with current bundled references.

## Compliance notes

1. Do not copy protected assets or proprietary files.
2. Keep references as style guidance, not as brand endorsement.
3. Preserve attribution and source links for traceability.
