---
name: webapp-testing
description: Test local web applications through CN-Codex's built-in Browser Skill and WebView JS Injection runtime.
license: Complete terms in LICENSE.txt
---

# Web Application Testing

Use this skill when a task needs rendered browser verification for a local web app: navigation, screenshots, DOM/text checks, clicks, keyboard input, forms, console observation, or visual smoke testing.

## Required Approach

- First read `codey/skills/browser/SKILL.md`.
- Use the built-in `browser_run` tool for browser simulation and verification.
- Runtime is Tauri WebView + Rust JS Injection through CDP.
- Do not write standalone Python/Node Playwright scripts for ordinary browser work.
- Do not connect to an unrelated Chrome, remote Browser Use session, or external browser harness unless the user explicitly asks for that alternate path.
- For local apps, start or reuse the dev server first, then call `browser_run` against the local URL.

## Workflow

1. Identify or start the local server (use `exec_command`, see below).
2. Poll terminal output to confirm the server started without errors.
3. Call `browser_run` with the target URL — inject console capture script first.
4. Perform interactions (clicks, fills, navigation).
5. Read back console errors and network failures.
6. Capture a screenshot or read rendered text after meaningful interactions.
7. Base follow-up actions on the visible page state + error findings.

Example:

```json
{
  "url": "http://localhost:5173",
  "actions": [
    { "type": "wait_for_selector", "selector": "#root" },
    { "type": "screenshot", "fullPage": true },
    { "type": "text", "selector": "body" }
  ]
}
```

## Server Startup Observation

When starting a dev server for testing, use `exec_command` (NOT one-shot `shell`) so the process stays alive and you can observe its output:

1. Use `exec_command` to start the server — it returns a `session_id` and partial output.
2. Immediately use `write_stdin` with empty `chars` to poll initial output.
3. Check for errors: "EADDRINUSE", "Error:", "failed", "FATAL", stack traces, non-zero exit.
4. Only proceed to browser testing when server reports ready (e.g. "ready on http://...", "Local:", "listening on port").
5. If errors are found, report them and attempt to fix before continuing.

Example flow:

```
Tool: exec_command
Args: {"command": "npm run dev", "workdir": "/project"}
Result: { "session_id": "abc-123", "output": "starting dev server..." }

Tool: write_stdin
Args: {"session_id": "abc-123", "chars": ""}
Result: { "output": "  VITE v5.0.0  ready in 320 ms\n  -> Local: http://localhost:5173/" }
-> Server is ready, proceed to browser testing.
```

If output contains errors:
```
Tool: write_stdin
Args: {"session_id": "abc-123", "chars": ""}
Result: { "output": "Error: EADDRINUSE: address already in use :::5173" }
-> Report error, attempt fix (kill port or change port), restart.
```

## Browser Console Error Checking

After navigating to the page, ALWAYS inject a console capture script and check for errors. This catches JS runtime errors, unhandled promise rejections, and failed API calls that would otherwise go unnoticed.

### Step 1 — Inject capture (do this FIRST, before interacting with the page):

```json
{
  "actions": [
    {
      "type": "eval",
      "expression": "window.__TEST_CONSOLE_LOGS__=[];['error','warn','log'].forEach(m=>{const o=console[m].bind(console);console[m]=(...a)=>{window.__TEST_CONSOLE_LOGS__.push({level:m,msg:a.map(x=>typeof x==='string'?x:JSON.stringify(x)).join(' '),ts:Date.now()});o(...a);}});window.__TEST_NETWORK_ERRORS__=[];const _xhrOpen=XMLHttpRequest.prototype.open;XMLHttpRequest.prototype.open=function(m,u){this.__url=u;const _send=this.send.bind(this);this.send=function(...a){this.addEventListener('loadend',()=>{if(this.status>=400)window.__TEST_NETWORK_ERRORS__.push({url:this.__url,status:this.status});});_send(...a);};_xhrOpen.apply(this,arguments);};const _fetch=window.fetch;window.fetch=async(...a)=>{const r=await _fetch(...a);if(!r.ok)window.__TEST_NETWORK_ERRORS__.push({url:typeof a[0]==='string'?a[0]:a[0]?.url||'unknown',status:r.status});return r;};"
    }
  ]
}
```

### Step 2 — After interactions/page load, read errors:

```json
{
  "actions": [
    {
      "type": "eval",
      "expression": "JSON.stringify({console:window.__TEST_CONSOLE_LOGS__?.filter(l=>l.level==='error')||[],network:window.__TEST_NETWORK_ERRORS__||[]})"
    }
  ]
}
```

### Step 3 — Report findings:

- Any `console.error` entries → report as JS runtime errors (test failure)
- Any network errors (4xx/5xx) → report as API failures (test failure)
- Include error messages, URLs, and timestamps in the test report
- If no errors found, explicitly state "No console errors or network failures detected"

## Notes

- Prefer stable selectors for app tests.
- Use coordinate clicks only when the visible target is clear and selectors are awkward.
- If the runner reports a visible-browser fallback note, include that detail in the final response.
