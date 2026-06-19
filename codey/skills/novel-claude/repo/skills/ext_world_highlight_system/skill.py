import json
from core.base_skill import BaseSkill

class WorldHighlightSkill(BaseSkill):
    def __init__(self, context):
        super().__init__(context)
        self.name = "WorldHighlightSystem"
        # 定义此插件私有数据的相对路径 (相对于 .novel 目录)
        self.state_rel_path = "skills_data/world_highlight_state.json"
        self._init_state()

    def _init_state(self):
        """初始化或加载高亮名词状态"""
        state = self.context.workspace.safe_read_json(self.state_rel_path)
        if not state:
            # 设初始值并写入
            initial_data = {
                "highlight_terms": {
                    "factions": ["天剑宗", "魔教", "圣光教会", "暗影议会", "龙族", "精灵族"],
                    "cultivation_methods": ["九转金丹诀", "魔神功", "圣光术", "暗影步", "龙象般若功", "御剑术"],
                    "special_items": ["上古神器", "灵丹妙药", "秘籍", "法宝"]
                }
            }
            self._save_state(initial_data)

    def _save_state(self, data):
        """安全保存状态数据"""
        self.context.workspace.safe_write_json(self.state_rel_path, data)

    def _get_highlight_terms(self):
        """获取当前高亮名词列表"""
        state = self.context.workspace.safe_read_json(self.state_rel_path)
        if not state or "highlight_terms" not in state:
            return self._init_state()
        return state["highlight_terms"]

    def _update_highlight_terms(self, category, terms):
        """更新高亮名词列表"""
        state = self.context.workspace.safe_read_json(self.state_rel_path)
        if not state:
            state = {"highlight_terms": {}}
        
        state["highlight_terms"][category] = terms
        self._save_state(state)

    # ================= 1. 上下文注入 (Hook) =================
    def on_before_scene_write(self, prompt_payload: list, beat_data: dict) -> list:
        """在 LLM 动笔前，将高亮名词用【】框包裹"""
        state = self.context.workspace.safe_read_json(self.state_rel_path)
        if not state or "highlight_terms" not in state:
            return prompt_payload
        
        highlight_terms = state["highlight_terms"]
        if not highlight_terms:
            return prompt_payload
        
        # 构建替换规则
        replacement_rules = []
        
        # 处理所有类别的名词
        for category, terms in highlight_terms.items():
            for term in terms:
                if term:  # 确保术语不为空
                    # 创建正则表达式模式，匹配全角和半角括号
                    pattern = term.replace("(", "\\(").replace(")", "\\)")
                    replacement_rules.append((pattern, f"【{term}】"))
        
        # 如果没有规则，直接返回
        if not replacement_rules:
            return prompt_payload
        
        # 构建高亮提示文本
        highlight_prompt = "\n<world_highlight>\n在接下来的文本中，请将以下特殊名词用【】框包裹：\n"
        
        for category, terms in highlight_terms.items():
            if terms:
                highlight_prompt += f"- {category}：{', '.join(terms)}\n"
        
        highlight_prompt += "注意：请确保所有出现这些名词的地方都被正确高亮，包括在对话、描述和叙述中。\n</world_highlight>\n"
        
        prompt_payload.append(highlight_prompt)
        return prompt_payload

    # ================= 2. 工具注册 (MCP Tools) =================
    def get_llm_tools(self) -> list[dict]:
        """告诉 LLM 它有权管理高亮名词"""
        return [
            {
                "type": "function",
                "function": {
                    "name": "add_highlight_term",
                    "description": "添加一个新的高亮名词到指定类别",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "category": {"type": "string", "description": "类别（factions/cultivation_methods/special_items）"},
                            "term": {"type": "string", "description": "要添加的名词"}
                        },
                        "required": ["category", "term"]
                    }
                }
            },
            {
                "type": "function",
                "function": {
                    "name": "remove_highlight_term",
                    "description": "从指定类别中移除一个高亮名词",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "category": {"type": "string", "description": "类别（factions/cultivation_methods/special_items）"},
                            "term": {"type": "string", "description": "要移除的名词"}
                        },
                        "required": ["category", "term"]
                    }
                }
            },
            {
                "type": "function",
                "function": {
                    "name": "list_highlight_terms",
                    "description": "列出当前所有高亮名词",
                    "parameters": {
                        "type": "object",
                                               "properties": {}
                    }
                }
            }
        ]

    # ================= 3. 工具执行 (Tool Execution) =================
    def execute_tool(self, tool_name: str, kwargs: dict) -> str:
        """LLM 调用工具时，执行纯代码计算"""
        if tool_name == "add_highlight_term":
            category = kwargs.get("category")
            term = kwargs.get("term")
            
            if not category or not term:
                return "参数不完整，需要提供类别和名词。"
            
            state = self.context.workspace.safe_read_json(self.state_rel_path)
            if not state:
                state = {"highlight_terms": {}}
            
            if "highlight_terms" not in state:
                state["highlight_terms"] = {}
            
            if category not in state["highlight_terms"]:
                state["highlight_terms"][category] = []
            
            # 避免重复添加
            if term not in state["highlight_terms"][category]:
                state["highlight_terms"][category].append(term)
                self._save_state(state)
                return f"已成功添加名词【{term}】到{category}类别。"
            else:
                return f"名词【{term}】已在{category}类别中存在。"
                
        elif tool_name == "remove_highlight_term":
            category = kwargs.get("category")
            term = kwargs.get("term")
            
            if not category or not term:
                return "参数不完整，需要提供类别和名词。"
            
            state = self.context.workspace.safe_read_json(self.state_rel_path)
            if not state or "highlight_terms" not in state or category not in state["highlight_terms"]:
                return f"类别{category}不存在或没有名词。"
            
            if term in state["highlight_terms"][category]:
                state["highlight_terms"][category].remove(term)
                self._save_state(state)
                return f"已成功移除名词【{term}】从{category}类别。"
            else:
                return f"名词【{term}】在{category}类别中不存在。"
                
        elif tool_name == "list_highlight_terms":
            state = self.context.workspace.safe_read_json(self.state_rel_path)
            if not state or "highlight_terms" not in state:
                return "当前没有设置任何高亮名词。"
            
            result = "当前高亮名词列表：\n"
            for category, terms in state["highlight_terms"].items():
                if terms:
                    result += f"- {category}：{', '.join(terms)}\n"
            
            return result.strip()
        
        return "Unknown Tool"