# 🚀 Novel-Claude V3: Agentic Novel Generation Framework

[简体中文](README.md) | [English](README_EN.md) | [日本語](README_JP.md)

> [!WARNING]
> **本项目还处于测试阶段，还有诸多功能不完善，请谨慎使用。**

Novel-Claude 是一个基于大语言模型（如智谱 GLM-4）构建的全自动长篇小说生成管线。在 V3 版本中，它从传统的线性脚本流水线彻底进化为具有极高扩展性的 **微内核 + 插件生态架构 (Microkernel & Plugin Architecture)**。

通过底层的 `EventBus` 事件引擎与动态 `PluginManager`，它支持极其复杂的社区插件生态（Skills）以及基于 ReAct 多轮交互的复杂智能体（Agents）。

## ✨ 核心特性

- **微内核插件系统 (Microkernel & Plugin Ecosystem)**: 所有的附加功能（如动态检索记忆 RAG、战斗合理性检测等）被剥离为主时间线之外的插件（Skills）。支持热重载（Hot-Reload）与错误护航隔离，单个插件崩溃不影响数小时的生成进程。
- **复杂智能体支撑 (Complex Agents)**:
  - 🖋️ **Editor Agent (毒舌主编智能体)**: 在章节生成末尾挂载，启用 ReAct 多轮循环思考对草稿进行严格审稿，自动修复视角跳跃和上下文割裂。
  - 🤖 **Skill Builder Agent (元生成器)**: 系统级别的 Meta-Generation。在 CLI 中输入一行自然语言，系统将**自动撰写并编译合法外挂插件 (Skills)** 落盘生效。
- **降本提效 (Batch API)**: 原生支持智谱/OpenAI 格式的 Batch API 提交流水线，支持离线 5 折并发生成海量章节，并自动拼接、回调。

---

## 🏗️ 架构总览

整个生成管线被切分为三大核心引擎，引擎之间通过 `NovelContext` 共享白板及 `EventBus` 广播串行：

1. `world_builder.py` (世界观造物主): 根据一句话创意（Logline），构建严格 JSON 格式的背景设定、阵营、人物列表。
2. `volume_planner.py` (分卷派单员): 定制 10 卷纲要，并将大段卷纲拆解为精准到场景的小说打点（Beats），且通过算法强制归一化控制为每章 5000 字精确产出。
3. `scene_writer.py` (执笔车间): 孵化 Subagents 无死角执行场景任务，并交由 Director Agent 进行定稿。

```text
novel_claude/
├── core/                       # 发动机微内核
│   ├── event_bus.py            # 全局事件总线（Fault Tolerance）
│   ├── plugin_manager.py       # 动态插件扫描与加载器
│   ├── base_skill.py           # V3 标准化插件基类
│   ├── novel_context.py        # 共享生命周期上下文
│   └── agents/                 # 复杂推理智能体
│       ├── editor_agent.py     # 毒舌主编 ReAct Agent
│       └── skill_builder_agent.py  # Meta-Generation 元生成器
├── skills/                     # 插件挂载文件夹（放入即生效）
│   └── core_memory_rag/        # 原生的 RAG 记忆流媒体检索插件
├── world_builder.py            # 核心引擎一：设定构建
├── volume_planner.py           # 核心引擎二：分卷与场景切分
├── scene_writer.py             # 核心引擎三：片段执笔与合并
├── cli.py                      # 终端入口
└── utils/                      # 配置文件与 LLM 客户端 API 层
```

---

## 🛠️ 安装与使用

### 1. 环境准备
确保您的 Python >= 3.10。
```bash
# 激活环境后安装依赖
uv pip install -r requirements.txt
```

### 2. CLI 快速使用

#### 基础生成流程
```bash
# 阶段 1：一句话初始化世界观
uv run python cli.py init "一个在修真界利用赛博插件强开灵根的科幻转玄幻故事"

# 阶段 2：规划宏观 10 卷的主线大纲
uv run python cli.py plan

# 阶段 2.5：为第 1 卷生成细分到微观单元的 50 章 Scene Beats
uv run python cli.py plan --volume 1

# 阶段 3：召唤执笔集群实时码字生成第 1 卷 1 到 5 章
uv run python cli.py write --volume 1 --chapters "1-5"

# 阶段 4：使用 AI 进行局部审阅与多文件关联修改
uv run python cli.py review -f ".novel/factions.json" -f ".novel/power_levels.json" -i "将金丹期统一改为结丹期"
```

#### Batch API 批量写作
```bash
# 构建 JSONL 请求文件
uv run python cli.py batch-build --volume 1 --chapters "1-50"

# 提交异步任务（会返回 Batch ID，请妥善保存）
uv run python cli.py batch-submit .batch/vol_01_ch_1_50_req.jsonl

# 同步合并成稿（轮询状态并自动下载合并）
uv run python cli.py batch-sync <batch_id>
```

#### V3 插件管理命令
```bash
# 列出所有插件（包含已加载、已禁用、报错）
uv run python cli.py skills list

# 禁用/启用某个插件
uv run python cli.py skills disable ext_gold_finger
uv run python cli.py skills enable ext_gold_finger

# 重载全部插件（修改代码后使之生效）
uv run python cli.py skills reload

# 用自然语言让 AI 自动生成一个插件！
uv run python cli.py skills build "帮我写一个Skill，在每次生成前注入一句主角很帅"
```

---

## 🔌 V3 插件与外挂生态

V3 引擎的精髓在于无穷无尽的功能扩展。所有的扩展统称为 `Skill`，必须继承 `BaseSkill` 基类。插件会被 `PluginManager` 动态接管并拦截底层 `EventBus` 各个周期的发信事件。

### 快速上手：手动创建插件

1. 在 `skills/` 目录下创建文件夹，如 `skills/my_awesome_skill/`
2. 在其中创建 `skill.py`：
```python
from core.base_skill import BaseSkill

class MyAwesomeSkill(BaseSkill):
    def __init__(self, context):
        super().__init__(context)
        self.name = "MyAwesomeSkill"

    def on_init(self):
        print(f"[{self.name}] 插件已初始化！")

    def on_before_scene_write(self, prompt_payload, beat_data):
        # 在每次写作前注入自定义提示
        prompt_payload.append("\n[系统注入] 注意：主角的表现需要冷酷且理智。")
        return prompt_payload
```
3. 保存后执行 `uv run python cli.py skills reload`

### 生命周期钩子一览

| 钩子方法 | 触发时机 | 用途 |
|---------|---------|------|
| `on_init()` | 插件加载后 | 初始化资源 |
| `on_volume_planning()` | 分卷规划时 | 干预/修改大纲 |
| `on_before_scene_write()` | 写作生成前 | 注入记忆/设定 |
| `on_after_scene_write()` | 写作生成后 | 统计/入库 |
| `on_chapter_render()` | 章节终渲染时 | 替换占位符 |
| `get_llm_tools()` | LLM 调用时 | 注册工具 |

### 主动工具调用 (Active Tool Calling)

V3 插件不仅可以被动注入上下文，还可以为 AI 提供**主动操作**的工具箱。继承 `BaseSkill` 后，可以覆写：

1. `get_llm_tools()`: 返回 OpenAI 格式的 JSON Schema 工具定义。
2. `execute_tool(tool_name, kwargs)`: 处理 AI 的调用逻辑并返回字符串结果。

**示例：Gold Finger (金手指系统面板)**
在 `skills/ext_gold_finger/` 中，我们实现了一个典型的"主角面板"插件：
- **被动**：每章开始前将主角的等级、金钱、技能熟练度注入 Prompt。
- **主动**：提供 `simplify_skill` 工具，AI 可以在剧情中决定花费"碎银"来"简化"功法。

### 插件开关机制

系统通过在插件文件夹下生成 `.disabled` 标记文件来实现开关逻辑，通过 CLI `skills enable/disable` 切换。

### 自动创建插件 (Meta-Generation)

运行以下命令让大模型自动生成插件代码：
```bash
uv run python cli.py skills build "帮我写一个检测战斗描写合理性的插件"
```
系统会自动调用大模型，按照开发规范生成合法代码，落盘到 `skills/` 目录并热重载生效。

---

## 📂 目录结构说明

| 目录/文件 | 说明 |
|-----------|------|
| `cli.py` | CLI 终端入口，所有命令的入口 |
| `world_builder.py` | 世界观初始化引擎 |
| `volume_planner.py` | 分卷大纲规划引擎 |
| `scene_writer.py` | 场景写作与合并引擎 |
| `core/` | 微内核核心模块 |
| `core/agents/` | 智能体实现 |
| `skills/` | 插件目录，放入即生效 |
| `utils/` | 工具模块（LLM客户端、配置等）|
| `docs/CLI_COMMANDS.md` | 完整 CLI 命令文档 |

---

## 📜 内置插件

| 插件 | 说明 |
|------|------|
| `ext_gold_finger` | 金手指插件 |
| `ext_world_highlight_system` | 世界高亮系统 |
| `ext_handsome_protagonist` | 主角光环插件 |
| `core_memory_rag` | 核心记忆 RAG 系统 |