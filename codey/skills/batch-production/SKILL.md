---
name: batch-production
description: Generate multiple angles and shot types of a single product for ecommerce-ready image sets. Produces a coherent set (hero 3/4, front, side, back, detail macro, scale, packaging, in-hand) keeping product identity consistent across shots. Use when the user says "different angles", "product shots", "ecommerce image set", "shot list", "PDP gallery", "batch production", or needs a multi-view set of one product. Part of the Creative Production set. Source: DKeken/codex-skills-alternative (MIT).
---

# Batch Production — Product Shot Explorer

Produce a consistent multi-angle shot set of ONE product. Consistency across the set is the whole point — a PDP gallery where the product subtly changes shape, color, or finish between shots looks broken and erodes trust.

## When to use
- Building a product-detail-page (PDP) gallery or ecommerce image set.
- You need multiple clean angles of the same object on a controlled background.
- Catalog, marketplace listings, spec/feature callouts.
- Batch production of campaign assets from a single product.

## When NOT to use
- Lifestyle/in-use moments → `creative-scene`.
- Promo/offer merchandising → `creative-offer`.
- No look locked → `creative-explore` or `design-automation` first.

## Inputs (ask only what's missing)
- **Product**: description or — ideally — a reference image for identity lock.
- **Surface/background style** + locked direction if available.
- **Which shots** are needed (default: full PDP set).

## Workflow
1. **Define the shot list**: hero 3/4, straight-on front, side profile, back, detail macro, scale/in-hand, packaging, optional exploded or feature-callout.
2. **Lock a consistency anchor**: same product, same lighting setup, same surface, same palette across every shot. If a reference image exists, pass it to your image tool as a reference so identity holds.
3. **Generate each shot** with your image tool (`1024x1024` or `1792x1024`), reusing the anchor prompt DNA and only changing the camera angle/crop. Save to `./creative/shot/<angle>.png`.
4. **Review as a grid**: scan for identity drift (color shift, shape change, branding moved). Regenerate any shot that breaks the set.
5. **Output the gallery** in PDP order (hero first).

## Worked example
Product: a matte-black mechanical keyboard.
```
shot list: hero-34 | front | side | back | detail-macro(keycap) | in-hand | packaging
anchor: "matte grey seamless backdrop, soft top + fill light, 85mm, matte-black
keyboard with white legends, consistent across all shots"
per-shot delta: only the camera position/crop changes; lighting + product fixed
[grid: ./creative/shot/hero-34.png ... ./creative/shot/packaging.png]
review note: side shot drifted glossy → regenerated with "matte, no specular highlights"
```

## Quality bar
- Product identity (shape, color, material, branding placement) is identical across every shot.
- Lighting and surface are consistent — the set looks shot in one session.
- Each angle adds information (don't ship two near-identical views).
- Hero shot leads; detail/macro shots are genuinely sharp on the feature.

## Common pitfalls
- **Identity drift** — the #1 failure. Color/finish/branding wandering between shots. Lock with a reference image and regenerate outliers.
- **Inconsistent lighting** — different shadows/temperature per shot breaks the set.
- **Redundant angles** — front and 3/4 that show the same thing; vary purposefully.
- **Soft macros** — a detail shot that isn't sharp defeats its purpose.

## Batch scaling
For large catalogs (10+ products):
1. Define a master template (background, lighting, angles) once.
2. Swap only the product per iteration.
3. Use consistent naming: `product-name/angle.png`.
4. QA pass per batch: spot-check 3 random products for drift.

## Handoff
Winning set → `creative-polish` for finish (background cleanup, consistent grade, exact aspect/crop for the storefront). Reuse the `design-automation` stub for brand alignment.

## Tooling
Best with an image tool that accepts a reference image (for identity lock). Without one, the set will need extra regeneration passes. Without any image tool, deliver the shot list + anchor + per-shot prompts.

If Photoshop UXP MCP is available (`@bubblydoo/photoshop-mcp`), use it to:
- Batch-process backgrounds (remove/replace)
- Apply consistent color grading across the set
- Resize/crop to platform specs (Amazon, Shopify, etc.)
