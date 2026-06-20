"""
批量规划脚本：为10卷共300章生成章节细纲
直接使用LLM的generate_json分阶段生成，避免超长上下文导致的JSON异常
"""
import sys
import os
sys.path.insert(0, os.path.dirname(__file__))

import json
from pathlib import Path
from typing import List
from pydantic import BaseModel, Field

from utils.config import SETTINGS_DIR, VOLUMES_DIR
from utils.llm_client import generate_json, _clean_response_content

# ============================================================
# 卷宏观信息
# ============================================================
VOLUME_MACRO = {
    1:  {"name": "星火初燃", "target": "觉醒角木蛟、亢金龙、氐土貉，离开青云宗踏上星宿修行路", "cap": "金丹九重"},
    2:  {"name": "苍龙之怒", "target": "点亮房日兔、心月狐、尾火虎、箕水豹，觉醒苍龙七宿", "cap": "元婴九重"},
    3:  {"name": "群星逐鹿", "target": "参与五行秘境试炼，结识各大星宿传承者，初识篡天盟阴谋", "cap": "元婴九重"},
    4:  {"name": "北海玄冰", "target": "点亮斗木獬、牛金牛、女土蝠，前往北域寻找玄武阁", "cap": "化神三重"},
    5:  {"name": "玄武七宿", "target": "点亮虚日鼠、危月燕、室火猪、壁水貐，觉醒玄武之力", "cap": "化神九重"},
    6:  {"name": "西极问剑", "target": "点亮奎木狼、娄金狗、胃土雉、昴日鸡，西行白虎堂", "cap": "化神九重"},
    7:  {"name": "白虎杀伐", "target": "点亮毕月乌、觜火猴、参水猿，觉醒白虎之力战白无咎", "cap": "大乘三重"},
    8:  {"name": "南域焚天", "target": "点亮井木犴、鬼金羊、柳土獐、星日马，南行朱雀楼", "cap": "大乘六重"},
    9:  {"name": "朱雀涅槃", "target": "点亮张月鹿、翼火蛇、轸水蚓，觉醒朱雀之力，四象齐聚", "cap": "大乘九重"},
    10: {"name": "天道终章", "target": "终极传承，攻入混沌界，决战浑天道人，恢复天道秩序", "cap": "真仙"}
}

class ChapterOutline(BaseModel):
    chapter_number: int
    title: str
    overview: str
    entity_list: List[str] = Field(default_factory=list)

class VolumeChaptersSchema(BaseModel):
    volume_id: int
    volume_name: str
    chapters: List[ChapterOutline]

def ensure_stage_dir(volume_id: int):
    d = Path(VOLUMES_DIR) / f"vol_{volume_id:02d}_chapters"
    d.mkdir(parents=True, exist_ok=True)
    return d

def load_macro(volume_id: int) -> dict:
    p = Path(VOLUMES_DIR) / f"vol_{volume_id:02d}_outline.json"
    if p.exists():
        with open(p, 'r', encoding='utf-8') as f:
            return json.load(f)
    return VOLUME_MACRO.get(volume_id, {})

def load_setting(name: str) -> dict:
    p = Path(SETTINGS_DIR) / f"{name}.json"
    if p.exists():
        with open(p, 'r', encoding='utf-8') as f:
            data = json.load(f)
            return data.get("content", data)
    return {}

def plan_volume_chapters(volume_id: int):
    """为指定卷生成30章细纲"""
    macro = load_macro(volume_id)
    vol_name = macro.get("volume_name", VOLUME_MACRO.get(volume_id, {}).get("name", f"第{volume_id}卷"))
    main_target = macro.get("main_target", VOLUME_MACRO.get(volume_id, {}).get("target", ""))
    power_cap = macro.get("power_level_cap", VOLUME_MACRO.get(volume_id, {}).get("cap", ""))
    branch_line = macro.get("branch_line", "")
    
    one_sentence = load_setting("one_sentence")
    story_outline = load_setting("story_outline")
    world_setting = load_setting("world_setting")
    blueprint = load_setting("core_blueprint")
    
    start_ch = (volume_id - 1) * 30 + 1
    
    prompt = f"""你是网文白金主编。为小说《二十八星宿：逆天改命》的第 {volume_id} 卷规划30章细纲。

【卷信息】
- 卷名：{vol_name}
- 卷号：{volume_id}（第{start_ch}章-第{volume_id*30}章）
- 主线目标：{main_target}
- 辅线：{branch_line}
- 战力天花板：{power_cap}

【一句话梗概】
{json.dumps(one_sentence, ensure_ascii=False)}

【故事大纲】
{json.dumps(story_outline, ensure_ascii=False)}

【世界观】
{json.dumps(world_setting, ensure_ascii=False)}

【核心角色】
{json.dumps(blueprint.get('character_cards', []), ensure_ascii=False)}

【结构要求】
- 本卷30章，分为6个弧段，每弧段5章
- 弧段1（第{start_ch}-{start_ch+4}章）：开场/诱因
- 弧段2（第{start_ch+5}-{start_ch+9}章）：推进/阻力
- 弧段3（第{start_ch+10}-{start_ch+14}章）：风险升级
- 弧段4（第{start_ch+15}-{start_ch+19}章）：重大转折
- 弧段5（第{start_ch+20}-{start_ch+24}章）：危机
- 弧段6（第{start_ch+25}-{start_ch+29}章）：卷高潮

【输出要求】
- 每章标题要吸引人，包含悬念或爽点
- 每章概述100-200字，写清核心冲突、战斗场面、修炼突破、情节反转
- 包含参与的实体列表（角色名/组织名）
- 每一章都要有爽点：实力提升、打脸、装逼、悟道、获得宝物等

【风格要求】
融合爽文写法：废材逆袭、境界突破、星宿觉醒、装逼打脸、越级反杀、后宫互动
融合道家思想：天人合一、阴阳五行、心性考验、因果循环、大道至简

输出JSON格式，包含volume_id, volume_name, chapters数组（每项含chapter_number, title, overview, entity_list）。"""
    
    schema_model = VolumeChaptersSchema
    data = generate_json(prompt, schema_model)
    data_dict = data if isinstance(data, dict) else data.model_dump()
    
    ch_dir = ensure_stage_dir(volume_id)
    for ch in data_dict.get("chapters", []):
        ch_num = ch["chapter_number"]
        ch_path = ch_dir / f"ch_{ch_num:03d}_outline.json"
        with open(ch_path, 'w', encoding='utf-8') as f:
            json.dump(ch, f, ensure_ascii=False, indent=2)
    
    # Also save a combined version
    combined_path = Path(VOLUMES_DIR) / f"vol_{volume_id:02d}_all_chapters.json"
    with open(combined_path, 'w', encoding='utf-8') as f:
        json.dump(data_dict, f, ensure_ascii=False, indent=2)
    
    ch_count = len(data_dict.get("chapters", []))
    print(f"[✓] 第{volume_id}卷《{vol_name}》{ch_count}章细纲已生成")
    return True

if __name__ == "__main__":
    import sys
    volumes = sys.argv[1] if len(sys.argv) > 1 else "1-10"
    
    if "-" in volumes:
        start, end = map(int, volumes.split("-"))
        vlist = list(range(start, end + 1))
    else:
        vlist = [int(volumes)]
    
    for v in vlist:
        try:
            plan_volume_chapters(v)
        except Exception as e:
            print(f"[ERROR] 第{v}卷规划失败: {str(e)[:200]}")
            # Try one more time
            try:
                print(f"[RETRY] 第{v}卷重试...")
                plan_volume_chapters(v)
            except Exception as e2:
                print(f"[FAIL] 第{v}卷最终失败: {str(e2)[:200]}")