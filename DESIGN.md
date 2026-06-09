# CN-Codex Design System

Dark-first AI coding assistant desktop application. Emerald accent on void-black canvas,
developer-tool typography with CJK optimization.

## Visual Theme

- **Mood**: Focused, technical, confident. IDE-native dark surface.
- **Density**: Medium-high. Compact but breathable.
- **Philosophy**: Quiet dark canvas with a single emerald accent. No visual noise.

## Color Palette

### Brand & Accent
| Token | Hex | Role |
|-------|-----|------|
| `--accent` | `#22c55e` | Primary CTA, active states, selection |
| `--accent-strong` | `#4ade80` | Hover emphasis, active labels |
| `--accent-soft` | `rgba(34,197,94,0.12)` | Subtle background tint |
| `--accent-border` | `rgba(34,197,94,0.24)` | Focus ring, active card border |

### Surface
| Token | Hex | Role |
|-------|-----|------|
| `--app-bg` | `#1a1a1a` | Page floor / root background |
| `--surface-main` | `rgba(30,30,30,0.92)` | Main content area |
| `--surface-soft` | `rgba(38,38,38,0.9)` | Secondary panels, button default |
| `--surface-raised` | `rgba(42,42,42,0.96)` | Hover surface |
| `--surface-elevated` | `rgba(48,48,48,0.92)` | Dropdowns, popovers |
| `--surface-sidebar` | `rgba(22,22,22,0.96)` | Sidebar background |
| `--surface-panel` | `rgba(32,32,32,0.96)` | Cards, settings panels |

### Text
| Token | Hex | Role |
|-------|-----|------|
| `--text-strong` | `#f0f0f0` | Headlines, emphasis |
| `--text-base` | `#c8c8c8` | Body copy |
| `--text-muted` | `#888888` | Secondary info |
| `--text-faint` | `#666666` | Timestamps, labels |

### Border
| Token | Hex | Role |
|-------|-----|------|
| `--border-subtle` | `rgba(255,255,255,0.08)` | Card edges, dividers |
| `--border-strong` | `rgba(255,255,255,0.16)` | Hover borders, scrollbar thumb |

### Semantic
| Token | Hex | Role |
|-------|-----|------|
| `--danger` | `#ef4444` | Errors, destructive actions |
| `--warning` | `#f59e0b` | Caution states |

## Typography

### Font Stack
- **UI**: `"Segoe UI Variable Display", "Segoe UI", "PingFang SC", "Microsoft YaHei UI", sans-serif`
- **Code**: `"JetBrains Mono", "Cascadia Code", "SFMono-Regular", Consolas, monospace`

### Type Scale (4-tier hierarchy)

| Token | Size | Weight | Line Height | Use |
|-------|------|--------|-------------|-----|
| `display` | 18px | 600 | 1.3 | Page titles, settings headers |
| `body` | 14px | 400 | 1.5 | Messages, prose, form labels |
| `caption` | 12px | 400 | 1.4 | Tool metadata, timestamps, secondary info |
| `micro` | 11px | 500 | 1.3 | Status bar, badges (use sparingly) |
| `code` | 13px | 400 | 1.5 | Code blocks, monospace surfaces |

### Rules
- Minimum font size: **11px**. Never use 10px or smaller.
- User messages and assistant messages share the same body size (14px).
- Markdown headings: h1=18px, h2=16px, h3=15px (all > body 14px).
- CJK text needs at minimum 14px body for comfortable reading.

## Component Tokens

### Buttons
| Variant | Background | Text | Radius | Padding | Font |
|---------|-----------|------|--------|---------|------|
| Primary | `--accent` | `#fff` | 10px | `8px 16px` | 13px / 500 |
| Secondary | `--surface-soft` | `--text-base` | 8px | `7px 14px` | 13px / 500 |
| Danger | `--danger-soft` | `--danger` | 10px | `8px 16px` | 13px / 500 |
| Icon | `--surface-soft` | `--text-base` | 6px | centered 28x28 | — |

### Inputs
| Property | Value |
|----------|-------|
| Background | `--surface-contrast` |
| Border | `1px solid --border-subtle` |
| Radius | 8px |
| Padding | `10px 12px` |
| Font size | 14px |
| Focus ring | `0 0 0 3px --accent-soft` |

### Cards
| Property | Value |
|----------|-------|
| Background | `--surface-panel` |
| Border | `1px solid --border-subtle` |
| Radius | 10px |
| Padding | 16px |
| Shadow | none (hairline borders only) |

### Chat Messages
| Element | Font Size | Spacing |
|---------|-----------|---------|
| User message | 14px body | `px-4 py-3`, border-radius 8px |
| Assistant prose | 14px body, lh 1.6 | paragraph margin 8px |
| Tool card | 12px caption | `px-3 py-2` |
| Code block | 13px code | `px-3 py-3` |
| Status line | 12px caption | gap 6px |
| Inline code | 13px code | `px-6px py-2px` |

### Sidebar (256px wide)
| Element | Font Size |
|---------|-----------|
| App title | 13px / 600 |
| Search input | 13px |
| Thread title | 12px |
| Thread timestamp | 11px micro |
| Section label | 11px / 600, uppercase, 0.06em tracking |
| Footer buttons | 12px |

## Layout & Spacing

### Spacing Scale
| Token | Value | Use |
|-------|-------|-----|
| `xxs` | 4px | Inline gaps |
| `xs` | 8px | Tight component padding |
| `sm` | 12px | Standard gap |
| `base` | 16px | Card padding, section spacing |
| `md` | 20px | Content margins |
| `lg` | 24px | Section gaps |
| `xl` | 32px | Large section spacing |

### Border Radius
| Token | Value |
|-------|-------|
| `sm` | 6px |
| `md` | 8px |
| `lg` | 10px |
| `xl` | 12px |
| `pill` | 9999px |

## Depth & Elevation

- **No drop shadows** on cards. Use 1px hairline borders.
- Composer shadow: `0 20px 55px rgba(0,0,0,0.26)` (only for floating input).
- Scrollbar: thin, `--border-strong` thumb on transparent track.

## Do's and Don'ts

### Do
- Use the 4-tier type scale consistently
- Keep accent color usage sparse (CTAs, active states only)
- Use hairline borders instead of shadows
- Ensure all text is at least 11px
- Match user and assistant message font sizes

### Don't
- Use `text-[10px]` anywhere
- Mix rem and px for the same semantic purpose
- Use bold weight for display text (keep at 400-500)
- Add shadows to cards or panels
- Make Markdown h3 smaller than body text

## Responsive Behavior

- Sidebar collapses below 960px (flexbox column layout)
- Chat content max-width: 1180px, centered
- Input composer: sticky bottom with gradient fade
- Minimum touch target: 28px
