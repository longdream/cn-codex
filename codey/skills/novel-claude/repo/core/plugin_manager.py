import os
import sys
import importlib.util
from typing import Dict, Any

from core.novel_context import NovelContext
from core.event_bus import event_bus
from core.base_skill import BaseSkill

class PluginManager:
    """
    PluginManager: 负责从文件系统中扫描、动态加载和热更新所有的 Skills。
    """
    def __init__(self, context: NovelContext, skills_dir: str = "skills"):
        self.context = context
        self.skills_dir = skills_dir
        self.loaded_modules: Dict[str, Any] = {}

    def scan_and_load(self):
        """扫描 skills 目录，并实例化所有合法的插件到当前上下文中"""
        if not os.path.exists(self.skills_dir):
            os.makedirs(self.skills_dir, exist_ok=True)
            return

        print(f"[PluginManager] 正在扫描 {self.skills_dir} 目录下的插件...")
        for item in os.listdir(self.skills_dir):
            plugin_path = os.path.join(self.skills_dir, item)
            if os.path.isdir(plugin_path) and not item.startswith("__") and not item.startswith("."):
                skill_file = os.path.join(plugin_path, "skill.py")
                disabled_file = os.path.join(plugin_path, ".disabled")
                if os.path.exists(skill_file):
                    if not os.path.exists(disabled_file):
                        self._load_skill(item, skill_file)

    def _load_skill(self, module_name: str, file_path: str):
        """通过绝对路径动态导入 Python 模块"""
        try:
            spec = importlib.util.spec_from_file_location(module_name, file_path)
            if spec is None or spec.loader is None:
                return
            
            module = importlib.util.module_from_spec(spec)
            sys.modules[module_name] = module
            spec.loader.exec_module(module)
            
            # 约定: `skill.py` 文件中必须包含一个继承自 `BaseSkill` 的同名大驼峰类或寻找任何继承自 `BaseSkill` 的类
            skill_class = None
            for attr_name in dir(module):
                attr = getattr(module, attr_name)
                # 必须是一个类，且是 BaseSkill 的子类，且不是 BaseSkill 自身
                if isinstance(attr, type) and issubclass(attr, BaseSkill) and attr is not BaseSkill:
                    skill_class = attr
                    break
            
            if skill_class:
                skill_instance = skill_class(self.context)
                self.loaded_modules[module_name] = module
                self.context.active_skills[module_name] = skill_instance
                event_bus.register(skill_instance)
                
                # 触发 on_init
                try:
                    skill_instance.on_init()
                    print(f"  [✓] 加载插件成功: {skill_instance.name}")
                except Exception as e:
                    print(f"  [🚨] 插件 {skill_instance.name} 初始化(on_init)失败: {e}")
            else:
                print(f"  [WARN] 在 {file_path} 中未找到继承自 BaseSkill 的有效类。")

        except Exception as e:
            try:
                print(f"  [🚨] 加载插件模块 {module_name} 崩溃: {e}")
            except UnicodeEncodeError:
                print(f"  [ERROR] 加载插件模块 {module_name} 崩溃: {e}".encode('gbk', 'replace').decode('gbk'))

    def hot_reload(self, module_name: str):
        """
        热更新机制：允许 Meta-Generation 创建新代码后无缝接入系统，
        或者重新加载已被修改的插件。
        """
        print(f"[PluginManager] 正在热更新插件 {module_name}...")
        
        # 1. 尝试剔除老对象
        if module_name in self.context.active_skills:
            old_skill = self.context.active_skills.pop(module_name)
            event_bus.unregister(old_skill)
            
        if module_name in self.loaded_modules:
            del self.loaded_modules[module_name]
            
        if module_name in sys.modules:
            del sys.modules[module_name]

        # 2. 重新加载
        file_path = os.path.join(self.skills_dir, module_name, "skill.py")
        disabled_path = os.path.join(self.skills_dir, module_name, ".disabled")
        if os.path.exists(file_path):
            if not os.path.exists(disabled_path):
                self._load_skill(module_name, file_path)
                print(f"[PluginManager] {module_name} 热更新完毕！")
            else:
                print(f"[PluginManager] {module_name} 已处于禁用状态，已卸载。")
        else:
            print(f"[ERROR] 找不到此插件文件: {file_path}")

    def disable_skill(self, module_name: str):
        """禁用一个插件"""
        plugin_path = os.path.join(self.skills_dir, module_name)
        if os.path.exists(plugin_path):
            with open(os.path.join(plugin_path, ".disabled"), "w", encoding="utf-8") as f:
                f.write("disabled")
            self.hot_reload(module_name)
        else:
            print(f"[ERROR] 找不到插件目录: {plugin_path}")

    def enable_skill(self, module_name: str):
        """启用一个插件"""
        plugin_path = os.path.join(self.skills_dir, module_name)
        disabled_path = os.path.join(plugin_path, ".disabled")
        if os.path.exists(disabled_path):
            os.remove(disabled_path)
        if os.path.exists(plugin_path):
            self.hot_reload(module_name)
        else:
            print(f"[ERROR] 找不到插件目录: {plugin_path}")
