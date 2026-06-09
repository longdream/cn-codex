---
name: control-in-app-browser
description: Control CN-Codex's built-in browser with Playwright for local app testing, navigation, clicking, typing, screenshots, and page inspection.
---

# Browser

This plugin skill is adapted for CN-Codex. Use it whenever the user asks to open, inspect, navigate, test, click, type, screenshot, or verify a page in a real browser.

## CN-Codex Runtime

- Use the built-in `browser_run` tool for browser automation.
- `browser_run` opens or reuses the visible `cn-browser` Tauri WebView and controls it through Playwright CDP by default.
- Keep `use_visible_browser` enabled unless the user explicitly asks for a detached/headless fallback or you are running a narrow runner smoke test.
- Do not use a separate Playwright script, external browser automation server, or generic shell-launched browser for normal browser simulation.
- Do not use upstream Browser plugin bootstrap instructions that mention `Node REPL`, `browser-client`, or `agent.browsers.get("iab")` inside CN-Codex. Those belong to the host Codex desktop plugin runtime; CN-Codex's adapter is `browser_run`.

## Workflow

1. Start or reuse the target app server when testing a local app.
2. Call `browser_run` with the target URL and ordered actions.
3. Prefer a screenshot or rendered text read after meaningful interactions.
4. Base follow-up actions on the visible page state, not assumptions.

## `browser_run`

Common action types:

- `goto`: navigate to a URL.
- `click`: click by `selector`, or by `x` and `y`.
- `hover`: hover an element by selector.
- `fill`: fill an input by selector.
- `type`: type text into a focused element or selector.
- `press`: press a key, optionally scoped to a selector.
- `check` / `uncheck`: toggle a checkbox by selector.
- `select_option`: select a native `<select>` option by `value`, `values`, `label`, or `index`.
- `wait_for_selector`: wait for a selector.
- `wait_for_timeout`: wait for a fixed number of milliseconds.
- `screenshot`: capture a screenshot; use `fullPage: true` when useful.
- `set_viewport`: set the current page viewport with `width` and `height`.
- `reload`, `back`, `forward`: navigate browser history.
- `title` / `url`: read the current page title or URL.
- `html`: read the page HTML, optionally with `maxChars`.
- `snapshot`: read a compact page snapshot with title, URL, visible text, and key controls.
- `assets`: list page images, stylesheets, scripts, and links.
- `bundle_assets`: save selected page images/stylesheets/scripts/links into a local artifact directory with `manifest.json`.
- `eval`: evaluate JavaScript in the page.
- `text`: read visible text from a selector, defaulting to `body`.
- `list_tabs`: list known browser tabs/pages and the active tab.
- `new_tab`: open a new tab/page, optionally with `url`, and make it active.
- `switch_tab`: switch the active tab by `index`, `url_contains`, or `title_contains`.
- `close_tab`: close the active tab or a tab selected by `index`, then activate a remaining tab.

Example:

```json
{
  "url": "http://localhost:1420",
  "use_visible_browser": true,
  "actions": [
    { "type": "screenshot", "fullPage": true },
    { "type": "click", "selector": "button[aria-label='Settings']" },
    { "type": "text", "selector": "body" },
    { "type": "screenshot", "fullPage": true }
  ]
}
```

If Playwright cannot launch or connect, first report the visible-browser fallback note from the tool output. If no browser binary is installed for fallback mode, run `pnpm exec playwright install chromium` in the CN-Codex workspace, then retry.
