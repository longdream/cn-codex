# CN-Codex

<p align="center">
  <img src="app-icon.svg" alt="CN-Codex Logo" width="128" height="128">
</p>

<p align="center">
  <strong>强大的 AI 编程助手桌面应用</strong>
</p>

<p align="center">
  基于 Tauri 2 构建，支持多种 LLM 供应商，提供丰富的工具能力和插件系统
</p>

<p align="center">
  <a href="#功能特性">功能特性</a> •
  <a href="#快速开始">快速开始</a> •
  <a href="#配置指南">配置指南</a> •
  <a href="#工具能力">工具能力</a> •
  <a href="#插件系统">插件系统</a>
</p>

---

## 项目简介

CN-Codex 是一款功能强大的 AI 编程助手桌面应用程序，采用 **Tauri 2 + React** 技术栈构建。它集成了先进的 AI Agent 引擎，支持多轮对话、工具调用、上下文压缩等能力，旨在为开发者提供高效的编程辅助体验。

### 核心亮点

- 🚀 **开箱即用** - 下载即用，无需复杂配置
- 🤖 **多模型支持** - 支持 OpenAI、Anthropic Claude、火山引擎、智谱 AI 等
- 🔧 **丰富工具** - 内置 40+ 工具，覆盖文件操作、代码执行、浏览器自动化等
- 🧩 **插件系统** - 可扩展的插件架构，支持自定义技能和工作流
- 📱 **移动端支持** - 扫码连接手机，随时随地使用
- 🌍 **双语界面** - 支持中文和英文，可运行时切换

---

## 功能特性

### 💬 智能对话

- **多轮对话** - 支持上下文关联的多轮交互
- **流式输出** - 实时显示 AI 响应，支持中断
- **Markdown 渲染** - 完整支持 Markdown 格式和代码高亮
- **会话管理** - 会话历史、搜索、归档、分叉
- **Goal 模式** - 设置目标让 AI 自主完成复杂任务

### 🔧 工具能力

CN-Codex 内置丰富的工具集，让 AI 能够真正帮助您完成任务：

| 类别 | 工具 | 说明 |
|------|------|------|
| **文件操作** | `read_file`, `write_file`, `list_directory` | 读写文件、目录浏览 |
| **命令执行** | `shell`, `exec_command` | 执行 Shell 命令、持久会话 |
| **代码管理** | `code_review`, `apply_patch` | 代码审查、补丁应用 |
| **浏览器自动化** | `browser_run` | 网页导航、截图、交互 |
| **图像处理** | `view_image`, `image_generate` | 图像查看、AI 生成图像 |
| **文档处理** | PDF/Word/Excel 解析 | 多格式文档读取 |
| **MCP 协议** | MCP 服务器管理 | 模型上下文协议支持 |

### 🎯 Goal 模式

设置目标后，AI 会自主规划并执行多步骤任务：

1. 分析目标，制定计划
2. 逐步执行，调用工具
3. 遇到问题自动调整
4. 完成目标后汇报结果

### 📱 移动端支持

启动移动服务器后，扫描二维码即可在手机上使用：

- 自动生成连接二维码
- 局域网内设备互联
- 实时同步会话状态

---

## 技术架构

### 技术栈

| 层级 | 技术 |
|------|------|
| 桌面框架 | Tauri v2 |
| 后端 | Rust (Edition 2024) |
| 前端 | React 18 + TypeScript + Vite 6 |
| 样式 | Tailwind CSS 4 |
| 状态管理 | Zustand |
| 国际化 | react-intl |
| 数据库 | SQLite |

### 目录结构

```
cn-codex/
├── src/                    # 前端 React 源码
│   ├── components/         # UI 组件
│   ├── stores/             # Zustand 状态管理
│   ├── hooks/              # React Hooks
│   ├── api/                # Tauri IPC 封装
│   └── i18n/               # 国际化资源
├── src-tauri/              # Rust 后端
│   ├── src/
│   │   ├── agent.rs        # Agent 引擎核心
│   │   ├── adapter/        # LLM 适配层
│   │   ├── commands/       # Tauri 命令
│   │   └── ...
│   └── tauri.conf.json     # Tauri 配置
├── codey/                  # 运行时资源
│   ├── config.toml         # 用户配置
│   ├── sessions/           # 会话持久化
│   ├── skills/             # 技能定义
│   ├── plugins/            # 插件目录
│   └── memories/           # 记忆存储
├── relay-server/           # 移动端中继服务器
└── docs/                   # 技术文档
```

---

## 配置指南

### 配置文件位置

配置文件位于 `codey/config.toml`（与可执行文件同目录）。

### 基础配置示例

```toml
# LLM 供应商配置
[providers.openai]
name = "OpenAI"
api_key = "sk-..."
base_url = "https://api.openai.com/v1"

[providers.anthropic]
name = "Anthropic"
api_key = "sk-ant-..."
base_url = "https://api.anthropic.com"

# 模型配置
model = "gpt-4o"
model_context_window = 128000

# 审批策略
approval_policy = "suggest"  # suggest / auto-edit / auto-run

# 上下文压缩
model_auto_compact_token_limit = 115000
```

### 支持的 LLM 供应商

| 供应商 | API 格式 | 说明 |
|--------|----------|------|
| OpenAI | Chat Completions / Responses | GPT-4o, GPT-4-turbo 等 |
| Anthropic | Claude API | Claude 3.5 Sonnet, Claude 3 Opus 等 |
| 火山引擎 | Chat Completions | Doubao 系列模型 |
| 智谱 AI | Chat Completions | GLM-4 系列 |
| 百度 | Chat Completions | ERNIE 系列 |
| Google | Gemini API | Gemini Pro 等 |
| 自定义 | OpenAI 兼容 API | 任何 OpenAI 兼容服务 |

### 审批策略

CN-Codex 提供灵活的工具执行审批策略：

| 策略 | 说明 |
|------|------|
| `suggest` | 建议审批，大多数操作需要确认 |
| `auto-edit` | 自动批准文件编辑操作 |
| `auto-run` | 自动批准命令执行 |

---

## 工具能力

### 文件与目录操作

```
read_file        - 读取文件内容
write_file       - 写入文件
list_directory   - 列出目录内容
apply_patch      - 应用多文件补丁
```

### 命令执行

```
shell            - 执行 Shell 命令（短时）
exec_command     - 启动持久会话
write_stdin      - 向会话写入输入
close_exec_session - 关闭会话
```

### 代码管理

```
code_review      - 审查 Git 变更
apply_patch      - 应用代码补丁
update_plan      - 更新任务计划
```

### 浏览器自动化

```
browser_run      - 运行浏览器会话
  - goto         - 导航到 URL
  - click        - 点击元素
  - fill         - 填写表单
  - screenshot   - 截取屏幕
  - eval         - 执行 JavaScript
```

### 图像处理

```
view_image       - 查看本地图像
image_generate   - AI 生成图像（需配置）
```

### 子代理系统

```
spawn_agent      - 启动后台子代理
wait_agent       - 等待子代理完成
send_input       - 向子代理发送消息
list_agents      - 列出所有子代理
close_agent      - 关闭子代理
```

### MCP 协议

```
mcp_list_servers      - 列出 MCP 服务器
mcp_list_tools        - 列出 MCP 工具
mcp_call_tool         - 调用 MCP 工具
mcp_list_resources    - 列出 MCP 资源
mcp_read_resource     - 读取 MCP 资源
```

---

## 插件系统

### 内置插件

CN-Codex 附带多个功能插件：

| 插件 | 功能 |
|------|------|
| **browser** | 浏览器自动化与网页测试 |
| **computer-use** | Windows 应用控制 |
| **documents** | Word 文档创建与编辑 |
| **presentations** | PowerPoint 演示文稿生成 |
| **spreadsheets** | Excel 电子表格处理 |
| **sites** | 网站托管与部署 |
| **superpowers** | 高级技能集合 |

### 技能（Skills）

技能是可复用的指令包，帮助 AI 完成特定任务：

- **test-driven-development** - 测试驱动开发
- **systematic-debugging** - 系统化调试
- **code-review** - 代码审查
- **browser-harness** - 浏览器自动化
- **writing-plans** - 编写实现计划
- **verification-before-completion** - 完成前验证

### 机器人（Robots）

机器人是绑定技能的专业 AI 角色：

- 配置特定技能组合
- 定义工作流程
- 自动化复杂任务

---

## 国际化

CN-Codex 支持多语言界面：

- **中文（简体）** - 默认语言
- **English** - 英文界面

可在设置面板中随时切换语言。

---

## 安全与权限

### 权限请求

敏感操作会请求用户授权：

- 文件系统访问
- 网络请求
- 命令执行

### 安全特性

- API Key 不明文存储
- Tauri CSP 安全策略
- 最小权限原则

---

## 移动端使用

### 启动移动服务器

1. 在应用中启动移动服务器
2. 查看显示的二维码和连接 URL
3. 手机扫码或输入地址连接

### 技术原理

- 内置 Web 服务器（Axum）
- WebSocket 实时通信
- 局域网设备发现

---

## 开源协议

本项目采用 MIT 协议开源。

---

## 致谢

本项目灵感来源于 OpenAI Codex CLI/TUI，在此表示感谢。

---

<p align="center">
  <strong>CN-Codex</strong> - 让 AI 成为你的编程伙伴
</p>