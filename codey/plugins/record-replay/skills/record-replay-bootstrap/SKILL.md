---
name: record-replay
description: "Record user browser operations and generate reusable skills. Use when the user asks to record, capture, or demonstrate a browser workflow. Also use to replay previously recorded skills."
---

# Record & Replay — External Chrome

This skill enables two modes:

1. **Record mode** — Launch an external Chrome browser, let the user operate it while recording actions, then generate a reusable skill from the recording.
2. **Replay mode** — Execute a previously generated skill by operating the external Chrome via `browser_run`.

## Recording a User Demonstration

### Step 1: Launch browser and show recording toggle

When the user says "录制" / "record" / "帮我录制操作" / "record my workflow":

```
recording_control: {"action": "launch_browser"}
```

This launches an external Chrome window and shows the recording toggle in the CN-Codex UI. Tell the user:

> "已打开 Chrome 浏览器，请在 CN-Codex 界面右上角点击「开始录制」按钮，然后在 Chrome 中执行您的操作。操作完成后点击「停止录制」。"

### Step 2: Wait for the user to finish

Wait for the user to tell you they have finished recording. The recording is controlled by the floating toggle button in the CN-Codex UI — the user clicks "Start Recording", operates Chrome, then clicks "Stop Recording".

Do NOT perform any `browser_run` actions during recording. The user operates Chrome directly.

### Step 3: Read the recording trace

After the user says "done" / "完成" / "录好了" / "已停止录制":

First list available traces to find the latest one:

```
recording_control: {"action": "list_traces"}
```

Then read the specific trace:

```
recording_control: {"action": "read_trace", "session_id": "<session-id>"}
```

### Step 4: Generate skills from the trace

Analyze the trace data and generate one or more SKILL.md files. For each logical workflow in the trace:

1. Identify the goal/intent (e.g., "Login to application", "Search for products")
2. Create a SKILL.md file at `codey/skills/<slug>/SKILL.md`
3. The SKILL.md should contain:
   - A clear name and description
   - Step-by-step instructions using `browser_run` actions
   - Variable placeholders for user-specific data (emails, passwords, search terms)

Example generated skill structure:

```markdown
---
name: login-to-example
description: "Log in to example.com with provided credentials."
---

# Login to Example.com

## Variables
- `email` — Login email address
- `password` — Login password

## Steps

1. Navigate to the login page:
   browser_run: {"url": "https://example.com/login", "actions": []}

2. Enter email:
   browser_run: {"actions": [{"type": "fill", "selector": "input[name='email']", "text": "{{email}}"}]}

3. Enter password:
   browser_run: {"actions": [{"type": "fill", "selector": "input[name='password']", "text": "{{password}}"}]}

4. Click login button:
   browser_run: {"actions": [{"type": "click", "selector": "button[type='submit']"}]}

5. Verify login succeeded:
   browser_run: {"actions": [{"type": "wait_for_selector", "selector": ".dashboard"}]}
```

### Step 5: Report to user

Tell the user:
- What skills were generated
- What each skill does
- How to trigger replay (e.g., "执行'登录'skill" / "run the login skill")

## Replaying a Skill

When the user wants to replay a skill:

1. Read the skill file to get the steps
2. Ensure the external Chrome is launched:
   ```
   recording_control: {"action": "launch_browser"}
   ```
3. Execute each step using `browser_run`
4. Replace variable placeholders with actual values (ask user if needed)
5. Verify each step's outcome before proceeding

## browser_run Action Reference

The `browser_run` tool supports these action types for replay:

| type | Description | Key fields |
|------|------------|------------|
| `goto` | Navigate to URL | `url` |
| `click` | Click element | `selector` or `x,y` |
| `fill` | Clear and fill input | `selector`, `text` |
| `type` | Append text to input | `selector`, `text` |
| `press` | Press keyboard key | `key`, optional `selector` |
| `hover` | Hover over element | `selector` |
| `select_option` | Select dropdown option | `selector`, `value`/`label` |
| `wait_for_selector` | Wait for element | `selector` |
| `screenshot` | Capture screenshot | optional `path` |
| `snapshot` | Get page accessibility snapshot | — |

## Selector Priority for Replay

When the trace provides multiple selector candidates, prefer them in this order:
1. `#id` — most stable
2. `[data-testid="..."]` — explicit test attribute
3. `[name="..."]` or `[aria-label="..."]` — semantic
4. `[placeholder="..."]` — input-specific
5. `tag.class` — CSS class based (less stable)
6. `text="..."` — visible text match (least stable)
