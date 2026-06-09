---
name: browser-harness
description: Compatibility alias for CN-Codex's built-in Browser Skill. Use for browser automation, scraping, testing, screenshots, and page inspection through browser_run.
---

# Browser Harness

This skill is kept as a compatibility alias for prompts or imported workflows that mention a browser harness. In CN-Codex, browser automation must use the built-in Browser Skill.

## Required Approach

- First read `codey/skills/browser/SKILL.md`.
- Use `browser_run` for navigation, clicking, typing, screenshots, rendered text checks, and page inspection.
- Keep `use_visible_browser` enabled by default so Playwright controls the visible `cn-browser` Tauri WebView through CDP.
- Do not invoke an external `browser-harness` binary, remote Browser Use daemon, standalone Chrome, or separate Playwright script for ordinary browser simulation.
- Only use a detached or remote browser path when the user explicitly asks for it.

## Example

```json
{
  "url": "https://example.com",
  "use_visible_browser": true,
  "actions": [
    { "type": "screenshot", "fullPage": true },
    { "type": "text", "selector": "body" }
  ]
}
```

If `browser_run` cannot connect to the visible built-in browser, report the fallback note from the tool output and continue only if the fallback result is adequate for the task.
