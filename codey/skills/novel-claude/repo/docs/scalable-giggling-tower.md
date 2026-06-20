# Plan: Transform Novel-Claude into Claude Code-like CLI Tool

## Context

The user wants to transform Novel-Claude from a novel-generation pipeline tool into a Claude Code-like interactive CLI application with:
- Interactive REPL with slash commands
- Project management (create/switch/list)
- File operations (ls, cat, find)
- Command-line tool for executing CLI commands

The current implementation is a stateless one-shot command tool using Click. We need to add an interactive layer while maintaining backward compatibility.

---

## Implementation Plan

### Phase 1: Core REPL Infrastructure

**New file: `cli/repl.py`**
- Uses `prompt_toolkit` for interactive input with command history
- Main loop: prompt → parse → dispatch → display
- Prompt format: `[Novel: {project}] (vol:{v}, ch:{c}) > _`
- Multiline support for long commands

**New file: `cli/dispatcher.py`**
- Routes commands to handlers
- Supports slash commands (`/init`) and regular commands
- Integrates with existing Click commands for backward compatibility

**New file: `cli/completer.py`**
- Autocompletion for commands, project files, skill names, chapter numbers

---

### Phase 2: Project Management

**New file: `cli/project_manager.py`**
- `projects create <name> <logline>` — Creates `.novel_{name}/` workspace
- `projects switch <name>` — Switches active project
- `projects list` — Lists all projects
- `projects delete <name>` — Removes project
- `projects info` — Shows current project details

**Modified: `utils/workspace.py`**
- Add `list_projects()`, `get_project_path()`
- Thread-safe operations

**Modified: `utils/config.py`**
- Add `LAST_ACTIVE_PROJECT` config

---

### Phase 3: Interactive Commands

**New directory: `cli/commands/`**

| Command | Description |
|---------|-------------|
| `/init <logline>` | Stage 1: World building |
| `/plan [volume]` | Stage 2: Generate outlines |
| `/write --volume N --chapters X-Y` | Stage 3: Real-time writing |
| `/batch build/submit/sync` | Batch API workflow |
| `/skills list/enable/disable/reload/build` | Plugin management |
| `/ls [path]` | List directory |
| `/cat <file>` | Show file contents |
| `/find <pattern>` | Find files |
| `/cd <path>` | Change directory |
| `/pwd` | Print working directory |
| `/projects create/switch/list/delete/info` | Project management |
| `/agent review <files> <instruction>` | AI multi-file review |
| `/settings set/show` | Configuration |
| `/help, /exit, /clear, /history` | Built-in commands |

---

### Phase 4: Modify `cli.py` for Interactive Mode

**Modified: `cli.py`**
- Add `--interactive` / `-i` flag to enter REPL
- Add `--project <name>` flag to set project context
- Lazy-load REPL to avoid overhead
- Keep all existing commands functional (backward compatible)

---

### Phase 5: Backward Compatibility

**New file: `cli/compat.py`**
- Wraps existing Click commands for REPL use
- Original CLI syntax unchanged: `uv run python cli.py init "logline"`

---

## File Changes Summary

| File | Action |
|------|--------|
| `cli.py` | Modify — add `--interactive` flag, lazy-load REPL |
| `cli/repl.py` | New — Core REPL engine |
| `cli/dispatcher.py` | New — Command routing |
| `cli/completer.py` | New — Autocompletion |
| `cli/project_manager.py` | New — Project lifecycle |
| `cli/permissions.py` | New — Permission system |
| `cli/commands/` | New — Command implementations |
| `cli/compat.py` | New — Backward-compatible wrappers |
| `utils/workspace.py` | Modify — add project enumeration |
| `utils/config.py` | Modify — add project config |

---

## Critical Files to Reuse

- `cli.py` — existing Click commands (wrap, don't rewrite)
- `world_builder.py` — `run_world_builder()` function
- `volume_planner.py` — `plan_macro_outlines()`, `run_volume_planner()`
- `scene_writer.py` — `run_scene_writer()`
- `core/plugin_manager.py` — `PluginManager` class
- `core/event_bus.py` — `event_bus` singleton

---

## Verification

1. Run `uv run python cli.py --interactive` to enter REPL
2. Test `/help` command shows available commands
3. Test `projects create test_novel "test story"` creates workspace
4. Test `projects switch test_novel` changes context
5. Test `/ls` and `/cat` for file operations
6. Test `/init "a story"` runs world builder
7. Test existing `uv run python cli.py init "x"` still works