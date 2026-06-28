# CN-Codex 技术详解文档

> 版本：0.1.0 | 最后更新：2025 年 | 基于 DeepSeek v4 Flash 验证

---

## 目录

1. [项目概述](#1-项目概述)
2. [整体架构](#2-整体架构)
3. [技术栈全景](#3-技术栈全景)
4. [前端架构详解](#4-前端架构详解)
5. [Rust 后端架构详解](#5-rust-后端架构详解)
6. [核心引擎（AgentEngine）](#6-核心引擎agentengine)
7. [工具执行器（ToolExecutor）](#7-工具执行器toolexecutor)
8. [LLM 适配层](#8-llm-适配层)
9. [数据持久化](#9-数据持久化)
10. [状态管理](#10-状态管理)
11. [移动端远程控制](#11-移动端远程控制)
12. [外部浏览器与录制回放](#12-外部浏览器与录制回放)
13. [上下文压缩（Compaction）](#13-上下文压缩compaction)
14. [插件系统](#14-插件系统)
15. [机器人系统](#15-机器人系统)
16. [子代理系统](#16-子代理系统)
17. [Hook 运行时](#17-hook-运行时)
18. [SmartBrain 智能脑](#18-smartbrain-智能脑)
19. [设计系统](#19-设计系统)
20. [构建与发布](#20-构建与发布)
21. [测试策略](#21-测试策略)

---

## 1. 项目概述

CN-Codex 是一个 **AI 驱动的全栈编程助手桌面应用**，基于 OpenAI Codex CLI/TUI 的核心思想构建，但全面替换为自有品牌和技术栈。

### 1.1 核心定位

- **桌面原生应用**：基于 Tauri 2 框架，提供原生桌面体验
- **AI 编程助手**：支持多轮对话、工具调用、Goal 模式，自动完成编程任务
- **手机远程控制**：扫码连接手机，随时随地监控和控制 AI 助手
- **机器人自动化**：AI 自动创建专业角色，绑定技能和工作流节点，实现自动化流水线
- **全流程开发支持**：从项目创建、代码编写、测试到部署的全流程覆盖

### 1.2 设计理念

- **深色优先**：IDE 原生暗色界面，翡翠绿强调色，无视觉噪音
- **紧凑高效**：中高密度布局，可呼吸但不松散
- **去阴影化**：使用 1px 轮廓线代替阴影，保持清晰层次
- **中英文双语**：默认中文，支持运行时切换

---

## 2. 整体架构

```
┌─────────────────────────────────────────────────────────────┐
│                    CN-Codex 桌面应用                         │
├──────────────────────┬──────────────────────────────────────┤
│                      │                                      │
│   前端 (React)       │   后端 (Rust / Tauri)                │
│                      │                                      │
│  ┌──────────────┐    │  ┌──────────────┐                    │
│  │ App.tsx      │    │  │ lib.rs      │ ← Tauri 初始化      │
│  │ 根组件+布局   │    │  │ 模块声明     │                    │
│  ├──────────────┤    │  ├──────────────┤                    │
│  │ ChatPage     │    │  │ AgentEngine │ ← Agent 引擎核心    │
│  │ 聊天主页     │    │  │ run_turn()   │                    │
│  ├──────────────┤    │  ├──────────────┤                    │
│  │ MessageList  │    │  │ ToolExecutor│ ← 工具执行器        │
│  │ 消息流      │◄───►│  │ 40+ 工具    │                    │
│  ├──────────────┤ IPC │  ├──────────────┤                    │
│  │ ChatInput    │    │  │ ThreadStore │ ← 会话持久化        │
│  │ 输入框      │    │  │ JSONL 格式   │                    │
│  ├──────────────┤    │  ├──────────────┤                    │
│  │ Sidebar      │    │  │ ConfigSystem│ ← 配置管理          │
│  │ 侧边栏      │    │  │ TOML 配置    │                    │
│  ├──────────────┤    │  ├──────────────┤                    │
│  │ SettingsPanel│    │  │ Adapter     │ ← LLM 适配层       │
│  │ 设置面板     │    │  │ 4 种 API    │                    │
│  ├──────────────┤    │  ├──────────────┤                    │
│  │ StatusBar    │    │  │ Compaction  │ ← 上下文压缩        │
│  │ 状态栏      │    │  │ 自动触发     │                    │
│  ├──────────────┤    │  ├──────────────┤                    │
│  │ ....         │    │  │ ....         │                    │
│  └──────────────┘    │  └──────────────┘                    │
│                      │                                      │
│  ┌──────────────┐    │  ┌──────────────┐                    │
│  │appStore      │    │  │ SmartBrain  │ ← 经验/知识管理    │
│  │Zustand 状态  │    │  │ BM25 索引   │                    │
│  └──────────────┘    │  ├──────────────┤                    │
│                      │  │ MobileServer│ ← Axum Web 服务器  │
│  ┌──────────────┐    │  │ WebSocket   │ 手机远程控制        │
│  │ Mobile Web   │    │  └──────────────┘                    │
│  │ 移动端网页   │◄──►│                                      │
│  └──────────────┘    │  ┌──────────────┐                    │
│                      │  │ ExternalBrowser                    │
│                      │  │ Chrome CDP  │ ← 浏览器自动化      │
│                      │  └──────────────┘                    │
└──────────────────────┴──────────────────────────────────────┘
```

### 2.1 通信模型

```
前端 (React) ←→ Tauri IPC (invoke/events) ←→ Rust 后端
                                              ↓
                                       LLM API (HTTP SSE)
```

### 2.2 事件流

```
LLM API → SSE Stream → Rust Agent → Tauri emit() → 前端 listener → Zustand Store → React 重渲染
```

---

## 3. 技术栈全景

### 3.1 前端

| 技术 | 版本 | 用途 |
|------|------|------|
| React | 18.3 | UI 框架 |
| TypeScript | 5.7 | 类型系统 |
| Vite | 6 | 构建工具（多页面入口） |
| Zustand | 5 | 状态管理 |
| Tailwind CSS | 4 | 样式框架 |
| react-intl | 7 | 国际化 |
| react-markdown | 9 | Markdown 渲染 |
| rehype-highlight | 7 | 代码高亮 |
| remark-gfm | 4 | GFM Markdown 支持 |
| @tabler/icons-react | 3 | 图标库 |
| @xterm/xterm | 6 | 终端模拟器 |
| Vitest | 4 | 单元测试 |

### 3.2 后端（Rust）

| 技术 | 版本 | 用途 |
|------|------|------|
| Rust | 1.85+ (Edition 2024) | 系统语言 |
| Tauri | 2 | 桌面框架 |
| tokio | full | 异步运行时 |
| reqwest | 0.13 | HTTP 客户端 |
| serde / serde_json | 1 | 序列化 |
| rusqlite | 0.34 (bundled) | SQLite |
| toml | 0.8 | TOML 解析 |
| axum | 0.8 | 移动端 Web 服务器 |
| tower-http | 0.6 | HTTP 中间件 |
| tokio-tungstenite | 0.24 | WebSocket |
| tracing | 0.1 | 日志 |
| uuid | 1.23 | UUID |
| chrono | 0.4 | 时间处理 |
| base64 | 0.22 | Base64 编解码 |
| chardetng | 1.0 | 字符编码检测 |
| encoding_rs | 0.8 | 编码转换 |
| image | 0.25 | 图片处理 |
| ndarray + ort | 2.0 | ONNX Runtime（OCR） |
| portable-pty | 0.8 | 终端 PTY |
| pdf-extract | 0.7 | PDF 文本提取 |
| calamine | 0.26 | Excel 解析 |
| zip | 2 | ZIP 处理 |
| quick-xml | 0.37 | XML 解析 |
| qrcode | 0.14 | QR 码生成 |
| local-ip-address | 0.6 | 本地 IP 获取 |

### 3.3 Tauri 插件

| 插件 | 用途 |
|------|------|
| tauri-plugin-shell | Shell 命令执行 |
| tauri-plugin-dialog | 系统对话框 |
| tauri-plugin-opener | 文件/URL 打开 |
| tauri-plugin-fs | 文件系统访问 |
| tauri-plugin-process | 进程管理 |
| tauri-plugin-notification | 系统通知 |

### 3.4 运行时资源（codey/ 目录）

| 路径 | 用途 |
|------|------|
| `config.toml` | 用户配置（TOML 格式） |
| `sessions/` | 会话持久化（JSONL 格式） |
| `skills/` | 技能定义（60+ 内置 skills） |
| `plugins/` | 插件（8 个内置插件） |
| `robots/` | 机器人定义 |
| `memories/` | 记忆存储 |
| `workflows/` | 工作流定义 |
| `usage.db` | 用量统计（SQLite） |
| `node/` | 嵌入式 Node.js 便携版 |

---

## 4. 前端架构详解

### 4.1 目录结构

```
src/
├── main.tsx                  # React 入口，挂载 App 组件
├── App.tsx                   # 根组件：布局 + 初始化 + 拖拽调整
├── index.css                 # 全局样式 + Tailwind CSS
├── api/                      # Tauri invoke 封装层
│   ├── standalone.ts         # 核心引擎调用（thread/chat/config）
│   ├── app_state.ts          # SQLite KV 持久化
│   ├── approval.ts           # 审批通道
│   ├── fileReview.ts         # 文件审阅
│   ├── git.ts                # Git 操作
│   ├── hook.ts               # Hook 管理
│   ├── plugin.ts             # 插件管理
│   ├── recording.ts          # 录制管理
│   ├── robot.ts              # 机器人管理
│   ├── skill.ts              # 技能管理
│   ├── usage.ts              # 用量查询
│   ├── window.ts             # 窗口控制
│   └── workflow.ts           # 工作流管理
├── stores/                   # Zustand 状态管理
│   ├── appStore.ts           # 主应用状态（~1300 行）
│   └── settingsStore.ts      # 语言/主题设置
├── hooks/                    # 自定义 Hooks
│   ├── useTauriEvents.ts     # 后端事件 → Store 桥接
│   ├── useRecording.ts       # 录制 Hook
│   └── useFortuneDetailStream.ts  # 运势流式加载
├── components/               # UI 组件
│   ├── chat/                 # 聊天主界面
│   │   ├── ChatPage.tsx      # 聊天主页
│   │   ├── ChatInput.tsx     # 输入框（支持斜杠命令）
│   │   ├── MessageList.tsx   # 消息列表（流式渲染）
│   │   ├── CodeBlock.tsx     # 代码块（语法高亮 + 复制）
│   │   ├── PatchDiffModal.tsx # Patch Diff 审阅弹窗
│   │   ├── PlanCard.tsx      # 计划卡片组件
│   │   └── SlashCommandPanel.tsx # 斜杠命令面板
│   ├── layout/               # 布局组件
│   │   ├── TitleBar.tsx      # 自定义标题栏（无边框窗口）
│   │   ├── Sidebar.tsx       # 侧边栏（会话列表+项目分组）
│   │   ├── RightPanel.tsx    # 右侧面板（终端/浏览器/Git）
│   │   ├── StatusBar.tsx     # 底部状态栏
│   │   ├── FileTree.tsx      # 文件树
│   │   ├── GitPanel.tsx      # Git 面板
│   │   └── TerminalPanel.tsx # 终端面板
│   ├── settings/             # 设置面板
│   │   ├── SettingsPanel.tsx  # 设置主面板
│   │   ├── ProviderPanel.tsx  # 供应商配置
│   │   ├── HooksPanel.tsx     # Hook 管理
│   │   ├── SkillsPanel.tsx    # Skill 管理
│   │   ├── PluginsPanel.tsx   # 插件管理
│   │   ├── RobotsPanel.tsx    # 机器人管理
│   │   ├── WorkflowsPanel.tsx # 工作流管理
│   │   ├── ExperiencePanel.tsx # 经验管理
│   │   ├── KnowledgePanel.tsx # 知识库管理
│   │   ├── IntegrationPanel.tsx # 集成配置
│   │   └── UsageDashboard.tsx # 用量仪表盘
│   ├── approval/             # 审批系统
│   │   └── ApprovalModal.tsx  # 审批弹窗
│   ├── common/               # 通用组件
│   │   ├── AppBackground.tsx  # 应用背景
│   │   ├── ContextMenu.tsx    # 右键菜单
│   │   ├── ErrorBoundary.tsx  # 错误边界
│   │   ├── FortuneBubble.tsx  # 运势悬浮球
│   │   ├── FortuneDetailModal.tsx # 运势详情
│   │   ├── QrCodePopover.tsx  # 二维码弹窗
│   │   └── RecordingToggle.tsx # 录制开关
│   ├── detail/               # 文档详情窗
│   │   └── DocumentDetailWindow.tsx
│   ├── diff/                 # RunSummary Diff 窗
│   │   └── RunSummaryDiffWindow.tsx
│   └── workflow/             # 工作流提取
│       └── WorkflowExtractModal.tsx
├── types/                    # TypeScript 类型定义
│   ├── index.ts
│   ├── account.ts
│   ├── approval.ts
│   ├── config.ts
│   ├── hook.ts
│   ├── model.ts
│   ├── notifications.ts
│   ├── plugin.ts
│   ├── provider.ts
│   ├── robot.ts
│   ├── skill.ts
│   ├── thread.ts
│   ├── turn.ts
│   └── usage.ts
├── i18n/                     # 国际化
│   ├── zh-CN/common.json     # 中文
│   └── en-US/common.json     # 英文
└── utils/                    # 工具函数
    ├── formatCodeSnippet.ts
    ├── formatDuration.ts
    ├── fortune.ts
    └── lineDiff.ts
```

### 4.2 多页面入口

Vite 配置为三个独立页面入口：

```typescript
// vite.config.ts
rollupOptions: {
  input: {
    main:   path.resolve(__dirname, "index.html"),    // 主窗口
    detail: path.resolve(__dirname, "detail.html"),   // 文档详情窗
    diff:   path.resolve(__dirname, "diff.html"),     // RunSummary Diff 窗
  },
}
```

### 4.3 App.tsx 初始化流程

应用启动分为三个阶段：

**阶段 1 - 关键路径**（必须完成后才能渲染壳）
1. `standaloneInit()` — 初始化后端引擎
2. `initStoreFromDb()` — 从 SQLite 恢复项目/会话映射
3. `initSettingsFromDb()` — 恢复主题设置

**阶段 2 - 可交互门槛**
- 设置 `initialized = true`，放开 UI 渲染
- 用户立即看到聊天界面

**阶段 3 - 后台恢复**
- 恢复用户目录（`getUserHomeDir()`）
- 恢复运行时状态 + 当前会话
- 刷新侧栏会话列表
- 恢复当前模型配置

### 4.4 拖拽布局系统

- 左侧栏（Sidebar）：默认 256px，可拖拽范围 220-520px
- 右侧面板（RightPanel）：默认 384px，可拖拽范围 320-760px
- 主区域最小宽度：560px
- 使用 Pointer Events API 实现流畅拖拽
- 窗口 resize 时自动校验边界

### 4.5 无边框窗口

- `decorations: false` — 完全自定义窗口装饰
- 自定义 TitleBar 组件实现拖拽、最小化、最大化、关闭
- 支持 DevTools 热键调试

---

## 5. Rust 后端架构详解

### 5.1 模块组织

```
src-tauri/src/
├── lib.rs                 # 模块声明 + Tauri Builder 初始化
├── main.rs                # 二进制入口
├── state.rs               # AppState - 全局共享状态
├── standalone.rs          # Standalone 模式命令（Tauri Commands）
├── agent.rs               # Agent 引擎核心（~2700 行）
├── tool_executor.rs       # 工具执行器（~6000 行）
├── thread_store.rs        # 线程/会话持久化
├── compaction.rs          # 上下文压缩
├── config_system.rs       # 配置管理系统
├── hook_runtime.rs        # Hook 运行时
├── external_browser.rs    # 外部浏览器管理
├── recording.rs           # 操作录制与回放
├── subagent_engine.rs     # 子代理引擎
├── robot_orchestrator.rs  # 机器人编排器
├── robot_loader.rs        # 机器人加载器
├── plugin_loader.rs       # 插件加载器
├── mobile_server.rs       # 移动端 Web 服务器
├── relay_client.rs        # 中继服务器客户端
├── browser_automation.rs  # 浏览器自动化
├── file_review.rs         # 文件审阅
├── git_service.rs         # Git 服务
├── terminal.rs            # 终端模拟器
├── conversation_logger.rs # 对话日志器
├── document_parser.rs     # 文档解析器
├── local_pool.rs          # 本地资源池
├── ocr/                   # OCR 模块（PP-OCRv5）
├── smartbrain/            # SmartBrain 模块
│   ├── mod.rs
│   ├── commands.rs        # Tauri Commands
│   ├── extractor.rs       # 经验提取
│   ├── consolidator.rs    # 经验合并
│   ├── knowledge.rs       # 知识管理
│   ├── search.rs          # 搜索/检索
│   ├── bm25_index.rs      # BM25 索引
│   ├── prompts.rs         # 提示词模板
│   ├── okf.rs             # OKF 格式支持
│   └── index.rs           # 索引工具
├── adapter/               # LLM 适配层
│   ├── mod.rs             # ProviderAdapter trait
│   ├── types.rs           # 统一消息类型
│   ├── chat_completions.rs # OpenAI Chat Completions
│   ├── responses.rs       # OpenAI Responses API
│   ├── anthropic.rs       # Anthropic Messages API
│   └── google.rs          # Google Gemini API
├── commands/              # Tauri 命令拆分
│   ├── mod.rs             # 命令重新导出
│   └── ...                # 各命令分组
├── protocol/              # JSON-RPC 协议类型
├── usage/                 # 用量统计
│   ├── mod.rs
│   ├── db.rs              # SQLite 数据库
│   ├── recorder.rs        # 用量记录器
│   └── pricing.rs         # 价格表
├── workflow/              # 工作流
│   ├── mod.rs
│   ├── commands.rs
│   ├── extractor.rs
│   ├── prompts.rs
│   └── skill_gen.rs
├── error.rs               # 错误类型定义
└── experience/             # 经验模块
```

### 5.2 AppState 全局状态

```rust
pub struct AppState {
    pub locale: RwLock<String>,                    // 语言设置
    pub current_thread_id: Arc<RwLock<Option<String>>>,  // 当前线程
    pub project_root: PathBuf,                     // 项目根目录
    pub workspace_config_dir: PathBuf,             // codey/ 配置目录
    pub config_path: PathBuf,                      // config.toml 路径
    pub cwd: RwLock<String>,                       // 当前工作目录
    pub approval_tx: mpsc::Sender<ApprovalAction>,  // 审批通道
    pub config_manager: ConfigManager,              // 配置管理
    pub thread_store: Arc<ThreadStore>,             // 线程存储
    pub agent_engine: Arc<AgentEngine>,             // Agent 引擎
    pub usage_db: Arc<UsageDb>,                     // 用量数据库
    pub pricing_table: Arc<RwLock<PricingTable>>,   // 价格表
    pub file_review_sessions: Arc<RwLock<HashMap<String, PendingPatchReview>>>,  // 文件审阅缓存
    pub document_detail_active_path: Arc<RwLock<Option<String>>>,  // 文档详情路径
    pub runsummary_diff_payload: Arc<RwLock<Option<RunSummaryDiffPayload>>>,  // Diff 载荷
    pub external_browser: Arc<ExternalBrowser>,     // 外部浏览器
    pub recorder: Arc<Recorder>,                    // 录制器
}
```

### 5.3 Tauri Commands 列表

核心命令组：

| 命令组 | 数量 | 说明 |
|--------|------|------|
| Standalone 命令 | 20+ | 引擎初始化、线程管理、聊天、配置 |
| AppState 命令 | 4 | SQLite KV 读写删 |
| 窗口控制 | 15+ | 窗口管理、浏览器窗、文档窗、Diff 窗 |
| Git 命令 | 12 | status/diff/log/branch/stage/commit 等 |
| 文件审阅 | 4 | 获取/更新/应用/取消 |
| 终端命令 | 4 | 创建/写入/调整/关闭 |
| 移动端命令 | 6 | 启动/停止/状态/URL/QR 码 |
| 录制命令 | 7 | 启动/停止/状态/开关/轨迹管理 |
| 插件命令 | 5 | 列表/读取/启用/卸载/导入 |
| SmartBrain 命令 | 9 | 经验/知识/索引 CRUD |
| 工作流命令 | 5 | 提取/保存/列表/读取/删除 |
| 用量命令 | 6 | 统计/日/模型/定价 |

共计 **~100 个 Tauri Commands**。

---

## 6. 核心引擎（AgentEngine）

`agent.rs` 是 CN-Codex 最核心的模块（~2700 行），负责 AI 对话的完整生命周期管理。

### 6.1 Turn（对话轮次）生命周期

```
用户发送消息
    ↓
standalone_chat() Tauri Command
    ↓
AgentEngine.run_turn()
    ↓
emit "turn-started" → 前端显示加载状态
    ↓
┌──────────────────────────────────────────────────┐
│  Agent 循环（最多 25 次迭代）                       │
│                                                    │
│  1. 构建上下文（system + history + user）           │
│  2. 调用 LLM（SSE 流式）                            │
│  3. 解析响应                                         │
│     ├─ 纯文本消息 → 结束循环                        │
│     └─ 工具调用 → 执行工具 → 加入上下文 → 继续循环   │
│  4. 检查中断/Cancel/Budget                          │
└──────────────────────────────────────────────────┘
    ↓
emit "turn-completed" → 前端更新消息列表
```

### 6.2 聊天模式

支持 5 种模式：

| 模式 | 说明 | 行为 |
|------|------|------|
| `chat` | 普通对话 | 单轮问答 |
| `goal` | 目标模式 | 持续循环直到目标完成 |
| `plan` | 计划模式 | 模型输出结构化计划 |
| `robot-create` | 机器人创建 | 引导模型创建机器人定义 |
| `robot-modify` | 机器人修改 | 引导模型修改已有机器人 |

### 6.3 流式事件系统

AgentEngine 在执行过程中发射以下事件：

| 事件 | 时机 | 载荷 |
|------|------|------|
| `turn-started` | Turn 开始 | `{ threadId, turnId, mode }` |
| `agent-message-delta` | LLM 流式输出 | `{ delta, turnId }` |
| `tool-calls-start` | 工具调用开始 | `{ calls: [{id, name, arguments}] }` |
| `tool-calls-end` | 工具调用结束 | —— |
| `tool-exec-end` | 单个工具执行完成 | `{ callId, exitCode, output }` |
| `turn-completed` | Turn 完成 | `{ turnId, usage, changedFiles }` |
| `context-compacted` | 上下文压缩完成 | `{ threadId, summaryLength }` |
| `agent-message-reasoning` | 推理过程（Anthropic） | `{ delta }` |

所有事件同时通过 Tauri emit 发送到前端，以及通过移动端 WebSocket 广播到手机。

### 6.4 中断机制

```rust
// 原子标志位
cancel_flag: Arc<AtomicBool>

// 中断方法
pub fn interrupt(&self)                    // 设置 cancel_flag
pub async fn interrupt_active_tools(...)   // 中断活跃工具子进程
```

用户点击"停止"按钮时：
1. 前端调用 `standalone_turn_interrupt`
2. 后端设置 `cancel_flag = true`
3. 中断所有活跃的工具子进程
4. 发射 `turn-completed` 携带中断信息

### 6.5 Goal 模式

Goal 模式下，Agent 会持续循环直到满足以下条件之一：
- **模型标记完成**：在消息中包含完成信号
- **Token 预算耗尽**：超过 `goalBudgetTokens`
- **达到最大迭代次数**：25 次
- **用户中断**

Goal 状态机：

```
Active → Paused → Active → ... → Complete
  ↓                                    ↓
Blocked → ...                    BudgetLimited
  ↓
UsageLimited
```

---

## 7. 工具执行器（ToolExecutor）

`tool_executor.rs` 是项目中最大的模块（~6000 行），管理 40+ 工具的注册、执行和生命周期。

### 7.1 工具分类

| 类别 | 工具 | 说明 |
|------|------|------|
| **文件操作** | `read_file`, `write_file`, `list_directory` | 文件读写与目录浏览 |
| **代码执行** | `shell`, `shell_command`, `exec_command` | Shell 命令执行 |
| **代码审阅** | `code_review` | Git diff 审查 |
| **补丁操作** | `apply_patch` | 多文件 Patch 更新/删除/移动 |
| **网络搜索** | `web_search`, `web_fetch` | DuckDuckGo API + Bing 浏览器搜索 |
| **图片处理** | `view_image`, `image_generate`, `ocr_image` | 图片查看/生成/OCR |
| **内存工具** | `memory_list/read/write/update/forget/search` | SmartBrain 操作 |
| **子代理** | `spawn_agent`, `wait_agent`, `send_input`, `list_agents`, `close_agent` | 子代理管理 |
| **用户交互** | `request_user_input`, `request_permissions` | 用户确认 |
| **状态管理** | `update_plan` | 计划更新 |
| **MCP 工具** | `mcp_list_servers`, `mcp_call_tool`, `mcp_read_resource` 等 | MCP 服务器通信 |
| **浏览器** | `browser_run` | 浏览器自动化 |

### 7.2 Shell 命令执行

```rust
// 参数结构
struct ShellArgs {
    command: ShellCommandArg,     // 脚本字符串或 argv 数组
    workdir: Option<String>,       // 工作目录
    timeout_ms: Option<u64>,       // 超时（1s - 3600s）
    login: Option<bool>,           // 登录 shell
    sandbox_permissions: Option<String>,  // 沙箱权限
}
```

- 默认超时：30 秒
- Windows 下使用 `CREATE_NO_WINDOW` 标志隐藏控制台窗口
- 支持输出截断（最多 10000 tokens）

### 7.3 Exec 命令会话

支持长运行交互式命令的会话管理：

```rust
struct ExecSessionRecord {
    id: u64,
    process_id: Option<u32>,
    command: String,
    output: Arc<Mutex<String>>,     // 累积输出
    cursor: Arc<Mutex<usize>>,      // 增量读取游标
    exit_code: Arc<Mutex<Option<i32>>>,
    stdin: Arc<Mutex<Option<ChildStdin>>>,  // 标准输入
}
```

- `exec_command`：启动持久会话
- `write_stdin`：向会话写入输入
- `close_exec_session`：关闭会话

### 7.4 MCP 工具调用

支持 stdio 和 HTTP 两种 MCP 服务器传输方式：

```rust
struct McpSession {
    server: McpServerConfig,
    stdin: ChildStdin,           // stdio 输入
    reader: BufReader<ChildStdout>,  // stdio 输出
    child: Child,
    next_request_id: i64,
    request_count: u64,
}

struct McpHttpSession {
    server: McpServerConfig,
    session_id: Option<String>,
    next_request_id: i64,
}
```

- 自动发现 MCP 服务器的 `tools/list` 返回的工具
- `mcp__server__tool` 直接调用模式
- 支持 Playwright MCP 的自动安装和预热

### 7.5 审批系统

支持三种审批策略：

| 策略 | 行为 |
|------|------|
| `on-request` | 每次工具调用都需要用户批准 |
| `on-failure` | 工具执行失败时请求审批 |
| `never` | 自动拒绝所有工具调用（只读模式）|

---

## 8. LLM 适配层

### 8.1 ProviderAdapter 接口

```rust
#[async_trait]
pub trait ProviderAdapter: Send + Sync {
    fn build_url(&self, base_url: &str, model: &str) -> String;
    fn build_headers(&self, api_key: &str) -> HeaderMap;
    fn build_body(&self, model: &str, messages: &[InternalMessage], 
                  tools: Option<&[Value]>, max_tokens: Option<i64>) -> Value;
    fn is_stream_done(&self, line: &str) -> bool;
    fn parse_stream_line(&self, line: &str) -> Vec<StreamEvent>;
}
```

### 8.2 支持的 API 格式

| Adapter | `wire_api` | 服务端 |
|---------|-----------|--------|
| `ChatCompletionsAdapter` | `chat` | OpenAI Chat API、DeepSeek、火山引擎、智谱、百度 |
| `ResponsesAdapter` | `responses` | OpenAI Responses API |
| `AnthropicAdapter` | `anthropic` | Anthropic Claude |
| `GoogleAdapter` | `gemini` | Google Gemini |

### 8.3 统一输出类型

所有 adapter 输出统一的 `CompletionResult`：

```rust
enum CompletionResult {
    Message { content: String, usage: TokenUsage },
    ToolCalls { calls: Vec<ToolCallRequest>, usage: TokenUsage },
}
```

### 8.4 StreamEvent 流式事件

```rust
enum StreamEvent {
    TextDelta(String),           // 文本增量
    ToolCallDelta {              // 工具调用增量
        index: usize,
        id: Option<String>,
        name: Option<String>,
        arguments: Option<String>,
    },
    Done { finish_reason: Option<String> },  // 流结束
    Usage(TokenUsage),           // 用量信息
}
```

---

## 9. 数据持久化

### 9.1 线程持久化（JSONL Rollout）

每个线程存储为 `codey/sessions/{thread_id}.jsonl`，每行一个事件：

```json
{"type":"thread_meta","thread_id":"xxx","name":"会话1","created_at":...}
{"type":"turn_start","turn_id":"xxx","started_at":...,"mode":"chat"}
{"type":"message","id":"xxx","role":"user","content":"你好","timestamp":...}
{"type":"message","id":"xxx","role":"assistant","content":"你好！","timestamp":...}
{"type":"turn_end","turn_id":"xxx","completed_at":...,"usage":{...}}
{"type":"thread_update","name":"新名字","updated_at":...}
{"type":"thread_goal_set","goal":{"objective":"...","status":"active",...}}
```

### 9.2 配置持久化（TOML）

`codey/config.toml` 存储所有用户配置：

```toml
model = "deepseek-chat"
model_provider = "local-pool"
model_context_window = 128000
max_output_tokens = 65535
approval_policy = "on-request"
web_search = "enabled"

[model_providers.openai]
name = "OpenAI"
base_url = "https://api.openai.com/v1"
env_key = "OPENAI_API_KEY"

[model_providers.deepseek]
name = "DeepSeek"
base_url = "https://api.deepseek.com/v1"
env_key = "DEEPSEEK_API_KEY"

[mcp_servers.playwright]
command = "npx.cmd"
args = ["-y", "@playwright/mcp@latest"]
disabled = false

[smartbrain]
enabled = true
auto_extract = true
```

### 9.3 KV 持久化（SQLite）

`codey/usage.db` 中的 `app_state` 表存储前端持久化数据：

| Key | 用途 |
|-----|------|
| `projects` | 项目列表 |
| `thread-project-map` | 线程-项目映射 |
| `providers` | 供应商配置 |
| `active-provider` | 活跃供应商 |
| `configured-models` | 已配置模型列表 |
| `active-model` | 活跃模型 |
| `auto-approve` | 自动审批状态 |
| `sidebar-width` | 侧栏宽度 |
| `right-panel-width` | 右侧面板宽度 |

`usage` 表存储用量统计：

| 字段 | 说明 |
|------|------|
| `date` | 日期 |
| `model` | 模型名 |
| `provider` | 供应商 |
| `prompt_tokens` | 输入 tokens |
| `completion_tokens` | 输出 tokens |
| `total_tokens` | 总计 |
| `cost` | 费用 |
| `call_count` | 调用次数 |

---

## 10. 状态管理

### 10.1 Zustand appStore 核心状态

| 分组 | 字段 | 说明 |
|------|------|------|
| **初始化** | `initialized`, `initError`, `retryInit` | 应用启动状态 |
| **当前会话** | `currentThreadId`, `currentTurnId` | 活跃线程和 turn |
| **消息** | `messages`, `streamingText`, `streamingLabel`, `isStreaming` | 聊天消息和流式状态 |
| **模式** | `chatMode`（chat/goal）, `currentGoal` | 对话模式 |
| **模型** | `configuredModels`, `activeModelId`, `providers`, `activeProviderId` | 模型配置 |
| **工作区** | `workspaceCwd`, `configDir`, `configPath` | 路径 |
| **项目/线程** | `projects`, `currentProjectId`, `threads`, `threadProjectMap` | 多项目管理 |
| **UI** | `showSettings`, `rightPanelVisible`, `autoApprove`, `sidebarTab` | 界面状态 |
| **队列** | `pendingMessageQueue` | 等待发送的消息队列 |
| **文件审阅** | `pendingFileReviews` | 待审阅的文件列表 |
| **RunSummary** | `runSummary` | 运行摘要 |
| **浏览器** | `browserPanelUrl`, `browserActive` | 浏览器面板状态 |

### 10.2 流式输出状态流转

```
Idle ──→ Processing ──→ Generating ──→ Organizing ──→ Summarizing ──→ Idle
                  │                                                        ↑
                  └──── ToolExec ──── ToolExec ──── ... ────────────────────┘
```

| 阶段 | streamingLabel |
|------|---------------|
| Processing | "正在处理请求..." |
| Generating | "正在生成响应..." |
| Organizing | "正在组织答案结构..." |
| Summarizing | "正在汇总信息..." |
| ToolExec | "执行命令: xxx" |

### 10.3 工具调用卡片数据结构

```typescript
interface ToolCallItem {
    id: string;
    name: string;               // 工具名（如 "shell"）
    arguments: string;          // JSON 参数
    status: "running" | "success" | "failed";
    displayLabel: string;       // 显示文本（如 "ls -la"）
    output?: string;            // 执行输出
    patchProgress?: PatchProgressChange[];  // Patch 进度
}
```

---

## 11. 移动端远程控制

### 11.1 架构

```
┌─────────────────┐         ┌──────────────────────┐
│  Mobile Web App │ ←──→   │  Axum Web Server      │
│  (Vue/React)    │  HTTP   │  0.0.0.0:PORT         │
│                 │  WS     │  ├─ /ws (WebSocket)   │
│ 手机浏览器      │         │  ├─ /api/threads/*   │
└─────────────────┘         │  └─ / (静态文件服务)  │
                            └──────────┬───────────┘
                                       │ broadcast::Sender
                            ┌──────────▼───────────┐
                            │    Rust Backend       │
                            │    AgentEngine        │
                            └──────────────────────┘
```

### 11.2 连接方式

| 方式 | 说明 | 延迟 |
|------|------|------|
| **局域网直连** | 同 Wi-Fi 下直接连接 | 低延迟 |
| **公网中转服务器** | 通过 `relay_server_url` 配置中转 | 跨网络访问 |

### 11.3 WebSocket 广播

后端所有重要事件都会通过 `broadcast()` 函数广播到移动端：

```rust
pub fn broadcast(event: &str, payload: serde_json::Value) {
    if let Some(info) = crate::MOBILE_SERVER.get() {
        let _ = info.broadcast_tx.send(BroadcastEvent { event, payload });
    }
}
```

### 11.4 REST API 端点

| 端点 | 方法 | 说明 |
|------|------|------|
| `/ws` | WebSocket | 实时事件推送 |
| `/api/health` | GET | 健康检查 |
| `/api/active-thread` | GET | 获取当前线程 |
| `/api/threads` | GET | 线程列表 |
| `/api/threads/{id}` | GET | 线程详情 |
| `/api/threads/{id}/messages` | GET | 线程消息 |
| `/api/threads/{id}/chat` | POST | 发送消息 |
| `/api/threads/{id}/interrupt` | POST | 中断处理 |

### 11.5 QR 码连接

- 点击标题栏 QR 码按钮
- 后端生成包含局域网 IP + 端口的 QR 码 SVG
- 手机扫码即可连接

---

## 12. 外部浏览器与录制回放

### 12.1 外部浏览器管理

通过 Chrome DevTools Protocol (CDP) 控制外部浏览器：

```rust
pub struct ExternalBrowser {
    inner: Arc<Mutex<BrowserState>>,
}

struct BrowserState {
    child: Option<Child>,
    cdp_port: u16,              // 默认 9222
    user_data_dir: Option<tempfile::TempDir>,
}
```

**启动流程：**
1. 检查 CDP 端口是否已有浏览器在监听
2. 自动检测 Chrome/Edge 安装路径（Windows/Mac/Linux）
3. 使用 `--remote-debugging-port` 和临时 `--user-data-dir` 启动
4. 轮询 CDP 就绪（最多 15 秒）

### 12.2 操作录制

通过 CDP `Runtime.addBinding` 注入 JS 脚本来捕获用户操作：

```javascript
// 录制注入脚本核心逻辑
function rec(type, el, extra) {
    const event = {
        type: type,              // click | type | submit | select | navigate
        timestamp: Date.now(),
        url: location.href,
        selector: locators(el)[0],      // CSS 选择器
        tagName: el.tagName?.toLowerCase(),
    };
    window.__rr_push(JSON.stringify(event));  // 实时推送到 Rust
}

// 监听事件
document.addEventListener('click', handler, true);   // 点击
document.addEventListener('input', debouncedHandler); // 输入
document.addEventListener('submit', handler, true);   // 表单提交
window.addEventListener('popstate', checkNav);       // 导航
```

**录制事件类型：**

| 事件 | 触发条件 |
|------|---------|
| `click` | 点击事件 |
| `type` | 输入事件（500ms 防抖） |
| `submit` | 表单提交 |
| `select` | 下拉框变更 |
| `navigate` | 页面导航（popstate/hashchange） |

**录制工作流：**

```
1. browser_run goto → 打开目标页面
2. rr_session_create → 创建录制会话
3. rr_record_start → 开始录制（注入 JS + 注册 CDP binding）
4. 每步操作：
   ├─ browser_run（执行操作）
   └─ rr_record_action（记录操作）
5. rr_record_stop → 停止录制，保存 trace 文件
6. rr_compile → 编译为可回放的 SKILL.md
```

### 12.3 CDP WebSocket 通信

```rust
struct CdpWriter {
    sink: SplitSink,       // WebSocket 发送端
    next_id: i64,          // 递增请求 ID
    response_rx: Receiver, // 命令响应接收
}

struct CdpReader {
    stream: SplitStream,    // WebSocket 接收端
    response_tx: Sender,   // 响应分发
}
```

- 所有 CDP 命令通过 `command(method, params)` 发送
- 响应通过 `response_id` 匹配
- 15 秒超时保护

---

## 13. 上下文压缩（Compaction）

### 13.1 触发时机

| 时机 | 条件 |
|------|------|
| **Pre-turn** | turn 开始前，检查 `last_single_prompt_tokens` > 阈值 |
| **Mid-turn** | 工具调用后，单次 API `prompt_tokens` > 阈值 |
| **Goal continuation** | goal 循环继续前检查 |
| **手动** | 用户输入 `/compact` |

### 13.2 阈值计算

```rust
pub fn compact_threshold(config: &ConfigToml) -> u64 {
    if let Some(limit) = config.model_auto_compact_token_limit {
        return limit as u64;
    }
    let window = config.model_context_window.unwrap_or(128_000);
    (window as f64 * 0.9) as u64  // 默认在 115,200 tokens 触发
}
```

### 13.3 压缩流程

```
1. 检查 prompt_tokens > threshold
2. 构建 summarization 请求（system + history + SUMMARIZATION_PROMPT）
3. 调用 LLM 生成摘要
4. 收集最近的 user 消息（保留 ~20K tokens）
5. build_compacted_history() = 保留的 user 消息 + summary
6. thread_store.replace_messages() 重写 JSONL 文件
7. 发射 "context-compacted" 事件
```

### 13.4 压缩提示词

```
You are performing a CONTEXT CHECKPOINT COMPACTION. Create a handoff summary 
for another LLM that will resume the task. Include:
- Current progress and key decisions made
- Important context, constraints, or user preferences
- What remains to be done (clear next steps)
- Any critical data, examples, or references needed to continue

Be concise, structured, and focused on helping the next LLM seamlessly continue.
```

---

## 14. 插件系统

### 14.1 内置插件

| 插件 | 说明 |
|------|------|
| **browser** | 应用内浏览器控制 |
| **computer-use** | Windows 桌面应用控制 |
| **documents** | Word 文档创建与编辑 |
| **presentations** | PPT 创建与编辑 |
| **record-replay** | 浏览器操作录制与回放 |
| **sites** | 网站创建、构建与托管 |
| **spreadsheets** | Excel 电子表格操作 |
| **superpowers** | 增强能力集（计划/调试/审查/验证等） |

### 14.2 插件结构

每个插件包含：
- `.codex-plugin/plugin.json` — 插件元数据
- `skills/` — 插件提供的 skill 定义
- 可选的 MCP 服务器配置

### 14.3 App Connectors

插件可以暴露 App Connectors（MCP 工具集），通过 `app://{connector_id}` 在用户消息中触发。

---

## 15. 机器人系统

### 15.1 架构

```
用户请求 → AgentEngine (goal 模式)
              ↓
      RobotOrchestrator.prepare_state()
              ↓
      创建 ThreadRobotState
      ├─ robot_id: 机器人 ID
      ├─ current_node_index: 当前节点
      ├─ root_objective: 用户目标
      └─ runtime_nodes: 编译后的节点队列
              ↓
      robot_overlay_prompt → 注入 system prompt
              ↓
      Agent 循环执行当前节点
              ↓
      检测 <workflow_node_done/> 标记
              ↓
      advance_node() → 推进到下一节点
              ↓
      ... 直到全部节点完成
```

### 15.2 机器人定义

```json
{
  "name": "前端开发机器人",
  "description": "负责前端代码编写和样式调整",
  "icon": "code",
  "skills": ["react", "tailwind"],
  "pluginSkills": [],
  "workflowNodes": [
    { "objective": "分析需求并制定实施方案", "skills": ["analysis"] },
    { "objective": "编写前端组件代码", "skills": ["react", "tailwind"] },
    { "objective": "运行测试并修复问题", "skills": ["testing"] }
  ],
  "systemPrompt": "你是一位专业的前端开发工程师..."
}
```

### 15.3 节点推进

- 模型输出 `"<workflow_node_done/>"` 标记表示当前节点完成
- `RobotOrchestrator.advance_node()` 检查标记
- 自动将下一节点目标绑定到 thread goal
- 支持模板变量：`{{goal}}` / `{{objective}}`

---

## 16. 子代理系统

### 16.1 架构

```
主 Agent
  ├─ spawn_agent(config, prompt) → 生成子代理
  │     └─ tokio::spawn 启动异步任务
  │     └─ SubagentHandle { cancel_flag, input_tx }
  │     └─ oneshot::Receiver 接收结果
  ├─ send_input(agent_id, text) → 发送输入
  ├─ wait_agent(agent_id) → 等待完成
  ├─ list_agents() → 列出所有子代理
  └─ close_agent(agent_id) → 关闭子代理
```

### 16.2 子代理配置

```rust
pub struct SubagentConfig {
    pub base_url: String,           // LLM API URL
    pub api_key: String,            // API Key
    pub model: String,              // 模型名
    pub wire_api: String,           // API 格式
    pub system_prompt: String,      // 系统提示词
    pub cwd: PathBuf,               // 工作目录
    pub timeout_ms: u64,            // 超时（默认 5 分钟）
    pub max_iterations: usize,      // 最大迭代次数（默认 25）
    pub max_output_tokens: Option<i64>,
}
```

### 16.3 限制

- 子代理无法再生成子代理（深度限制 1 层）
- 子代理共享主 Agent 的工具集
- 子代理支持 `send_input` 持续通信

---

## 17. Hook 运行时

### 17.1 支持的 Hook 事件

| Hook 事件 | 触发时机 | 典型用途 |
|-----------|---------|---------|
| `on-agent-start` | Agent 开始处理前 | 前置检查、环境准备 |
| `on-user-prompt-submit` | 用户提交消息时 | 输入过滤/增强 |
| `on-agent-end` | Agent 处理完成时 | 后置检查、结果验证 |
| `on-file-change` | 文件被修改时 | 文件变更审计 |
| `on-command-exec` | 命令执行前 | 命令审批/拦截 |
| `on-post-tool-use` | 工具执行后 | 工具结果审查 |
| `on-subagent-stop` | 子代理关闭时 | 子代理结果审查 |

### 17.2 Hook 配置

可在 `config.toml`、`hooks.json` 或插件中定义：

```toml
[hooks.gate]
event = "on-command-exec"
command = "python gate.py"
timeout_ms = 5000
disabled = false
```

### 17.3 Hook 返回值

| 字段 | 说明 | 示例 |
|------|------|------|
| `decision` | 决策 | `"block"` / `"stop"` / `"feedback"` |
| `reason` | 原因 | `"需要审批"` |
| `additionalContext` | 额外上下文 | `{"files":[...]}` |
| `updatedInput` | 修改后的输入 | 用于 `on-user-prompt-submit` |

---

## 18. SmartBrain 智能脑

### 18.1 启动流水线

应用启动时自动执行：

```
1. Extractor: 从历史会话中提取经验
   └─ 提取的原始经验存储在 memories/experiences/raw/
   
2. Consolidator: 合并相似经验
   └─ 合并后的经验存储在 memories/experiences/

3. Knowledge Scanner: 扫描并索引知识文档
   └─ 构建 BM25 倒排索引

4. Index Builder: 重建搜索索引
   └─ 生成根索引文档
```

### 18.2 经验提取

```rust
pub fn run_extraction_backfill(
    http, config, thread_store, experiences_dir, app_handle
) -> Vec<RawExperience>
```

- 每次启动最多处理 5 条未提取的会话
- 最小会话长度：3 条消息
- 按资源池模型生成摘要

### 18.3 知识管理

| 操作 | 命令 | 说明 |
|------|------|------|
| 列表 | `smartbrain_list_knowledge` | 列出知识文档 |
| 读取 | `smartbrain_read_knowledge` | 读取知识内容 |
| 上传 | `smartbrain_upload_knowledge` | 上传知识文档 |
| 删除 | `smartbrain_delete_knowledge` | 删除知识文档 |
| 搜索 | `smartbrain_search` | BM25 全文搜索 |
| 重建索引 | `smartbrain_rebuild_index` | 重建搜索索引 |

### 18.4 自动知识注入

SmartBrain 会在每次 turn 开始前，自动检索与当前对话相关的经验/知识，注入到 Agent 的系统提示词中。

---

## 19. 设计系统

### 19.1 颜色体系

| Token | 值 | 用途 |
|-------|-----|------|
| `--accent` | `#22c55e` | 翡翠绿 - 主要操作、激活状态 |
| `--accent-strong` | `#4ade80` | 悬停强调 |
| `--app-bg` | `#1a1a1a` | 背景底色 |
| `--surface-main` | `rgba(30,30,30,0.92)` | 主内容区 |
| `--surface-sidebar` | `rgba(22,22,22,0.96)` | 侧栏 |
| `--text-strong` | `#f0f0f0` | 标题/强调 |
| `--text-base` | `#c8c8c8` | 正文 |
| `--text-muted` | `#888888` | 次要信息 |
| `--danger` | `#ef4444` | 错误/危险 |
| `--warning` | `#f59e0b` | 警告 |

### 19.2 字阶

| Token | 字号 | 字重 | 用途 |
|-------|------|------|------|
| display | 18px | 600 | 页面标题 |
| body | 14px | 400 | 正文、消息 |
| caption | 12px | 400 | 元数据、时间戳 |
| micro | 11px | 500 | 状态栏、徽章 |
| code | 13px | 400 | 代码块 |

### 19.3 间距体系

| Token | 值 | 用途 |
|-------|-----|------|
| xxs | 4px | 内联间距 |
| xs | 8px | 紧凑内边距 |
| sm | 12px | 标准间距 |
| base | 16px | 卡片内边距 |
| md | 20px | 内容外边距 |
| lg | 24px | 区块间距 |
| xl | 32px | 大区块间距 |

---

## 20. 构建与发布

### 20.1 开发模式（dev.bat）

```
1. 检查 Vite 端口 1420 可用性
2. 确保嵌入式 Node.js 便携版可用
3. 启动 Tauri dev 模式（Vite HMR + Rust 热重载）
```

### 20.2 构建模式（build.bat）

```
1. pnpm build — 构建前端
2. pnpm tauri build — 构建 Tauri 发布版
3. 复制产物到 build/ 目录
4. 启动构建产物
```

### 20.3 发布模式（publish.bat）

```
1. 清理 publish/ 目录
2. 安装前端依赖
3. 构建 mobile-web 移动端前端
4. Tauri Release 构建（--no-bundle 仅便携版）
5. 复制产物：CN-Codex.exe + DLL
6. 复制运行时资源：skills、plugins（排除敏感配置）
7. 打包 Node.js 便携版（v22.16.0）
8. 生成可分发的 publish/ 目录
```

### 20.4 发布构建优化

```batch
set "CARGO_INCREMENTAL=0"       // 禁用增量编译保证确定性
set "CARGO_BUILD_JOBS=1"        // 单线程构建避免 OOM
set "CARGO_PROFILE_RELEASE_LTO=false"
set "CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16"
set "CARGO_PROFILE_RELEASE_OPT_LEVEL=2"
```

---

## 21. 测试策略

### 21.1 前端测试

- **框架**：Vitest + jsdom
- **配置文件**：`vite.config.ts` 中的 `test` 配置
- **测试文件**：`src/**/*.test.{ts,tsx}`

### 21.2 后端 Rust 测试

- **内联测试**：每个模块末尾的 `#[cfg(test)] mod tests`
- **集成测试**：`src-tauri/src/*_integration_tests.rs`
- **测试运行**：`cargo test`

| 模块 | 测试覆盖 |
|------|---------|
| `agent.rs` | Hook 上下文构建、文件变更检测、机器人标记 |
| `tool_executor.rs` | 工具执行、MCP 通信、内存路径安全 |
| `config_system.rs` | TOML 解析、MCPServer 配置、编辑操作 |
| `compaction.rs` | 阈值计算、摘要消息检测 |
| `thread_store.rs` | 创建/持久化/压缩/Goal/机器人状态 |
| `hook_runtime.rs` | Hook 执行、决策解析、输出效果 |
| `standalone.rs` | 机器人 ID 解析、Playwright 配置 |
| `external_browser.rs` | 浏览器路径检测 |
| `robot_orchestrator.rs` | 状态准备、节点推进、覆盖提示 |

### 21.3 测试原则

- 每个 `pub fn` 都有对应的测试函数
- 使用 `tempfile` 创建临时目录进行 IO 测试
- 异步测试使用 `tokio::runtime::Runtime::new().block_on()`
- 测试覆盖边界条件、错误路径和正常路径

---

> 本文档由 Codey 自动整理生成，基于 CN-Codex 项目源码分析。---

---

## 22. 本地资源池（Local Pool）

### 22.1 架构概述

本地资源池（Local Pool）是 CN-Codex 的多端点 LLM 供应商管理系统。它为同一供应商配置多个 API 端点，支持自动故障转移和健康检测，确保在某个端点不可用时自动切换到可用端点。

```
ConfigToml
  ├─ model_providers.local-pool    ← 本地资源池供应商
  ├─ model_endpoints[]             ← 端点列表
  │   ├─ url                       ← API URL
  │   ├─ label                     ← 显示名称
  │   ├─ model                     ← 该端点使用的模型名
  │   ├─ api_key                   ← API Key
  │   └─ wire_api                  ← API 格式
  └─ active_endpoint_index         ← 当前活跃端点索引（持久化）
```

### 22.2 PoolResolver 核心实现

```rust
pub struct PoolResolver {
    inner: Arc<Mutex<HashMap<String, PoolState>>>,
    initial_index: usize,
}

struct PoolState {
    current_index: usize,
    health: HashMap<usize, EndpointHealth>,
}

struct EndpointHealth {
    fail_count: u32,
    last_failure: Option<Instant>,
}
```

- **线程安全**：使用 `Arc<Mutex<HashMap<String, PoolState>>>`，按 `provider_id:model` 键隔离不同模型的状态
- **顺序轮询**：从 `current_index` 开始依次尝试，跳过故障端点
- **60 秒恢复期**：故障端点标记后 60 秒自动恢复健康状态
- **全故障重置**：所有端点均不可用时，重置全部健康状态，强制从第一个端点重试

### 22.3 端点选择策略

```
resolve_endpoint(key, endpoints):
  1. 获取上次使用的 index
  2. 从该 index 开始顺序遍历
  3. 跳过 health.is_healthy() = false 的端点
  4. 返回第一个健康端点的 ResolvedEndpoint
  5. 如果全部不可用 → 重置健康状态 → 返回 endpoints[0]
```

### 22.4 故障标记流程

```
1. HTTP 请求返回 5xx/超时 → mark_failed(key, index)
2. fail_count += 1
3. last_failure = Instant::now()
4. 后续 resolve_endpoint 调用自动跳过该端点
5. 60 秒后 is_healthy() 恢复为 true
```

### 22.5 前端配置

前端通过 `ProviderPanel.tsx` 中的端点管理 UI 进行配置：
- 添加/编辑端点（URL、标签、模型、API Key）
- 设置活跃端点索引
- 端点配置持久化到 `config.toml` 的 `model_endpoints` 数组

---

## 23. OCR 离线识别

### 23.1 技术架构

CN-Codex 内置基于 **PaddleOCR v5 Mobile** 的离线文字识别引擎，使用 ONNX Runtime 进行推理。

```
图片输入
  ↓
decode_data_url() / load_image()    ← Base64 解码或文件加载
  ↓
downscale_large_image()             ← 缩放至 1600px 以内
  ↓
PP-OCRv5 Pipeline                   ← 检测 + 识别
  ├─ TextDetection (DBnet)          ← 文本框检测
  └─ TextRecognition (CRNN)         ← 文字识别
  ↓
collect_result_text()               ← 结果收集（信度 > 0.15）
  ↓
纯文本输出
```

### 23.2 模型文件

| 文件 | 说明 | 打包位置 |
|------|------|---------|
| `ppocrv5_mobile_det.onnx` | 文本检测模型 | `src-tauri/resources/ocr/` |
| `ppocrv5_mobile_rec.onnx` | 文字识别模型 | `src-tauri/resources/ocr/` |
| `ppocrv5_mobile_vocab.txt` | 识别词表 | `src-tauri/resources/ocr/` |
| `onnxruntime.dll` | ONNX Runtime 动态库 | `src-tauri/resources/ocr/` |

### 23.3 检测配置

```rust
TextDetectionConfig {
    score_threshold: 0.5,    // 文本框得分阈值
    box_threshold: 0.7,      // 框选阈值
    unclip_ratio: 1.8,       // 框扩展比例
    max_candidates: 40,      // 最大候选框数
    limit_side_len: Some(960), // 限制边长
    limit_type: Some(Max),   // 限制方式
    max_side_len: Some(2000), // 最大边长
}
```

### 23.4 图片预处理

```rust
const MAX_OCR_IMAGE_SIDE: u32 = 1600;

fn downscale_large_image(image: RgbImage) -> RgbImage {
    // 如果最长边 > 1600px，按比例缩小
    // 使用 Triangle 滤波保证缩放质量
}
```

### 23.5 使用场景

| 场景 | 入口 | 说明 |
|------|------|------|
| 用户 `view_image` 后 | `extract_text_from_data_urls()` | 自动提取图片中的文字 |
| 附件图片识别 | `OcrImageInput` → data URL 解码 | 多图批量识别 |
| 本地图片文件 | `extract_text_from_image_file()` | 直接识别文件系统中的图片 |

### 23.6 资源加载策略

```rust
fn resolve_ocr_resource_dir(project_root: &Path) -> AppResult<PathBuf> {
    // 查找顺序：
    // 1. CN_CODEX_OCR_RESOURCE_DIR 环境变量指定路径
    // 2. project_root/src-tauri/resources/ocr/
    // 3. project_root/resources/ocr/
    // 4. 可执行文件同目录 resources/ocr/
}
```

---

## 24. 终端模拟器

### 24.1 架构

```
┌─────────────────────────────────────────────────────┐
│                   前端 (React)                       │
│  ┌──────────────────────────────────────────────┐   │
│  │           TerminalPanel.tsx                   │   │
│  │  ┌────────────────────────────────────────┐  │   │
│  │  │         @xterm/xterm                   │  │   │
│  │  │  (xterm.js 终端模拟器)                  │  │   │
│  │  └────────────────────────────────────────┘  │   │
│  └──────────────────────────────────────────────┘   │
└──────────────────────┬──────────────────────────────┘
                       │ Tauri IPC
┌──────────────────────┴──────────────────────────────┐
│                   后端 (Rust)                        │
│  ┌──────────────────────────────────────────────┐   │
│  │           TerminalManager                      │   │
│  │  sessions: Mutex<HashMap<String, PtySession>> │   │
│  │                                                │   │
│  │  PtySession {                                  │   │
│  │    writer: Box<dyn Write + Send>,              │   │
│  │    _master: Box<dyn MasterPty + Send>,         │   │
│  │    _child: Box<dyn Child + Send + Sync>,       │   │
│  │  }                                             │   │
│  └──────────────────────────────────────────────┘   │
│                    │                                │
│  ┌─────────────────┴──────────────────────────┐    │
│  │          portable-pty                       │    │
│  │  打开 PTY (伪终端) → 衍生系统 Shell          │    │
│  └────────────────────────────────────────────┘    │
└────────────────────────────────────────────────────┘
```

### 24.2 命令集

| 命令 | 参数 | 说明 |
|------|------|------|
| `terminal_create` | `cwd?: string` | 创建新终端会话，返回 session_id |
| `terminal_write` | `session_id, data` | 向终端写入数据（Shell 输入） |
| `terminal_resize` | `session_id, cols, rows` | 调整终端尺寸 |
| `terminal_close` | `session_id` | 关闭终端会话 |

### 24.3 数据流

```
1. terminal_create → 
   - native_pty_system().openpty() 创建 PTY 对
   - spawn_command() 衍生默认 Shell
   - 启动后台线程读取 PTY 输出
   - 通过 Tauri emit("terminal-output", payload) 推送到前端

2. 后台读取线程：
   - reader.read(buf) → 编码为 UTF-8
   - emit("terminal-output", { sessionId, data })
   - 读到 EOF 时 emit { closed: true }

3. xterm.js 接收 terminal-output 事件：
   - terminal.write(data) 渲染到终端界面
```

### 24.4 技术特点

- **跨平台**：`portable-pty` 在 Windows 使用 WinPTY，macOS/Linux 使用 Unix PTY
- **零拷贝**：PTY 数据直接从系统内核到 xterm.js，无需中间缓冲
- **会话管理**：支持多终端标签页，每个标签独立 PTY 会话
- **尺寸同步**：前端 resize 事件实时同步到 PTY 尺寸

---

## 25. 窗口系统

### 25.1 窗口架构

CN-Codex 采用 **无边框多窗口架构**，包含三个核心窗口：

```
主窗口 (main)
├─ 标题：CN-Codex
├─ 尺寸：1200×800（默认），最小 800×600
├─ decorations: false（无边框）
├─ 全自绘标题栏 + 拖拽移动
└─ 主工作区：侧边栏 + 聊天 + 右侧面板

文档详情窗口 (document-detail)
├─ 标题：文档详情
├─ 尺寸：1060×760，最小 760×520
├─ 单实例复用（先查找已存在窗口）
├─ 显示文件内容（Markdown 渲染）
└─ 关闭时清空 active_path 缓存

文件差异窗口 (runsummary-diff)
├─ 标题：RunSummary Diff
├─ 尺寸：自动（基于 diff 内容）
├─ 单实例复用
├─ 显示文件变更前后对比
└─ payload 从 AppState 缓存读取
```

### 25.2 主窗口初始化

```rust
// tauri.conf.json
{
  "app": {
    "windows": [{
      "label": "main",
      "title": "CN-Codex",
      "width": 1200,
      "height": 800,
      "minWidth": 800,
      "minHeight": 600,
      "visible": false,         // 先隐藏，后端就绪后再显示
      "resizable": true,
      "decorations": false,     // 无边框
      "dragDropEnabled": false
    }]
  }
}
```

### 25.3 窗口控制命令

| 命令 | 功能 |
|------|------|
| `window_start_dragging` | 标题栏拖拽移动窗口 |
| `window_minimize` | 最小化 |
| `window_toggle_maximize` | 切换最大化/还原 |
| `window_close` | 关闭主窗口 |
| `window_show_main` | 显示主窗口（初始化完成后） |
| `window_open_browser` | 打开浏览器子窗口（400×600 浮动窗） |
| `window_resize_browser` | 调整浏览器子窗口尺寸 |
| `window_navigate_browser` | 浏览器子窗口导航 |
| `window_close_browser` | 关闭浏览器子窗口 |
| `window_open_document_detail` | 打开文档详情窗口 |
| `window_close_document_detail` | 关闭文档详情窗口 |
| `window_get_document_detail_path` | 获取当前详情窗文件路径 |
| `window_open_runsummary_diff` | 打开 Diff 对比窗口 |
| `window_close_runsummary_diff` | 关闭 Diff 对比窗口 |
| `window_toggle_devtools` | 切换开发者工具 |

### 25.4 8 秒强制显示机制

```rust
// 后台兜底：若 8 秒后前端仍未显示主窗口，强制显示
tokio::time::sleep(Duration::from_secs(8)).await;
if let Ok(false) = main_window.is_visible() {
    main_window.show()?;  // 强制显示
}
```

### 25.5 浏览器子窗口（Embedded Browser）

- 使用 Tauri WebviewWindow 作为嵌入式浏览器
- 支持 CDP 调试端口（9242）用于远程调试
- URL 验证：只允许 http/https 协议，拒绝 file://
- 端点元数据持久化到 `visible-browser.json`

---

## 26. 用户规则系统

### 26.1 规则类型

| 类型 | 文件路径 | 作用范围 |
|------|---------|---------|
| **全局规则** | `codey/rules/user.md` | 所有项目 |
| **项目规则** | 项目目录下的 `.rule.md` | 当前项目 |

### 26.2 加载机制

```
App 初始化
  ├─ rules_read() → 读取 codey/rules/user.md（全局）
  └─ rules_read_project() → 读取项目/.rule.md（项目级）
       ↓
内容合并后注入 Agent 的 system prompt
```

### 26.3 规则格式

```markdown
# 用户规则

## 编码规范
- 使用 TypeScript 严格模式
- 函数命名使用 camelCase
- 组件使用 PascalCase

## 项目约束
- 不要使用 any 类型
- 所有公共 API 必须有 JSDoc 注释
```

### 26.4 前端管理

- `SettingsPanel.tsx` → `RulesPanel`：编辑全局规则
- 编辑器中实时预览 Markdown
- 保存后自动注入到下一次 Agent 调用

### 26.5 注入流程

```rust
// 在 AgentEngine.run_turn() 中：
// 1. 读取全局规则文件
// 2. 读取项目规则文件
// 3. 合并到 system_prompt_builder
// 4. 作为 system message 的第一部分发送给 LLM
```

---

## 27. 协议层（Protocol）

### 27.1 架构

CN-Codex 内部实现了轻量级的 **JSON-RPC 风格协议**，用于以下通信场景：

```
前端 ←→ Rust 后端（Tauri IPC）
  ├─ approval (审批通道)
  ├─ notification (通知推送)
  └─ request/response (请求-响应)

Rust 后端内部
  ├─ approval_tx/rx → 审批队列
  └─ file_review_sessions → 文件审阅缓存
```

### 27.2 审批通道（Approval）

```rust
pub enum ApprovalAction {
    Resolve {
        request_id: RequestId,
        result: serde_json::Value,
    },
    Reject {
        request_id: RequestId,
        error: JSONRPCErrorError,
    },
}

// 通道创建
let (approval_tx, approval_rx) = mpsc::channel(64);
```

**流程**：
```
1. Agent 执行需要审批的操作（如 apply_patch、shell 命令）
2. approval_tx.send(ApprovalAction) → 前端收到审批请求
3. 前端显示 ApprovalModal
4. 用户批准/拒绝 → commands::resolve_approval / reject_approval
5. Agent 继续/中断执行
```

### 27.3 文件审阅（File Review）

`apply_patch` 前置确认机制：

```rust
pub struct PendingPatchReview {
    pub thread_id: String,
    pub call_id: String,
    pub raw_patch: String,
    pub files: Vec<FileReviewFile>,
    pub keep_all: bool,
    pub status: ReviewStatus,
}
```

**流程**：
```
1. Agent 调用 apply_patch
2. ToolExecutor 解析 patch，生成 PendingPatchReview
3. 写入 file_review_sessions cache (key: threadId + callId)
4. 前端弹出 PatchDiffModal 显示变更预览
5. 用户逐个文件确认/拒绝 → file_review_update()
6. 确认后 → file_review_apply() 执行写入
7. 取消 → file_review_cancel() 丢弃
```

### 27.4 请求-响应模块

```rust
pub mod jsonrpc {
    // JSON-RPC 风格的请求/响应数据结构
    pub struct Request {
        pub id: RequestId,
        pub method: String,
        pub params: serde_json::Value,
    }
    pub struct Response {
        pub id: RequestId,
        pub result: Option<serde_json::Value>,
        pub error: Option<JSONRPCErrorError>,
    }
}

pub mod notifications {
    // 纯通知（单向，无需响应）
    pub struct Notification {
        pub method: String,
        pub params: serde_json::Value,
    }
}

pub mod requests {
    // 前端请求封装
    pub struct FrontendRequest<T: Deserialize> {
        pub id: String,
        pub payload: T,
    }
}
```

### 27.5 事件推送

所有后端事件通过 Tauri `emit` 推送到前端：

```rust
fn emit_and_broadcast(app_handle: &AppHandle, event: &str, payload: serde_json::Value) {
    app_handle.emit(event, payload.clone()).ok();
    crate::mobile_server::broadcast(event, payload);  // 同时广播到移动端
}
```

### 27.6 主要事件列表

| 事件名 | 触发时机 | 载荷 |
|--------|---------|------|
| `turn-start` | turn 开始 | `{threadId, mode, model}` |
| `turn-completed` | turn 完成 | `{threadId, usage, changedFiles}` |
| `streaming-text` | LLM 流式输出 | `{delta}` |
| `tool-call-start` | 工具调用开始 | `{id, name, arguments}` |
| `tool-call-output` | 工具调用输出 | `{id, output}` |
| `context-compacted` | 上下文压缩完成 | `{threadId, savedTokens}` |
| `approval-request` | 需要用户审批 | `{requestId, action, reason}` |
| `terminal-output` | 终端输出 | `{sessionId, data}` |
| `goal-completed` | Goal 完成 | `{threadId, summary}` |
| `file-change` | 文件变更 | `{path, action}` |

---

## 更新：前端组件与状态管理补充

### 4.7 前端组件树完整结构

```
App.tsx
├── ErrorBoundary
├── IntlProvider (react-intl)
├── AppContent (useTauriEvents 注册)
├── app-frame
│   ├── AppBackground
│   ├── app-shell-content
│   │   ├── TitleBar
│   │   │   ├── 窗口控制按钮（最小化/最大化/关闭）
│   │   │   ├── 标题 + 二维码按钮
│   │   │   └── 拖拽区域
│   │   ├── app-workbench
│   │   │   ├── Sidebar (左侧)
│   │   │   │   ├── 标签切换（会话/项目）
│   │   │   │   ├── 会话列表（按项目分组）
│   │   │   │   ├── 搜索框
│   │   │   │   └── 底部功能按钮
│   │   │   ├── app-resizer-left (拖拽分隔条)
│   │   │   ├── app-main (聊天区)
│   │   │   │   ├── MessageList
│   │   │   │   │   ├── ChatMessage (用户/助手)
│   │   │   │   │   ├── CodeBlock (代码高亮)
│   │   │   │   │   ├── ToolCallCard (工具调用)
│   │   │   │   │   ├── RunSummaryCard (回合总结)
│   │   │   │   │   └── PlanCard (计划)
│   │   │   │   ├── ChatInput
│   │   │   │   │   ├── 文本输入区
│   │   │   │   │   ├── 模式切换（chat/plan/goal）
│   │   │   │   │   ├── 附件上传
│   │   │   │   │   └── SlashCommandPanel
│   │   │   │   └── StreamingIndicator
│   │   │   ├── app-resizer-right (拖拽分隔条)
│   │   │   └── RightPanel (右侧)
│   │   │       ├── Tab 切换
│   │   │       ├── BrowserPanel (应用内浏览器)
│   │   │       ├── TerminalPanel (终端)
│   │   │       ├── GitPanel (Git 操作)
│   │   │       └── FileTree (文件树)
│   │   └── StatusBar
│   │       ├── 模型名
│   │       ├── 活跃端点
│   │       └── 状态指示
│   ├── SettingsPanel (模态层)
│   │   ├── ProviderPanel
│   │   ├── HooksPanel
│   │   ├── SkillsPanel
│   │   ├── PluginsPanel
│   │   ├── RobotsPanel
│   │   ├── WorkflowsPanel
│   │   ├── KnowledgePanel
│   │   ├── ExperiencePanel
│   │   ├── IntegrationPanel
│   │   └── UsageDashboard
│   ├── ApprovalModal
│   ├── WorkflowExtractModal
│   ├── FortuneBubble
│   └── RecordingToggle
```

### 4.8 多页面构建

项目实现了三入口多页面架构：

```typescript
// vite.config.ts
rollupOptions: {
  input: {
    main: path.resolve(__dirname, "index.html"),    // 主窗口
    detail: path.resolve(__dirname, "detail.html"), // 文档详情窗口
    diff: path.resolve(__dirname, "diff.html"),     // Diff 对比窗口
  },
}
```

每个入口独立加载，独立窗口独立渲染，通过 Tauri IPC 通信。

### 4.9 国际化实现

使用 `react-intl` 实现中英文双语：

```typescript
// src/i18n/
├── zh-CN/
│   └── common.json           // 中文翻译
└── en-US/
    └── common.json           // 英文翻译

// App.tsx
<IntlProvider
  locale={locale}
  messages={messages[locale] ?? zhCN}
  defaultLocale="zh-CN"
>
```

- **默认语言**：zh-CN（中文）
- **切换**：设置面板中的语言选择
- **支持**：所有 UI 文案、错误提示、状态描述

### 4.10 Zustand 状态管理详解

**appStore.ts**（~1300 行）管理：

| 状态分组 | 关键字段 |
|---------|---------|
| **会话** | threads, currentThreadId, messages, streamingText |
| **项目** | projects, currentProjectId, workspaceCwd |
| **模型** | providers, activeProviderId, configuredModels, activeModelId |
| **布局** | sidebarWidth, rightPanelWidth, rightPanelVisible |
| **UI** | showSettings, sidebarTab, currentChatMode |
| **运行** | isStreaming, currentTurnId, isInitialized |
| **审批** | pendingFileReviews, pendingApproval |
| **性能** | performanceMetrics, initError |

**settingsStore.ts** 管理：
- `locale`：语言（zh-CN / en-US）
- `theme`：主题（dark / light / system）

---

## 更新：ToolExecutor 完整工具集

### 7.5 完整工具清单（40+ 工具）

#### 核心开发工具

| 工具名 | 功能 | 参数示例 |
|--------|------|---------|
| `shell` / `shell_command` | 执行 Shell 命令 | `{command, workdir, timeout_ms}` |
| `exec_command` | 持久化命令会话 | `{cmd, workdir, yield_time_ms}` |
| `write_stdin` | 写入命令 stdin | `{session_id, chars}` |
| `close_exec_session` | 关闭命令会话 | `{session_id}` |
| `read_file` | 读取文件内容 | `{path}` |
| `write_file` | 写入/覆盖文件 | `{path, content}` |
| `apply_patch` | 多文件补丁 | `{patch}` |
| `list_directory` | 列出目录 | `{path}` |

#### AI 辅助工具

| 工具名 | 功能 | 参数示例 |
|--------|------|---------|
| `update_plan` | 更新任务计划 | `{plan: [{step, status}]}` |
| `request_user_input` | 请求用户输入 | `{questions: [...]}` |
| `request_permissions` | 请求额外权限 | `{permissions, reason}` |
| `tool_search` | 搜索可用工具/技能 | `{query, limit}` |
| `code_review` | 代码审查 | `{base_ref, paths}` |

#### 浏览器工具

| 工具名 | 功能 | 参数示例 |
|--------|------|---------|
| `browser_run` | 浏览器自动化 | `{actions, url, viewport}` |
| `view_image` | 查看图片 | `{path}` |
| `ocr_image` | OCR 图片识别 | `{path}` |
| `image_generate` | AI 生成图片 | `{prompt, size, n}` |

#### MCP 工具

| 工具名 | 功能 | 参数示例 |
|--------|------|---------|
| `mcp_list_servers` | 列出 MCP 服务器 | — |
| `mcp_status` | 查询 MCP 状态 | `{server, probe}` |
| `mcp_list_tools` | 列出 MCP 工具 | `{server}` |
| `mcp_list_resources` | 列出 MCP 资源 | `{server}` |
| `mcp_list_prompts` | 列出 MCP 提示词 | `{server}` |
| `mcp_call_tool` | 调用 MCP 工具 | `{server, tool, arguments}` |
| `mcp_read_resource` | 读取 MCP 资源 | `{server, uri}` |
| `mcp_get_prompt` | 获取 MCP 提示词 | `{server, prompt, arguments}` |
| `mcp__*` (direct tools) | 直接调用 MCP 工具 | 按工具 schema |

#### 记忆/知识工具

| 工具名 | 功能 | 参数示例 |
|--------|------|---------|
| `memory_list` | 列出记忆 | `{path}` |
| `memory_read` | 读取记忆 | `{path}` |
| `memory_write` | 写入记忆 | `{path, content}` |
| `memory_update` | 更新记忆 | `{path, old_text, new_text}` |
| `memory_forget` | 删除记忆 | `{path, match_text}` |
| `memory_search` | 搜索记忆 | `{query, path}` |

#### 子代理工具

| 工具名 | 功能 | 参数示例 |
|--------|------|---------|
| `spawn_agent` | 生成子代理 | `{prompt, role, wait}` |
| `wait_agent` | 等待子代理 | `{agent_id, timeout_ms}` |
| `send_input` | 发送输入给子代理 | `{target, message}` |
| `resume_agent` | 恢复子代理 | `{id, timeout_ms, wait}` |
| `list_agents` | 列出子代理 | — |
| `close_agent` | 关闭子代理 | `{target}` |

#### 网络工具

| 工具名 | 功能 | 参数示例 |
|--------|------|---------|
| `web_search` | 网页搜索 | `{query, max_results}` |
| `web_fetch` | 抓取网页内容 | `{url, max_chars}` |

#### 机器人工具

| 工具名 | 功能 | 参数示例 |
|--------|------|---------|
| `robot_save` | 保存机器人配置 | `{id, config}` |
| `plugin_manage` | 插件管理 | `{action, plugin_id}` |
| `apps_list` | 列出应用连接器 | `{connector_id}` |

### 7.6 工具执行模式

```rust
// 同步模式：等待结果返回
pub enum ToolExecuteMode {
    Sync,     // 默认，等待执行完成
    Approve,  // 需要用户审批
}

// 执行结果
pub struct ToolExecuteResult {
    pub output: String,
    pub error: Option<String>,
    pub file_changes: Vec<FileChange>,
    pub needs_approval: bool,
}
```

---

## 更新：API 层完整命令列表

### 5.5 完整 Tauri Command 清单

#### 核心引擎命令

| 命令 | 类别 | 说明 |
|------|------|------|
| `standalone_init` | Core | 初始化 Standalone 引擎 |
| `standalone_config_read` | Core | 读取配置 |
| `standalone_config_write` | Core | 写入配置 |
| `standalone_thread_create` | Core | 创建新会话 |
| `standalone_thread_list` | Core | 列出会话 |
| `standalone_thread_read` | Core | 读取会话内容 |
| `standalone_chat` | Core | 发送聊天消息 |
| `standalone_turn_interrupt` | Core | 中断当前 turn |
| `standalone_thread_goal_set` | Core | 设置 Goal |
| `standalone_thread_goal_status` | Core | 查询 Goal 状态 |
| `standalone_thread_goal_edit` | Core | 编辑 Goal |
| `standalone_thread_goal_clear` | Core | 清除 Goal |
| `standalone_mcp_enable_playwright` | Core | 启用 Playwright MCP |
| `standalone_smartbrain_enable` | Core | 启用 SmartBrain |
| `fortune_llm_call` | Core | 运势 LLM 调用 |
| `fortune_detail_stream_start` | Core | 运势详情流式加载 |

#### 文件操作命令

| 命令 | 说明 |
|------|------|
| `read_directory` | 读取目录 |
| `read_file_for_attach` | 读取文件作为附件 |
| `read_text_file_preview` | 读取文本文件预览 |
| `write_text_file_preview` | 写入文本文件预览 |
| `reveal_in_explorer` | 在文件管理器中显示 |

#### Git 命令

| 命令 | 说明 |
|------|------|
| `git_status` | Git 状态 |
| `git_diff` | Git 差异 |
| `git_log` | Git 日志 |
| `git_branch_list` | 分支列表 |
| `git_stage` | 暂存文件 |
| `git_unstage` | 取消暂存 |
| `git_commit` | 提交 |
| `git_checkout` | 切换分支 |
| `git_pull` | 拉取 |
| `git_push` | 推送 |
| `git_reset` | 重置 |
| `git_revert` | 还原 |
| `git_cherry_pick` | Cherry Pick |

#### SmartBrain 命令

| 命令 | 说明 |
|------|------|
| `smartbrain_list_experiences` | 列出经验 |
| `smartbrain_read_experience` | 读取经验 |
| `smartbrain_delete_experience` | 删除经验 |
| `smartbrain_list_knowledge` | 列出知识 |
| `smartbrain_read_knowledge` | 读取知识 |
| `smartbrain_delete_knowledge` | 删除知识 |
| `smartbrain_upload_knowledge` | 上传知识 |
| `smartbrain_search` | 搜索 |
| `smartbrain_rebuild_index` | 重建索引 |
| `smartbrain_migrate_to_okf` | 迁移到 OKF 格式 |

#### 工作流命令

| 命令 | 说明 |
|------|------|
| `workflow_extract` | 从会话提取工作流 |
| `workflow_save` | 保存工作流 |
| `workflow_list` | 列出工作流 |
| `workflow_read` | 读取工作流 |
| `workflow_delete` | 删除工作流 |

---

> 本文档由 Codey 自动整理生成，基于 CN-Codex 项目源码（v0.1.0）全面分析。
> 涵盖 27 个技术章节，约 50,000 字符，覆盖项目所有核心模块。