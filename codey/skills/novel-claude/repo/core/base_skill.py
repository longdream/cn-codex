from typing import List, Dict, Any
from core.novel_context import NovelContext

class BaseSkill:
    """
    插件 (Skill) 在 Novel-Claude V3 架构中的标准基类协议。
    所有扩展插件都应当继承此类。
    """
    def __init__(self, context: NovelContext):
        self.context = context
        self.name = "UnknownSkill"

    # =========================================================================
    # 生命周期钩子 (Lifecycle Hooks)
    # =========================================================================

    def on_init(self) -> None:
        """插件初始化完成后触发。可用于创建数据文件、建立连接等。"""
        pass

    def on_volume_planning(self, outline_draft: dict) -> dict:
        """分卷规划期间触发，有权干预或修改大纲 draft 数据。"""
        return outline_draft

    def on_before_scene_write(self, prompt_payload: List[str], beat_data: dict) -> List[str]:
        """
        在子智能体提笔编写场景前触发（最常见的 Hook）。
        常用于将插件维护的上下文（经验值、理智值、设定卡）强制注入到 `prompt_payload` 中。
        """
        return prompt_payload

    def on_after_scene_write(self, beat_data: dict, raw_text: str) -> None:
        """
        场景生成完毕落盘后触发。
        用于计算收益、扣除消耗、或是异步更新向量数据库 ChromaDB。
        """
        pass

    def on_chapter_render(self, full_text: str, chapter_id: int) -> str:
        """
        最终合成该章节并渲染文本时触发。
        用于拦截特定的 Markdown 占位符并替换为复杂的表格或格式化文本。
        """
        return full_text

    # =========================================================================
    # MCP 工具注册 (Tool Calling Interface)
    # =========================================================================

    def get_llm_tools(self) -> List[Dict[str, Any]]:
        """返回一份符合 OpenAI Tool API 标准的 JSON Schema 列表。"""
        return []

    def execute_tool(self, tool_name: str, kwargs: dict) -> str:
        """当大模型决议调用此插件名下的工具时被触发。"""
        return ""
