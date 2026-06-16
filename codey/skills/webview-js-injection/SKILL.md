---
name: webview-js-injection
description: Use CN-Codex browser_run with Tauri WebView + Rust JS Injection automation. Trigger for browser testing, UI interaction, screenshots, rendered DOM checks, and web scraping without external Playwright scripts.
---

# WebView JS Injection

Use this skill when browser automation is needed inside CN-Codex.

## Required Approach

- Always use the built-in `browser_run` tool.
- Treat `engine` as compatibility input only; runtime is unified to `webview-js-injection`.
- Do not create standalone Node/Python Playwright scripts for normal browser tasks.
- Prefer short action batches and take screenshots after important UI changes.
- For local web apps, start/reuse the server first, then run `browser_run` on the local URL.

## Supported Actions

`goto`, `reload`, `back`, `forward`, `click`, `hover`, `fill`, `type`, `press`, `check`, `uncheck`, `select_option`, `wait_for_selector`, `wait_for_timeout`, `screenshot`, `set_viewport`, `title`, `url`, `html`, `snapshot`, `assets`, `bundle_assets`, `eval`, `text`, `list_tabs`, `new_tab`, `switch_tab`, `close_tab`

## Example

```json
{
  "url": "http://localhost:5173",
  "actions": [
    { "type": "wait_for_selector", "selector": "#root" },
    { "type": "click", "selector": "button[type='submit']" },
    { "type": "screenshot", "fullPage": true },
    { "type": "text", "selector": "body" }
  ]
}
```

## Failure Triage

- If CDP fails, first ensure the built-in `cn-browser` window is available.
- If selector actions fail, capture one screenshot and inspect `html` or `snapshot` before retrying.
- If a long sequence times out, split into smaller action batches.
