import json
import os
from pathlib import Path
from typing import List, Dict, Any

from core.base_skill import BaseSkill
from core.novel_context import NovelContext

class GoldFingerSkill(BaseSkill):
    """
    主角专属金手指插件：系统面板 (System Panel)。
    负责在每次场景生成前向 AI 注入当前最新的系统面板状态，
    并提供工具让 AI 在剧情中花费银子简化技能和功法。
    """
    def __init__(self, context: NovelContext):
        super().__init__(context)
        self.name = "GoldFingerSystem"
        
        # 插件私有状态存储路径，存放于 .novel/skills_data/gold_finger.json
        self.state_path = Path(context.workspace.base_dir) / ".novel" / "skills_data" / "gold_finger.json"

    def on_init(self) -> None:
        """插件初始化，如果不存在存档则建立初始状态"""
        if not self.state_path.exists():
            self.state_path.parent.mkdir(parents=True, exist_ok=True)
            initial_state = {
                "name": "陈越",
                "identity": "雷州府合浦珠寨珠户",
                "status": "溺水后遗症(轻微)、长期营养不良",
                "skills": {
                    "采珠": {"level": "熟练", "exp": 21, "max_exp": 300},
                    "翻浪呼吸法": {"level": "未入门", "exp": 0, "max_exp": 100}
                },
                "money": 5.0  # 假设初始有 5 两白银
            }
            self._save_state(initial_state)

    def _load_state(self) -> dict:
        with open(self.state_path, "r", encoding="utf-8") as f:
            return json.load(f)

    def _save_state(self, state: dict) -> None:
        with open(self.state_path, "w", encoding="utf-8") as f:
            json.dump(state, f, ensure_ascii=False, indent=2)

    def _format_panel(self, state: dict) -> str:
        """将内部 JSON 状态格式化为给大模型看的面板文本"""
        skills_str = "、".join([f"{name}({s['level']}{s['exp']}/{s['max_exp']})" for name, s in state["skills"].items()])
        panel = (
            f"【姓名:{state['name']}\n"
            f"身份:{state['identity']}\n"
            f"状态:{state['status']}\n"
            f"拥有白银:{state['money']}两\n"
            f"技能:{skills_str}】"
        )
        return panel

    # =========================================================================
    # 1. 记忆注入 (Hook: 被动增强)
    # =========================================================================
    def on_before_scene_write(self, prompt_payload: List[str], beat_data: dict) -> List[str]:
        """在每次写场景前，强行将最新的面板状态塞入 Prompt"""
        state = self._load_state()
        panel_text = self._format_panel(state)
        
        injection = (
            "\n<system_panel_state>\n"
            "以下是主角陈越当前的【外挂系统面板】实时状态，你在描写时必须绝对遵循此状态，绝不能前后矛盾！\n"
            f"{panel_text}\n"
            "</system_panel_state>\n"
        )
        prompt_payload.append(injection)
        return prompt_payload

    # =========================================================================
    # 2. 注册 LLM 主动工具 (Tool Calling)
    # =========================================================================
    def get_llm_tools(self) -> List[Dict[str, Any]]:
        """向 LLM 暴露改变面板状态的动作"""
        return [
            {
                "type": "function",
                "function": {
                    "name": "simplify_skill",
                    "description": "当小说剧情中主角发现新技能，或者决定花费白银来简化提升某个现有技能时，调用此工具更新系统面板。每次消耗1两白银。",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "skill_name": {
                                "type": "string",
                                "description": "要简化的技能或功法名称，如'采珠'、'翻浪呼吸法'"
                            },
                            "narrative_context": {
                                "type": "string",
                                "description": "剧情表现描述，例如：'发现技能，是否花费一两白银，简化采珠？'"
                            }
                        },
                        "required": ["skill_name", "narrative_context"]
                    }
                }
            }
        ]

    # =========================================================================
    # 3. 工具执行逻辑 (Tool Execution)
    # =========================================================================
    def execute_tool(self, tool_name: str, kwargs: dict) -> str:
        """当大模型决定调用工具时，执行的 Python 逻辑"""
        if tool_name == "simplify_skill":
            skill_name = kwargs.get("skill_name")
            state = self._load_state()
            
            # 校验余额
            if state["money"] < 1.0:
                return f"[系统拒绝] 余额不足！当前只有 {state['money']} 两白银，无法简化 {skill_name}。"
                
            # 扣钱
            state["money"] -= 1.0
            
            # 更新技能状态
            if skill_name in state["skills"]:
                # 如果是已有的技能，提升熟练度或等级
                skill = state["skills"][skill_name]
                if skill["level"] == "未入门":
                    skill["level"] = "入门"
                    skill["exp"] = 0
                    skill["max_exp"] = 200
                elif skill["level"] == "入门":
                    skill["level"] = "熟练"
                    skill["exp"] = 0
                    skill["max_exp"] = 300
                elif skill["level"] == "熟练":
                    skill["level"] = "精通"
                    skill["exp"] = 0
                    skill["max_exp"] = 500
                else:
                    skill["level"] = "圆满"
                    skill["exp"] = skill["max_exp"]
            else:
                # 如果是新发现的技能
                state["skills"][skill_name] = {"level": "入门", "exp": 0, "max_exp": 100}
                
            self._save_state(state)
            
            # 返回给大模型的结果
            new_panel = self._format_panel(state)
            return f"[系统执行成功] 已消耗1两白银简化 {skill_name}。最新面板为：\n{new_panel}"
        
        return "Unknown Tool"
