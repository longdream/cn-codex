---
name: novel-claude
description: AI-powered long-form web novel generation framework (Novel-Claude V3). Use when the user asks to generate novels, create fiction, build story worlds, plan novel outlines, write chapters, manage novel plugins/skills, or any creative writing task involving structured multi-chapter story generation. Triggers on keywords like "novel", "fiction", "story generation", "chapter writing", "world building", "volume planning", "scene writing", "web novel", "网文", "小说", "写作", "章节", "大纲", "世界观".
---

# Novel-Claude V3

AI-powered long-form web novel generation framework based on a microkernel + plugin architecture.

Source repo is bundled at `codey/skills/novel-claude/repo/`.

## Prerequisites

Ensure Python >= 3.10 and `uv` are available. Install dependencies before first use:

```bash
cd codey/skills/novel-claude/repo
uv venv
uv pip install -r requirements.txt
```

## LLM 配置（重要）

**本 skill 不维护任何独立的 LLM 配置。** 所有 LLM 调用（模型、API Key、Base URL）
完全继承自 cn-codex 的运行时状态，与主程序界面上选择的配置保持一致。

### 读取优先级

1. **`codey/usage.db`**（SQLite）— cn-codex 运行时存储的 active provider / model，
   这是界面上实际选择的配置，始终以此为准
2. **`codey/config.toml`** — 仅在 usage.db 不存在或读取失败时作为回退

### 禁止事项

- **禁止**在 skill 代码中硬编码任何 API key、base_url 或模型名称
- **禁止**在 `config.json` 中添加 LLM 相关配置项来覆盖运行时配置
- **禁止**使用环境变量（`.env` 文件）配置 LLM
- **禁止**切换到 cn-codex 以外的任何 LLM 配置源

### 修改 LLM 配置

如需更换模型或提供商，直接在 cn-codex 设置界面中修改即可。
本 skill 会在下次调用时自动读取最新的运行时配置。

## Writing Configuration

Edit `codey/skills/novel-claude/repo/config.json` to set project name and writing parameters:

```json
{
  "workspace": { "novel_name": "小说名称" },
  "writing": {
    "target_word_count": 7000,
    "min_word_count": 5000,
    "max_word_count": 9000,
    "history_chapters_count": 3,
    "previous_chapter_chars": 2000
  },
  "review": { "deep_review_enabled": true, "auto_fix_title": true, "word_count_check": true },
  "generation": { "temperature": 0.85, "max_retries": 3, "timeout": 120, "retry_delay": 5 }
}
```

## Core Workflow (3-Stage Pipeline)

All commands run from the `codey/skills/novel-claude/repo/` directory.

### Stage 1: World Building

```bash
uv run python cli.py init "一句话创意描述"
```

Generates world settings, factions, power systems, and characters in `.novel_{name}/settings/`.

### Stage 2: Volume Planning

```bash
# Macro outline for all 10 volumes
uv run python cli.py plan

# Micro-level scene beats for a specific volume (50 chapters)
uv run python cli.py plan --volume 1
```

### Stage 3: Chapter Writing

```bash
# Write chapters 1-5 of volume 1
uv run python cli.py write --volume 1 --chapters "1-5"
```

Supports resume from interruption via checkpoint files.

### Batch API (cost-efficient bulk generation)

```bash
uv run python cli.py batch-build --volume 1 --chapters "1-50"
uv run python cli.py batch-submit .batch/vol_01_ch_1_50_req.jsonl
uv run python cli.py batch-sync <batch_id>
```

### Review & Edit

```bash
uv run python cli.py review -f ".novel/factions.json" -i "修改指令"
```

### Interactive REPL

```bash
uv run python cli.py --interactive
```

## Plugin System

Plugins (Skills) live in `repo/skills/` and hook into the EventBus lifecycle:

| Hook | When | Purpose |
|------|------|---------|
| `on_init()` | Plugin loaded | Initialize resources |
| `on_volume_planning()` | Volume planning | Modify outlines |
| `on_before_scene_write()` | Before writing | Inject memory/settings |
| `on_after_scene_write()` | After writing | Stats/storage |
| `on_chapter_render()` | Final render | Replace placeholders |
| `get_llm_tools()` | LLM calls | Register AI tools |

Manage plugins:

```bash
uv run python cli.py skills list
uv run python cli.py skills enable <name>
uv run python cli.py skills disable <name>
uv run python cli.py skills reload
uv run python cli.py skills build "自然语言描述"
```

## Output Structure

Generated content is stored in `.novel_{name}/`:

```
.novel_{name}/
├── settings/           # World settings (JSON)
├── volumes/            # Volume outlines + chapter beats
├── manuscripts/        # Final chapter files (Markdown)
└── memory/             # RAG memory store
```

## Architecture Reference

For detailed architecture, agent design, and development guide, read `codey/skills/novel-claude/repo/CLAUDE.md`.
