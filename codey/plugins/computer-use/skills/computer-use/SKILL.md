---
name: computer-use
description: Control Windows apps from CN-Codex through the computer-use MCP server
---

# Computer Use

Use this skill to automate Microsoft Windows desktop apps from CN-Codex.

CN-Codex does **not** use Codex `node_repl` for Computer Use. Use the **computer-use MCP server tools** directly.

## Preferred tools

Prefer these MCP tools (names may appear as `mcp__computer-use__*` or via `mcp_call_tool` with `server="computer-use"`):

1. `list_apps` — list installed apps and their open targetable windows
2. `list_windows` — list currently open targetable windows
3. `get_window` — rehydrate a window by `id` / `app`
4. `launch_app` — launch by app id or `.exe` path
5. `activate_window` — bring a window to the foreground
6. `get_window_state` — screenshot and/or accessibility tree
7. `click` / `type_text` / `press_key` / `scroll` / `drag`
8. `set_value` / `perform_secondary_action`

Do **not**:

- import `scripts/computer-use-client.mjs`
- import `@oai/sky` directly
- spawn `codex-computer-use.exe` yourself
- fall back to PowerShell `SendKeys`, mouse-click scripts, or other foreground automation before trying these MCP tools

## Recommended workflow

1. Call `list_apps` first for app-control tasks.
2. Choose one app and one of its returned windows.
3. If the app has no open window, call `launch_app`, then poll `list_apps` / `list_windows`.
4. Call `activate_window` once before the first snapshot when the task needs control (not pure passive inspection).
5. Call `get_window_state` with `include_screenshot=true` and, when needed, `include_text=true`.
6. Prefer accessibility `element_index` clicks when the tree is available; otherwise use screenshot coordinates.
7. After each `get_window_state`, treat the returned `window` as the canonical target for subsequent actions.
8. If a window handle becomes stale, recover with `get_window({ id, app })` or re-list apps/windows.

## First-call smoke path

When starting Computer Use in a session:

1. `list_windows` or `list_apps`
2. If that succeeds, continue with the chosen app/window
3. If it times out once, wait briefly and retry the same lightweight call once
4. If it still fails, report that the Windows Computer Use helper is unavailable and stop

## Action rules

- Always pass the full window object (`app` + `id`, plus `title` when available) into action tools.
- Keep using the same window object until recovery is required.
- Do not reconstruct window ids from memory after a failure; re-query.
- Do not open Start Menu / Search UI just to launch apps; use `launch_app`.
- If the user stops Computer Use, or a tool reports it was stopped, end the turn and report that control stopped.

## Tool argument sketch

```json
// list_apps / list_windows
{}

// get_window
{ "id": 12345, "app": "process:C:\\\\Path\\\\App.exe" }

// launch_app
{ "app": "process:C:\\\\Path\\\\App.exe" }
// or
{ "app": "C:\\\\Path\\\\App.exe" }

// activate_window
{ "window": { "app": "...", "id": 12345, "title": "..." } }

// get_window_state
{
  "window": { "app": "...", "id": 12345 },
  "include_screenshot": true,
  "include_text": true
}

// click
{
  "window": { "app": "...", "id": 12345 },
  "element_index": 12
}
// or coordinate click
{
  "window": { "app": "...", "id": 12345 },
  "x": 120,
  "y": 80,
  "screenshotId": "..."
}

// type_text
{ "window": { "app": "...", "id": 12345 }, "text": "hello" }

// press_key
{ "window": { "app": "...", "id": 12345 }, "key": "Return" }
```

## Troubleshooting

- If MCP tools for `computer-use` are missing, say that the Computer Use MCP server is unavailable.
- If `list_apps` / `list_windows` fails after one retry, stop and report the helper connection failure.
- Do not dig through source code or invent alternative Windows automation paths before retrying the MCP tools.
- Never mention internal client library files (`computer-use-client.mjs`) to the user unless they explicitly ask for implementation details.
