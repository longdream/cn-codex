"""
Volume Planner - Stage-level planning with rhythm control
"""

import os
import json
from pathlib import Path
from typing import List, Optional, Dict, Any
from pydantic import BaseModel, Field
from utils.config import SETTINGS_DIR, VOLUMES_DIR, MANUSCRIPTS_DIR
from utils.llm_client import generate_json
from core.context_assembler import assemble_context

# ============================================================================
# Schema Definitions
# ============================================================================

class VolumeOutlineSchema(BaseModel):
    volume_id: int
    volume_name: str
    word_count_target: int = 180000
    stage_count: int = 6
    main_target: str
    branch_line: str = ""
    power_level_cap: str
    new_character_cards: List[Dict] = Field(default_factory=list)
    new_scene_cards: List[Dict] = Field(default_factory=list)
    entity_action_list: str = ""


class VolumeOutlinesSchema(BaseModel):
    volumes: List[VolumeOutlineSchema]


class ChapterOutlineSchema(BaseModel):
    chapter_number: int
    title: str
    overview: str
    entity_list: List[str] = Field(default_factory=list)


class StageOutlineSchema(BaseModel):
    stage_number: int
    stage_name: str
    reference_chapter: List[int]
    analysis: str
    overview: str
    entity_snapshot: str
    chapter_outline_list: List[ChapterOutlineSchema]
    stage_goal: str = ""
    main_line_progress: str = ""
    subplot_insert: str = ""
    conflict_point: str = ""
    suspense_hook: str = ""


class VolumeStagesSchema(BaseModel):
    volume_id: int
    volume_name: str
    stages: List[StageOutlineSchema]


# ============================================================================
# Context Gathering
# ============================================================================

def get_world_context() -> str:
    context = []
    for card_type in ["one_sentence", "story_outline", "world_setting", "core_blueprint"]:
        path = Path(SETTINGS_DIR) / f"{card_type}.json"
        if path.exists():
            with open(path, 'r', encoding='utf-8') as f:
                data = json.load(f)
                content = data.get("content", data)
                context.append(f"### {card_type}\n{json.dumps(content, ensure_ascii=False)}")
    return "\n".join(context)


def get_core_blueprint() -> dict:
    path = Path(SETTINGS_DIR) / "core_blueprint.json"
    if path.exists():
        with open(path, 'r', encoding='utf-8') as f:
            return json.load(f)
    return {}


# ============================================================================
# Macro Planning
# ============================================================================

def plan_macro_outlines(total_volumes: int = 10):
    print(f"[INFO] 正在生成 {total_volumes} 卷宏观大纲...")
    blueprint = get_core_blueprint()
    world_context = get_world_context()
    
    prompt = f"""你是顶尖网络小说架构师。请根据以下全局设定，规划 {total_volumes} 卷的核心大纲。
确保战力递进合理，不崩坏。每卷字数目标约 180000 字（30章，每章6000字）。

【世界观与设定】:
{world_context}

【核心蓝图】:
卷数: {blueprint.get('content', {}).get('volume_count', total_volumes)}

请为每卷输出：卷号、卷名、核心冲突、战力天花板、阶段数量（6个阶段）、主线目标、辅线。"""
    
    schema_model = VolumeOutlinesSchema
    data = generate_json(prompt, schema_model)
    data_dict = data if isinstance(data, dict) else data.model_dump()
    data_list = [data_dict]
    data_list = event_bus_emit_pipeline("on_volume_planning", data_list)
    data_dict = data_list[0] if data_list else data_dict
    
    for vol in data_dict.get("volumes", []):
        vol_id = vol["volume_id"]
        path = Path(VOLUMES_DIR) / f"vol_{vol_id:02d}_outline.json"
        path.parent.mkdir(parents=True, exist_ok=True)
        with open(path, 'w', encoding='utf-8') as f:
            json.dump(vol, f, ensure_ascii=False, indent=2)
    print(f"[✓] {len(data_dict.get('volumes', []))} 卷宏观大纲已生成并落盘。")


def event_bus_emit_pipeline(event_name: str, initial_data: Any, *args, **kwargs) -> Any:
    from core.event_bus import event_bus
    return event_bus.emit_pipeline(event_name, initial_data, *args, **kwargs)


# ============================================================================
# Micro Planning (plan --volume N) - 每卷30章 = 6阶段 × 5章/阶段
# ============================================================================

def plan_volume_stages(volume_id: int):
    print(f"[INFO] 启动分卷调度器，目标：第 {volume_id} 卷阶段大纲...")
    
    vol_path = Path(VOLUMES_DIR) / f"vol_{volume_id:02d}_outline.json"
    if not vol_path.exists():
        print(f"[ERROR] 找不到卷 {volume_id} 的大纲，请先执行宏观规划！")
        return False
    
    with open(vol_path, 'r', encoding='utf-8') as f:
        vol_outline = json.load(f)
    
    world_context = get_world_context()
    blueprint = get_core_blueprint()
    content = blueprint.get("content", blueprint)
    characters = content.get("character_cards", [])
    scenes = content.get("scene_cards", [])
    organizations = content.get("organization_cards", [])
    
    stage_count = vol_outline.get("stage_count", 6)
    
    # Calculate chapter start for this volume (each volume = 30 chapters)
    start_ch = (volume_id - 1) * 30 + 1
    end_ch = volume_id * 30
    
    prompt = f"""你是网文白金主编。当前任务是为第 {volume_id} 卷设计 {stage_count} 个阶段的细纲。
本卷共30章（第{start_ch}-{end_ch}章），6个阶段，每阶段5章。

【卷信息】:
- 卷名：{vol_outline.get('volume_name', '')}
- 主线目标：{vol_outline.get('main_target', '')}
- 辅线：{vol_outline.get('branch_line', '')}
- 战力天花板：{vol_outline.get('power_level_cap', '')}

【全局设定参考】:
{world_context}

【角色卡】:
{json.dumps(characters, ensure_ascii=False, indent=2)}

【场景卡】:
{json.dumps(scenes, ensure_ascii=False, indent=2)}

【组织卡】:
{json.dumps(organizations, ensure_ascii=False, indent=2)}

【节奏控制要求】（强约束）:
- 本卷共 {stage_count} 个阶段
- 每个阶段5章，每章6000字左右
- 阶段1（第{start_ch}-{start_ch+4}章）：开端/铺垫/诱发事件（主线仅启动，不要解决核心矛盾）
- 阶段2（第{start_ch+5}-{start_ch+9}章）：第一次推进（表面进展但引出更大阻力）
- 阶段3（第{start_ch+10}-{start_ch+14}章）：中段推进（风险升级，主线达成度≤50%）
- 阶段4（第{start_ch+15}-{start_ch+19}章）：中点/重大转折（新的阻力来源）
- 阶段5（第{start_ch+20}-{start_ch+24}章）：危机/失利（核心资源受限）
- 阶段6（第{start_ch+25}-{start_ch+29}章）：卷内高潮与阶段性收束（保留更高层悬念）

每个阶段必须包含：
1. 阶段目标（面向问题的陈述）
2. 主线推进点（≤该阶段允许的推进幅度）
3. 辅线穿插点
4. 冲突与反转
5. 悬念钩子（跨到下一阶段的问题）
6. 参与的实体列表
7. 该阶段5个章节的详细大纲（每章需包含章节号、标题、500字概述、参与者实体列表）

章节号从 {start_ch} 到 {end_ch} 连续递增。"""
    
    schema_model = VolumeStagesSchema
    data = generate_json(prompt, schema_model)
    data_dict = data if isinstance(data, dict) else data.model_dump()
    
    vol_stages_dir = Path(VOLUMES_DIR) / f"vol_{volume_id:02d}_stages"
    vol_stages_dir.mkdir(parents=True, exist_ok=True)
    
    total_chapters = 0
    for stage in data_dict.get("stages", []):
        stage_num = stage["stage_number"]
        stage_path = vol_stages_dir / f"stage_{stage_num:02d}.json"
        with open(stage_path, 'w', encoding='utf-8') as f:
            json.dump(stage, f, ensure_ascii=False, indent=2)
        
        chapter_dir = Path(VOLUMES_DIR) / f"vol_{volume_id:02d}_chapters"
        chapter_dir.mkdir(parents=True, exist_ok=True)
        
        for ch_outline in stage.get("chapter_outline_list", []):
            ch_num = ch_outline["chapter_number"]
            ch_path = chapter_dir / f"ch_{ch_num:03d}_outline.json"
            with open(ch_path, 'w', encoding='utf-8') as f:
                json.dump(ch_outline, f, ensure_ascii=False, indent=2)
            total_chapters += 1
    
    print(f"[✓] 第 {volume_id} 卷 {stage_count} 个阶段、{total_chapters} 章大纲已生成并落盘。")
    return True


def run_volume_planner(volume_id: int = None):
    if volume_id is None:
        plan_macro_outlines()
    else:
        plan_volume_stages(volume_id)