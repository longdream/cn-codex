import json  
from core.base_skill import BaseSkill

class HandsomeProtagonistSkill(BaseSkill):  
    def __init__(self, context):  
        super().__init__(context)  
        self.name = "HandsomeProtagonistSystem"  
        # 定义此插件私有数据的相对路径 (相对于 .novel 目录)
        self.state_rel_path = "skills_data/handsome_state.json"
        self._init_state()

    def _init_state(self):  
        """初始化或加载状态"""  
        # 使用 context.workspace 读取数据
        state = self.context.workspace.safe_read_json(self.state_rel_path)
        if not state:
            # 设初始值并写入
            self._save_state({"enabled": True})

    def _save_state(self, data):  
        # 使用 context.workspace.safe_write_json 安全落盘
        self.context.workspace.safe_write_json(self.state_rel_path, data)

    # ================= 1. 上下文注入 (Hook) =================  
    def on_before_scene_write(self, prompt_payload: list, beat_data: dict) -> list:  
        """在 LLM 动笔前，强行将'主角很帅'塞进它的脑子里"""  
        state = self.context.workspace.safe_read_json(self.state_rel_path)
        if not state or not state.get("enabled", True):
            return prompt_payload
          
        handsome_prompt = "\n<protagonist_appearance>\n主角很帅。\n</protagonist_appearance>\n"  
          
        prompt_payload.append(handsome_prompt)  
        return prompt_payload

    # ================= 2. 工具注册 (MCP Tools) =================  
    def get_llm_tools(self) -> list[dict]:  
        """告诉 LLM 它有权控制是否启用此功能"""
        return [{  
            "type": "function",  
            "function": {  
                "name": "toggle_handsome_protagonist",  
                "description": "启用或禁用'主角很帅'的自动注入功能。",  
                "parameters": {  
                    "type": "object",  
                    "properties": {  
                        "enabled": {"type": "boolean", "description": "是否启用此功能"}  
                    },  
                    "required": ["enabled"]  
                }  
            }  
        }]

    # ================= 3. 工具执行 (Tool Execution) =================  
    def execute_tool(self, tool_name: str, kwargs: dict) -> str:  
        """LLM 调用工具时，执行纯代码计算，绝对严谨"""  
        if tool_name == "toggle_handsome_protagonist":  
            state = self.context.workspace.safe_read_json(self.state_rel_path)
            
            state["enabled"] = kwargs["enabled"]  
            self._save_state(state)  
            return f"系统已成功{'启用' if kwargs['enabled'] else '禁用'}'主角很帅'的自动注入功能。"  
        return "Unknown Tool"