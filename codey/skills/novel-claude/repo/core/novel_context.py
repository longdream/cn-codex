import os
import json
import importlib
import importlib.util
from typing import Dict

from utils.workspace import WorkspaceManager

class NovelContext:
    """
    NovelContext 封装了当前生成的运行时状态。
    作为全局字典在各个插件（Skill）之间流转。
    """
    def __init__(self, workspace_mgr: WorkspaceManager):
        self.workspace = workspace_mgr
        
        # 当前进行的卷号和章号（由主控引擎在运行时动态设定）
        self.current_volume_id: int = 1
        self.current_chapter_id: int = 1
        
        # 保存当前加载的所有激活的 Skill 对象
        self.active_skills: Dict[str, object] = {}
        
        # 共享状态白板：供不同的独立插件之间进行临时数据交换
        self.shared_state: dict = {}
        
    def set_current_ids(self, volume_id: int, chapter_id: int):
        self.current_volume_id = volume_id
        self.current_chapter_id = chapter_id

    def get_shared(self, key: str, default=None):
        return self.shared_state.get(key, default)

    def set_shared(self, key: str, value):
        self.shared_state[key] = value
