---
name: design-automation
description: Pick a visual direction and automate design production for e-commerce posters, banners, and campaign creatives. Produces 3-6 named, distinct visual directions with rationale + sample prompts, then generates reference images per direction so the user can choose. Integrates with Photoshop UXP MCP for PSD manipulation. Use when the user says "explore visual direction", "poster design", "banner design", "e-commerce creative", "campaign assets", "design automation", or before batch production when no style is locked. Part of the Creative Production set. Source: DKeken/codex-skills-alternative (MIT) + @bubblydoo/photoshop-mcp.
---

# Design Automation — Visual Direction + E-commerce Creative Production

Converge on ONE visual direction before spending generation budget on variations. Picking a direction early is the single highest-leverage creative decision — everything downstream (scenes, shots, ads, polish) inherits it. Never jump straight to final assets without a locked direction.

## When to use
- Starting a new brand, campaign, or product visual from scratch.
- E-commerce poster/banner design automation.
- The user has a product but no agreed "look".
- Stakeholders disagree on aesthetic and you need concrete options to react to.
- Before `batch-production`, `creative-scene`, `creative-shot`, or `creative-ads-explorer` when no direction is locked.

## When NOT to use
- A direction/brand guide already exists → skip to `batch-production` or asset skills.
- You only need to finish one already-chosen image → `creative-polish`.
- The user wants team-wide mood alignment with territories → `creative-moodboard` (broader than this).

## Inputs to gather (ask only what's missing)
- **Product / brand** + what it does, in one line.
- **Audience** + the feeling it should evoke (trust? excitement? calm?).
- **Hard constraints**: locked colors, logo, fonts, explicit do-nots.
- **Medium**: web hero, paid ad, e-commerce banner, social, packaging, app store.

If the user can't answer "what feeling", propose 2-3 candidate feelings and let them pick — don't stall.

## Workflow
1. **Diverge on directions.** Propose **3-6 named directions**. Each must differ in *organizing principle*, not just palette. For each: name, one-line vibe, palette, typography feel, photographic/illustration style, when it wins, when it fails.
2. **Write a reusable prompt stub** per direction, structured `scene/backdrop → subject → details → constraints`. This stub is the DNA that downstream skills reuse for consistency.
3. **Generate ONE reference image per direction** with your image-generation tool (any text-to-image backend: DALL·E, Imagen, FLUX, SD, Midjourney). Keep it small/fast (`1024x1024`). Save to `./creative/explore/<direction-name>.png`.
4. **Present side by side** with the rationale table. Make a recommendation tied to the audience + medium.
5. **Lock one** (or fuse two). Hand the locked direction + prompt stub to the next skill.

## Worked example
Request: "We sell a $180 ceramic pour-over kettle. Need a look for the site hero."
```
## Direction A — "Quiet Luxury"
vibe: restrained, gallery-like, lets the object breathe
palette: bone white / warm grey / single matte-black accent
type: high-contrast serif display + grotesk body
style: soft north-window light, shallow DOF, seamless backdrop
wins when: price is the message (premium, trust-first)
fails when: audience is young/playful — reads cold
stub: "seamless bone backdrop -> kettle 3/4 center -> soft window light, matte black accent, 85mm f2 -> minimal, no props, no text"
[ref: ./creative/explore/quiet-luxury.png]

## Direction B — "Warm Ritual"
vibe: hands, steam, morning — the feeling of using it
palette: amber / cream / oxblood   type: humanist serif
style: golden-hour 35mm, lifestyle, real texture
wins when: selling the experience, social-first
fails when: needs to look clinical/spec-driven
stub: "wood counter, morning light -> hands pouring with kettle, steam -> 35mm, warm grade -> candid, real"
[ref: ./creative/explore/warm-ritual.png]
```
Recommendation: A for the hero (price-led, trust), keep B's stub for social. → lock A, pass stub to `batch-production`.

## Quality bar (don't ship until)
- Directions are genuinely distinct — a stranger could tell them apart blind.
- Each has an explicit "wins when / fails when" so the choice is reasoned, not vibes.
- Exactly one reference image per direction at this stage (no premature iteration).
- The locked stub is concrete enough that another skill can reuse it verbatim.

## Common pitfalls
- **Six shades of the same idea** — if they share palette + style, they're one direction. Force structural difference.
- **Over-generating** — one image per direction. Iterate AFTER selection, not before.
- **Inventing brand constraints** the user never stated — mark every assumption explicitly.
- **Skipping the stub** — without it, downstream skills drift and the set looks incoherent.

## E-commerce Poster/Banner Design Rules

When producing campaign creatives for e-commerce:

### Layout hierarchy
1. **Hero product** — 40-60% of visual space
2. **Price/discount** — bold, high contrast, top-right or center-bottom
3. **CTA** — single action, clear button or text
4. **Brand mark** — small, corner placement

### Typography for promotions
- Price: condensed bold, 2-3x body size
- Discount badge: contrasting color, rotated or badge shape
- Product name: medium weight, readable at thumbnail size
- Fine print: small, muted, bottom

### Safe zones
- Keep critical content within 85% inner frame (platform crops vary)
- Test at 1:1, 4:5, 16:9 for multi-platform delivery

---

## Photoshop UXP MCP Integration

When `@bubblydoo/photoshop-mcp` is configured, use it for production-grade PSD manipulation:

### MCP Server Configuration
```json
{
  "mcpServers": {
    "photoshop": {
      "url": "http://localhost:3020/mcp"
    }
  }
}
```

### Available Tool: `execute`
Executes JavaScript in Photoshop's UXP context. Code is bundled with esbuild; `@bubblydoo/uxp-toolkit` is available as an import.

### Workflow with Photoshop MCP

**Template-based batch production:**
1. Open the PSD template via MCP `execute`
2. Replace text layers (product name, price, CTA) via script
3. Replace smart object (product image)
4. Export to PNG/JPG at required dimensions
5. Repeat for each variant/product

**Example — Replace text and export:**
```javascript
import { app } from "@bubblydoo/uxp-toolkit";

const doc = app.activeDocument;
// Find and update text layer
const priceLayer = doc.layers.find(l => l.name === "price-text");
priceLayer.textItem.contents = "¥299";

// Find and update product name
const nameLayer = doc.layers.find(l => l.name === "product-name");
nameLayer.textItem.contents = "Premium Wireless Headphones";

// Export
await doc.saveAs.png("./output/banner-headphones.png", { quality: 95 });

export default { success: true, output: "./output/banner-headphones.png" };
```

**Example — Batch color variants:**
```javascript
import { app } from "@bubblydoo/uxp-toolkit";

const colors = ["#FF6B35", "#2EC4B6", "#E71D36"];
const doc = app.activeDocument;
const bgLayer = doc.layers.find(l => l.name === "background");

for (const color of colors) {
  bgLayer.fillColor = color;
  const name = color.replace("#", "");
  await doc.saveAs.png(`./output/variant-${name}.png`);
}

export default { success: true, variants: colors.length };
```

### Prerequisites
- Adobe Photoshop with UXP support and Developer Mode enabled
- `@bubblydoo/photoshop-mcp` server running (`pnpm dlx @bubblydoo/photoshop-mcp`)
- MCP server configured in CN-Codex settings (Integration panel)

## Handoff
Locked direction + prompt stub → `batch-production` (PDP set), `creative-scene` (lifestyle), `creative-ads-explorer` (ad batch), or `creative-offer` (promo).

## Tooling
Requires a text-to-image tool for direction exploration. If Photoshop MCP is available, use it for production-grade template manipulation and batch export. Without any image tool, deliver the named directions + prompt stubs and ask the user to run them, then resume at selection.
