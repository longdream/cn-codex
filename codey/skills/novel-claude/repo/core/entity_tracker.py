"""
Entity Tracker - Dynamic state tracking for characters, scenes, and organizations

Tracks state changes for entities over the course of the novel:
- Character cards: power changes, relationship changes
- Scene cards: state changes
- Organization cards: membership changes

After each chapter:
1. Extract entity state changes via LLM
2. Update entity cards
3. Next chapter auto-injects updated states
"""

import json
from pathlib import Path
from typing import List, Dict, Any, Optional
from pydantic import BaseModel
from utils.config import SETTINGS_DIR, VOLUMES_DIR, MANUSCRIPTS_DIR
from utils.llm_client import generate_json


# ============================================================================
# Schema Definitions
# ============================================================================

class EntityStateChange(BaseModel):
    """Schema for a single entity state change."""
    entity_name: str
    entity_type: str  # character / scene / organization
    change_type: str  # power_up / power_down / relationship_change / state_change / new_member / removed_member
    description: str
    before_state: str = ""
    after_state: str = ""


class EntityStateSnapshotSchema(BaseModel):
    """Schema for entity state changes after a chapter."""
    thinking: Optional[str] = None
    chapter_id: int
    volume_id: int
    character_changes: List[EntityStateChange] = []
    scene_changes: List[EntityStateChange] = []
    organization_changes: List[EntityStateChange] = []


# ============================================================================
# Entity Tracking
# ============================================================================

def load_core_blueprint() -> dict:
    """Load core blueprint for entity information."""
    path = Path(SETTINGS_DIR) / "core_blueprint.json"
    if path.exists():
        with open(path, 'r', encoding='utf-8') as f:
            return json.load(f)
    return {}


def load_entity_cards(entity_names: List[str]) -> Dict[str, List[dict]]:
    """Load entity cards (character, scene, organization) matching the given names."""
    result = {
        "characters": [],
        "scenes": [],
        "organizations": []
    }

    blueprint = load_core_blueprint()
    content = blueprint.get("content", blueprint)
    entity_name_set = set(entity_names)

    for char in content.get("character_cards", []):
        if char.get("name") in entity_name_set:
            result["characters"].append(char)

    for scene in content.get("scene_cards", []):
        if scene.get("name") in entity_name_set:
            result["scenes"].append(scene)

    for org in content.get("organization_cards", []):
        if org.get("name") in entity_name_set:
            result["organizations"].append(org)

    return result


def load_current_entity_states() -> dict:
    """Load current dynamic_state for all entities from core_blueprint."""
    blueprint = load_core_blueprint()
    content = blueprint.get("content", blueprint)

    states = {
        "characters": {},
        "scenes": {},
        "organizations": {}
    }

    for char in content.get("character_cards", []):
        states["characters"][char.get("name", "")] = {
            "dynamic_state": char.get("dynamic_state", ""),
            "dynamic_info": char.get("dynamic_info", "")
        }

    for scene in content.get("scene_cards", []):
        states["scenes"][scene.get("name", "")] = {
            "dynamic_state": scene.get("dynamic_state", "")
        }

    for org in content.get("organization_cards", []):
        states["organizations"][org.get("name", "")] = {
            "dynamic_state": org.get("dynamic_state", "")
        }

    return states


def save_entity_states(states: dict):
    """Save updated entity states back to core_blueprint.json."""
    blueprint_path = Path(SETTINGS_DIR) / "core_blueprint.json"
    if not blueprint_path.exists():
        return False

    with open(blueprint_path, 'r', encoding='utf-8') as f:
        blueprint = json.load(f)

    content = blueprint.get("content", blueprint)

    # Update character states
    for char in content.get("character_cards", []):
        name = char.get("name", "")
        if name in states.get("characters", {}):
            char["dynamic_state"] = states["characters"][name].get("dynamic_state", "")
            char["dynamic_info"] = states["characters"][name].get("dynamic_info", "")

    # Update scene states
    for scene in content.get("scene_cards", []):
        name = scene.get("name", "")
        if name in states.get("scenes", {}):
            scene["dynamic_state"] = states["scenes"][name].get("dynamic_state", "")

    # Update organization states
    for org in content.get("organization_cards", []):
        name = org.get("name", "")
        if name in states.get("organizations", {}):
            org["dynamic_state"] = states["organizations"][name].get("dynamic_state", "")

    with open(blueprint_path, 'w', encoding='utf-8') as f:
        json.dump(blueprint, f, ensure_ascii=False, indent=2)

    return True


def extract_chapter_entities(chapter_path: Path) -> dict:
    """Extract entity mentions and states from chapter content."""
    if not chapter_path.exists():
        return {"characters": [], "scenes": [], "organizations": []}

    with open(chapter_path, 'r', encoding='utf-8') as f:
        content = f.read()

    # Load entity names for matching
    blueprint = load_core_blueprint()
    content_data = blueprint.get("content", blueprint)

    entity_names = {
        "characters": [c.get("name") for c in content_data.get("character_cards", [])],
        "scenes": [s.get("name") for s in content_data.get("scene_cards", [])],
        "organizations": [o.get("name") for o in content_data.get("organization_cards", [])]
    }

    # Simple keyword matching - in production, use NER
    mentioned = {"characters": [], "scenes": [], "organizations": []}

    for name in entity_names["characters"]:
        if name in content:
            mentioned["characters"].append(name)

    for name in entity_names["scenes"]:
        if name in content:
            mentioned["scenes"].append(name)

    for name in entity_names["organizations"]:
        if name in content:
            mentioned["organizations"].append(name)

    return mentioned


def analyze_entity_changes(volume_id: int, chapter_id: int, chapter_content: str,
                          entity_list: List[str]) -> dict:
    """
    Analyze chapter content to extract entity state changes.
    Uses LLM to identify changes in character states, relationships, etc.
    """
    # Get current states
    current_states = load_current_entity_states()
    entities = load_entity_cards(entity_list)

    prompt = f"""你是网络小说编辑。请分析以下章节内容，提取实体状态变化。

【章节内容】:
{chapter_content[-3000:]}

【当前实体状态】:
角色：
{json.dumps(current_states.get("characters", {}), ensure_ascii=False, indent=2)}

场景：
{json.dumps(current_states.get("scenes", {}), ensure_ascii=False, indent=2)}

组织：
{json.dumps(current_states.get("organizations", {}), ensure_ascii=False, indent=2)}

【参与本章的实体】:
{json.dumps(entities, ensure_ascii=False, indent=2)}

请识别并输出：
1. 角色变化（战力提升/下降、关系变化、状态变化）
2. 场景变化（状态改变）
3. 组织变化（成员变化）

只输出有变化的实体，无变化则不输出。
"""

    schema_model = EntityStateSnapshotSchema
    result = generate_json(prompt, schema_model)

    if hasattr(result, 'model_dump'):
        result_dict = result.model_dump()
    else:
        result_dict = result if isinstance(result, dict) else {}

    # Inject volume_id and chapter_id into result
    result_dict["chapter_id"] = chapter_id
    result_dict["volume_id"] = volume_id
    return result_dict


def apply_entity_changes(volume_id: int, chapter_id: int, changes: dict):
    """
    Apply entity state changes to core_blueprint.json.
    Called after chapter content is generated.
    """
    if not changes:
        return

    current_states = load_current_entity_states()

    # Apply character changes
    for change in changes.get("character_changes", []):
        name = change.get("entity_name", "")
        if name in current_states["characters"]:
            # Update dynamic_state with new change
            current_desc = current_states["characters"][name].get("dynamic_info", "")
            new_change = f"[第{chapter_id}章] {change.get('description', '')}"
            if current_desc:
                current_states["characters"][name]["dynamic_info"] = current_desc + "\n" + new_change
            else:
                current_states["characters"][name]["dynamic_info"] = new_change

            # Update dynamic_state
            if change.get("after_state"):
                current_states["characters"][name]["dynamic_state"] = change.get("after_state", "")

    # Apply scene changes
    for change in changes.get("scene_changes", []):
        name = change.get("entity_name", "")
        if name in current_states["scenes"]:
            if change.get("after_state"):
                current_states["scenes"][name]["dynamic_state"] = change.get("after_state", "")

    # Apply organization changes
    for change in changes.get("organization_changes", []):
        name = change.get("entity_name", "")
        if name in current_states["organizations"]:
            if change.get("after_state"):
                current_states["organizations"][name]["dynamic_state"] = change.get("after_state", "")

    # Save updated states
    save_entity_states(current_states)
    print(f"[✓] 第 {chapter_id} 章实体状态已更新")


def track_chapter_entities(volume_id: int, chapter_id: int):
    """
    Track entity states for a chapter that was just written.
    Load chapter content, analyze changes, update entity cards.
    """
    chapter_path = Path(MANUSCRIPTS_DIR) / f"vol_{volume_id:02d}" / f"ch_{chapter_id:03d}_final.md"
    if not chapter_path.exists():
        print(f"[WARN] 找不到章节文件: {chapter_path}")
        return

    # Extract entities mentioned in chapter
    mentioned = extract_chapter_entities(chapter_path)

    if not any(mentioned.values()):
        print(f"[INFO] 第 {chapter_id} 章无实体变化可追踪")
        return

    # Load chapter content
    with open(chapter_path, 'r', encoding='utf-8') as f:
        content = f.read()

    # Analyze changes
    entity_list = (mentioned["characters"] + mentioned["scenes"] + mentioned["organizations"])
    changes = analyze_entity_changes(volume_id, chapter_id, content, entity_list)

    # Apply changes
    apply_entity_changes(volume_id, chapter_id, changes)


def get_entity_state_for_context(entity_name: str) -> Optional[str]:
    """Get current dynamic_state for a specific entity, for context injection."""
    states = load_current_entity_states()

    for char_name, state in states["characters"].items():
        if char_name == entity_name:
            return state.get("dynamic_state", "")

    for scene_name, state in states["scenes"].items():
        if scene_name == entity_name:
            return state.get("dynamic_state", "")

    for org_name, state in states["organizations"].items():
        if org_name == entity_name:
            return state.get("dynamic_state", "")

    return None


def get_all_updated_entities() -> dict:
    """Get all entities with their current dynamic_state for context injection."""
    return load_current_entity_states()
