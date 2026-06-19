"""
批量写作脚本：基于已有的章节大纲快速生成完整章节
使用generate_stream逐章生成，但跳过checkpoint
"""
import sys
import os
sys.path.insert(0, os.path.dirname(__file__))

import json
import time
from pathlib import Path
from typing import Optional
from openai import APIError, APITimeoutError

from utils.config import MANUSCRIPTS_DIR, VOLUMES_DIR, SETTINGS_DIR
from utils.llm_client import _get_client, MODEL_ID, _clean_response_content
from utils.config_loader import get_config

MAX_RETRIES = 3

def load_setting(name: str) -> dict:
    p = Path(SETTINGS_DIR) / f"{name}.json"
    if p.exists():
        with open(p, 'r', encoding='utf-8') as f:
            data = json.load(f)
            return data.get("content", data)
    return {}

def load_chapter_outline(volume_id: int, chapter_num: int) -> Optional[dict]:
    ch_dir = Path(VOLUMES_DIR) / f"vol_{volume_id:02d}_chapters"
    ch_path = ch_dir / f"ch_{chapter_num:03d}_outline.json"
    if ch_path.exists():
        with open(ch_path, 'r', encoding='utf-8') as f:
            return json.load(f)
    return None

def get_manuscript_path(volume_id: int, chapter_num: int) -> Path:
    d = Path(MANUSCRIPTS_DIR) / f"vol_{volume_id:02d}"
    d.mkdir(parents=True, exist_ok=True)
    return d / f"ch_{chapter_num:03d}_final.md"

def get_temp_path(volume_id: int, chapter_num: int) -> Path:
    d = Path(MANUSCRIPTS_DIR) / f"vol_{volume_id:02d}"
    d.mkdir(parents=True, exist_ok=True)
    return d / f"ch_{chapter_num:03d}_temp.md"

def get_checkpoint_path(volume_id: int) -> Path:
    d = Path(MANUSCRIPTS_DIR) / f"vol_{volume_id:02d}"
    d.mkdir(parents=True, exist_ok=True)
    return d / "checkpoint.json"

def load_checkpoint(volume_id: int) -> set:
    cp = get_checkpoint_path(volume_id)
    if cp.exists():
        with open(cp, 'r', encoding='utf-8') as f:
            return set(json.load(f).get("done", []))
    return set()

def save_checkpoint(volume_id: int, done: set):
    cp = get_checkpoint_path(volume_id)
    with open(cp, 'w', encoding='utf-8') as f:
        json.dump({"done": sorted(list(done))}, f, ensure_ascii=False)

def build_chapter_prompt(outline: dict, prev_chapters: str, world_context: str) -> tuple:
    """Build prompt and system message for chapter writing"""
    ch_num = outline["chapter_number"]
    title = outline["title"]
    overview = outline.get("overview", "")
    entities = outline.get("entity_list", [])
    
    system_msg = f"""你是顶尖网文创作者。你的任务是写出高质量的小说章节，风格对标《凡人修仙传》《仙逆》《完美世界》，融合东方玄幻与道教哲学思想。

写作核心要求：
1. 【叙事口吻】第三人称，以主角叶尘视角为主，偶尔切换到对手或被救者视角
2. 【爽点设定】每章至少2-3个爽点：境界突破、装逼打脸、越级反杀、星宿觉醒、宝物到手、美人倾心、悟道顿悟
3. 【道家深度】自然融入道家思想：天人合一、阴阳五行、性命双修、心性考验、因果循环、上善若水
4. 【节奏把控】6000-8000字/章，开篇设悬念，中段冲突升级，结尾留钩子
5. 【文笔要求】用词华丽但不浮夸，战斗描写要有画面感，情感描写要有代入感
6. 【打斗描写】层次分明，先试探后爆发，有技能名称、特效描写、心理博弈
7. 【禁止内容】不降智打脸，反派也要有智商，主角成长要有理有据
8. 【道教28星宿核心】每个星宿点亮都是一次心性考验，融合对应的人格修养"""
    
    prev_context = ""
    if prev_chapters:
        # Get last ~1000 chars from previous chapters for continuity
        prev_context = f"\n【前文回顾（最近章节摘要）】\n{prev_chapters[-2000:]}\n"
    
    # Load key settings
    story_outline = load_setting("story_outline")
    world_setting = load_setting("world_setting")
    blueprint = load_setting("core_blueprint")
    
    prompt = f"""【小说名称】二十八星宿：逆天改命
【当前章节】第{ch_num}章 {title}
【章节概述】{overview}
【参与角色】{', '.join(entities)}"
{prev_context}

【世界观核心】
{json.dumps(world_setting.get('world_view', ''), ensure_ascii=False)[:1000]}

【故事主线】
{json.dumps(story_outline.get('overview', ''), ensure_ascii=False)[:1000]}

【关键角色设定】
{json.dumps(blueprint.get('character_cards', []), ensure_ascii=False)[:1500]}

【本章包含的星宿设定】
道教二十八星宿：
- 东方苍龙七宿：角木蛟(木·仁)、亢金龙(金·义)、氐土貉(土·信)、房日兔(火·礼)、心月狐(火·智)、尾火虎(火·勇)、箕水豹(水·毅)
- 北方玄武七宿：斗木獬(木·正)、牛金牛(金·刚)、女土蝠(土·柔)、虚日鼠(火·明)、危月燕(火·险)、室火猪(火·旺)、壁水貐(水·润)
- 西方白虎七宿：奎木狼(木·威)、娄金狗(金·忠)、胃土雉(土·稳)、昴日鸡(火·信)、毕月乌(火·智)、觜火猴(火·灵)、参水猿(水·变)
- 南方朱雀七宿：井木犴(木·法)、鬼金羊(金·烈)、柳土獐(土·敏)、星日马(火·奔)、张月鹿(火·悦)、翼火蛇(火·速)、轸水蚓(水·泽)

【写作要求】
1. 开篇用吸引人的段落引入，直接进入冲突或悬念
2. 保持网文风格，对话自然，战斗激烈
3. 适当加入心理描写和内心独白，增强代入感
4. 结尾必须留下钩子或悬念
5. 严格遵循概述中的情节走向
6. 如果本章涉及星宿觉醒，详细描写心性考验的过程
7. 章节标题放在文件开头：'# 第{ch_num}章 {title}'
8. 最后一行是'——未完待续——'

写出6000-8000字的完整章节内容。"""
    
    return system_msg, prompt


def write_single_chapter(volume_id: int, chapter_num: int) -> bool:
    """Write a single chapter using the LLM"""
    final_path = get_manuscript_path(volume_id, chapter_num)
    if final_path.exists():
        print(f"  [SKIP] 第{chapter_num}章已存在")
        return True
    
    outline = load_chapter_outline(volume_id, chapter_num)
    if not outline:
        print(f"  [ERROR] 第{chapter_num}章纲不存在")
        return False
    
    # Get previous chapter content for continuity
    prev_content = ""
    prev_ch = chapter_num - 1
    while prev_ch >= 1 and not prev_content:
        pp = get_manuscript_path(volume_id, prev_ch)
        if pp.exists():
            prev_content = pp.read_text(encoding='utf-8')
        prev_ch -= 1
    
    world_context = json.dumps(load_setting("world_setting"), ensure_ascii=False)
    system_msg, prompt = build_chapter_prompt(outline, prev_content, world_context)
    
    print(f"  [WRITE] 第{chapter_num}章《{outline['title']}》...", end="", flush=True)
    
    for attempt in range(MAX_RETRIES):
        try:
            messages = [
                {"role": "system", "content": system_msg},
                {"role": "user", "content": prompt}
            ]
            
            response = _get_client().chat.completions.create(
                model=MODEL_ID,
                messages=messages,
                temperature=get_config("generation.temperature", 0.85),
                stream=True
            )
            
            accumulated = []
            for chunk in response:
                delta = chunk.choices[0].delta
                if hasattr(delta, 'content') and delta.content:
                    accumulated.append(delta.content)
            
            content = "".join(accumulated)
            
            # Clean content
            if '{"op":"done"}' in content:
                content = content.split('{"op":"done"}')[0].strip()
            if content.startswith("```"):
                content = content.split("\n", 1)[1] if "\n" in content else content[3:]
            if content.endswith("```"):
                content = content.rsplit("```", 1)[0]
            
            # Ensure chapter title
            title_text = f"第{chapter_num}章 {outline['title']}"
            if not content.startswith("# " + title_text) and not content.startswith("# 第"):
                content = f"# {title_text}\n\n" + content
            
            # Add ending marker if missing
            if '未完待续' not in content:
                content += '\n\n——未完待续——'
            
            # Save temp
            temp_path = get_temp_path(volume_id, chapter_num)
            temp_path.write_text(content, encoding='utf-8')
            
            # Save final
            final_path.write_text(content, encoding='utf-8')
            
            char_count = len(content)
            print(f" ✓ {char_count}字")
            return True
            
        except (APIError, APITimeoutError) as e:
            if attempt < MAX_RETRIES - 1:
                wait = get_config("generation.retry_delay", 5) * (attempt + 1)
                print(f"\n  [RETRY] API错误({e}), {wait}秒后重试...")
                time.sleep(wait)
            else:
                print(f"\n  [FAIL] API失败: {str(e)[:100]}")
                return False
        except Exception as e:
            print(f"\n  [ERROR] {str(e)[:150]}")
            return False
    
    return False


def batch_write(volume_id: int, chapters_range: str):
    """Write a range of chapters for a volume"""
    if "-" in chapters_range:
        start, end = map(int, chapters_range.split("-"))
    else:
        start = end = int(chapters_range)
    
    # Load checkpoint
    done = load_checkpoint(volume_id)
    print(f"[INFO] 卷{volume_id}已有{len(done)}章通过检查点")
    
    success_count = 0
    fail_count = 0
    
    for ch in range(start, end + 1):
        final_path = get_manuscript_path(volume_id, ch)
        if ch in done and final_path.exists():
            print(f"  [SKIP] 第{ch}章(检查点已确认)")
            success_count += 1
            continue
        
        ok = write_single_chapter(volume_id, ch)
        if ok:
            done.add(ch)
            success_count += 1
        else:
            fail_count += 1
            if fail_count >= 3:
                print(f"[WARN] 连续{fail_count}次失败，暂停防止无限循环")
                break
        
        # Save checkpoint after each chapter
        save_checkpoint(volume_id, done)
    
    print(f"\n{'='*50}")
    print(f"[RESULT] 卷{volume_id}章{start}-{end}: 成功{success_count}, 失败{fail_count}")
    print(f"{'='*50}")


if __name__ == "__main__":
    vol = int(sys.argv[1]) if len(sys.argv) > 1 else 1
    ch_range = sys.argv[2] if len(sys.argv) > 2 else "1-5"
    batch_write(vol, ch_range)