# CN-Codex

<p align="center">
  <img src="app-icon.svg" alt="CN-Codex Logo" width="128" height="128">
</p>

<p align="center">
  <strong>强大的 AI 编程助手桌面应用</strong>
</p>

<p align="center">
  基于 Tauri 2 构建，支持多种 LLM 供应商，提供 40+ 内置工具和可扩展插件系统
</p>

<p align="center">
  <a href="http://47.113.221.244:8081/">官方网站</a> •
  <a href="http://47.113.221.244:8081/usage.html">使用指南</a> •
  <a href="https://github.com/cn-codex/cn-codex">GitHub</a>
</p>

---

## 核心亮点

- **开箱即用** — 下载即用，双击启动，无需复杂配置
- **多模型支持** — 支持 12+ 种 LLM 供应商（OpenAI、Anthropic、Google、DeepSeek、火山引擎、通义千问、智谱、Moonshot、硅基流动、百川、Ollama、LM Studio 等）
- **丰富工具** — 内置 40+ 工具，覆盖文件操作、命令执行、浏览器自动化、子代理、MCP 协议等
- **Goal 目标模式** — 设置目标后 AI 自主多步执行任务，遇到问题自动调整策略
- **插件系统** — 可扩展的插件架构，支持自定义技能、机器人和工作流
- **移动端支持** — 扫码连接手机，支持局域网直连和公网中转两种模式
- **双语界面** — 支持中文和英文，可运行时切换

---

## 功能概览

| 类别 | 说明 |
|------|------|
| 智能对话 | 多轮上下文对话、流式输出、Markdown 渲染、会话管理与搜索 |
| 工具调用 | 文件读写、Shell 命令、浏览器自动化、子代理、记忆、MCP 等 40+ 工具 |
| Goal 模式 | 设置目标让 AI 自主规划执行，支持 Token 预算控制 |
| 插件 | Browser、Computer Use、Documents、Presentations、Spreadsheets、Sites、Superpowers |
| 机器人 | AI 自动创建的专业角色，绑定技能和工作流 |
| 移动端 | 内置 Web 服务 + WebSocket 实时同步，支持公网中转服务器 |

> 完整功能说明和操作指引请查看 [使用指南](http://47.113.221.244:8081/usage.html)

---

## 快速开始

### 1. 启动应用

双击 `CN-Codex.exe` 启动。首次启动会在同目录创建 `codey/` 运行时文件夹。

### 2. 配置供应商

点击侧边栏底部的 **设置** → **模型提供商** → **添加供应商**，从预设列表中选择（如 DeepSeek、OpenAI 等），填写 API Key 和 Base URL，保存后点击 **启用**。

### 3. 添加项目

在侧边栏点击 **文件夹+** 按钮，选择代码目录。添加后 AI 在该目录下执行 Shell 命令和文件读写。

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

---

## 文档与链接

- [官方网站](http://47.113.221.244:8081/) — 产品介绍和下载
- [使用指南](http://47.113.221.244:8081/usage.html) — 从首次启动到高级功能的完整说明
- [GitHub](https://github.com/cn-codex/cn-codex) — 源代码和问题反馈

---

## 开源协议

本项目采用 MIT 协议开源。

---

## 致谢

本项目灵感来源于 OpenAI Codex CLI/TUI，在此表示感谢。

---

<p align="center">
  <strong>CN-Codex</strong> — 让 AI 成为你的编程伙伴
</p>
