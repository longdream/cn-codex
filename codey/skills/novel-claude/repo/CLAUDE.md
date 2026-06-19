# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Setup

```bash
# Create virtual environment and install dependencies
uv venv
uv pip install -r requirements.txt
```

## Common Commands

```bash
# CLI mode (one-shot commands)
uv run python cli.py --help

# Interactive REPL mode
uv run python cli.py --interactive

# Run tests (if any)
pytest

# Development workflow
uv run python cli.py skills reload   # After modifying skills code
```

## Configuration

All core parameters are managed via `config.json`:

```json
{
  "workspace": {
    "novel_name": "我的小说名"    // 工作区名称，决定 .novel_{name} 目录
  },
  "writing": {
    "target_word_count": 7000,      // 目标字数
    "min_word_count": 5000,         // 最少字数
    "max_word_count": 9000,          // 最多字数
    "history_chapters_count": 3,    // 历史章节回顾数量
    "previous_chapter_chars": 2000 // 前章结尾字符数
  },
  "review": {
    "deep_review_enabled": true,    // 是否启用深度审阅
    "auto_fix_title": true,         // 是否自动修复标题
    "word_count_check": true         // 是否检查字数
  },
  "generation": {
    "temperature": 0.85,
    "max_retries": 3,               // API中断重试次数
    "timeout": 120,                 // 请求超时（秒）
    "retry_delay": 5                // 重试间隔（秒）
  }
}
```

## Architecture

### Three-Stage Novel Generation Pipeline

1. **world_builder.py** - Initializes world settings (factions, power levels, characters, rules) from a logline
2. **volume_planner.py** - Plans multi-volume macro outline, then generates stage/chapter outlines for each volume
3. **scene_writer.py** - Writes chapters with progressive saving and state machine

### Microkernel & Plugin System

The V3 architecture uses a plugin system (`skills/`) that hooks into the EventBus:

- **EventBus** (`core/event_bus.py`) - Central singleton for event dispatch with fault isolation
  - `emit()` - broadcast to all subscribers
  - `emit_pipeline()` - sequential chain (output → next input)
  - `collect()` - gather results from all skills

- **PluginManager** (`core/plugin_manager.py`) - Dynamically loads skills from `skills/` directory

- **BaseSkill** (`core/base_skill.py`) - Standard plugin base class with lifecycle hooks:
  - `on_init()` - after plugin loads
  - `on_before_scene_write()` - before each scene generation
  - `on_after_scene_write()` - after scene completes
  - `get_llm_tools()` - register AI tools

### Key Files

| File | Purpose |
|------|---------|
| `cli.py` | Click-based CLI entry, also hosts interactive REPL |
| `cli/repl.py` | Interactive REPL implementation |
| `cli/dispatcher.py` | Command routing for REPL |
| `cli/project_manager.py` | Multi-project state management |
| `core/agents/editor_agent.py` | ReAct-based chapter merger |
| `core/agents/skill_builder_agent.py` | Auto-generates new skills |
| `core/entity_tracker.py` | Tracks entity state changes across chapters |
| `core/context_assembler.py` | @DSL syntax for dynamic card reference injection |
| `utils/llm_client.py` | LLM client with progressive saving and retry |
| `utils/chapter_state.py` | Chapter generation state machine |
| `utils/config_loader.py` | Unified config management |
| `config.json` | All core parameters (workspace, writing, review, generation) |

### Project Workspace

Projects are stored in `.novel_{name}/` where `name` is defined in `config.json`:

```
.novel_{name}/
├── settings/              # 世界观设定文件
│   ├── goldfinger.json   # 金手指
│   ├── one_sentence.json # 一句话梗概
│   ├── story_outline.json# 故事大纲
│   ├── world_setting.json# 世界观
│   └── core_blueprint.json# 角色/场景/组织卡
├── volumes/               # 卷大纲
│   ├── vol_01_outline.json
│   ├── vol_01_stages/    # 阶段细纲
│   └── vol_01_chapters/   # 章节大纲
├── manuscripts/           # 成稿
│   └── vol_01/
│       ├── ch_001_final.md
│       └── ch_001_temp.md # 临时文件（生成中）
└── memory/               # RAG记忆库
```

## Skill Development

Skills live in `skills/{name}/skill.py` and must inherit from `BaseSkill`:

```python
from core.base_skill import BaseSkill

class MySkill(BaseSkill):
    def __init__(self, context):
        super().__init__(context)
        self.name = "MySkill"

    def on_before_scene_write(self, prompt_payload, beat_data):
        prompt_payload.append("\n[System] Custom injection")
        return prompt_payload
```

After creating/modifying a skill, run `uv run python cli.py skills reload` to hot-reload.

## Progressive Saving & State Machine

`scene_writer.py` supports resume from interruption:

- State tracking: `vol_XX_chapter_states.json`
- Temporary files: `ch_XXX_temp.md` (deleted after completion)
- On restart, skipped chapters with `ch_XXX_final.md` > 1000 bytes

## Pre-Push Security Check

Before every `git push`, always check for API key leaks:

```bash
# Check for leaked keys before pushing
git diff --cached --name-only | xargs grep -l "api_key\|API_KEY\|ANTHROPIC\|zhipuai" 2>/dev/null
git diff HEAD --name-only | xargs grep -l "api_key\|API_KEY\|ANTHROPIC\|zhipuai" 2>/dev/null

# Also check untracked files
grep -r "api_key\|API_KEY\|ANTHROPIC\|zhipuai" --include="*.py" --include="*.json" --include="*.env" . 2>/dev/null | grep -v ".venv" | grep -v "__pycache__"
```

If any secrets are found, remove them before pushing. The `env` file and `.novel_*/` are in `.gitignore` but always verify.

**Note**: `config.json` contains only non-sensitive parameters (workspace name, writing settings) and is safe to commit.

## Pre-Push Workflow

Before every `git push`, always run:

```bash
uv run python scripts/pre_push.py
```

This script:
1. Updates `README.md` with current project structure
2. Checks for API key leaks in staged/unstaged/untracked files