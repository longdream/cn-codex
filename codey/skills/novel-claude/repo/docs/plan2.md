# Plan: Integrate NovelForge Workflow + Create Test Novel

## Context

Novel-Claude's pipeline has several issues discovered during exploration:
1. Event bus hooks are broken (on_volume_planning never called properly)
2. Context assembler database never loaded - @DSL syntax incomplete
3. Skills system has issues (ext_gold_finger, ext_handsome_protagonist, ext_world_highlight_system are not well implemented)
4. Entity tracker not integrated into scene_writer

User requirements:
- Remove underperforming skills (ext_gold_finger, ext_handsome_protagonist, ext_world_highlight_system)
- Follow NovelForge's workflow more closely
- Create test novel: "我在妖武乱世当仵作"
- 5 volumes, 100 chapters each (but only write 100 chapters first), 6000 words/chapter

---

## Implementation Plan

### Step 1: Clean Up Skills

**Remove problematic skills:**
- `skills/ext_gold_finger/` - poorly implemented
- `skills/ext_handsome_protagonist/` - poorly implemented
- `skills/ext_world_highlight_system/` - poorly implemented

**Keep:**
- `skills/core_memory_rag/` - RAG memory system (useful for context)

### Step 2: Fix Core Pipeline Issues

**Fix `scene_writer.py`** (line ~178):
```python
# Current (broken):
prompt = event_bus.emit_pipeline("on_before_scene_write", [prompt], beat_data)

# Should be:
prompt_parts = [prompt]
prompt_parts = event_bus.emit_pipeline("on_before_scene_write", prompt_parts, beat_data)
prompt = "\n".join(prompt_parts)
```

**Fix `context_assembler.py`:**
- Remove `_cache.clear()` on every assemble (line 80)
- Add missing type mappings (角色卡 → characters, etc.)
- Implement filter expressions `[filter:...]`
- Load card database on initialization

**Fix `volume_planner.py`:**
- Properly call `on_volume_planning` hook for skills

**Integrate `entity_tracker.py` into scene_writer:**
- Call `track_chapter_entities()` after each chapter is written

### Step 3: Create NovelForge-style Workflow Files

Following NovelForge's workflow triggers:

**`workflows/core_blueprint.wf`** - Triggered when core_blueprint is saved:
1. Read blueprint
2. Generate volume outlines based on volume_count
3. Extract character_cards → create character cards
4. Extract scene_cards → create scene cards

**`workflows/volume_outline.wf`** - Triggered when volume outline is saved:
1. Read volume outline
2. Generate stage outlines based on stage_count
3. Create writing_guide

**`workflows/stage_outline.wf`** - Triggered when stage outline is saved:
1. Read stage outline
2. Generate chapter_outline cards from chapter_outline_list
3. Generate chapter_content (parallel)

### Step 4: Update Prompts for NovelForge Context Injection

Update prompts to use NovelForge's @DSL syntax:
- `@作品标签` - reference work tag
- `@金手指` - goldfinger ability
- `@一句话梗概` - one sentence hook
- `@故事大纲` - story outline
- `@世界观设定` - world setting
- `@type:角色卡[index=filter:...]` - filtered character references
- `@parent` - parent card
- `@self` - current card

### Step 5: Create the Test Novel

**Logline:** "我在妖武乱世当仵作"

**Steps:**
1. `init "我在妖武乱世当仵作"` - Generate goldfinger + one_sentence
2. `expand` - Generate story_outline
3. `world` - Generate world_setting
4. `blueprint` - Generate core_blueprint with characters/scenes/orgs
5. `plan` - Generate 5 volume outlines
6. `plan --volume 1` - Generate stages for volume 1
7. `write --volume 1 --chapters 1-100` - Write first 100 chapters

**Novel Details:**
- Title: 我在妖武乱世当仵作
- Goldfinger: 无字尸书 (验尸系统) - 物理勘验+逻辑闭环触发
- Main character: 楚平, 仵作
- Theme: 妖武乱世, 县衙底层破案, 系统流

---

## File Changes Summary

| File | Action |
|------|--------|
| `skills/ext_gold_finger/` | DELETE |
| `skills/ext_handsome_protagonist/` | DELETE |
| `skills/ext_world_highlight_system/` | DELETE |
| `scene_writer.py` | FIX - event bus usage, integrate entity tracker |
| `volume_planner.py` | FIX - proper hook calls |
| `context_assembler.py` | FIX - caching, type mappings, filter expressions |
| `core/entity_tracker.py` | INTEGRATE - call after chapter write |
| `workflows/` | NEW - NovelForge-style workflow triggers |
| `prompts/` | UPDATE - NovelForge @DSL syntax |

---

## Verification

1. Remove skills: `rm -rf skills/ext_gold_finger skills/ext_handsome_protagonist skills/ext_world_highlight_system`
2. Fix code issues listed above
3. Test with: `uv run python cli.py init "我在妖武乱世当仵作"`
4. Run full pipeline and verify chapters generate correctly
5. Check that entity states are tracked and chapters are ~6000 words

---

## Critical Files to Modify

- `scene_writer.py:178` - Fix event bus call
- `scene_writer.py:226` - Integrate entity tracking
- `volume_planner.py:142` - Fix on_volume_planning hook
- `context_assembler.py:80` - Remove cache clearing
- `context_assembler.py` - Add missing type mappings
