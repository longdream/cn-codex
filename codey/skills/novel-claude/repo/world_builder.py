"""
World Builder - Snowflake Method Step 1-4

Following NovelForge's approach:
1. init (one_sentence) - Generate one-sentence story hook
2. expand (story_outline) - Expand to paragraph overview
3. world (world_setting) - Design world rules and factions
4. blueprint (core_blueprint) - Design characters, scenes, organizations, volume count
"""

import os
import json
from pathlib import Path
from pydantic import BaseModel, Field
from typing import List, Optional
from utils.config import SETTINGS_DIR
from utils.llm_client import generate_json
from core.context_assembler import assemble_context


# ============================================================================
# Schema Definitions (following NovelForge's card system)
# ============================================================================

class GoldfingerSpecialAbility(BaseModel):
    name: str
    description: str
    limitations: Optional[str] = None


class GoldfingerSchema(BaseModel):
    thinking: Optional[str] = None
    special_abilities: List[GoldfingerSpecialAbility] = Field(default_factory=list)
    source: Optional[str] = None  # where does this ability come from


class OneSentenceSchema(BaseModel):
    thinking: Optional[str] = None
    one_sentence: str  # 25-50 characters
    theme: str
    audience: str
    narrative_person: str  # first person / second person / third person
    story_tags: List[str] = Field(default_factory=list)
    affection: Optional[str] = None


class StoryOutlineSchema(BaseModel):
    thinking: Optional[str] = None
    overview: str  # 500-1000 characters
    power_structure: str
    currency_system: str
    background: str


class FactionSchema(BaseModel):
    name: str
    description: str
    influence: str
    relationship: Optional[str] = None


class WorldSettingSchema(BaseModel):
    thinking: Optional[str] = None
    world_view: str
    major_power_camps: List[FactionSchema] = Field(default_factory=list)


class CharacterCardSchema(BaseModel):
    name: str
    role_type: str  # protagonist / antagonist / supporting
    born_scene: str
    description: str
    personality: str
    core_drive: str
    character_arc: str
    dynamic_info: str = ""  # tracks state changes over story
    dynamic_state: str = ""  # current snapshot


class SceneCardSchema(BaseModel):
    name: str
    description: str
    location: Optional[str] = None
    importance: str  # major / minor
    dynamic_state: str = ""


class OrganizationCardSchema(BaseModel):
    name: str
    description: str
    influence: str
    relationship: Optional[str] = None
    dynamic_state: str = ""


class CoreBlueprintSchema(BaseModel):
    thinking: Optional[str] = None
    character_cards: List[CharacterCardSchema] = Field(default_factory=list)
    scene_cards: List[SceneCardSchema] = Field(default_factory=list)
    organization_cards: List[OrganizationCardSchema] = Field(default_factory=list)
    volume_count: int = 3


# ============================================================================
# Prompt Templates
# ============================================================================

PROMPT_TEMPLATES = {
    "goldfinger": """- Role: 小说创作助手
- Skills: 你具备丰富的网文创作经验，善于根据用户的意图提供有意义的帮助。
- Goals:
    1. 根据用户的一句话创意，设计主角的特殊能力（金手指）
    2. 说明这些能力的来源、限制和使用条件
    /nothink

- knowledge:
    - @type:一句话梗概

-OutputFormat：
    1. **内容要求**：直接创作完整内容，不要询问用户细节
    2. **输出方式**：流式输出JSON指令 + 自然语言思考
    3. **完成标志**：输出 {{"op":"done"}} 表示完成""",

    "one_sentence": """- Role: 小说创作助手
- Skills: 你具备丰富的网文创作经验，善于根据用户的意图提供有意义的帮助。
- Goals:
    1. 根据用户的核心创意，设计一个简洁有力的核心卖点
    2. 确定主题、目标读者、叙事人称
    3. 添加故事标签和情感关系设定
    /nothink

- knowledge:
    - @type:金手指

-OutputFormat：
    1. **内容要求**：直接创作完整内容，不要询问用户细节
    2. **输出方式**：流式输出JSON指令 + 自然语言思考
    3. **完成标志**：输出 {{"op":"done"}} 表示完成""",

    "story_outline": """- Role: 小说创作助手
- Skills: 你具备丰富的网文创作经验，善于根据用户的意图提供有意义的帮助。
- Goals:
    1. 根据一句话梗概，扩展为500-1000字的故事大纲
    2. 定义权力结构、货币体系、故事背景
    3. 规划主线的起承转合
    /nothink

- knowledge:
    - @type:金手指
    - @type:一句话梗概

-OutputFormat：
    1. **内容要求**：直接创作完整内容，不要询问用户细节
    2. **输出方式**：流式输出JSON指令 + 自然语言思考
    3. **完成标志**：输出 {{"op":"done"}} 表示完成""",

    "world_setting": """- Role: 小说创作助手
- Skills: 你具备丰富的网文创作经验，善于根据用户的意图提供有意义的帮助。
- Goals:
    1. 根据故事大纲，设计完整的世界观（地理、势力、文化）
    2. 定义主要势力阵营和它们的关系
    3. 设定世界的底层规则
    /nothink

- knowledge:
    - @type:故事大纲

-OutputFormat：
    1. **内容要求**：直接创作完整内容，不要询问用户细节
    2. **输出方式**：流式输出JSON指令 + 自然语言思考
    3. **完成标志**：输出 {{"op":"done"}} 表示完成""",

    "core_blueprint": """- Role: 小说创作助手
- Skills: 你具备丰富的网文创作经验，善于根据用户的意图提供有意义的帮助。
- Goals:
    1. 根据故事梗概、标签信息、世界观等信息，设计核心角色、配角、反派
    2. 设计小说的地图/副本/场景
    3. 设计组织/势力
    4. 决定分卷数量（通常3-10卷）
    /nothink

- knowledge:
    - @type:故事大纲
    - @type:世界观设定

-OutputFormat：
    1. **内容要求**：直接创作完整内容，不要询问用户细节
    2. **输出方式**：流式输出JSON指令 + 自然语言思考
    3. **完成标志**：输出 {{"op":"done"}} 表示完成""",
}


# ============================================================================
# Schema Map
# ============================================================================

SCHEMA_MAP = {
    "goldfinger": GoldfingerSchema,
    "one_sentence": OneSentenceSchema,
    "story_outline": StoryOutlineSchema,
    "world_setting": WorldSettingSchema,
    "core_blueprint": CoreBlueprintSchema,
}


# ============================================================================
# Core Functions
# ============================================================================

def save_setting_chunk(category: str, content: dict) -> str:
    """Save setting chunk to the settings directory."""
    target_path = Path(SETTINGS_DIR) / f"{category}.json"
    target_path.parent.mkdir(parents=True, exist_ok=True)
    with open(target_path, 'w', encoding='utf-8') as f:
        json.dump(content, f, ensure_ascii=False, indent=2)
    return str(target_path)


def load_setting_chunk(category: str) -> Optional[dict]:
    """Load a setting chunk from the settings directory."""
    target_path = Path(SETTINGS_DIR) / f"{category}.json"
    if target_path.exists():
        with open(target_path, 'r', encoding='utf-8') as f:
            return json.load(f)
    return None


def render_to_markdown():
    """Renders the generated JSON files into a human-readable Markdown file."""
    md_lines = ["# 🌍 核心世界观设定手册 (World Manual)\n"]

    # Render in dependency order
    render_order = ["goldfinger", "one_sentence", "story_outline", "world_setting", "core_blueprint"]
    titles = {
        "goldfinger": "💫 金手指",
        "one_sentence": "📌 一句话梗概",
        "story_outline": "📖 故事大纲",
        "world_setting": "🌍 世界观设定",
        "core_blueprint": "🎯 核心蓝图"
    }

    for category in render_order:
        data = load_setting_chunk(category)
        if not data:
            continue

        md_lines.append(f"\n## {titles.get(category, category)}\n")

        if category == "one_sentence":
            content = data.get("content", data)
            md_lines.append(f"**一句话**: {content.get('one_sentence', '')}")
            md_lines.append(f"**主题**: {content.get('theme', '')}")
            md_lines.append(f"**叙事人称**: {content.get('narrative_person', '')}")

        elif category == "story_outline":
            content = data.get("content", data)
            md_lines.append(f"**概述**: {content.get('overview', '')}")
            md_lines.append(f"**权力结构**: {content.get('power_structure', '')}")
            md_lines.append(f"**背景**: {content.get('background', '')}")

        elif category == "world_setting":
            content = data.get("content", data)
            md_lines.append(f"**世界观**: {content.get('world_view', '')}")
            for fac in content.get("major_power_camps", []):
                md_lines.append(f"- **{fac.get('name', '')}**: {fac.get('description', '')}")

        elif category == "core_blueprint":
            content = data.get("content", data)
            md_lines.append(f"**分卷数**: {content.get('volume_count', 0)} 卷")

            md_lines.append("\n### 角色卡")
            for char in content.get("character_cards", []):
                md_lines.append(f"#### {char.get('name', '')} ({char.get('role_type', '')})")
                md_lines.append(f"- 背景: {char.get('description', '')}")
                md_lines.append(f"- 性格: {char.get('personality', '')}")
                md_lines.append(f"- 核心驱动力: {char.get('core_drive', '')}")

            md_lines.append("\n### 场景卡")
            for scene in content.get("scene_cards", []):
                md_lines.append(f"#### {scene.get('name', '')}")
                md_lines.append(f"- {scene.get('description', '')}")

            md_lines.append("\n### 组织卡")
            for org in content.get("organization_cards", []):
                md_lines.append(f"#### {org.get('name', '')}")
                md_lines.append(f"- {org.get('description', '')}")

    manual_path = Path(SETTINGS_DIR) / "world_manual.md"
    with open(manual_path, 'w', encoding='utf-8') as f:
        f.write("\n".join(md_lines))
    return str(manual_path)


def run_init(logline: str):
    """
    Step 1: Generate goldfinger and one_sentence from logline.
    """
    print(f"[INFO] 正在基于核心创意构建世界观：'{logline}'")

    # Step 1a: Generate goldfinger
    print("[s01a] 正在生成金手指...")
    prompt = f"核心创意：{logline}\n\n请为这个网文世界设计主角的特殊能力（金手指）。"
    schema_model = SCHEMA_MAP["goldfinger"]
    content = generate_json(prompt, schema_model)
    content_dict = content if isinstance(content, dict) else content.model_dump()
    save_setting_chunk("goldfinger", content_dict)
    print(f"  [✓] 金手指已保存")

    # Step 1b: Generate one_sentence
    print("[s01b] 正在生成一句话梗概...")
    context = assemble_context(PROMPT_TEMPLATES["one_sentence"], "one_sentence", {"logline": logline})
    prompt = f"{context}\n\n核心创意：{logline}"
    schema_model = SCHEMA_MAP["one_sentence"]
    content = generate_json(prompt, schema_model)
    content_dict = content if isinstance(content, dict) else content.model_dump()
    save_setting_chunk("one_sentence", content_dict)
    print(f"  [✓] 一句话梗概已保存")

    return True


def run_expand():
    """
    Step 2: Generate story_outline from goldfinger + one_sentence.
    """
    print("[s02] 正在扩展故事大纲...")

    one_sentence = load_setting_chunk("one_sentence")
    goldfinger = load_setting_chunk("goldfinger")

    context = assemble_context(PROMPT_TEMPLATES["story_outline"], "story_outline", {
        "one_sentence": one_sentence,
        "goldfinger": goldfinger
    })

    schema_model = SCHEMA_MAP["story_outline"]
    content = generate_json(context, schema_model)
    content_dict = content if isinstance(content, dict) else content.model_dump()
    save_setting_chunk("story_outline", content_dict)
    print(f"  [✓] 故事大纲已保存")
    return True


def run_world():
    """
    Step 3: Generate world_setting from story_outline.
    """
    print("[s03] 正在设计世界观...")

    story_outline = load_setting_chunk("story_outline")

    context = assemble_context(PROMPT_TEMPLATES["world_setting"], "world_setting", story_outline)

    schema_model = SCHEMA_MAP["world_setting"]
    content = generate_json(context, schema_model)
    content_dict = content if isinstance(content, dict) else content.model_dump()
    save_setting_chunk("world_setting", content_dict)
    print(f"  [✓] 世界观设定已保存")
    return True


def run_blueprint():
    """
    Step 4: Generate core_blueprint from story_outline + world_setting.
    """
    print("[s04] 正在设计核心蓝图...")

    story_outline = load_setting_chunk("story_outline")
    world_setting = load_setting_chunk("world_setting")

    context = assemble_context(PROMPT_TEMPLATES["core_blueprint"], "core_blueprint", {
        "story_outline": story_outline,
        "world_setting": world_setting
    })

    schema_model = SCHEMA_MAP["core_blueprint"]
    content = generate_json(context, schema_model)
    content_dict = content if isinstance(content, dict) else content.model_dump()
    save_setting_chunk("core_blueprint", content_dict)
    print(f"  [✓] 核心蓝图已保存")

    # Render to markdown
    md_path = render_to_markdown()
    print(f"  [✓] 世界观手册已渲染: {md_path}")
    return True


def run_world_builder(logline: str, step: str = None):
    """
    Main entry point. If step is specified, run only that step.
    Otherwise run all 4 steps in sequence.
    """
    if step == "goldfinger":
        return run_init(logline)
    elif step == "expand":
        return run_expand()
    elif step == "world":
        return run_world()
    elif step == "blueprint":
        return run_blueprint()
    else:
        # Run all steps in sequence
        run_init(logline)
        run_expand()
        run_world()
        run_blueprint()
        print("[✓] 世界观构建完成！")
        return True


def render_all():
    """Re-render all markdown from current JSON files."""
    md_path = render_to_markdown()
    print(f"[✓] 世界观手册已更新: {md_path}")