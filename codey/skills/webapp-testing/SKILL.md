---
name: webapp-testing
description: Test local web applications through CN-Codex's built-in Browser Skill and Playwright-controlled browser.
license: Complete terms in LICENSE.txt
---

# Web Application Testing

Use this skill when a task needs rendered browser verification for a local web app: navigation, screenshots, DOM/text checks, clicks, keyboard input, forms, console observation, or visual smoke testing.

## Required Approach

- First read `codey/skills/browser/SKILL.md`.
- Use the built-in `browser_run` tool for browser simulation and verification.
- Keep `use_visible_browser` enabled by default so Playwright controls the visible `cn-browser` Tauri WebView through CDP.
- Do not write standalone Python, Node, or Playwright scripts for ordinary browser work.
- Do not connect to an unrelated Chrome, remote Browser Use session, or external browser harness unless the user explicitly asks for that alternate path.
- For local apps, start or reuse the dev server first, then call `browser_run` against the local URL.

## Workflow

1. Identify or start the local server.
2. Call `browser_run` with the target URL and a small set of actions.
3. Capture a screenshot or read rendered text after meaningful interactions.
4. Base follow-up actions on the visible page state.

Example:

```json
{
  "url": "http://localhost:5173",
  "use_visible_browser": true,
  "actions": [
    { "type": "wait_for_selector", "selector": "#root" },
    { "type": "screenshot", "fullPage": true },
    { "type": "text", "selector": "body" }
  ]
}
```

## Notes

- Prefer stable selectors for app tests.
- Use coordinate clicks only when the visible target is clear and selectors are awkward.
- If the runner reports a visible-browser fallback note, include that detail in the final response.
