from typing import Callable, List, Any, Dict

class EventBus:
    """
    中央事件总线 (单例模式)
    负责所有的钩子 (Hooks) 派发，并在执行插件代码时包裹严格的容错隔离层。
    """
    _instance = None

    def __new__(cls, *args, **kwargs):
        if not cls._instance:
            cls._instance = super(EventBus, cls).__new__(cls)
            cls._instance.subscribers = []
        return cls._instance

    def register(self, skill):
        """注册一个 Skill 到总线"""
        if skill not in self.subscribers:
            self.subscribers.append(skill)
            
    def unregister(self, skill):
        """注销一个 Skill"""
        if skill in self.subscribers:
            self.subscribers.remove(skill)
            
    def clear(self):
        """清空所有订阅者"""
        self.subscribers.clear()

    def emit(self, event_name: str, *args, **kwargs):
        """
        触发所有订阅了此事件的插件。
        【容错隔离】: 如果单个插件崩溃，不会影响主线程和其他插件。
        """
        results = []
        for skill in self.subscribers:
            if hasattr(skill, event_name):
                method = getattr(skill, event_name)
                try:
                    res = method(*args, **kwargs)
                    results.append(res)
                except Exception as e:
                    print(f"\\n[🚨 EventBus 容错警报] 插件 '{skill.name}' 在执行 '{event_name}' 时崩溃: {e}")
                    print(f"[EventBus] 已自动隔离该错误并跳过此插件，生成进度继续。\\n")
        return results

    def emit_pipeline(self, event_name: str, initial_data: Any, *args, **kwargs) -> Any:
        """
        串行处理模式：将第一个插件的处理结果传给下一个插件。
        用于 `prompt_payload` 这类需要累加处理的场景。
        """
        data = initial_data
        for skill in self.subscribers:
            if hasattr(skill, event_name):
                method = getattr(skill, event_name)
                try:
                    data = method(data, *args, **kwargs)
                except Exception as e:
                    print(f"\\n[🚨 EventBus 容错警报] 插件 '{skill.name}' 在执行 '{event_name}' 串行处理时崩溃: {e}")
                    print(f"[EventBus] 已自动隔离该错误，保留前一次的有效数据并继续。\\n")
        return data

    def collect(self, method_name: str, *args, **kwargs) -> List[Any]:
        """
        按列表收集所有插件的方法返回结果（如收集 active_tools）。
        """
        collected = []
        for skill in self.subscribers:
            if hasattr(skill, method_name):
                method = getattr(skill, method_name)
                try:
                    res = method(*args, **kwargs)
                    if isinstance(res, list):
                        collected.extend(res)
                    else:
                        collected.append(res)
                except Exception as e:
                    print(f"\\n[🚨 EventBus 容错警报] 插件 '{skill.name}' 在调用 '{method_name}' 时崩溃: {e}")
        return collected

# 创建全局单例暴露供使用
event_bus = EventBus()
