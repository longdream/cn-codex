# 🚀 Novel-Claude V3: Agentic Novel Generation Framework

> [!WARNING]
> **This project is currently in the testing phase. Many features are still incomplete. Please use with caution.**

Novel-Claude is a fully automated long-form novel generation pipeline built on Large Language Models (such as Zhipu GLM-4). In version V3, it has evolved from a traditional linear script pipeline into a highly extensible **Microkernel & Plugin Architecture**.

Through the underlying `EventBus` engine and dynamic `PluginManager`, it supports an extremely complex community plugin ecosystem (Skills) and complex agents based on ReAct multi-turn interactions.

## ✨ Core Features

- **Microkernel Plugin System**: All additional functions (such as dynamic RAG memory, combat rationality detection, etc.) are decoupled from the main timeline as plugins (Skills). It supports Hot-Reload and fault-tolerance isolation, ensuring that a single plugin crash does not affect hours of generation progress.
- **Complex Agent Support**:
  - 🖋️ **Editor Agent**: Uses a multi-turn ReAct reasoning loop to strictly review drafts, automatically fixing point-of-view jumps and context fragmentation.
  - 🤖 **Skill Builder Agent**: System-level Meta-Generation. Enter a line of natural language in the CLI, and the system will **automatically write and compile a valid plugin (Skill)**.
- **Cost Reduction & Efficiency (Batch API)**: Native support for Zhipu/OpenAI format Batch API pipelines, supporting 50% discount for offline concurrent generation of massive chapters, with automatic merging and callbacks.

---

## 🏗️ Architecture Overview

The entire generation pipeline is divided into three core engines, which share the state via `NovelContext` and communicate through `EventBus` broadcasts:

1. `world_builder.py` (World Creator): Builds strict JSON-formatted background settings, factions, and character lists based on a single-line idea (Logline).
2. `volume_planner.py` (Volume Planner): Customizes 10-volume outlines and decomposes large volume outlines into precise scene-by-scene beats, with algorithms enforcing a normalized output of 5,000 words per chapter.
3. `scene_writer.py` (Writing Workshop): Spawns Subagents to execute scene tasks without dead ends, with a final edit by the Editor Agent.

```text
novel_claude/
├── core/                       # Microkernel Engine
│   ├── event_bus.py            # Global Event Bus (Fault Tolerance)
│   ├── plugin_manager.py       # Dynamic Plugin Scanner & Loader
│   ├── base_skill.py           # V3 Standardized Plugin Base Class
│   ├── novel_context.py        # Shared Lifecycle Context
│   └── agents/                 # Complex Reasoning Agents
│       ├── editor_agent.py     # ReAct Editor Agent
│       └── skill_builder_agent.py  # Meta-Generation Agent
├── skills/                     # Plugin Directory
│   └── core_memory_rag/        # Native RAG Memory Retrieval Plugin
├── world_builder.py            # Engine 1: Setting Construction
├── volume_planner.py           # Engine 2: Volume & Scene Segmentation
├── scene_writer.py             # Engine 3: Scene Writing & Merging
├── cli.py                      # Terminal Entry
└── utils/                      # Config & LLM Client API Layer
```

---

## 🛠️ Installation & Usage

### 1. Environment Setup
Ensure you have Python >= 3.10.
```bash
# Install dependencies
uv pip install -r requirements.txt
```

### 2. CLI Quick Start

#### Basic Generation Flow
```bash
# Stage 1: Initialize the worldview with one line
uv run python cli.py init "A sci-fi turned fantasy story where the protagonist uses cybernetic nodes to force-open his spiritual root in a cultivation world."

# Stage 2: Plan the macro 10-volume main storyline outline
uv run python cli.py plan

# Stage 2.5: Generate detailed scene beats for 50 chapters of Volume 1
uv run python cli.py plan --volume 1

# Stage 3: Start the writing cluster to generate Volume 1, Chapters 1-5
uv run python cli.py write --volume 1 --chapters "1-5"
```

#### Batch API Flow
```bash
# Build JSONL request file
uv run python cli.py batch-build --volume 1 --chapters "1-50"

# Submit async task (returns Batch ID, please keep it safe)
uv run python cli.py batch-submit .batch/vol_01_ch_1_50_req.jsonl

# Sync and merge results (polls status and auto-downloads)
uv run python cli.py batch-sync <batch_id>
```

#### V3 Plugin Management Commands
```bash
# List all plugins
uv run python cli.py skills list

# Disable/Enable a specific plugin (e.g., Gold Finger)
uv run python cli.py skills disable ext_gold_finger
uv run python cli.py skills enable ext_gold_finger

# Reload all plugins
uv run python cli.py skills reload

# Generate a plugin using natural language!
uv run python cli.py skills build "Help me write a Skill that injects 'The protagonist is very handsome' before every generation"
```

---

## 🔌 V3 Plugin Ecosystem

The essence of the V3 engine lies in its endless functional extensibility. All extensions are called `Skills` and must inherit from the `BaseSkill` base class. Plugins are dynamically managed by the `PluginManager` and intercept `EventBus` events throughout the lifecycle.

### Quick Start: Creating a Plugin Manually

1. Create a folder in `skills/`, e.g., `skills/my_awesome_skill/`
2. Create `skill.py` inside it:
```python
from core.base_skill import BaseSkill

class MyAwesomeSkill(BaseSkill):
    def __init__(self, context):
        super().__init__(context)
        self.name = "MyAwesomeSkill"

    def on_init(self):
        print(f"[{self.name}] Plugin initialized!")

    def on_before_scene_write(self, prompt_payload, beat_data):
        # Inject custom prompt before each generation
        prompt_payload.append("\n[System Injection] Note: The protagonist's behavior should be cold and rational.")
        return prompt_payload
```
3. Save and run `uv run python cli.py skills reload`.

### Lifecycle Hooks Overview

| Hook Method | Trigger Timing | Purpose |
|---------|---------|------|
| `on_init()` | After plugin load | Initialize resources |
| `on_volume_planning()` | During volume planning | Intervene/Modify outline |
| `on_before_scene_write()` | Before scene generation | Inject memory/settings |
| `on_after_scene_write()` | After scene generation | Statistics/Storage |
| `on_chapter_render()` | Final chapter rendering | Replace placeholders |
| `get_llm_tools()` | LLM tool call cycle | Register tools |

### Active Tool Calling

V3 plugins can not only passively inject context but also provide a **toolbox for active operations** to the AI.

1. `get_llm_tools()`: Returns OpenAI-formatted JSON Schema tool definitions.
2. `execute_tool(tool_name, kwargs)`: Handles the AI's call logic and returns a string result.

**Example: Gold Finger (System Panel)**
Implemented in `skills/ext_gold_finger/`:
- **Passive**: Injects protagonist stats (silver, skills) before each chapter.
- **Active**: Provides the `simplify_skill` tool, allowing the AI to decide to spend "silver" to "simplify" techniques within the story.

### Plugin Toggle Mechanism

The system implements toggle logic by generating a `.disabled` tag file in the plugin folder. Toggle via CLI `skills enable/disable`.

### Auto-Generation (Meta-Generation)
```bash
uv run python cli.py skills build "Write a plugin that checks the rationality of combat descriptions"
```
The system will automatically generate code following the standards and apply it via hot-reload.

---

## 📂 Directory Structure

| Path | Description |
|------|-------------|
| `cli.py` | CLI terminal entry point |
| `world_builder.py` | Worldview initialization engine |
| `volume_planner.py` | Volume outline planning engine |
| `scene_writer.py` | Scene writing and merging engine |
| `core/` | Microkernel core modules |
| `core/agents/` | Agent implementations |
| `skills/` | Plugin directory (drop-in生效) |
| `utils/` | Utilities (LLM client, config, etc.) |
| `docs/CLI_COMMANDS.md` | Complete CLI command documentation |

---

## 📜 Built-in Plugins

| Plugin | Description |
|--------|-------------|
| `ext_gold_finger` | Gold Finger plugin |
| `ext_world_highlight_system` | World highlight system |
| `ext_handsome_protagonist` | Protagonist halo plugin |
| `core_memory_rag` | Core memory RAG system |