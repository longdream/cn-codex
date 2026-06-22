<div align="center">

<img src="app-icon.svg" alt="CN-Codex" width="120" height="120">

# CN-Codex

**AI-Powered Desktop Coding Assistant with Mobile Remote Control & Bot Automation**

用手机遥控的 AI 全栈编程助手 — 机器人自动化 · 多模型支持 · 解压即用

[![GitHub Stars](https://img.shields.io/github/stars/longdream/cn-codex?style=flat&logo=github&color=yellow)](https://github.com/longdream/cn-codex/stargazers)
[![GitHub Release](https://img.shields.io/github/v/release/longdream/cn-codex?style=flat&logo=github)](https://github.com/longdream/cn-codex/releases)
[![License](https://img.shields.io/badge/license-Apache%202.0-blue?style=flat)](https://github.com/longdream/cn-codex/blob/main/LICENSE)
[![Platform](https://img.shields.io/badge/platform-Windows-0078D4?style=flat&logo=windows)](https://github.com/longdream/cn-codex/releases)
[![Built with Tauri](https://img.shields.io/badge/built%20with-Tauri%20v2-FFC131?style=flat&logo=tauri)](https://v2.tauri.app/)

[**下载最新版**](https://github.com/longdream/cn-codex/releases) ·
[官方网站](http://47.113.221.244:8081/) ·
[使用指南](http://47.113.221.244:8081/usage.html)

</div>

<br>

<p align="center">
  <img src="codexweb/assets/screenshot-main.png" alt="CN-Codex 主界面" width="800">
</p>

---

## 为什么选择 CN-Codex？

- **手机遥控编程** — 扫码连接，在手机上实时监控 AI 进度、发送指令、审批操作，通勤路上也能写代码
- **机器人自动化流水线** — AI 自动创建前端、测试、架构师等专业机器人，协作完成从需求到部署的全流程
- **国产模型深度适配** — 已在 DeepSeek v4 Flash 完成全面验证，同时支持 OpenAI、火山引擎等多种供应商
- **40+ 内置工具** — 文件读写、Shell 命令、浏览器自动化、子代理、MCP 等开箱即用
- **解压即用零门槛** — 下载 → 解压 → 双击启动，不需要 Node.js、Python 或任何运行时
- **完全开源** — Apache 2.0 协议，代码透明，自由定制

---

## 快速开始

### 1. 下载启动

从 [GitHub Releases](https://github.com/longdream/cn-codex/releases) 下载最新版本，解压后双击 `CN-Codex.exe` 即可启动。

### 2. 配置模型

进入 **设置** → **模型提供商** → 添加供应商（DeepSeek / OpenAI / 火山引擎等），填写 API Key 保存即可。

<p align="center">
  <img src="codexweb/assets/screenshot-provider.png" alt="供应商配置" width="700">
</p>

### 3. 开始编程

添加项目文件夹，在聊天框输入需求，AI 即开始自主编码。支持选择不同机器人角色来执行专业化任务。

<p align="center">
  <img src="codexweb/assets/screenshot-project.png" alt="项目管理与机器人" width="700">
</p>

---

## 核心功能

### 手机远程控制

扫码连接手机，随时随地掌控 AI 编程助手。

- **实时监控** — 在手机上查看 AI 执行进度、对话记录和工具调用状态
- **远程操作** — 发送消息、审批文件操作、中断或继续任务
- **双重连接** — 局域网直连（低延迟）+ 公网中转服务器（跨网络访问）

### 机器人系统

AI 自动创建专业角色，绑定技能和工作流，实现自动化流水线。

- **智能角色** — 根据任务自动创建前端开发、测试工程师、架构师等专业机器人
- **技能绑定** — 每个机器人可配置专属技能集（测试框架、部署脚本、文档生成等）
- **工作流编排** — 多机器人协作：需求分析 → 架构设计 → 编码 → 测试 → 部署

### 操作录制与回放

通过 CDP 协议连接外部浏览器，实时录制用户操作并生成可回放的工作流。

- **自动检测浏览器** — 优先启动本地 Chrome，自动开启远程调试端口
- **实时录制** — 捕获点击、输入、导航等操作，自动转换为可编辑的工作流 JSON
- **智能回放** — 基于录制的操作序列自动执行浏览器操作

### 智能网页搜索

内置双引擎搜索（DuckDuckGo + Bing），AI 可自主搜索互联网获取实时信息。

- **中文优化** — 中文问题自动使用中文关键词，Bing 中文版提供高质量结果
- **防循环策略** — 同一问题最多搜索 3 次，搜索后立即分析，防止无限搜索循环
- **内容抓取** — `web_fetch` 工具可读取任意 URL 并转换为可读文本

<details>
<summary><strong>查看完整功能列表</strong></summary>

<br>

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
| 网页搜索 | DuckDuckGo API + Bing 浏览器搜索双引擎，内置防循环策略 |
| 外部浏览器 | 自动检测并启动 Chrome/Edge，通过 CDP 协议录制和回放用户操作 |
| 本地资源池 | 管理多端点 LLM 供应商，自动故障转移，智能选择活跃端点 |

</details>

---

## 技术栈

| 层级 | 技术 |
|------|------|
| 桌面框架 | Tauri v2 |
| 后端 | Rust (Edition 2024) |
| 前端 | React 18 + TypeScript + Vite 6 |
| 样式 | Tailwind CSS 4 |
| 状态管理 | Zustand |
| 数据库 | SQLite |

---

## Star History

<a href="https://star-history.com/#longdream/cn-codex&Date">
 <picture>
   <source media="(prefers-color-scheme: dark)" srcset="https://api.star-history.com/svg?repos=longdream/cn-codex&type=Date&theme=dark" />
   <source media="(prefers-color-scheme: light)" srcset="https://api.star-history.com/svg?repos=longdream/cn-codex&type=Date" />
   <img alt="Star History Chart" src="https://api.star-history.com/svg?repos=longdream/cn-codex&type=Date" width="600" />
 </picture>
</a>

---

## 致谢

本项目灵感来源于 [OpenAI Codex](https://github.com/openai/codex) CLI/TUI，在此表示感谢。

## 开源协议

[Apache License 2.0](LICENSE)

---

<div align="center">

**如果 CN-Codex 对你有帮助，请给一个 Star 支持一下！**

[![Star](https://img.shields.io/github/stars/longdream/cn-codex?style=social)](https://github.com/longdream/cn-codex)

</div>
