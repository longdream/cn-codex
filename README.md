# CN-Codex

<p align="center">
  <img src="app-icon.svg" alt="CN-Codex Logo" width="128" height="128">
</p>

<h3 align="center">AI 驱动的编程助手桌面应用</h3>

<p align="center">
  不只是聊天，是真正能帮你写代码、执行命令、自动完成任务的 AI 工作台
</p>

<p align="center">
  <a href="README.md">中文</a> |
  <a href="README.en.md">English</a> |
  <a href="README.ja.md">日本語</a> |
  <a href="README.fr.md">Français</a> |
  <a href="README.de.md">Deutsch</a>
</p>

<p align="center">
  <a href="http://47.113.221.244:8081/">官方网站</a> •
  <a href="http://47.113.221.244:8081/usage.html">使用指南</a> •
  <a href="https://github.com/longdream/cn-codex">GitHub</a>
</p>

---

## 为什么选择 CN-Codex？

传统 AI 编程助手只能对话。CN-Codex 是一个**完整的 AI 工作台**——它能直接读写你的代码、执行 Shell 命令、操控浏览器、管理子代理并行工作，甚至通过手机远程监控 AI 执行进度。

- **40+ 内置工具** — 文件操作、命令执行、浏览器自动化、子代理、MCP 协议，一应俱全
- **12+ LLM 供应商** — OpenAI、Anthropic、Google、DeepSeek、火山引擎、通义千问、智谱、Moonshot、硅基流动、百川、Ollama、LM Studio
- **Goal 目标模式** — 设定目标，AI 自主规划执行多步骤任务，无需逐步手动指令
- **插件 + 技能 + 机器人** — 可扩展的自动化体系，打造专属 AI 工作流
- **手机扫码同步** — 局域网直连或公网中转，随时随地掌控你的 AI 助手
- **开箱即用** — 下载双击启动，无需安装 CLI、无需配置环境变量

---

## 界面预览

<p align="center">
  <img src="docs/screenshot-main.png" alt="主界面" width="800">
</p>
<p align="center"><em>首次启动 — 简洁的深色界面，左侧项目管理，中间对话区域</em></p>

<br>

<p align="center">
  <img src="docs/screenshot-chat.png" alt="聊天界面" width="800">
</p>
<p align="center"><em>智能对话 — 流式输出、模型切换、自动审批，底部状态栏一目了然</em></p>

<br>

<p align="center">
  <img src="docs/screenshot-project.png" alt="项目模式" width="800">
</p>
<p align="center"><em>项目模式 — 支持聊天/目标双模式切换，机器人选择器，AI 在你的项目目录下工作</em></p>

<br>

<p align="center">
  <img src="docs/screenshot-provider.png" alt="供应商设置" width="800">
</p>
<p align="center"><em>供应商配置 — GUI 可视化管理多个 LLM 供应商，支持模型列表和视觉能力标记</em></p>

---

## 核心能力一览

| 能力 | 说明 |
|------|------|
| 智能对话 | 多轮上下文、流式输出、Markdown 渲染、会话搜索与分叉 |
| 工具调用 | 文件读写、Shell、浏览器自动化、子代理、记忆、MCP 等 40+ 工具 |
| Goal 模式 | AI 自主多步执行，支持 Token 预算控制和状态监控 |
| 插件系统 | Browser、Computer Use、Documents、Presentations、Spreadsheets、Sites、Superpowers |
| 机器人 | AI 自动创建专业角色，绑定技能和工作流配置 |
| 移动端 | 内置 Web 服务 + WebSocket，支持局域网直连和公网中转 |
| 终端面板 | 内嵌 xterm.js 终端，多标签页，与 AI 并行工作 |
| Hooks | 事件驱动的自动化钩子，支持 Agent 生命周期各阶段 |

> 完整功能文档和操作指引请查看 **[使用指南](http://47.113.221.244:8081/usage.html)**

---

## 快速开始

### 1. 启动应用

双击 `CN-Codex.exe` 启动。首次运行会在同目录创建 `codey/` 运行时文件夹。

### 2. 配置供应商

**设置** → **模型提供商** → **添加供应商** → 选择预设（如 DeepSeek、OpenAI）→ 填写 API Key 和 Base URL → **保存** → **启用**

### 3. 添加项目并开始

侧边栏点击 **文件夹+** 添加代码目录，选择模型，开始对话。AI 将在你的项目目录下执行所有操作。

> 详细步骤和高级配置请查看 **[使用指南](http://47.113.221.244:8081/usage.html)**

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

## 相关链接

| 链接 | 说明 |
|------|------|
| [官方网站](http://47.113.221.244:8081/) | 产品介绍与下载 |
| [使用指南](http://47.113.221.244:8081/usage.html) | 从首次启动到高级功能的完整文档 |
| [GitHub](https://github.com/longdream/cn-codex) | 源代码与问题反馈 |

---

## 开源协议

MIT License

---

<p align="center">
  <strong>CN-Codex</strong> — 让 AI 成为你的编程伙伴
</p>
