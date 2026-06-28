#!/usr/bin/env python3
"""生成 CN-Codex 技术讲解 PPTX 演示文稿"""

from pptx import Presentation
from pptx.util import Inches, Pt, Emu
from pptx.dml.color import RGBColor
from pptx.enum.text import PP_ALIGN, MSO_ANCHOR
from pptx.enum.shapes import MSO_SHAPE
import os

# ============ 色彩体系（匹配项目 DESIGN.md） ============
ACCENT = RGBColor(0x22, 0xC5, 0x5E)        # 翡翠绿
ACCENT_STRONG = RGBColor(0x4A, 0xDE, 0x80)   # 亮绿
ACCENT_SOFT = RGBColor(0x1A, 0x3A, 0x2A)     # 暗绿底
BG_DARK = RGBColor(0x1A, 0x1A, 0x1A)         # 背景黑
BG_PANEL = RGBColor(0x26, 0x26, 0x26)        # 面板灰
BG_CARD = RGBColor(0x1E, 0x1E, 0x1E)         # 卡片黑
TEXT_STRONG = RGBColor(0xF0, 0xF0, 0xF0)     # 白色文字
TEXT_BASE = RGBColor(0xC8, 0xC8, 0xC8)       # 浅灰文字
TEXT_MUTED = RGBColor(0x88, 0x88, 0x88)      # 中灰文字
TEXT_SUBTLE = RGBColor(0xAA, 0xAA, 0xAA)     # 辅助文字
WHITE = RGBColor(0xFF, 0xFF, 0xFF)
LINE = RGBColor(0x33, 0x33, 0x33)            # 分隔线
BORDER = RGBColor(0x3A, 0x3A, 0x3A)

prs = Presentation()
prs.slide_width = Inches(13.333)   # 16:9 宽屏
prs.slide_height = Inches(7.5)

SLIDE_W = Inches(13.333)
SLIDE_H = Inches(7.5)

# ============ 工具函数 ============

def set_slide_bg(slide, color=BG_DARK):
    """设置幻灯片背景色"""
    bg = slide.background
    fill = bg.fill
    fill.solid()
    fill.fore_color.rgb = color

def add_textbox(slide, left, top, width, height, text, font_size=14,
                bold=False, color=TEXT_BASE, alignment=PP_ALIGN.LEFT,
                font_name="Segoe UI", line_spacing=1.2):
    """添加文本框"""
    txBox = slide.shapes.add_textbox(left, top, width, height)
    tf = txBox.text_frame
    tf.word_wrap = True
    p = tf.paragraphs[0]
    p.text = text
    p.font.size = Pt(font_size)
    p.font.bold = bold
    p.font.color.rgb = color
    p.font.name = font_name
    p.alignment = alignment
    p.space_after = Pt(2)
    if line_spacing != 1.0:
        p.line_spacing = Pt(font_size * line_spacing)
    return txBox

def add_bullet_text(slide, left, top, width, height, items, font_size=13,
                    color=TEXT_BASE, font_name="Segoe UI", bold_first=False,
                    bullet_char="\u25CF", line_spacing=1.3):
    """添加带符号的列表"""
    txBox = slide.shapes.add_textbox(left, top, width, height)
    tf = txBox.text_frame
    tf.word_wrap = True
    for i, item in enumerate(items):
        if i == 0:
            p = tf.paragraphs[0]
        else:
            p = tf.add_paragraph()
        p.text = f"{bullet_char} {item}"
        p.font.size = Pt(font_size)
        p.font.color.rgb = color
        p.font.name = font_name
        p.space_after = Pt(6)
        p.line_spacing = Pt(font_size * line_spacing)
        if bold_first and i == 0:
            p.font.bold = True
    return txBox

def add_section_header(slide, number, title, subtitle=None):
    """添加章节标题页"""
    # 顶部装饰线
    line = slide.shapes.add_shape(
        MSO_SHAPE.RECTANGLE, Inches(0.8), Inches(1.6), Inches(1.5), Pt(3)
    )
    line.fill.solid()
    line.fill.fore_color.rgb = ACCENT
    line.line.fill.background()

    # 章节编号
    add_textbox(slide, Inches(0.8), Inches(1.85), Inches(2), Inches(0.6),
                f"0{number}" if number < 10 else str(number),
                font_size=16, bold=True, color=ACCENT, font_name="Segoe UI")

    # 标题
    add_textbox(slide, Inches(0.8), Inches(2.5), Inches(11), Inches(1.2),
                title, font_size=36, bold=True, color=TEXT_STRONG,
                font_name="Segoe UI", line_spacing=1.1)

    # 副标题
    if subtitle:
        add_textbox(slide, Inches(0.8), Inches(3.5), Inches(10), Inches(1.0),
                    subtitle, font_size=16, color=TEXT_MUTED,
                    font_name="Segoe UI")

def add_accent_bar(slide, left, top, width=Pt(3), height=Inches(0.6)):
    """添加强调色条"""
    bar = slide.shapes.add_shape(MSO_SHAPE.RECTANGLE, left, top, width, height)
    bar.fill.solid()
    bar.fill.fore_color.rgb = ACCENT
    bar.line.fill.background()
    return bar

def add_card(slide, left, top, width, height, title, items, accent=True):
    """添加技术卡片"""
    card = slide.shapes.add_shape(
        MSO_SHAPE.ROUNDED_RECTANGLE, left, top, width, height
    )
    card.fill.solid()
    card.fill.fore_color.rgb = BG_PANEL
    card.line.color.rgb = BORDER
    card.line.width = Pt(0.5)

    if accent:
        bar = slide.shapes.add_shape(
            MSO_SHAPE.RECTANGLE, left + Inches(0.02), top + Inches(0.15),
            Pt(2.5), height - Inches(0.3)
        )
        bar.fill.solid()
        bar.fill.fore_color.rgb = ACCENT
        bar.line.fill.background()

    add_textbox(slide, left + Inches(0.2), top + Inches(0.15),
                width - Inches(0.4), Inches(0.4),
                title, font_size=14, bold=True, color=ACCENT_STRONG)

    add_bullet_text(slide, left + Inches(0.2), top + Inches(0.55),
                    width - Inches(0.4), height - Inches(0.7),
                    items, font_size=11, color=TEXT_SUBTLE, bullet_char="-")

def add_footer(slide, page_num, total=30):
    """添加页脚"""
    # 分隔线
    line = slide.shapes.add_shape(
        MSO_SHAPE.RECTANGLE, Inches(0.6), Inches(7.0), Inches(12.1), Pt(0.5)
    )
    line.fill.solid()
    line.fill.fore_color.rgb = LINE
    line.line.fill.background()

    add_textbox(slide, Inches(0.6), Inches(7.05), Inches(6), Inches(0.4),
                "CN-Codex 技术详解", font_size=9, color=TEXT_MUTED)

    add_textbox(slide, Inches(11), Inches(7.05), Inches(1.5), Inches(0.4),
                f"{page_num} / {total}", font_size=9, color=TEXT_MUTED,
                alignment=PP_ALIGN.RIGHT)


# ============================================================
# 第1页：封面
# ============================================================
slide = prs.slides.add_slide(prs.slide_layouts[6])  # 空白
set_slide_bg(slide)

# 装饰圆
for i in range(3):
    circle = slide.shapes.add_shape(
        MSO_SHAPE.OVAL, Inches(9.5 + i*1.8), Inches(-1.5), Inches(5), Inches(5)
    )
    circle.fill.solid()
    circle.fill.fore_color.rgb = RGBColor(0x15, 0x2A, 0x1A)
    circle.line.fill.background()
    circle.fill.fore_color.brightness = 0.0

# 顶部装饰条
add_accent_bar(slide, Inches(0.8), Inches(2.0), width=Inches(1.2))

# 主标题
add_textbox(slide, Inches(0.8), Inches(2.3), Inches(10), Inches(1.2),
            "CN-Codex", font_size=52, bold=True, color=TEXT_STRONG)

# 副标题
add_textbox(slide, Inches(0.8), Inches(3.4), Inches(10), Inches(0.8),
            "AI 驱动的全栈编程助手桌面应用", font_size=24, color=ACCENT_STRONG)

# 标语
add_textbox(slide, Inches(0.8), Inches(4.5), Inches(10), Inches(1.5),
            "手机远程控制  ·  机器人自动化  ·  全流程开发\n基于 Tauri 2 + React 18 + Rust，已在 DeepSeek v4 Flash 完成验证",
            font_size=13, color=TEXT_MUTED)

# 底部信息
add_textbox(slide, Inches(0.8), Inches(6.5), Inches(8), Inches(0.4),
            "v0.1.0  |  2025  |  Apache License 2.0",
            font_size=10, color=TEXT_MUTED)


# ============================================================
# 第2页：目录
# ============================================================
slide = prs.slides.add_slide(prs.slide_layouts[6])
set_slide_bg(slide)

add_section_header(slide, 0, "目录", "15 个章节，27 个技术模块")

toc_items = [
    ("01", "项目背景与设计理念"),
    ("02", "技术栈全景"),
    ("03", "整体架构与通信模型"),
    ("04", "前端架构详解"),
    ("05", "Rust 后端核心引擎"),
    ("06", "LLM 适配层"),
    ("07", "数据持久化"),
    ("08", "移动端远程控制"),
    ("09", "外部浏览器与录制回放"),
    ("10", "SmartBrain 智能脑"),
    ("11", "机器人系统"),
    ("12", "插件系统"),
    ("13", "Hook 运行时与子代理"),
    ("14", "本地资源池与 OCR"),
    ("15", "构建与发布"),
]

for i, (num, name) in enumerate(toc_items):
    row, col = divmod(i, 5)
    left = Inches(0.8 + col * 2.5)
    top = Inches(4.0 + row * 0.65)

    add_textbox(slide, left, top, Inches(0.4), Inches(0.4),
                num, font_size=12, bold=True, color=ACCENT,
                alignment=PP_ALIGN.RIGHT)

    add_textbox(slide, left + Inches(0.45), top, Inches(1.8), Inches(0.4),
                name, font_size=11, color=TEXT_BASE)

    # 分割虚线
    if row < 2:
        dot = slide.shapes.add_shape(
            MSO_SHAPE.RECTANGLE, left, top + Inches(0.45),
            Inches(2.2), Pt(0.5)
        )
        dot.fill.solid()
        dot.fill.fore_color.rgb = LINE
        dot.line.fill.background()

add_footer(slide, 2)


# ============================================================
# 第3页：项目背景
# ============================================================
slide = prs.slides.add_slide(prs.slide_layouts[6])
set_slide_bg(slide)
add_section_header(slide, 1, "项目背景与设计理念")

add_card(slide, Inches(0.8), Inches(4.0), Inches(5.5), Inches(2.8),
         "核心定位", [
             "基于 Tauri 2 的桌面原生 AI 编程助手",
             "多轮对话 + 工具调用 + Goal 模式 + 机器人自动化",
             "手机扫码远程监控和控制 AI 助手",
             "全流程覆盖：创建 → 编码 → 测试 → 部署",
         ])

add_card(slide, Inches(6.8), Inches(4.0), Inches(5.5), Inches(2.8),
         "设计理念", [
             "深色优先：IDE 原生暗色界面，无视觉噪音",
             "中高密度布局：紧凑可呼吸，不松散",
             "去阴影化：1px 轮廓线替代阴影",
             "中英文双语默认中文，运行时切换",
         ])

add_footer(slide, 3)


# ============================================================
# 第4页：核心亮点
# ============================================================
slide = prs.slides.add_slide(prs.slide_layouts[6])
set_slide_bg(slide)
add_section_header(slide, 1, "核心亮点")

highlights = [
    ("手机远程控制", "WebSocket 实时同步\nQR 码扫码连接\n局域网直连 / 公网中转"),
    ("机器人系统", "AI 自动创建专业角色\n技能绑定 + 工作流编排\n多机器人协作流水线"),
    ("操作录制回放", "Chrome CDP 协议捕获\n实时注入 Runtime.addBinding\n生成可回放工作流"),
    ("SmartBrain 智能脑", "从历史会话提取经验\nBM25 知识索引\n自动总结并复用知识"),
]

for i, (title, desc) in enumerate(highlights):
    left = Inches(0.8 + i * 3.1)
    top = Inches(4.0)

    card = slide.shapes.add_shape(
        MSO_SHAPE.ROUNDED_RECTANGLE, left, top, Inches(2.8), Inches(2.8)
    )
    card.fill.solid()
    card.fill.fore_color.rgb = BG_PANEL
    card.line.color.rgb = BORDER
    card.line.width = Pt(0.5)

    add_accent_bar(slide, left + Inches(0.15), top + Inches(0.15))

    add_textbox(slide, left + Inches(0.25), top + Inches(0.2),
                Inches(2.3), Inches(0.4),
                title, font_size=15, bold=True, color=ACCENT_STRONG)

    add_textbox(slide, left + Inches(0.25), top + Inches(0.7),
                Inches(2.3), Inches(1.8),
                desc, font_size=11, color=TEXT_SUBTLE, line_spacing=1.5)

add_footer(slide, 4)


# ============================================================
# 第5页：技术栈全景
# ============================================================
slide = prs.slides.add_slide(prs.slide_layouts[6])
set_slide_bg(slide)
add_section_header(slide, 2, "技术栈全景")

# 前端
add_textbox(slide, Inches(0.8), Inches(3.8), Inches(3), Inches(0.4),
            "前端 (React 18 + TypeScript 5.7)", font_size=14, bold=True, color=ACCENT_STRONG)

frontend_items = [
    "Vite 6 构建（三入口：main/detail/diff）",
    "Zustand 5 状态管理",
    "Tailwind CSS 4 + react-intl 国际化",
    "react-markdown + xterm.js 终端模拟",
    "Vitest 4 单元测试",
]
add_bullet_text(slide, Inches(0.8), Inches(4.2), Inches(5.5), Inches(2.5),
                frontend_items, font_size=11, color=TEXT_SUBTLE)

# 后端
add_textbox(slide, Inches(6.8), Inches(3.8), Inches(5), Inches(0.4),
            "后端 (Rust 1.85+ Edition 2024)", font_size=14, bold=True, color=ACCENT_STRONG)

backend_items = [
    "Tauri 2 桌面框架 + 6 个官方插件",
    "tokio 异步 + reqwest HTTP + axum Web 服务器",
    "rusqlite SQLite + serde 序列化 + toml 配置",
    "portable-pty 终端 + ONNX Runtime OCR",
    "tracing 日志 + uuid + chrono",
]
add_bullet_text(slide, Inches(6.8), Inches(4.2), Inches(5.5), Inches(2.5),
                backend_items, font_size=11, color=TEXT_SUBTLE)

add_footer(slide, 5)


# ============================================================
# 第6页：整体架构
# ============================================================
slide = prs.slides.add_slide(prs.slide_layouts[6])
set_slide_bg(slide)
add_section_header(slide, 3, "整体架构")

# 架构图 - 用文本框组合表示
arch_items = [
    "┌──────────────────────────────────────────────┐",
    "│              CN-Codex 桌面应用                │",
    "├───────────────────┬──────────────────────────┤",
    "│  前端 React       │     后端 Rust / Tauri     │",
    "│  App.tsx (根布局) │     lib.rs (初始化)       │",
    "│  ChatPage (聊天)  │←→  AgentEngine (核心)     │",
    "│  MessageList (流) │IPC  ToolExecutor (工具)   │",
    "│  Sidebar (侧栏)   │    ThreadStore (会话)     │",
    "│  SettingsPanel    │    ConfigSystem (配置)    │",
    "│  Zustand Store    │    Adapter (4种API格式)   │",
    "├───────────────────┼──────────────────────────┤",
    "│  Mobile Web 移动端 │←→ SmartBrain (经验+知识) │",
    "│  QR 扫码连接      │    MobileServer (Axum)   │",
    "│                    │    ExternalBrowser(CDP)  │",
    "└───────────────────┴──────────────────────────┘",
]

for i, line in enumerate(arch_items):
    color = ACCENT_STRONG if "CN-Codex" in line else (
        TEXT_BASE if "┌" in line or "┐" in line or "├" in line or "┤" in line or "└" in line or "┘" in line else TEXT_SUBTLE
    )
    add_textbox(slide, Inches(1.5), Inches(3.6 + i * 0.28), Inches(10.5), Inches(0.3),
                line, font_size=10, color=color, font_name="Consolas")

# 通信模型标注
add_textbox(slide, Inches(1.5), Inches(6.9), Inches(10), Inches(0.4),
            "通信: 前端 ←Tauri IPC→ Rust 后端 ←HTTP SSE→ LLM API",
            font_size=10, color=TEXT_MUTED)

add_footer(slide, 6)


# ============================================================
# 第7页：前端架构
# ============================================================
slide = prs.slides.add_slide(prs.slide_layouts[6])
set_slide_bg(slide)
add_section_header(slide, 4, "前端架构详解")

# 左侧 - 组件树
add_textbox(slide, Inches(0.8), Inches(3.8), Inches(3), Inches(0.4),
            "组件层级", font_size=13, bold=True, color=ACCENT_STRONG)

tree = [
    "App.tsx — 根组件 (布局 + 初始化 + 拖拽)",
    "  ├─ TitleBar — 自定义标题栏",
    "  ├─ Sidebar — 会话/项目列表",
    "  ├─ app-main — 聊天区",
    "  │   ├─ MessageList — 流式消息",
    "  │   ├─ ChatInput — 输入框",
    "  │   └─ PatchDiffModal — Patch 审阅",
    "  ├─ RightPanel — 右侧面板",
    "  │   └─ Terminal/Git/FileTree/Browser",
    "  ├─ StatusBar — 状态栏",
    "  └─ SettingsPanel — 设置面板",
]

for i, line in enumerate(tree):
    color = ACCENT_STRONG if "App.tsx" in line else TEXT_SUBTLE
    add_textbox(slide, Inches(0.8), Inches(4.2 + i * 0.28), Inches(5.5), Inches(0.3),
                line, font_size=10, color=color, font_name="Consolas")

# 右侧 - 状态管理
add_textbox(slide, Inches(7.0), Inches(3.8), Inches(5), Inches(0.4),
            "Zustand 状态管理 (appStore.ts ~1300行)", font_size=13, bold=True, color=ACCENT_STRONG)

state_items = [
    "会话: threads, messages, streamingText",
    "项目: projects, currentProjectId",
    "模型: providers, activeModelId",
    "布局: sidebarWidth, rightPanelWidth",
    "运行时: isStreaming, isInitialized",
    "审批: pendingFileReviews, pendingApproval",
]
add_bullet_text(slide, Inches(7.0), Inches(4.2), Inches(5.5), Inches(2.0),
                state_items, font_size=11, color=TEXT_SUBTLE, bullet_char="·")

# 下方 - 国际化
add_textbox(slide, Inches(7.0), Inches(5.8), Inches(5), Inches(0.4),
            "国际化 (react-intl)", font_size=13, bold=True, color=ACCENT_STRONG)

add_textbox(slide, Inches(7.0), Inches(6.2), Inches(5), Inches(0.5),
            "zh-CN / en-US 双语言 | 默认中文 | 运行时切换",
            font_size=11, color=TEXT_SUBTLE)

add_footer(slide, 7)


# ============================================================
# 第8页：Rust 后端架构
# ============================================================
slide = prs.slides.add_slide(prs.slide_layouts[6])
set_slide_bg(slide)
add_section_header(slide, 5, "Rust 后端核心架构")

backends = [
    ("AppState (全局状态)", [
        "thread_store, agent_engine, config_manager",
        "usage_db, pricing_table",
        "file_review_sessions, external_browser",
        "standalone, locale, cwd",
    ]),
    ("AgentEngine (Agent 引擎)", [
        "run_turn() — 核心处理循环",
        "LLM API 流式通信 + 工具调用",
        "Hook 运行时集成 (7种事件)",
        "Goal/Plan/Robot-Create 多种模式",
    ]),
    ("ToolExecutor (工具执行器)", [
        "40+ 内置工具 (shell/file/git/browser)",
        "MCP 服务器收发",
        "子代理创建与管理",
        "文件变更追踪与 Diff 生成",
    ]),
    ("ConfigSystem (配置管理)", [
        "TOML 格式读写",
        "供应商/model/MCP/资源池管理",
        "SmartBrain/Hook 配置",
        "运行时热更新 (apply_edit)",
    ]),
]

for i, (title, items) in enumerate(backends):
    left = Inches(0.8 + (i % 2) * 6.2)
    top = Inches(4.0 + (i // 2) * 1.6)

    add_textbox(slide, left, top, Inches(5.5), Inches(0.3),
                title, font_size=13, bold=True, color=ACCENT_STRONG)
    add_bullet_text(slide, left, top + Inches(0.35), Inches(5.5), Inches(1.2),
                    items, font_size=10, color=TEXT_SUBTLE, bullet_char="·")

add_footer(slide, 8)


# ============================================================
# 第9页：AgentEngine 核心引擎
# ============================================================
slide = prs.slides.add_slide(prs.slide_layouts[6])
set_slide_bg(slide)
add_section_header(slide, 5, "AgentEngine 核心引擎")

# run_turn 流程
add_textbox(slide, Inches(0.8), Inches(3.8), Inches(5), Inches(0.4),
            "run_turn() 执行流程", font_size=13, bold=True, color=ACCENT_STRONG)

flow = [
    "1. 解析输入模式 (chat/goal/plan/robot)",
    "2. 读取配置 (model/provider/pool)",
    "3. 构建 system prompt + 注入规则/Hook/SmartBrain",
    "4. LLM API 流式通信 (HTTP SSE)",
    "5. 流式解析: TextDelta / ToolCallDelta / Done",
    "6. 执行工具调用 (同步/审批模式)",
    "7. 文件变更追踪 + 压缩检查",
    "8. 发射 turn-completed 事件",
    "9. Goal 模式: 自动循环继续",
]
add_bullet_text(slide, Inches(0.8), Inches(4.2), Inches(5.5), Inches(2.5),
                flow, font_size=11, color=TEXT_SUBTLE, bullet_char="→")

# 中断机制
add_textbox(slide, Inches(7.0), Inches(3.8), Inches(5), Inches(0.4),
            "中断与取消机制", font_size=13, bold=True, color=ACCENT_STRONG)

cancel_items = [
    "AtomicBool cancel_flag — 优雅停止",
    "interrupt() — 设置标志 + 终止子进程",
    "interrupt_active_tools() — 进程级中断",
    "interrupt_all_active_tools() — 全局停止",
]
add_bullet_text(slide, Inches(7.0), Inches(4.2), Inches(5.5), Inches(1.2),
                cancel_items, font_size=11, color=TEXT_SUBTLE, bullet_char="·")

# 压缩机制
add_textbox(slide, Inches(7.0), Inches(5.5), Inches(5), Inches(0.4),
            "上下文压缩 (Compaction)", font_size=13, bold=True, color=ACCENT_STRONG)

comp_items = [
    "阈值: context_window × 90% (默认115K tokens)",
    "触发点: turn-start / mid-turn / goal-continuation",
    "LLM 生成摘要 → 保留最近20K tokens + 摘要",
    "支持手动触发: 用户输入 /compact",
]
add_bullet_text(slide, Inches(7.0), Inches(5.9), Inches(5.5), Inches(1.2),
                comp_items, font_size=11, color=TEXT_SUBTLE, bullet_char="·")

add_footer(slide, 9)


# ============================================================
# 第10页：LLM 适配层
# ============================================================
slide = prs.slides.add_slide(prs.slide_layouts[6])
set_slide_bg(slide)
add_section_header(slide, 6, "LLM 适配层 (Adapter)")

add_textbox(slide, Inches(0.8), Inches(3.8), Inches(11), Inches(0.4),
            "ProviderAdapter Trait — 统一接口，四种实现", font_size=15, bold=True, color=ACCENT_STRONG)

# 四种适配器
adapters = [
    ("ChatCompletionsAdapter", "OpenAI Chat\n格式", [
        "默认 API 格式",
        "OpenAI / DeepSeek / Qwen",
    ]),
    ("ResponsesAdapter", "OpenAI Responses\nAPI", [
        "OpenAI 新版 API",
        "支持流式工具调用",
    ]),
    ("AnthropicAdapter", "Anthropic\nMessages API", [
        "Claude 系列模型",
        "Content Block 格式",
    ]),
    ("GoogleAdapter", "Google Gemini\nAPI", [
        "Gemini 系列模型",
        "InlineData 图片支持",
    ]),
]

for i, (name, api_type, features) in enumerate(adapters):
    left = Inches(0.8 + i * 3.1)

    add_textbox(slide, left, Inches(4.5), Inches(2.8), Inches(0.4),
                name, font_size=11, bold=True, color=ACCENT_STRONG)
    add_textbox(slide, left, Inches(4.9), Inches(2.8), Inches(0.5),
                api_type, font_size=10, color=TEXT_MUTED)
    add_bullet_text(slide, left, Inches(5.4), Inches(2.8), Inches(1.2),
                    features, font_size=10, color=TEXT_SUBTLE, bullet_char="·")

# 底部 - 统一事件流
add_textbox(slide, Inches(0.8), Inches(6.6), Inches(11), Inches(0.5),
            "统一 StreamEvent: TextDelta | ToolCallDelta | Done | Usage  →  agent.rs 统一处理",
            font_size=11, color=TEXT_MUTED)

add_footer(slide, 10)


# ============================================================
# 第11页：LLM 流式处理
# ============================================================
slide = prs.slides.add_slide(prs.slide_layouts[6])
set_slide_bg(slide)
add_section_header(slide, 6, "LLM 流式处理与工具调用")

# 流式处理
add_textbox(slide, Inches(0.8), Inches(3.8), Inches(5), Inches(0.4),
            "HTTP SSE 流式处理", font_size=14, bold=True, color=ACCENT_STRONG)

sse_flow = [
    "1. POST 请求带 messages + tools",
    "2. SSE stream: bytes_stream()",
    "3. 逐行解析: buffer + \\n 分割",
    "4. StreamEvent 分发:",
    "   - TextDelta → 追加到 text buffer",
    "   - ToolCallDelta → 碎片拼接",
    "   - Done → 设置 finish_reason",
    "   - Usage → 收集 token 用量",
]
add_bullet_text(slide, Inches(0.8), Inches(4.2), Inches(5.5), Inches(2.5),
                sse_flow, font_size=11, color=TEXT_SUBTLE, bullet_char="→")

# 工具调用
add_textbox(slide, Inches(7.0), Inches(3.8), Inches(5), Inches(0.4),
            "工具调用累积器", font_size=14, bold=True, color=ACCENT_STRONG)

tool_flow = [
    "ToolCallAccumulator { id, name, arguments }",
    "SSE 碎片逐步拼接完整 tool call",
    "支持同时多个工具调用 (index 区分)",
    "最终调用 ToolExecutor.execute()",
    "执行模式: Sync / Approve",
    "结果格式: ToolCallResult { output, changes }",
]
add_bullet_text(slide, Inches(7.0), Inches(4.2), Inches(5.5), Inches(2.5),
                tool_flow, font_size=11, color=TEXT_SUBTLE, bullet_char="·")

add_footer(slide, 11)


# ============================================================
# 第12页：数据持久化
# ============================================================
slide = prs.slides.add_slide(prs.slide_layouts[6])
set_slide_bg(slide)
add_section_header(slide, 7, "数据持久化")

persist_items = [
    ("SQLite (AppState/Usage)", [
        "app_state: KV 存储 (项目/模型/布局设置)",
        "usage.db: token 用量 + 费用统计",
        "PricingTable: 按供应商/模型定价",
    ]),
    ("JSONL (ThreadStore 会话)", [
        "sessions/ 目录下 .jsonl 文件",
        "每行一个消息: role/content/toolCalls",
        "压缩: 摘要替换历史消息",
    ]),
    ("TOML (ConfigSystem 配置)", [
        "config.toml: 模型/供应商/MCP/Hook/SmartBrain",
        "运行时热更新: apply_edit() 方法",
        "内置预设: OpenAI/DeepSeek/Qwen/Volcengine",
    ]),
    ("文件系统 (运行时资源)", [
        "skills/ — 60+ 技能定义 (SKILL.md)",
        "plugins/ — 8 个内置插件",
        "memories/ — SmartBrain 经验+知识",
        "workflows/ — 工作流定义",
    ]),
]

for i, (title, items) in enumerate(persist_items):
    left = Inches(0.8 + (i % 2) * 6.2)
    top = Inches(4.0 + (i // 2) * 1.5)

    add_textbox(slide, left, top, Inches(5.5), Inches(0.3),
                title, font_size=13, bold=True, color=ACCENT_STRONG)
    add_bullet_text(slide, left, top + Inches(0.35), Inches(5.5), Inches(1.0),
                    items, font_size=10, color=TEXT_SUBTLE, bullet_char="·")

add_footer(slide, 12)


# ============================================================
# 第13页：移动端远程控制
# ============================================================
slide = prs.slides.add_slide(prs.slide_layouts[6])
set_slide_bg(slide)
add_section_header(slide, 8, "移动端远程控制")

# 左侧 - 架构
add_textbox(slide, Inches(0.8), Inches(3.8), Inches(5), Inches(0.4),
            "技术架构", font_size=14, bold=True, color=ACCENT_STRONG)

mobile_arch = [
    "Axum 0.8 — 高性能异步 Web 服务器",
    "tokio-tungstenite — WebSocket 实时通信",
    "axum::extract::ws — 原生 WebSocket 支持",
    "broadcast::Sender — 事件广播到所有移动端",
    "QR 码生成 (qrcode + svg) — 扫码即连",
    "双连接模式: 局域网直连 / 公网中转",
]
add_bullet_text(slide, Inches(0.8), Inches(4.2), Inches(5.5), Inches(2.0),
                mobile_arch, font_size=11, color=TEXT_SUBTLE, bullet_char="·")

# 右侧 - API
add_textbox(slide, Inches(7.0), Inches(3.8), Inches(5), Inches(0.4),
            "移动端 API (REST + WebSocket)", font_size=14, bold=True, color=ACCENT_STRONG)

api_items = [
    "GET /health — 健康检查",
    "GET /api/threads — 会话列表",
    "GET /api/threads/{id}/messages — 消息",
    "POST /api/threads/{id}/chat — 发送消息",
    "POST /api/threads/{id}/interrupt — 中断",
    "WS /ws — WebSocket 实时事件推送",
]
add_bullet_text(slide, Inches(7.0), Inches(4.2), Inches(5.5), Inches(2.0),
                api_items, font_size=11, color=TEXT_SUBTLE, bullet_char="·")

add_footer(slide, 13)


# ============================================================
# 第14页：外部浏览器与录制
# ============================================================
slide = prs.slides.add_slide(prs.slide_layouts[6])
set_slide_bg(slide)
add_section_header(slide, 9, "外部浏览器与操作录制")

# 外部浏览器
add_textbox(slide, Inches(0.8), Inches(3.8), Inches(5.5), Inches(0.4),
            "ExternalBrowser — Chrome CDP 控制", font_size=14, bold=True, color=ACCENT_STRONG)

browser_flow = [
    "1. 自动检测 Chrome / Edge (Windows/macOS/Linux)",
    "2. 启动 `--remote-debugging-port=9222`",
    "3. 临时 user-data-dir 隔离",
    "4. CDP WebSocket 连接 (tokio-tungstenite)",
    "5. 支持: navigate / click / type / evaluate / screenshot",
    "6. 自动销毁会话 (Kill 子进程)",
]
add_bullet_text(slide, Inches(0.8), Inches(4.2), Inches(5.5), Inches(2.0),
                browser_flow, font_size=11, color=TEXT_SUBTLE, bullet_char="→")

# 录制回放
add_textbox(slide, Inches(7.0), Inches(3.8), Inches(5.5), Inches(0.4),
            "Recorder — 操作录制与回放", font_size=14, bold=True, color=ACCENT_STRONG)

rec_flow = [
    "Runtime.addBinding('__rr_push') — 实时推送事件",
    "JS 注入: 捕获 click/input/submit/navigate",
    "locators(): 智能选择器生成 (id/aria-label/name)",
    "分散事件累积: Arc<Mutex<Vec<RecordingEvent>>>",
    "输出: TraceFile JSON (事件序列 + 截图路径)",
]
add_bullet_text(slide, Inches(7.0), Inches(4.2), Inches(5.5), Inches(2.0),
                rec_flow, font_size=11, color=TEXT_SUBTLE, bullet_char="→")

add_footer(slide, 14)


# ============================================================
# 第15页：SmartBrain 智能脑
# ============================================================
slide = prs.slides.add_slide(prs.slide_layouts[6])
set_slide_bg(slide)
add_section_header(slide, 10, "SmartBrain 智能脑")

add_textbox(slide, Inches(0.8), Inches(3.8), Inches(11), Inches(0.4),
            "启动流水线 (App 启动时自动执行)", font_size=14, bold=True, color=ACCENT_STRONG)

pipeline = [
    "1. Extractor (提取器) → 从历史会话提取经验 (每次最多5条, 最少3条消息)",
    "2. Consolidator (合并器) → 相似经验自动合并去重",
    "3. Knowledge Scanner (知识扫描) → 扫描并索引知识文档",
    "4. BM25 Index Builder → 构建倒排索引 → OKF 格式存储",
]
add_bullet_text(slide, Inches(0.8), Inches(4.2), Inches(11), Inches(1.5),
                pipeline, font_size=12, color=TEXT_SUBTLE, bullet_char="▸")

# 管理命令
add_textbox(slide, Inches(0.8), Inches(5.6), Inches(5), Inches(0.4),
            "经验管理", font_size=13, bold=True, color=ACCENT_STRONG)
exp_items = [
    "smartbrain_list_experiences",
    "smartbrain_read_experience",
    "smartbrain_delete_experience",
]
add_bullet_text(slide, Inches(0.8), Inches(5.95), Inches(5), Inches(1.0),
                exp_items, font_size=11, color=TEXT_SUBTLE, bullet_char="·")

add_textbox(slide, Inches(6.8), Inches(5.6), Inches(5), Inches(0.4),
            "知识管理", font_size=13, bold=True, color=ACCENT_STRONG)
know_items = [
    "smartbrain_list_knowledge",
    "smartbrain_search (BM25)",
    "smartbrain_upload_knowledge",
]
add_bullet_text(slide, Inches(6.8), Inches(5.95), Inches(5), Inches(1.0),
                know_items, font_size=11, color=TEXT_SUBTLE, bullet_char="·")

add_footer(slide, 15)


# ============================================================
# 第16页：机器人系统
# ============================================================
slide = prs.slides.add_slide(prs.slide_layouts[6])
set_slide_bg(slide)
add_section_header(slide, 11, "机器人系统")

add_textbox(slide, Inches(0.8), Inches(3.8), Inches(11), Inches(0.4),
            "多机器人协作流水线", font_size=14, bold=True, color=ACCENT_STRONG)

# 水平流
robots = ["需求分析\n机器人", "架构设计\n机器人", "前端开发\n机器人", "后端开发\n机器人", "测试\n机器人"]
for i, name in enumerate(robots):
    left = Inches(0.8 + i * 2.5)
    top = Inches(4.5)

    card = slide.shapes.add_shape(
        MSO_SHAPE.ROUNDED_RECTANGLE, left, top, Inches(2.1), Inches(1.0)
    )
    card.fill.solid()
    card.fill.fore_color.rgb = ACCENT_SOFT
    card.line.color.rgb = ACCENT
    card.line.width = Pt(0.5)

    add_textbox(slide, left, top + Inches(0.15), Inches(2.1), Inches(0.7),
                name, font_size=11, bold=True, color=ACCENT_STRONG,
                alignment=PP_ALIGN.CENTER)

    if i < len(robots) - 1:
        arrow = slide.shapes.add_shape(
            MSO_SHAPE.RIGHT_ARROW, left + Inches(2.15), top + Inches(0.3),
            Inches(0.3), Inches(0.4)
        )
        arrow.fill.solid()
        arrow.fill.fore_color.rgb = ACCENT
        arrow.line.fill.background()

# 关键技术
add_textbox(slide, Inches(0.8), Inches(5.8), Inches(11), Inches(0.4),
            "关键技术", font_size=13, bold=True, color=ACCENT_STRONG)

robot_tech = [
    "RobotOrchestrator — 编排器，管理节点推进和 goal 绑定",
    "<workflow_node_done/> 标记 — 模型输出自动检测节点完成",
    "robot_overlay_prompt — 注入当前节点要求和技能到 system prompt",
    "内置 3 个机器人: fullstack-dev-bot / qa-test-bot / requirements-design-bot",
]
add_bullet_text(slide, Inches(0.8), Inches(6.15), Inches(11), Inches(1.0),
                robot_tech, font_size=11, color=TEXT_SUBTLE, bullet_char="·")

add_footer(slide, 16)


# ============================================================
# 第17页：插件系统
# ============================================================
slide = prs.slides.add_slide(prs.slide_layouts[6])
set_slide_bg(slide)
add_section_header(slide, 12, "插件系统")

plugins = [
    ("browser", "应用内浏览器控制"),
    ("computer-use", "Windows 桌面应用控制"),
    ("documents", "Word 文档创建编辑"),
    ("presentations", "PPT 创建编辑"),
    ("record-replay", "浏览器操作录制回放"),
    ("sites", "网站创建构建托管"),
    ("spreadsheets", "Excel 电子表格操作"),
    ("superpowers", "增强能力集 (调试/审查/验证)"),
]

for i, (name, desc) in enumerate(plugins):
    row, col = divmod(i, 4)
    left = Inches(0.8 + col * 3.1)

    card = slide.shapes.add_shape(
        MSO_SHAPE.ROUNDED_RECTANGLE, left, Inches(3.8 + row * 1.4),
        Inches(2.8), Inches(1.1)
    )
    card.fill.solid()
    card.fill.fore_color.rgb = BG_PANEL
    card.line.color.rgb = BORDER
    card.line.width = Pt(0.5)

    add_textbox(slide, left + Inches(0.2), Inches(3.95 + row * 1.4),
                Inches(2.4), Inches(0.4),
                name, font_size=14, bold=True, color=ACCENT_STRONG)
    add_textbox(slide, left + Inches(0.2), Inches(4.25 + row * 1.4),
                Inches(2.4), Inches(0.4),
                desc, font_size=10, color=TEXT_SUBTLE)

add_textbox(slide, Inches(0.8), Inches(6.7), Inches(11), Inches(0.4),
            "App Connectors: 插件暴露 MCP 工具集，通过 app://{connector_id} 触发",
            font_size=11, color=TEXT_MUTED)

add_footer(slide, 17)


# ============================================================
# 第18页：Hook 运行时
# ============================================================
slide = prs.slides.add_slide(prs.slide_layouts[6])
set_slide_bg(slide)
add_section_header(slide, 13, "Hook 运行时")

# 7种事件
events = [
    ("on-agent-start", "Agent 开始前", "前置检查"),
    ("on-user-prompt-submit", "用户提交消息", "输入过滤/增强"),
    ("on-agent-end", "Agent 完成时", "结果验证"),
    ("on-file-change", "文件被修改", "变更审计"),
    ("on-command-exec", "命令执行前", "命令审批/拦截"),
    ("on-post-tool-use", "工具执行后", "结果审查"),
    ("on-subagent-stop", "子代理关闭", "结果审查"),
]

# 表格 header
headers = ["事件名", "触发时机", "典型用途"]
for j, h in enumerate(headers):
    left = Inches(0.8 + j * 3.5)
    add_textbox(slide, left, Inches(3.8), Inches(3.2), Inches(0.4),
                h, font_size=12, bold=True, color=ACCENT_STRONG)

for i, (event, timing, usage) in enumerate(events):
    row_top = Inches(4.25 + i * 0.38)
    add_textbox(slide, Inches(0.8), row_top, Inches(3.2), Inches(0.35),
                event, font_size=11, color=TEXT_BASE)
    add_textbox(slide, Inches(4.3), row_top, Inches(3.2), Inches(0.35),
                timing, font_size=11, color=TEXT_SUBTLE)
    add_textbox(slide, Inches(7.8), row_top, Inches(3.2), Inches(0.35),
                usage, font_size=11, color=TEXT_SUBTLE)

    if i < len(events) - 1:
        dot = slide.shapes.add_shape(
            MSO_SHAPE.RECTANGLE, Inches(0.8), row_top + Inches(0.35),
            Inches(10.5), Pt(0.25)
        )
        dot.fill.solid()
        dot.fill.fore_color.rgb = LINE
        dot.line.fill.background()

# 配置方式
add_textbox(slide, Inches(0.8), Inches(6.9), Inches(11), Inches(0.4),
            "配置方式: config.toml / hooks.json / 插件系统 | 返回值: block / stop / feedback",
            font_size=11, color=TEXT_MUTED)

add_footer(slide, 18)


# ============================================================
# 第19页：子代理系统
# ============================================================
slide = prs.slides.add_slide(prs.slide_layouts[6])
set_slide_bg(slide)
add_section_header(slide, 13, "子代理系统")

# 架构
add_textbox(slide, Inches(0.8), Inches(3.8), Inches(5), Inches(0.4),
            "架构设计", font_size=14, bold=True, color=ACCENT_STRONG)

subagent_arch = [
    "tokio::spawn — 每个子代理独立异步任务",
    "SubagentHandle { cancel_flag, input_tx }",
    "oneshot::Receiver — 等待完成结果",
    "共享 ToolExecutor (禁止嵌套 spawn)",
    "最大 25 次迭代 / 5 分钟超时",
]
add_bullet_text(slide, Inches(0.8), Inches(4.2), Inches(5.5), Inches(2.0),
                subagent_arch, font_size=11, color=TEXT_SUBTLE, bullet_char="·")

# 完整工具集
add_textbox(slide, Inches(7.0), Inches(3.8), Inches(5), Inches(0.4),
            "子代理管理工具", font_size=14, bold=True, color=ACCENT_STRONG)

agent_tools = [
    "spawn_agent — 生成子代理",
    "send_input — 持续通信",
    "wait_agent — 等待完成",
    "resume_agent — 恢复已停止的子代理",
    "list_agents — 列出所有子代理",
    "close_agent — 关闭子代理",
]
add_bullet_text(slide, Inches(7.0), Inches(4.2), Inches(5.5), Inches(2.0),
                agent_tools, font_size=11, color=TEXT_SUBTLE, bullet_char="·")

add_footer(slide, 19)


# ============================================================
# 第20页：本地资源池 (Local Pool)
# ============================================================
slide = prs.slides.add_slide(prs.slide_layouts[6])
set_slide_bg(slide)
add_section_header(slide, 14, "本地资源池 (Local Pool)")

add_textbox(slide, Inches(0.8), Inches(3.8), Inches(11), Inches(0.4),
            "多端点 LLM 供应商管理 — 自动故障转移", font_size=15, bold=True, color=ACCENT_STRONG)

pool_tech = [
    ("故障转移机制", [
        "PoolResolver: 按 provider:model 键隔离不同模型",
        "顺序轮询 + 60 秒恢复期，自动跳过故障端点",
        "全故障时重置健康状态，强制从第一个重试",
    ]),
    ("抗生素检测", [
        "HTTP 5xx / 超时 → mark_failed(index)",
        "fail_count 累加 + last_failure 时间戳",
        "is_healthy() 60 秒后自动恢复",
    ]),
    ("配置与管理", [
        "model_endpoints TOML 数组 (url/label/model/api_key/wire_api)",
        "active_endpoint_index 持久化到 config.toml",
        "前端 ProviderPanel 可视化端点管理",
    ]),
]

for i, (title, items) in enumerate(pool_tech):
    left = Inches(0.8 + i * 4.1)

    add_textbox(slide, left, Inches(4.5), Inches(3.8), Inches(0.4),
                title, font_size=13, bold=True, color=ACCENT_STRONG)
    add_bullet_text(slide, left, Inches(4.9), Inches(3.8), Inches(1.8),
                    items, font_size=10, color=TEXT_SUBTLE, bullet_char="·")

add_footer(slide, 20)


# ============================================================
# 第21页：OCR 离线识别
# ============================================================
slide = prs.slides.add_slide(prs.slide_layouts[6])
set_slide_bg(slide)
add_section_header(slide, 14, "OCR 离线文字识别")

add_textbox(slide, Inches(0.8), Inches(3.8), Inches(11), Inches(0.4),
            "PP-OCRv5 Mobile — 本地离线文字识别引擎", font_size=14, bold=True, color=ACCENT_STRONG)

# 模型
add_textbox(slide, Inches(0.8), Inches(4.3), Inches(5), Inches(0.4),
            "模型文件 (ONNX 格式)", font_size=13, bold=True, color=ACCENT_STRONG)
ocr_items = [
    "ppocrv5_mobile_det.onnx — 文本检测 (DBnet)",
    "ppocrv5_mobile_rec.onnx — 文字识别 (CRNN)",
    "ppocrv5_mobile_vocab.txt — 识别词表",
    "onnxruntime.dll — ONNX Runtime 动态库 (~120MB)",
]
add_bullet_text(slide, Inches(0.8), Inches(4.7), Inches(5.5), Inches(1.5),
                ocr_items, font_size=11, color=TEXT_SUBTLE, bullet_char="·")

# 配置
add_textbox(slide, Inches(7.0), Inches(4.3), Inches(5), Inches(0.4),
            "检测配置与预处理", font_size=13, bold=True, color=ACCENT_STRONG)
ocr_cfg = [
    "score_threshold: 0.5, box_threshold: 0.7",
    "unclip_ratio: 1.8, max_candidates: 40",
    "最长边缩小到 1600px (Triangle 滤波)",
    "置信度低于 0.15 的结果丢弃",
]
add_bullet_text(slide, Inches(7.0), Inches(4.7), Inches(5.5), Inches(1.5),
                ocr_cfg, font_size=11, color=TEXT_SUBTLE, bullet_char="·")

# 场景
add_textbox(slide, Inches(0.8), Inches(6.2), Inches(11), Inches(0.4),
            "使用场景: view_image → extract_text_from_data_urls() / extract_text_from_image_file()",
            font_size=11, color=TEXT_MUTED)

add_footer(slide, 21)


# ============================================================
# 第22页：终端模拟器 + 窗口系统
# ============================================================
slide = prs.slides.add_slide(prs.slide_layouts[6])
set_slide_bg(slide)
add_section_header(slide, 14, "终端模拟器与窗口系统")

# 终端
add_textbox(slide, Inches(0.8), Inches(3.8), Inches(5.5), Inches(0.4),
            "终端模拟器 (portable-pty + xterm.js)", font_size=14, bold=True, color=ACCENT_STRONG)

term_items = [
    "terminal_create — 创建 PTY 会话 (衍生系统 Shell)",
    "terminal_write — 输入数据 (Shell 命令)",
    "terminal_resize — 调整尺寸 (cols/rows)",
    "terminal_close — 关闭会话",
    "后台线程读取 PTY 输出 → Tauri emit → xterm.js 渲染",
]
add_bullet_text(slide, Inches(0.8), Inches(4.2), Inches(5.5), Inches(1.8),
                term_items, font_size=11, color=TEXT_SUBTLE, bullet_char="·")

# 窗口系统
add_textbox(slide, Inches(7.0), Inches(3.8), Inches(5.5), Inches(0.4),
            "三窗口架构 (无边框)", font_size=14, bold=True, color=ACCENT_STRONG)

win_items = [
    "main (1200×800) — 主聊天窗口",
    "document-detail (1060×760) — 文档详情",
    "runsummary-diff — 文件变更对比",
    "cn-browser — 嵌入式浏览器子窗口 (400×600)",
    "8秒强制显示兜底 + 关闭时清空缓存",
]
add_bullet_text(slide, Inches(7.0), Inches(4.2), Inches(5.5), Inches(1.8),
                win_items, font_size=11, color=TEXT_SUBTLE, bullet_char="·")

# 规则系统
add_textbox(slide, Inches(0.8), Inches(6.0), Inches(11), Inches(0.4),
            "用户规则系统: 全局规则 (codey/rules/user.md) + 项目规则 (.rule.md) → 自动注入 system prompt",
            font_size=11, color=TEXT_MUTED)

add_footer(slide, 22)


# ============================================================
# 第23页：协议与事件系统
# ============================================================
slide = prs.slides.add_slide(prs.slide_layouts[6])
set_slide_bg(slide)
add_section_header(slide, 14, "协议层与事件系统")

# 协议
add_textbox(slide, Inches(0.8), Inches(3.8), Inches(5.5), Inches(0.4),
            "JSON-RPC 协议", font_size=14, bold=True, color=ACCENT_STRONG)

proto_items = [
    "Approval: request/resolve/reject 审批通道",
    "FileReview: apply_patch 前置确认 (逐文件审阅)",
    "Notification: 单向事件推送",
    "FrontendRequest: 前端请求封装",
]
add_bullet_text(slide, Inches(0.8), Inches(4.2), Inches(5.5), Inches(1.4),
                proto_items, font_size=11, color=TEXT_SUBTLE, bullet_char="·")

# 事件
add_textbox(slide, Inches(7.0), Inches(3.8), Inches(5.5), Inches(0.4),
            "核心事件 (Tauri emit → 前端)", font_size=14, bold=True, color=ACCENT_STRONG)

event_items = [
    "turn-start / turn-completed — 回合生命周期",
    "streaming-text — LLM 流式输出 (delta)",
    "tool-call-start / tool-call-output — 工具调用",
    "file-change — 文件变更通知",
    "goal-completed — Goal 完成",
    "approval-request — 审批弹窗触发",
]
add_bullet_text(slide, Inches(7.0), Inches(4.2), Inches(5.5), Inches(1.8),
                event_items, font_size=11, color=TEXT_SUBTLE, bullet_char="·")

# 广播
add_textbox(slide, Inches(0.8), Inches(6.3), Inches(11), Inches(0.4),
            "emit_and_broadcast: 同时推送 → Tauri 主窗口 + Mobile WebSocket 移动端",
            font_size=11, color=TEXT_MUTED)

add_footer(slide, 23)


# ============================================================
# 第24页：工具集总览
# ============================================================
slide = prs.slides.add_slide(prs.slide_layouts[6])
set_slide_bg(slide)
add_section_header(slide, 14, "40+ 工具集总览")

tools_data = [
    ("开发工具", "shell, exec_command, read_file,\nwrite_file, apply_patch, list_directory"),
    ("AI 辅助", "update_plan, request_user_input,\nrequest_permissions, tool_search"),
    ("浏览器", "browser_run, view_image,\nocr_image, image_generate"),
    ("MCP", "mcp_call_tool, mcp_list_servers,\nmcp_status, mcp_read_resource"),
    ("记忆/知识", "memory_read/write/update,\nmemory_search, memory_forget"),
    ("子代理", "spawn_agent, wait_agent,\nsend_input, close_agent"),
    ("网络", "web_search, web_fetch"),
    ("Git", "git_status/diff/log, git_stage,\ngit_commit, git_push"),
]

for i, (cat, tools) in enumerate(tools_data):
    row, col = divmod(i, 4)
    left = Inches(0.8 + col * 3.1)
    top = Inches(4.0 + row * 1.4)

    add_textbox(slide, left, top, Inches(2.8), Inches(0.3),
                cat, font_size=12, bold=True, color=ACCENT_STRONG)
    add_textbox(slide, left, top + Inches(0.3), Inches(2.8), Inches(0.9),
                tools, font_size=10, color=TEXT_SUBTLE, line_spacing=1.4)

add_footer(slide, 24)


# ============================================================
# 第25页：构建与发布
# ============================================================
slide = prs.slides.add_slide(prs.slide_layouts[6])
set_slide_bg(slide)
add_section_header(slide, 15, "构建与发布流程")

# 三种模式
build_modes = [
    ("dev.bat — 开发模式", [
        "pnpm dev (Vite HMR, port 1420)",
        "pnpm tauri dev (Rust 热重载)",
        "调试模式自动打开 DevTools",
    ]),
    ("build.bat — 构建模式", [
        "pnpm build (前端构建)",
        "pnpm tauri build (Tauri 发布版)",
        "产物 → build/ 目录",
    ]),
    ("publish.bat — 发布模式", [
        "清理 → 安装依赖 → 构建 mobile-web",
        "Tauri Release 构建 (--no-bundle)",
        "复制 CN-Codex.exe + DLL + skills + plugins",
        "打包 Node.js v22.16.0 便携版",
        "生成可分发的 publish/ 目录",
    ]),
]

for i, (title, items) in enumerate(build_modes):
    left = Inches(0.8 + i * 4.1)

    card = slide.shapes.add_shape(
        MSO_SHAPE.ROUNDED_RECTANGLE, left, Inches(3.8),
        Inches(3.8), Inches(2.5)
    )
    card.fill.solid()
    card.fill.fore_color.rgb = BG_PANEL
    card.line.color.rgb = BORDER
    card.line.width = Pt(0.5)

    add_textbox(slide, left + Inches(0.2), Inches(3.95),
                Inches(3.4), Inches(0.4),
                title, font_size=13, bold=True, color=ACCENT_STRONG)
    add_bullet_text(slide, left + Inches(0.2), Inches(4.4),
                    Inches(3.4), Inches(1.8),
                    items, font_size=11, color=TEXT_SUBTLE, bullet_char="→")

# 构建优化
add_textbox(slide, Inches(0.8), Inches(6.5), Inches(11), Inches(0.4),
            "Release 优化: LTO=true | strip=true | codegen-units=1 | opt-level='s' | 最小体积优先",
            font_size=11, color=TEXT_MUTED)

add_footer(slide, 25)


# ============================================================
# 第26页：测试策略
# ============================================================
slide = prs.slides.add_slide(prs.slide_layouts[6])
set_slide_bg(slide)
add_section_header(slide, 15, "测试策略")

# 前端
add_textbox(slide, Inches(0.8), Inches(3.8), Inches(5), Inches(0.4),
            "前端测试 (Vitest + jsdom)", font_size=14, bold=True, color=ACCENT_STRONG)

front_test = [
    "框架: Vitest 4 + jsdom 环境",
    "文件匹配: src/**/*.test.{ts,tsx}",
    "配置: vite.config.ts test 字段",
]
add_bullet_text(slide, Inches(0.8), Inches(4.2), Inches(5.5), Inches(1.2),
                front_test, font_size=11, color=TEXT_SUBTLE, bullet_char="·")

# 后端
add_textbox(slide, Inches(7.0), Inches(3.8), Inches(5), Inches(0.4),
            "后端测试 (cargo test)", font_size=14, bold=True, color=ACCENT_STRONG)

back_test = [
    "内联测试: 每个模块 mod tests",
    "集成测试: *_integration_tests.rs",
    "工具: tempfile 临时目录, tokio::Runtime",
]
add_bullet_text(slide, Inches(7.0), Inches(4.2), Inches(5.5), Inches(1.2),
                back_test, font_size=11, color=TEXT_SUBTLE, bullet_char="·")

# 覆盖
add_textbox(slide, Inches(0.8), Inches(5.5), Inches(11), Inches(0.4),
            "关键模块测试覆盖", font_size=13, bold=True, color=ACCENT_STRONG)

coverage_items = [
    "agent.rs: Hook 上下文、文件变更、机器人标记",
    "tool_executor.rs: 工具执行、MCP 通信、路径安全",
    "config_system.rs: TOML 解析、MCPServer 配置、编辑操作",
    "compaction.rs: 阈值计算、摘要消息检测",
    "thread_store.rs: 创建/持久化/压缩/Goal/机器人状态",
    "hook_runtime.rs: Hook 执行、决策解析、输出效果",
]
add_bullet_text(slide, Inches(0.8), Inches(5.9), Inches(11), Inches(1.2),
                coverage_items, font_size=11, color=TEXT_SUBTLE, bullet_char="·")

add_footer(slide, 26)


# ============================================================
# 第27页：视频演示/截图
# ============================================================
slide = prs.slides.add_slide(prs.slide_layouts[6])
set_slide_bg(slide)
add_section_header(slide, 15, "界面示意图", "实际演示效果")

# 主界面描述
screens = [
    ("主聊天界面", [
        "三栏布局: 侧栏(256px) + 聊天区 + 右面板(384px)",
        "翡翠绿强调色 + 深色背景",
        "流式消息渲染 + 工具调用卡片",
        "底部输入框 + 模式切换 (chat/plan/goal)",
    ]),
    ("设置面板", [
        "供应商管理 (多 LLM 端点)",
        "模型选择 + 上下文窗口配置",
        "Skill/Plugin/Robot 可视化管理",
        "SmartBrain 经验知识管理",
    ]),
    ("移动端远程控制", [
        "QR 码扫码连接",
        "实时消息同步 (WebSocket)",
        "发送消息 + 中断 + 查看进度",
        "局域网直连 / 公网中转",
    ]),
]

for i, (title, desc) in enumerate(screens):
    left = Inches(0.8 + i * 4.1)

    card = slide.shapes.add_shape(
        MSO_SHAPE.ROUNDED_RECTANGLE, left, Inches(4.0),
        Inches(3.8), Inches(2.5)
    )
    card.fill.solid()
    card.fill.fore_color.rgb = BG_PANEL
    card.line.color.rgb = BORDER
    card.line.width = Pt(0.5)

    add_textbox(slide, left + Inches(0.2), Inches(4.15),
                Inches(3.4), Inches(0.4),
                title, font_size=14, bold=True, color=ACCENT_STRONG)
    add_bullet_text(slide, left + Inches(0.2), Inches(4.6),
                    Inches(3.4), Inches(1.8),
                    desc, font_size=11, color=TEXT_SUBTLE, bullet_char="·")

add_footer(slide, 27)


# ============================================================
# 第28页：技术优势总结
# ============================================================
slide = prs.slides.add_slide(prs.slide_layouts[6])
set_slide_bg(slide)
add_section_header(slide, 15, "技术优势总结")

advantages = [
    ("原生桌面体验", "Tauri 2 + Rust 后端\n原生性能，低内存占用\n无边框窗口，自定义标题栏"),
    ("多模型支持", "4种 API 格式适配器\n本地资源池故障转移\n支持 OpenAI/DeepSeek/Qwen/Claude/Gemini"),
    ("全流程自动化", "Goal 模式自主规划执行\n机器人工作流编排\n上下文压缩防 Token 溢出"),
    ("移动端控制", "WebSocket 实时同步\n扫码即连，远程审批\n局域网/公网双模式"),
    ("离在线混合", "本地 OCR (PP-OCRv5)\n本地经验/知识索引\n在线 LLM + 网页搜索"),
    ("可扩展性", "插件系统 (8 个内置)\n60+ Skills 技能库\nHook 运行时 + MCP 协议"),
]

for i, (title, desc) in enumerate(advantages):
    row, col = divmod(i, 3)
    left = Inches(0.8 + col * 4.1)
    top = Inches(3.8 + row * 1.6)

    card = slide.shapes.add_shape(
        MSO_SHAPE.ROUNDED_RECTANGLE, left, top,
        Inches(3.8), Inches(1.35)
    )
    card.fill.solid()
    card.fill.fore_color.rgb = BG_PANEL
    card.line.color.rgb = BORDER
    card.line.width = Pt(0.5)

    bar = slide.shapes.add_shape(
        MSO_SHAPE.RECTANGLE, left + Inches(0.15), top + Inches(0.15),
        Pt(2), Inches(1.05)
    )
    bar.fill.solid()
    bar.fill.fore_color.rgb = ACCENT
    bar.line.fill.background()

    add_textbox(slide, left + Inches(0.25), top + Inches(0.1),
                Inches(3.3), Inches(0.3),
                title, font_size=13, bold=True, color=ACCENT_STRONG)
    add_textbox(slide, left + Inches(0.25), top + Inches(0.4),
                Inches(3.3), Inches(0.85),
                desc, font_size=10, color=TEXT_SUBTLE, line_spacing=1.5)

add_footer(slide, 28)


# ============================================================
# 第29页：下一步规划
# ============================================================
slide = prs.slides.add_slide(prs.slide_layouts[6])
set_slide_bg(slide)
add_section_header(slide, 15, "下一步规划")

future_items = [
    ("短期规划", [
        "完善测试覆盖，提升 CI/CD 自动化",
        "增加更多 LLM 供应商支持",
        "优化首屏加载性能 (WebView 预热)",
        "增强 SmartBrain 经验提取精度",
    ]),
    ("中期规划", [
        "社区插件市场 (Plugin Marketplace)",
        "多用户/团队协作模式",
        "可视化工作流编辑器",
        "更多内置机器人模板",
    ]),
    ("长期规划", [
        "Web 版本支持 (PWA)",
        "VSCode / JetBrains 插件集成",
        "私有化部署方案",
        "企业级安全审计和合规",
    ]),
]

for i, (title, items) in enumerate(future_items):
    left = Inches(0.8 + i * 4.1)

    add_textbox(slide, left, Inches(3.8), Inches(3.8), Inches(0.4),
                title, font_size=14, bold=True, color=ACCENT_STRONG)
    add_bullet_text(slide, left, Inches(4.3), Inches(3.8), Inches(2.0),
                    items, font_size=11, color=TEXT_SUBTLE, bullet_char="·")

# 中间引用
add_textbox(slide, Inches(0.8), Inches(6.5), Inches(11), Inches(0.5),
            "我们期待社区的贡献！GitHub: https://github.com/longdream/cn-codex",
            font_size=12, color=ACCENT_STRONG, alignment=PP_ALIGN.CENTER)

add_footer(slide, 29)


# ============================================================
# 第30页：结尾
# ============================================================
slide = prs.slides.add_slide(prs.slide_layouts[6])
set_slide_bg(slide)

# 装饰圆
for i in range(2):
    circle = slide.shapes.add_shape(
        MSO_SHAPE.OVAL, Inches(8 + i*3), Inches(-2), Inches(6), Inches(6)
    )
    circle.fill.solid()
    circle.fill.fore_color.rgb = RGBColor(0x15, 0x2A, 0x1A)
    circle.line.fill.background()

add_accent_bar(slide, Inches(4.8), Inches(2.5), height=Inches(0.6))

add_textbox(slide, Inches(2), Inches(3.2), Inches(9), Inches(1.0),
            "谢谢！", font_size=48, bold=True, color=TEXT_STRONG,
            alignment=PP_ALIGN.CENTER)

add_textbox(slide, Inches(2), Inches(4.3), Inches(9), Inches(0.8),
            "手机在手 · 代码我有", font_size=20, color=ACCENT_STRONG,
            alignment=PP_ALIGN.CENTER)

add_textbox(slide, Inches(2), Inches(5.3), Inches(9), Inches(1.5),
            "GitHub: https://github.com/longdream/cn-codex\n官方网站: http://47.113.221.244:8081/\n使用指南: http://47.113.221.244:8081/usage.html",
            font_size=12, color=TEXT_MUTED, alignment=PP_ALIGN.CENTER,
            line_spacing=1.6)

add_footer(slide, 30)


# ============ 保存 ============
output_path = os.path.join(os.path.dirname(__file__), "CN-Codex-技术讲解.pptx")
prs.save(output_path)
print(f"PPTX 已生成: {output_path}")
print(f"共 {len(prs.slides)} 页幻灯片")