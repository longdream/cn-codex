# CN-Codex

<p align="center">
  <img src="app-icon.svg" alt="CN-Codex Logo" width="128" height="128">
</p>

<p align="center">
  <strong>AI 驱动的全栈编程助手 — 手机远程控制 · 机器人自动化 · 全流程开发</strong>
</p>

<p align="center">
  基于 Tauri 2 构建，支持多种 LLM 供应商，已在 DeepSeek v4 Flash 模型下完成全面测试验证
</p>

<p align="center">
  <a href="https://github.com/longdream/cn-codex/releases"><strong>[ 立即下载 ]</strong></a> •
  <a href="http://47.113.221.244:8081/">官方网站</a> •
  <a href="http://47.113.221.244:8081/usage.html">使用指南</a> •
  <a href="https://github.com/longdream/cn-codex">GitHub</a>
</p>

---

## 下载安装

前往 **[GitHub Releases](https://github.com/longdream/cn-codex/releases)** 下载最新版本，解压后双击 `CN-Codex.exe` 即可启动，无需安装。

---

## 核心亮点

### 手机远程控制

扫码连接手机，随时随地掌控 AI 编程助手。

- **实时监控** — 在手机上查看 AI 执行进度、对话记录和工具调用状态
- **远程操作** — 发送消息、审批文件操作、中断或继续任务
- **双重连接** — 支持局域网直连（低延迟）和公网中转服务器（跨网络访问）
- **安全通信** — WebSocket 实时同步，消息加密传输

### 机器人系统

AI 自动创建专业角色，绑定技能和工作流节点，实现自动化流水线。

- **智能角色** — 根据任务需求自动创建前端开发、测试工程师、架构师等专业机器人
- **技能绑定** — 每个机器人可配置专属技能集（如测试框架、部署脚本、文档生成）
- **工作流编排** — 多机器人协作完成复杂任务：需求分析 → 架构设计 → 编码 → 测试 → 部署

### 全栈开发流程

从项目创建到测试部署，AI 自主规划执行完整开发流程。

- **项目创建** — 根据需求描述自动初始化项目结构、配置依赖
- **代码开发** — 多文件协同编辑、代码审查、重构优化
- **自动测试** — 编写测试用例、运行测试、分析覆盖率
- **持续迭代** — 根据测试结果自动修复 Bug、优化性能

> 已在 **DeepSeek v4 Flash** 模型下完成全栈项目开发、测试的全流程验证。

### 操作录制与回放

通过外部浏览器 CDP 协议，实时录制用户操作并生成可回放的工作流。

- **自动检测浏览器** — 优先启动本地 Chrome，自动开启远程调试端口
- **实时录制** — 注入 CDP 脚本捕获点击、输入、导航等操作，通过 `Runtime.addBinding` 实时推送事件
- **工作流生成** — 录制的操作序列自动转换为可编辑的工作流 JSON 文件
- **智能回放** — 基于录制的操作序列自动执行浏览器操作

### 智能网页搜索

内置网页搜索引擎，AI 可自主搜索互联网获取实时信息。

- **双引擎搜索** — DuckDuckGo API 为主，Bing 浏览器搜索为备选，避免百度验证码拦截
- **防循环策略** — 同一问题最多搜索 3 次，搜索后立即分析结果，防止无限搜索循环
- **中文优化** — 中文问题自动使用中文关键词搜索，Bing 中文版 (cn.bing.com) 提供高质量中文结果
- **内容抓取** — `web_fetch` 工具可读取任意 URL 并转换为可读文本

### 本地资源池

管理多端点 LLM 供应商，智能选择可用端点。

- **多端点管理** — 为同一供应商配置多个 API 端点，自动故障转移
- **活跃端点检测** — 自动识别并使用当前活跃的端点，避免 502 错误
- **运势功能** — 每日运势查询，使用本地资源池的活跃端点调用 LLM

---

## 功能概览

| 类别 | 说明 |
|------|------|
| 智能对话 | 多轮上下文对话、流式输出、Markdown 渲染、会话管理与搜索 |
| 工具调用 | 文件读写、Shell 命令、浏览器自动化、子代理、记忆、MCP 等 40+ 工具 |
| Goal 模式 | 设置目标让 AI 自主规划执行，支持 Token 预算控制 |
| 插件 | Browser、Computer Use、Documents、Presentations、Spreadsheets、Sites、Superpowers |
| 机器人 | AI 自动创建的专业角色，绑定技能和工作流 |
| 移动端 | 扫码连接手机，实时监控、远程操作，支持局域网直连和公网中转 |
| 用户规则 | 用户级全局规则和项目级规则，Markdown 格式，自动注入系统提示词 |
| SmartBrain | 经验积累和知识库管理，AI 自动总结经验并在后续任务中复用 |
| 子代理 | 复杂任务自动拆分为子代理并行执行，结果汇总后继续主流程 |
| 网页搜索 | 智能网页搜索与内容抓取，DuckDuckGo API + Bing 浏览器搜索双引擎，内置防循环策略 |
| 外部浏览器 | 自动检测并启动 Chrome/Edge，通过 CDP 协议录制和回放用户操作 |
| 运势功能 | 每日运势查询，支持本地资源池端点智能选择 |
| 本地资源池 | 管理多端点 LLM 供应商，自动选择 active 端点，避免 502 错误 |

> 完整功能说明和操作指引请查看 [使用指南](http://47.113.221.244:8081/usage.html)

---

## 快速开始

### 1. 下载并启动

从 [GitHub Releases](https://github.com/longdream/cn-codex/releases) 下载最新版本，解压后双击 `CN-Codex.exe` 启动。首次启动会在同目录创建 `codey/` 运行时文件夹。

### 2. 配置供应商

点击侧边栏底部的 **设置** → **模型提供商** → **添加供应商**，从预设列表中选择（如 DeepSeek、OpenAI 等），填写 API Key 和 Base URL，保存后点击 **启用**。

### 3. 添加项目

在侧边栏点击 **文件夹+** 按钮，选择代码目录。添加后 AI 在该目录下执行 Shell 命令和文件读写。

### 4. 手机连接

点击标题栏的 **二维码** 按钮，用手机扫码即可远程控制 AI 助手。

> 详细的配置说明和界面操作请查看 [使用指南](http://47.113.221.244:8081/usage.html)

---

## 技术栈

| 层级 | 技术 |
|------|------|
| 桌面框架 | Tauri v2 |
| 后端 | Rust (Edition 2024) |
| 前端 | React 18 + TypeScript + Vite 6 |
| 样式 | Tailwind CSS 4 |
| 状态管理 | Zustand |
| 国际化 | react-intl |
| 数据库 | SQLite |
| 测试模型 | DeepSeek v4 Flash |

---

## 进程排查：为什么会看到 `codex.exe`

- CN-Codex 主程序进程名是 `cn-codex.exe`，不是 `codex.exe`。
- 如果系统里出现 `codex.exe`，通常是命令执行链路触发（例如 `shell` / `exec_command` 实际执行了 `codex ...`）。
- 也可能来自自定义配置：`codey/config.toml` 里的 MCP `command` 或 `codey/hooks.json` / 插件 hooks 命令中显式写了 `codex`。
- 建议按顺序排查：最近工具调用记录 → MCP 配置命令字段 → Hook 命令字段。
- 若不需要 Codex CLI，可移除相关配置或从 PATH 中去掉 `codex.exe`。

---

## 文档与链接

- [下载最新版本](https://github.com/longdream/cn-codex/releases) — 免安装，解压即用
- [官方网站](http://47.113.221.244:8081/) — 产品介绍
- [使用指南](http://47.113.221.244:8081/usage.html) — 从首次启动到高级功能的完整说明
- [GitHub](https://github.com/longdream/cn-codex) — 源代码和问题反馈

---

## 开源协议

本项目采用 Apache License 2.0 开源。

---

## 致谢

本项目灵感来源于 OpenAI Codex CLI/TUI，在此表示感谢。

---

<p align="center">
  <strong>CN-Codex</strong> — 手机在手，代码我有
</p>
