# CN-Codex 需求规格说明书

## 1. 项目概述

CN-Codex 是基于 OpenAI Codex CLI/TUI 核心功能构建的 Tauri v2 桌面应用，
提供中英文双语界面（默认中文），支持 LLM 多档位配置以节约 token 消耗。

### 1.0 项目初衷
- 目标：让普通开发者长期用上“廉价又好用”的 AI 编程 IDE
- 成本路线：优先适配高性价比模型与 free plan（如商汤 free coding plan 的 DeepSeek v4 Flash）
- 渠道自由：原生支持官方源、中转站、本地模型；可自定义 Base URL / API Key / 模型 ID / Wire API
- 真实用法：日常用廉价/免费渠道做全流程开发，复杂任务用中转强模型（如 Grok 4.5）攻坚
- 能力完整：不只聊天改代码，而是手机遥控 + 机器人流水线 + SmartBrain 经验沉淀

### 1.1 技术栈
| 层级 | 技术 |
|------|------|
| 桌面框架 | Tauri v2 |
| 后端 | Rust (edition 2024) |
| 前端 | React 18 + TypeScript + Vite 6 |
| 样式 | Tailwind CSS 4 |
| 状态管理 | Zustand |
| 国际化 | fluent-rs (Rust) + react-intl (前端) |
| 核心引擎 | codex-app-server-client (in-process) |

### 1.2 核心原则
- 所有功能严格对标 Codex CLI/TUI，不添加非 CLI/TUI 功能
- 品牌全面替换：codex → cn-codex
- 中文为默认语言，支持运行时切换

---

## 2. 功能需求

### FR-001 应用初始化
- 启动时初始化 InProcessAppServerClient
- 加载用户配置（~/.codex/config.toml）
- 建立事件循环，监听 ServerNotification

### FR-002 会话管理
- 创建新会话 (thread/start)
- 恢复历史会话 (thread/resume)
- 会话列表与搜索 (thread/list)
- 会话归档/取消归档 (thread/archive, thread/unarchive)
- 会话分叉 (thread/fork)

### FR-003 对话交互
- 发送用户消息 (turn/start)
- 流式接收 Agent 回复 (AgentMessageDelta)
- 中断当前回复 (turn/interrupt)
- 引导方向 (turn/steer)
- Markdown 渲染 + 代码高亮

### FR-004 审批系统
- 命令执行审批 (item/commandExecution/requestApproval)
- 文件修改审批 (item/fileChange/requestApproval)
- 权限审批 (item/permissions/requestApproval)
- 审批策略配置 (untrusted/on-request/on-failure/never)

### FR-005 配置管理
- 读取/写入配置 (config/read, config/value/write)
- 模型选择 (model/list)
- 权限配置文件 (permissionProfile/list)
- 工作目录设置

### FR-006 认证系统
- API Key 登录 (account/login/start)
- 账户状态查看 (account/read)
- 登出 (account/logout)
- 速率限制信息 (account/rateLimits/read)

### FR-007 LLM 多档位配置
- 三档位模型选择：低/中/高 (low/medium/high)
- 场景绑定：根据任务类型自动选择档位
- Token 预算管理：日限额、警告阈值、自动降级
- 用量统计面板

### FR-007b 多模型渠道与中转
- 支持 12+ 供应商预设与自定义供应商实例
- 支持自定义 Base URL / API Key / 模型 ID / Wire API（Chat / Responses / Anthropic / Gemini）
- 同类型供应商可创建多个实例并行保存，运行时切换
- 本地资源池：同一模型多 endpoint，故障探测与自动 failover
- 适合 free plan、中转站、官方源与本地模型混用

### FR-008 国际化
- 中文 (zh-CN) 为默认语言
- 英文 (en-US) 可选
- 运行时语言切换
- Rust 后端：fluent-rs 消息格式
- 前端：react-intl 消息格式

### FR-009 命令输出
- 命令执行输出流式显示 (CommandExecutionOutputDelta)
- 文件变更摘要 (fileChange/outputDelta, fileChange/patchUpdated)
- 推理过程显示 (ReasoningTextDelta, ReasoningSummaryTextDelta)

### FR-010 计划模式
- Plan/Goal 模式切换
- 计划增量更新 (PlanDelta)
- 目标设置/清除 (thread/goal/set, thread/goal/clear)

### FR-011 斜杠命令
- /help - 帮助信息
- /model - 切换模型
- /approval - 更改审批模式
- /clear - 清除上下文
- /compact - 压缩历史 (thread/compact/start)
- /history - 查看历史

### FR-012 Skills 集成
- 技能列表查看 (skills/list)
- 技能启用/禁用 (skills/config/write)
- 技能变更通知 (skills/changed)

### FR-013 MCP 服务器管理
- MCP 服务器状态列表 (mcpServerStatus/list)
- 服务器重载 (config/mcpServer/reload)
- 资源读取 (mcpServer/resource/read)

### FR-014 侧边栏
- 会话历史列表
- 项目分组
- 搜索功能
- 新会话创建

### FR-015 设置面板
- 个人设置（语言、主题、外观）
- LLM 档位配置
- MCP 集成管理
- Hooks 管理

---

## 3. 非功能需求

### NFR-001 性能
- 首次启动 < 5s
- 消息延迟 < 100ms（从后端到界面渲染）
- 流式渲染帧率 ≥ 30fps

### NFR-002 安全
- Tauri CSP 策略
- 能力权限最小化
- API Key 不明文存储

### NFR-003 可用性
- Windows 10+ 支持
- 窗口最小尺寸 800x600
- 响应式布局

### NFR-004 可维护性
- 模块化 Rust 后端（命令/状态/桥接/LLM 分离）
- 前端组件化（每个页面独立组件）
- TypeScript 严格模式

---

## 4. 架构约束

### 4.1 通信模型
```
Frontend (React) ←→ Tauri IPC ←→ Rust Backend ←→ InProcessAppServerClient ←→ Codex Core
```

### 4.2 事件流
```
Codex Core → ServerNotification → Rust Event Loop → Tauri emit() → Frontend listener
```

### 4.3 请求流
```
Frontend invoke() → Tauri Command → RequestHandle.request() → Codex Core → Response
```
