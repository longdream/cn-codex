 Plan: Refactor Novel-Claude to adopt NovelForge's novel creation approach      
                                                                                
 Context

 After studying NovelForge's documentation, I identified several key
 improvements for Novel-Claude's novel generation pipeline:

 1. NovelForge's "Snowflake Method" - Hierarchical refinement from one-sentence
  → outline → world → blueprint → volumes → stages → chapters
 2. Card-based data model - Each creation unit is a card with type, schema,
 parent-child relationships
 3. @DSL context injection - Dynamic references to other cards in prompts
 4. Workflow automation - Cards auto-trigger downstream creation on save
 5. Entity state tracking - Characters/scenes/organizations have dynamic_state
 6. Instruction flow generation - Streaming output with JSON Schema validation
 7. Stage-level planning - Chapters grouped into stages with rhythm control

 Novel-Claude currently uses a simpler pipeline: init → plan (macro) → plan
 (micro beats) → write. We need to adopt NovelForge's layered approach.

 ---
 Implementation Plan

 Phase 1: Enhance Schema Design

 New file: schemas/ directory

 Create structured Pydantic schemas inspired by NovelForge's card system:

 ┌────────────────────┬─────────────────────────────────────────────────────┐
 │       Schema       │                     Description                     │
 ├────────────────────┼─────────────────────────────────────────────────────┤
 │ goldfinger.py      │ 金手指 - protagonist's special abilities            │
 ├────────────────────┼─────────────────────────────────────────────────────┤
 │ one_sentence.py    │ 一句话梗概 - core story hook                        │
 ├────────────────────┼─────────────────────────────────────────────────────┤
 │ story_outline.py   │ 故事大纲 - paragraph overview                       │
 ├────────────────────┼─────────────────────────────────────────────────────┤
 │ world_setting.py   │ 世界观设定 - world rules                            │
 ├────────────────────┼─────────────────────────────────────────────────────┤
 │ core_blueprint.py  │ 核心蓝图 - characters, scenes, organizations,       │
 │                    │ volume count                                        │
 ├────────────────────┼─────────────────────────────────────────────────────┤
 │ volume_outline.py  │ 分卷大纲 - volume main/branch targets, stage count  │
 ├────────────────────┼─────────────────────────────────────────────────────┤
 │ stage_outline.py   │ 阶段大纲 - stage-level story with chapter outlines  │
 ├────────────────────┼─────────────────────────────────────────────────────┤
 │ chapter_outline.py │ 章节大纲 - chapter title, overview, entity_list     │
 ├────────────────────┼─────────────────────────────────────────────────────┤
 │ chapter_content.py │ 章节正文 - actual chapter text                      │
 └────────────────────┴─────────────────────────────────────────────────────┘

 Each schema includes thinking field for AI design reasoning, and entity_list
 for tracking participants.

 Phase 2: Implement @DSL Context Injection

 New file: core/context_assembler.py

 def assemble_context(card_type: str, current_card: dict, all_cards: dict) ->
 str

 Support syntax:
 - @卡片标题 - reference by title
 - @type:角色卡 - reference all cards of type
 - @type:角色卡[previous] - sibling reference
 - @parent - parent card
 - @self - current card
 - @type:角色卡[filter:content.name in $self.content.entity_list] - filtered
 reference

 Phase 3: Refactor world_builder.py

 Modify: world_builder.py

 Current: generates 4 flat JSON files (factions, power_levels, characters,
 world_rules)

 New approach (following NovelForge's one-sentence → outline → world →
 blueprint flow):

 1. init - Generate one_sentence (one-sentence story hook)
 2. expand - Generate story_outline (paragraph overview)
 3. world - Generate world_setting (rules, geography, factions)
 4. blueprint - Generate core_blueprint (characters, scenes, organizations,
 volume_count)

 Each step uses @DSL to inject context from previous steps.

 Phase 4: Improve volume_planner.py

 Modify: volume_planner.py

 Current: generates 50 chapters per volume with flat beats

 New approach (following NovelForge's volume → stage → chapter hierarchy):

 1. plan - Generate 10 volume outlines with stage_count
 2. plan --volume N - Instead of 50 chapters, generate N stages with rhythm
 control
 3. Each stage generates chapter outlines (not full beats)

 Key improvement: Stage-level rhythm control from NovelForge:
 - Stage 1: Setup (main line only starts, 1 subplot foreshadow)
 - Stage 2: First push (surface progress but bigger resistance)
 - Stage 3: Mid-section (risk escalation, ≤50% main line completion)
 - Stage 4: Mid-point转折 (new resistance, still can't resolve main conflict)
 - Stage 5: Crisis (core resources limited)
 - Stage 6: Climax & resolution (within volume level, not final resolution)

 Phase 5: Add stage-level workflow

 New file: core/stage_workflow.py

 Triggered when volume outline is saved:
 1. Auto-create stage_outline cards based on stage_count
 2. Each stage contains chapter_outline_list
 3. Stage saves trigger chapter outline and chapter content cards

 Phase 6: Improve scene_writer.py

 Modify: scene_writer.py

 Current: generates scenes directly from beats

 New approach:
 1. Generate chapter content from chapter_outline
 2. Use @DSL to inject: world_setting, organization cards, scene cards,
 character cards, previous chapter content, writing guide
 3. Add continuation support with word count control

 Phase 7: Entity State Tracking

 New file: core/entity_tracker.py

 Track dynamic_state for entities:
 - Character cards: power changes, relationship changes
 - Scene cards: state changes
 - Organization cards: membership changes

 After each chapter:
 1. Extract entity state changes via LLM
 2. Update entity cards
 3. Next chapter auto-injects updated states

 Phase 8: Audit/Review Mechanism

 Modify: review command

 Add stage audit prompts:
 - stage_audit.txt - review stage outline consistency
 - chapter_audit.txt - review chapter content consistency

 ---
 File Changes Summary

 ┌───────────────────────────┬──────────────────────────────────────────────┐
 │           File            │                    Action                    │
 ├───────────────────────────┼──────────────────────────────────────────────┤
 │ schemas/                  │ New - Pydantic schemas for all card types    │
 ├───────────────────────────┼──────────────────────────────────────────────┤
 │ core/context_assembler.py │ New - @DSL context injection                 │
 ├───────────────────────────┼──────────────────────────────────────────────┤
 │ core/stage_workflow.py    │ New - volume→stage→chapter workflow          │
 ├───────────────────────────┼──────────────────────────────────────────────┤
 │ core/entity_tracker.py    │ New - entity state tracking                  │
 ├───────────────────────────┼──────────────────────────────────────────────┤
 │ world_builder.py          │ Modify - implement 4-step creation flow      │
 ├───────────────────────────┼──────────────────────────────────────────────┤
 │ volume_planner.py         │ Modify - add stage-level planning with       │
 │                           │ rhythm                                       │
 ├───────────────────────────┼──────────────────────────────────────────────┤
 │ scene_writer.py           │ Modify - use @DSL context, add continuation  │
 ├───────────────────────────┼──────────────────────────────────────────────┤
 │ prompts/                  │ New - structured prompts with @DSL syntax    │
 └───────────────────────────┴──────────────────────────────────────────────┘

 ---
 Verification

 1. uv run python cli.py init "穿越到修仙世界的工程师" generates one_sentence
 2. uv run python cli.py expand generates story_outline
 3. uv run python cli.py world generates world_setting
 4. uv run python cli.py blueprint generates core_blueprint with
 characters/scenes
 5. uv run python cli.py plan generates 10 volume outlines with stage_count
 6. uv run python cli.py plan --volume 1 generates stage outlines (not 50
 chapters directly)
 7. Each stage has chapter_outlines that trigger chapter_content creation
 8. Entity states are tracked and updated after each chapter
