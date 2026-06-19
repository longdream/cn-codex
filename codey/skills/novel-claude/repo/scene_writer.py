"""
Scene Writer - Chapter content generation with @DSL context injection

Following NovelForge's approach:
1. Generate chapter content from chapter_outline (not beats directly)
2. Use @DSL to inject: world_setting, organization cards, scene cards,
   character cards, previous chapter content, writing guide
3. Add continuation support with word count control
4. Progressive saving and state machine management
"""

import os
import json
import re
from pathlib import Path
from typing import List, Optional, Dict, Any, Tuple
from utils.config import SETTINGS_DIR, VOLUMES_DIR, MANUSCRIPTS_DIR, MODEL_ID
from utils.config_loader import get_config
from utils.llm_client import ProgressiveWriter, generate_stream
from core.context_assembler import assemble_context, get_assembler
from core.event_bus import event_bus
from utils.chapter_state import get_state_manager, STATE_PENDING, STATE_GENERATING, STATE_COMPLETED, STATE_FAILED


# ============================================================================
# Chapter Number Conversion
# ============================================================================
# The volume_planner.py generates outline files using GLOBAL chapter numbers
# (e.g., ch_061_outline.json for Volume 3 Ch 1), but scene_writer operates
# on VOLUME-RELATIVE chapter IDs (1-30). Each volume has 30 chapters.
# This mapping allows both conventions to work seamlessly.

CHARS_PER_VOLUME = 30

def _global_chapter_id(volume_id: int, chapter_id: int) -> int:
    """Convert volume-relative chapter_id to global chapter number.
    
    Example: volume_id=3, chapter_id=1 -> returns 61
             volume_id=3, chapter_id=15 -> returns 75
    """
    return (volume_id - 1) * CHARS_PER_VOLUME + chapter_id


# ============================================================================
# Core Functions
# ============================================================================

def load_chapter_outline(volume_id: int, chapter_id: int) -> Optional[dict]:
    """Load chapter outline from vol_NN_chapters/ch_XXX_outline.json
    
    First tries GLOBAL chapter number (e.g., ch_061 for volume 3, chapter 1),
    then falls back to volume-relative number for compatibility.
    """
    global_id = _global_chapter_id(volume_id, chapter_id)
    path = Path(VOLUMES_DIR) / f"vol_{volume_id:02d}_chapters" / f"ch_{global_id:03d}_outline.json"
    if path.exists():
        with open(path, 'r', encoding='utf-8') as f:
            return json.load(f)
    # Fallback: volume-relative path (for vol 1 or older plans)
    fallback_path = Path(VOLUMES_DIR) / f"vol_{volume_id:02d}_chapters" / f"ch_{chapter_id:03d}_outline.json"
    if fallback_path.exists():
        with open(fallback_path, 'r', encoding='utf-8') as f:
            return json.load(f)
    return None


def load_volume_outline(volume_id: int) -> Optional[dict]:
    """Load volume outline from volumes/vol_XX_outline.json"""
    path = Path(VOLUMES_DIR) / f"vol_{volume_id:02d}_outline.json"
    if path.exists():
        with open(path, 'r', encoding='utf-8') as f:
            return json.load(f)
    return None


def load_previous_chapter(volume_id: int, chapter_id: int) -> Optional[str]:
    """Load previous chapter content for context.
    
    Manuscripts use volume-relative numbering (vol_03/ch_001_final.md),
    so chapter_id is used directly (no global conversion needed).
    """
    if chapter_id <= 1:
        return None
    prev_chars = get_config("writing.previous_chapter_chars", 2000)
    path = Path(MANUSCRIPTS_DIR) / f"vol_{volume_id:02d}" / f"ch_{chapter_id-1:03d}_final.md"
    if path.exists():
        with open(path, 'r', encoding='utf-8') as f:
            content = f.read()
            return content[-prev_chars:]
    return None


def load_history_chapters(volume_id: int, chapter_id: int, count: int = None) -> str:
    """Load multiple previous chapters for deeper context."""
    if count is None:
        count = get_config("writing.history_chapters_count", 3)

    if chapter_id <= count:
        count = chapter_id - 1
    if count <= 0:
        return ""

    history = []
    for i in range(1, count + 1):
        prev_id = chapter_id - i
        if prev_id < 1:
            break
        path = Path(MANUSCRIPTS_DIR) / f"vol_{volume_id:02d}" / f"ch_{prev_id:03d}_final.md"
        if path.exists():
            with open(path, 'r', encoding='utf-8') as f:
                # Get key plot points from each chapter (first 200 and last 500 chars)
                content = f.read()
                first_part = content[:200] if len(content) > 200 else content
                last_part = content[-500:] if len(content) > 500 else content
                history.append(f"=== 第{prev_id}章梗概 ===\n{first_part}\n...（中间内容）...\n{last_part}")

    return "\n\n".join(history)


def load_next_chapter_outline(volume_id: int, chapter_id: int) -> Optional[dict]:
    """Load next chapter outline for continuity check.
    
    Uses GLOBAL chapter number for outline file lookup.
    """
    # Volume-internal next chapter id (max 30)
    next_local = chapter_id + 1
    if next_local > CHARS_PER_VOLUME:
        return None
    global_id = _global_chapter_id(volume_id, next_local)
    path = Path(VOLUMES_DIR) / f"vol_{volume_id:02d}_chapters" / f"ch_{global_id:03d}_outline.json"
    if path.exists():
        with open(path, 'r', encoding='utf-8') as f:
            return json.load(f)
    # Fallback: volume-relative path
    fallback_path = Path(VOLUMES_DIR) / f"vol_{volume_id:02d}_chapters" / f"ch_{next_local:03d}_outline.json"
    if fallback_path.exists():
        with open(fallback_path, 'r', encoding='utf-8') as f:
            return json.load(f)
    return None


def load_entity_cards(entity_names: List[str]) -> Dict[str, List[dict]]:
    """Load entity cards (character, scene, organization) matching the given names."""
    result = {
        "characters": [],
        "scenes": [],
        "organizations": []
    }

    blueprint_path = Path(SETTINGS_DIR) / "core_blueprint.json"
    if not blueprint_path.exists():
        return result

    with open(blueprint_path, 'r', encoding='utf-8') as f:
        blueprint = json.load(f)

    content = blueprint.get("content", blueprint)
    entity_name_set = set(entity_names)

    # Filter characters
    for char in content.get("character_cards", []):
        if char.get("name") in entity_name_set:
            result["characters"].append(char)

    # Filter scenes
    for scene in content.get("scene_cards", []):
        if scene.get("name") in entity_name_set:
            result["scenes"].append(scene)

    # Filter organizations
    for org in content.get("organization_cards", []):
        if org.get("name") in entity_name_set:
            result["organizations"].append(org)

    return result


def load_world_setting() -> dict:
    """Load world setting for context."""
    path = Path(SETTINGS_DIR) / "world_setting.json"
    if path.exists():
        with open(path, 'r', encoding='utf-8') as f:
            return json.load(f)
    return {}


def load_writing_guide(volume_id: int) -> Optional[str]:
    """Load writing guide for the volume if exists."""
    path = Path(VOLUMES_DIR) / f"vol_{volume_id:02d}_writing_guide.json"
    if path.exists():
        with open(path, 'r', encoding='utf-8') as f:
            data = json.load(f)
            return data.get("content", {}).get("content", "")
    return None


def generate_chapter_content(volume_id: int, chapter_id: int, state_manager=None) -> str:
    """
    Generate chapter content from chapter outline using @DSL context injection.
    Supports progressive saving via state_manager.
    """
    print(f"\n[INFO] 正在生成第 {volume_id} 卷第 {chapter_id} 章...")

    # Load chapter outline
    outline = load_chapter_outline(volume_id, chapter_id)
    if not outline:
        print(f"[ERROR] 找不到章节大纲: vol_{volume_id:02d} ch_{chapter_id:03d}")
        return ""

    chapter_title = outline.get("title", f"第{chapter_id}章")
    overview = outline.get("overview", "")
    entity_list = outline.get("entity_list", [])

    print(f"  章节: {chapter_title}")
    print(f"  概述: {overview[:50]}...")
    print(f"  参与者: {', '.join(entity_list)}")

    # Load context entities
    entities = load_entity_cards(entity_list)
    world_setting = load_world_setting()
    prev_chapter = load_previous_chapter(volume_id, chapter_id)
    history_chapters = load_history_chapters(volume_id, chapter_id, count=3)
    next_outline = load_next_chapter_outline(volume_id, chapter_id)
    writing_guide = load_writing_guide(volume_id)

    # Build prompt with @DSL context
    prompt_parts = [
        f"【章节大纲】:\n标题：{chapter_title}\n概述：{overview}\n",
        f"【参与者实体】:\n角色：{json.dumps(entities['characters'], ensure_ascii=False, indent=2)}\n",
        f"【场景】: {json.dumps(entities['scenes'], ensure_ascii=False, indent=2)}\n",
        f"【组织】: {json.dumps(entities['organizations'], ensure_ascii=False, indent=2)}\n",
    ]

    # Add world setting
    if world_setting:
        content = world_setting.get("content", world_setting)
        prompt_parts.append(f"【世界观设定】:\n{content.get('world_view', '')}\n")
        prompt_parts.append(f"【势力】:\n{json.dumps(content.get('major_power_camps', []), ensure_ascii=False, indent=2)}\n")

    # Add history chapters context (last 3 chapters)
    if history_chapters:
        prompt_parts.append(f"【历史章节剧情回顾】:\n{history_chapters}\n")

    # Add previous chapter context (immediate preceding chapter)
    if prev_chapter:
        prompt_parts.append(f"【前章结尾】:\n{prev_chapter}\n")

    # Add next chapter outline for continuity
    if next_outline:
        next_title = next_outline.get("title", "")
        next_overview = next_outline.get("overview", "")
        prompt_parts.append(f"【下一章预告】:\n{next_title}：{next_overview}\n")

    # Add writing guide if available
    if writing_guide:
        prompt_parts.append(f"【卷写作指南】:\n{writing_guide}\n")

    # ── 字数与格式要求 ──
    target_words = get_config("writing.target_word_count", 6000)
    min_words = get_config("writing.min_word_count", 4000)
    max_words = get_config("writing.max_word_count", 8000)
    
    prompt_parts.append(
        f"【字数与格式要求】:\n"
        f"- 目标字数：{target_words} 字左右（请严格控制在 {min_words}-{max_words} 字之间）\n"
        f"- 章节标题：{chapter_title}\n"
        f"- 写作风格：热血玄幻，打斗描写要精彩，节奏紧凑\n"
        f"- 直接输出正文，以 '# {chapter_title}' 开头\n"
        f"- 注意：无需分析或额外的说明文字，直接输出小说正文。\n"
    )

    prompt = "\n".join(prompt_parts)

    # ============================================================
    # Core Generation — ProgressiveWriter with streaming
    # ============================================================
    try:
        writer = ProgressiveWriter(
            prompt=prompt,
            model=MODEL_ID,
            temp=get_config("generation.temperature", 0.85),
        )
        
        content = ""
        progress_chunk_size = get_config("writing.progress_chunk_size", 1000)
        temp_dir = Path(MANUSCRIPTS_DIR) / f"vol_{volume_id:02d}"
        temp_dir.mkdir(parents=True, exist_ok=True)
        temp_path = temp_dir / f"ch_{chapter_id:03d}_temp.md"
        
        for i, chunk in enumerate(writer.generate()):
            if chunk:
                content += chunk
                # Progressive save every N chars
                if len(content) % (progress_chunk_size * 3) < 200:
                    with open(temp_path, 'w', encoding='utf-8') as f:
                        f.write(content)
                    print(f"  [进度] 已生成 {len(content)} 字...")
        
        if content and len(content) > 500:
            print(f"  [完成] 生成 {len(content)} 字")
            return content
        else:
            print(f"[WARN] 生成内容过短 ({len(content)} 字)，尝试重新生成...")
            # Simple fallback: use generate_stream directly
            content = generate_stream(prompt)
            if content and len(content) > 500:
                return content
            return ""
            
    except Exception as e:
        print(f"[ERROR] 生成失败: {e}")
        # Fallback to simple generation
        try:
            content = generate_stream(prompt)
            if content and len(content) > 500:
                return content
        except Exception as e2:
            print(f"[ERROR] 备选生成也失败: {e2}")
        return ""


def save_chapter_content(volume_id: int, chapter_id: int, content: str) -> str:
    """Save chapter content to markdown file."""
    output_dir = Path(MANUSCRIPTS_DIR) / f"vol_{volume_id:02d}"
    output_dir.mkdir(parents=True, exist_ok=True)
    
    output_path = output_dir / f"ch_{chapter_id:03d}_final.md"
    
    # Ensure content starts with a heading
    lines = content.strip().split('\n')
    first_line = lines[0].strip() if lines else ""
    
    # Load the outline to get the title
    outline = load_chapter_outline(volume_id, chapter_id)
    chapter_title = outline.get("title", f"第{chapter_id}章") if outline else f"第{chapter_id}章"
    
    if not first_line.startswith('#') and not first_line.startswith('#'):
        content = f"# {chapter_title}\n\n{content.strip()}"
    
    with open(output_path, 'w', encoding='utf-8') as f:
        f.write(content)
    
    print(f"  [保存] 已保存到: {output_path}")
    return str(output_path)


def run_scene_writer(volume_id: int, start_chapter: int, end_chapter: int):
    """
    Orchestrate multi-intelligent agents for chapter generation.
    
    For each chapter:
    1. Skip if final manuscript exists (from previous run)
    2. Resume from temp file if interruption detected
    3. Generate with progressive saving
    4. Save final manuscript
    5. Update state machine
    6. Emit events for RAG memory injection
    """
    print(f"\n{'='*60}")
    print(f"[INFO] 启动场景子智能体集群，目标：卷 {volume_id} 章 {start_chapter}-{end_chapter}")
    print(f"{'='*60}")
    
    # Load volume outline to get volume name
    volume_outline = load_volume_outline(volume_id)
    volume_name = volume_outline.get("volume_name", f"第{volume_id}卷") if volume_outline else f"第{volume_id}卷"
    print(f"[INFO] 卷名: {volume_name}")
    print(f"[INFO] 预计每章 4000-8000 字，共 {end_chapter - start_chapter + 1} 章")
    
    state_manager = get_state_manager(volume_id)
    
    # Ensure manuscripts directory exists
    vol_dir = Path(MANUSCRIPTS_DIR) / f"vol_{volume_id:02d}"
    vol_dir.mkdir(parents=True, exist_ok=True)
    
    completed = 0
    failed = 0
    skipped = 0
    
    for chapter_id in range(start_chapter, end_chapter + 1):
        state = state_manager.get_state(chapter_id)

        # Skip if already completed
        save_path = vol_dir / f"ch_{chapter_id:03d}_final.md"
        if save_path.exists() and save_path.stat().st_size > 1000:
            print(f"[Skip] 第 {chapter_id} 章已存在，跳过")
            skipped += 1
            continue

        # Check for temp file (resume from interruption)
        temp_path = vol_dir / f"ch_{chapter_id:03d}_temp.md"
        if temp_path.exists():
            print(f"[Resume] 检测到第 {chapter_id} 章的临时文件，将继续生成")
            # Delete temp file to restart fresh
            temp_path.unlink()

        print(f"\n{'='*60}")
        print(f"[INFO] 启动场景子智能体集群，目标：卷 {volume_id} 章 {chapter_id}")
        print(f"{'='*60}")

        # Mark as generating
        state_manager.mark_generating(chapter_id)

        try:
            # Generate chapter content (with progressive saving)
            content = generate_chapter_content(volume_id, chapter_id, state_manager)

            if content:
                # Save chapter
                final_path = save_chapter_content(volume_id, chapter_id, content)

                # Mark as completed
                state_manager.mark_completed(chapter_id)

                # Delete temp file if exists
                if temp_path.exists():
                    temp_path.unlink()

                # Emit after scene write hook (for RAG memory injection)
                beat_data = {"chapter_id": chapter_id, "beats": []}
                event_bus.emit("on_after_scene_write", beat_data, content)

                # Track entity states for this chapter
                from core.entity_tracker import track_chapter_entities
                track_chapter_entities(volume_id, chapter_id)

                completed += 1
            else:
                state_manager.mark_failed(chapter_id, "内容为空")
                failed += 1
        except Exception as e:
            error_msg = str(e)
            print(f"[ERROR] 第 {chapter_id} 章生成失败: {error_msg}")
            state_manager.mark_failed(chapter_id, error_msg)
            failed += 1

    print(f"\n{'='*60}")
    print(f"[INFO] 本批次生成完成：成功 {completed} 章，失败 {failed} 章，跳过 {skipped} 章")
    if failed > 0:
        print(f"[INFO] 可通过重新运行命令继续生成失败的章节")
    print(f"{'='*60}")


# ============================================================================
# Continuation Support (for later use)
# ============================================================================

def continue_chapter(volume_id: int, chapter_id: int, target_words: int = 3000) -> str:
    """
    Continue writing a chapter to reach target word count.
    Used for continuation mode similar to NovelForge's extension feature.
    """
    # Load current chapter content
    path = Path(MANUSCRIPTS_DIR) / f"vol_{volume_id:02d}" / f"ch_{chapter_id:03d}_final.md"
    if not path.exists():
        print(f"[ERROR] 找不到章节文件: {path}")
        return ""

    with open(path, 'r', encoding='utf-8') as f:
        current_content = f.read()

    current_words = len(current_content)
    if current_words >= target_words:
        print(f"[INFO] 章节字数已达标 ({current_words} >= {target_words})")
        return current_content

    remaining = target_words - current_words
    print(f"[INFO] 续写章节，目标增加约 {remaining} 字...")

    # Load chapter outline for context
    outline = load_chapter_outline(volume_id, chapter_id)
    next_outline = load_next_chapter_outline(volume_id, chapter_id)

    # Build continuation prompt
    prompt_parts = [
        f"【当前章节内容】:\n{current_content[-1000:]}\n",
        f"【章节大纲】:\n{outline.get('overview', '')}\n",
    ]

    if next_outline:
        prompt_parts.append(f"【下一章预告】:\n{next_outline.get('title', '')}: {next_outline.get('overview', '')}\n")

    prompt_parts.append(
        f"【续写要求】:\n"
        f"请继续上一段的剧情，续写约 {remaining} 字。\n"
        f"保持与前文的风格和节奏一致。\n"
        f"直接输出续写内容，不要分析。\n"
    )

    prompt = "\n".join(prompt_parts)

    # Generate continuation
    continuation = generate_stream(prompt)

    # Combine and save
    new_content = current_content + "\n\n" + continuation
    save_chapter_content(volume_id, chapter_id, new_content)

    return new_content


# ============================================================================
# Batch Mode (for compatibility)
# ============================================================================

def generate_batch_jsonl(volume_id: int, start_chap: int, end_chap: int, output_jsonl: str):
    """Generate batch JSONL for chapter outlines (not beats)."""
    requests = []

    for chapter_id in range(start_chap, end_chap + 1):
        outline = load_chapter_outline(volume_id, chapter_id)
        if not outline:
            continue

        custom_id = f"v{volume_id:02d}_ch{chapter_id:03d}"

        # Build prompt similar to generate_chapter_content
        prompt_parts = [
            f"【章节大纲】:\n标题：{outline.get('title', '')}\n概述：{outline.get('overview', '')}\n",
        ]

        # Inject entity context
        entity_list = outline.get("entity_list", [])
        entities = load_entity_cards(entity_list)
        if entities["characters"]:
            prompt_parts.append(f"【角色】: {json.dumps(entities['characters'], ensure_ascii=False)}\n")
        if entities["scenes"]:
            prompt_parts.append(f"【场景】: {json.dumps(entities['scenes'], ensure_ascii=False)}\n")

        # Add writing guide
        writing_guide = load_writing_guide(volume_id)
        if writing_guide:
            prompt_parts.append(f"【写作指南】: {writing_guide}\n")

        prompt_parts.append("请根据章节大纲创作正文，约6000字。直接输出正文。")

        request_obj = {
            "custom_id": custom_id,
            "method": "POST",
            "url": "/v4/chat/completions",
            "body": {
                "model": MODEL_ID,
                "messages": [{"role": "user", "content": "\n".join(prompt_parts)}],
                "temperature": 0.85
            }
        }
        requests.append(request_obj)

    with open(output_jsonl, 'w', encoding='utf-8') as f:
        for req in requests:
            f.write(json.dumps(req, ensure_ascii=False) + "\n")

    print(f"[✓] 已生成包含 {len(requests)} 个请求的 Batch 文件: {output_jsonl}")


def process_batch_results(result_jsonl: str):
    """Process batch results and save chapters."""
    if not os.path.exists(result_jsonl):
        print(f"[ERROR] 找不到结果文件: {result_jsonl}")
        return

    chapters_map = {}

    with open(result_jsonl, 'r', encoding='utf-8') as f:
        for line in f:
            data = json.loads(line)
            custom_id = data["custom_id"]
            try:
                content = data["response"]["body"]["choices"][0]["message"]["content"]
            except (KeyError, TypeError):
                content = "（该段场景生成失败）"

            chapters_map[custom_id] = content

    for custom_id, content in chapters_map.items():
        # Parse custom_id: v01_ch001
        parts = custom_id.split("_")
        vol_id = int(parts[0][1:])
        ch_id = int(parts[1][2:])

        save_chapter_content(vol_id, ch_id, content)


def get_world_context() -> str:
    """Get world context for backward compatibility."""
    from core.context_assembler import assemble_context
    path = Path(SETTINGS_DIR) / "world_setting.json"
    if path.exists():
        with open(path, 'r', encoding='utf-8') as f:
            return f.read()
    return ""