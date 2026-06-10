---
name: browser-harness
description: Compatibility alias for CN-Codex's built-in Browser Skill. Use for browser automation, scraping, testing, screenshots, and page inspection through browser_run.
---

# Browser Harness

This skill is kept as a compatibility alias for prompts or imported workflows that mention a browser harness. In CN-Codex, browser automation must use the built-in Browser Skill.

## Required Approach

- First read `codey/skills/browser/SKILL.md`.
- Use `browser_run` for navigation, clicking, typing, screenshots, rendered text checks, and page inspection.
- Runtime is WebView + JS Injection. Do not invoke external `browser-harness` binaries or Playwright scripts.
- Do not invoke a remote Browser Use daemon or unrelated standalone Chrome for ordinary browser simulation.
- Only use a detached or remote browser path when the user explicitly asks for it.

## Example

```json
{
  "url": "https://example.com",
  "actions": [
    { "type": "screenshot", "fullPage": true },
    { "type": "text", "selector": "body" }
  ]
}
```
