---
name: Browser
description: Use CN-Codex's built-in Playwright browser for browser automation, web app testing, screenshots, UI simulation, scraping, and page inspection.
tags: ["browser", "playwright", "automation", "testing"]
---

# Browser

Use this skill when a task requires a real browser: navigating pages, testing local web apps, clicking UI, filling forms, reading rendered DOM, capturing screenshots, inspecting console output, or verifying visual behavior.

## Required Approach

- This is the canonical Browser Skill for CN-Codex.
- Use CN-Codex's built-in `browser_run` tool for every browser simulation and verification task. It opens or reuses the visible `cn-browser` window and controls it through Playwright CDP.
- Keep `use_visible_browser` enabled unless the user explicitly asks for a detached/headless fallback or you are running a narrow runner smoke test.
- Do not start an unrelated Playwright script or standalone browser for ordinary browser work; call `browser_run` so the simulation happens through CN-Codex's built-in browser surface.
- If an imported Browser plugin skill mentions `Node REPL`, `browser-client`, or `agent.browsers.get("iab")`, treat that as upstream reference text. Inside CN-Codex, the runtime adapter is `browser_run`.
- Do not pretend to inspect a page without opening it in the browser.
- Prefer screenshots after every meaningful interaction so the next action is based on visible state.
- Prefer selectors for stable app testing, but use coordinate clicks when the visible target is clear and selectors are awkward.
- For local apps, start or reuse the app server first, then call `browser_run` against the local URL.

## `browser_run`

`browser_run` runs a Playwright-controlled browser session. It accepts a URL plus ordered actions. By default it opens or reuses the visible CN-Codex browser window; if CDP connection is unavailable it reports the fallback note and uses a standalone Chromium session.

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
  "url": "http://localhost:5173",
  "use_visible_browser": true,
  "actions": [
    { "type": "screenshot", "fullPage": true },
    { "type": "click", "selector": "text=Sign in" },
    { "type": "fill", "selector": "input[name=email]", "text": "user@example.com" },
    { "type": "screenshot", "fullPage": true }
  ]
}
```

If browser launch fails because no browser is installed, run `pnpm exec playwright install chromium` in the CN-Codex workspace, then retry.
