---
name: record-replay
description: "Record user browser operations and generate a Playwright Python replay script that can be run from the right-side Replay panel. Use when the user asks to record, capture, or demonstrate a browser workflow."
---

# Record & Replay — 录制浏览器操作并生成回放脚本

This skill enables two modes:

1. **Record mode** — 启动外部 Chrome 浏览器，让用户操作并录制。用户点击「停止录制」后，系统自动把录制记录转换成 Playwright Python 回放脚本（带选择器回退、等待与断言容错）。
2. **Replay mode** — 在右侧「回放」面板查看并运行回放脚本；运行失败时自动接入主链路修复（最多重试 5 次，最后一次总结失败原因）。

所有操作都可以通过对话完成：录制、查看、运行、修复、删除脚本，全部通过 `recording_control` 工具走主链路，右侧「回放」面板会实时同步刷新。

## 录制用户操作

### Step 1: 启动浏览器并开始录制

当用户说"录制" / "record" / "帮我录制操作" / "record my workflow" 时：

```
recording_control: {"action": "launch_browser"}
recording_control: {"action": "start_recording"}
```

`start_recording` 可以传可选的 `name` 作为录制名称。启动后会触发右侧面板的录制状态（计时器显示）。告诉用户：

> "已打开 Chrome 浏览器并开始录制。请直接在 Chrome 中执行您的操作，完成后告诉我，或点击界面上的「停止录制」。"

### Step 2: 等待用户完成录制

录制过程中不要执行任何 `browser_run` 操作，用户直接在 Chrome 中操作。

### Step 3: 停止录制，脚本自动生成

用户说"停止录制/录完了"，或点击界面「停止录制」后，系统自动把录制记录交给主链路生成 Playwright Python 回放脚本，无需你手动生成。

如需在对话中主动结束录制：

```
recording_control: {"action": "stop_recording"}
```

如需查看录制记录：

```
recording_control: {"action": "list_traces"}
recording_control: {"action": "read_trace", "session_id": "<session-id>"}
```

回放脚本存放在 `codey/recordings/scripts/` 目录下，文件名与录制 session 对应。

### Step 4: 汇报结果

告诉用户：

> "录制已停止，回放脚本已自动生成。可以在右侧面板点击「运行」回放，也可以随时在对话里让我运行、修改或删除脚本。"

不要再说"生成技能/skill"——正确的产物是回放脚本。

## 通过对话管理回放脚本

右侧面板里能做的所有操作，对话里都能做：

- 列出脚本卡片：`recording_control: {"action": "list_scripts"}`（返回 id、名称、步骤数、最近运行状态）
- 查看脚本内容：`recording_control: {"action": "read_script", "script_id": "<id>"}`
- 运行回放：`recording_control: {"action": "run_replay", "script_id": "<id>"}`（返回 ok/error/stdout/stderr；右侧面板同步刷新状态与报告）
- 删除脚本卡片：`recording_control: {"action": "delete_script", "script_id": "<id>"}`（删除前必须先跟用户确认）
- 面板刷新：以上操作后右侧「回放」面板会通过 `cn-codex:replay-updated` 事件自动刷新

修改脚本时用 `read_script` 读取内容，用 `apply_patch` 直接修改 `codey/recordings/scripts/<id>.py`，再用 `run_replay` 验证。

## 回放脚本失败时的自动修复

当用户点击「运行」失败时，前端会创建/复用独立的「回放修复」对话并接入主链路：

1. 前端把脚本路径、错误输出、页面 URL / 截图等信息发送到该对话。
2. 主链路用 `read_file` 读取脚本、`apply_patch` 修改脚本（保持选择器回退、等待与断言容错），必要时用 `browser_run` 打开页面核对。
3. 修复后前端重新运行脚本验证，成功则结束。
4. 运行或修复过程中，右侧面板的「停止回放」会终止当前 Python 回放、浏览器子进程和主链路请求，不再启动下一轮修复。
5. 最多重试 5 次；超过上限时主链路总结失败根因并显示在该对话中。

在修复过程中，你只负责读取脚本、检查错误与页面并修改脚本文件，不需要自己执行回放脚本。

## 回放脚本结构（供修复时参考）

生成的脚本使用 Playwright sync API，内置：

- 每个步骤等待目标选择器，支持多个候选选择器回退；
- 每步最多重试 3 次；
- 导航后等待页面稳定；
- 失败时输出结构化 JSON（step / kind / url / title / screenshot）并退出非 0。

## Selector Priority

录制事件会提供多个候选选择器，回放时优先顺序：
1. `#id` — most stable
2. `[data-testid="..."]` — explicit test attribute
3. `[name="..."]` or `[aria-label="..."]` — semantic
4. `[placeholder="..."]` — input-specific
5. `tag.class` — CSS class based (less stable)
6. `text="..."` — visible text match (least stable)
