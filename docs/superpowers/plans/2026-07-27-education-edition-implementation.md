# AI 自主学习教练系统教育版 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在 `D:\rustwork\cn-codex-edu` 创建独立教育版应用，保留淡色主题、学习任务与对话主链路，移除开发工具入口，并提供桌面成长面板和 Android 配套端基础能力。

**Architecture:** 新工程从 CN-Codex 的源文件复制，不复制 `node_modules`、`target`、`dist` 等构建产物。桌面端保持三栏布局：教育任务栏、现有对话区、成长面板；教育版以静态/本地学习数据作为 MVP 数据源，避免把开发工具和工作区上下文带入主链路。Android 端在现有 `mobile-web` 基础上调整为学习陪伴、语音球和相机采集界面，并定义本地关键帧与姿态事件的最小化传输契约。

**Tech Stack:** React 18、TypeScript、Vite、Tauri 2、Zustand、Tailwind CSS、现有 mobile-web React/Vite 工程。

---

## 文件结构与职责

### 新工程根目录：`D:\rustwork\cn-codex-edu`

- `src/App.tsx`：教育版桌面壳，固定淡色主题、三栏布局与教育专属面板装配。
- `src/components/education/EducationSidebar.tsx`：学习任务、材料库、学习小工具与回顾入口。
- `src/components/education/GrowthPanel.tsx`：今日状态、知识点、学习热力图、掌握趋势、材料进度、计划节奏与证据抽屉。
- `src/components/education/learningData.ts`：MVP 学习任务、知识点与证据的本地演示数据和类型。
- `src/components/layout/TitleBar.tsx`：教育版名称与无开发模式的窗口标题。
- `src/components/layout/RightPanel.tsx`、`FileTree.tsx`、`GitPanel.tsx`、`TerminalPanel.tsx`：从教育版入口与构建中移除或停止引用。
- `src/stores/settingsStore.ts`、`src/utils/applyTheme.ts`：将可选主题收敛为淡色主题，避免黑色/系统主题入口。
- `src/i18n/zh-CN/common.json`：教育版可见文案，去除开发、项目、终端、Git、代码 Skills 等入口。
- `mobile-web/src/*`：Android 配套端界面、语音球、姿态候选片段和试卷关键帧/OCR 的本地数据契约。
- `src-tauri/*`：只保留教育版启动所需能力；先完成前端教育 MVP，再按编译错误最小化裁剪后端开发功能。

## 复制边界

复制源工程时排除：`.git`、`node_modules`、`src-tauri/target`、`dist`、`build`、`release`、`mobile-dist`、`logs`、`outputs`、`publish`、临时脚本和本机构建日志。复制 `src`、`src-tauri`（不含 `target`）、`mobile-web`（不含 `node_modules`）、配置文件、`assets` 和必要文档。

## Task 1: 创建独立工程与可重复复制脚本

**Files:**
- Create: `D:\rustwork\cn-codex-edu\`
- Create: `D:\rustwork\cn-codex-edu\README.md`
- Modify: `D:\rustwork\cn-codex-edu\package.json`
- Modify: `D:\rustwork\cn-codex-edu\src-tauri\Cargo.toml`

- [ ] **Step 1: 复制源码并排除构建缓存**

运行 `robocopy`，仅复制必要源目录与根配置，排除体积大的构建缓存目录。预期：新工程存在 `src/`、`src-tauri/`、`mobile-web/`、`package.json`，且不存在源工程的 `node_modules/` 和 `src-tauri/target/`。

- [ ] **Step 2: 修改应用标识**

将前端包名和 Tauri 包名/产品名改为教育版标识；保留可追溯的源工程说明。

- [ ] **Step 3: 验证复制边界**

运行目录大小和关键路径检查。预期：新工程远小于源工程，不包含 `target/release/deps`。

## Task 2: 固定淡色主题并移除开发外壳

**Files:**
- Modify: `D:\rustwork\cn-codex-edu\src\App.tsx`
- Modify: `D:\rustwork\cn-codex-edu\src\stores\settingsStore.ts`
- Modify: `D:\rustwork\cn-codex-edu\src\utils\applyTheme.ts`
- Modify: `D:\rustwork\cn-codex-edu\src\components\layout\TitleBar.tsx`
- Test: `D:\rustwork\cn-codex-edu\src\__tests__\educationShell.test.tsx`

- [ ] **Step 1: 写出失败测试**

测试教育版渲染教育任务栏和成长面板，且文档根节点使用淡色主题标记。

- [ ] **Step 2: 运行测试确认失败**

运行 `pnpm test -- educationShell`。预期：失败，因为教育版组件尚未存在。

- [ ] **Step 3: 最小实现教育版应用壳**

在 `App.tsx` 以教育版组件替换项目侧栏与开发右栏，保留中间聊天区；固定文档主题为 light；窗口标题改为“AI 自主学习教练”。

- [ ] **Step 4: 运行测试并构建**

运行对应 Vitest 测试和 `pnpm build`。预期：测试通过，TypeScript 与 Vite 构建通过。

## Task 3: 实现左侧学习任务与学习小工具入口

**Files:**
- Create: `D:\rustwork\cn-codex-edu\src\components\education\learningData.ts`
- Create: `D:\rustwork\cn-codex-edu\src\components\education\EducationSidebar.tsx`
- Modify: `D:\rustwork\cn-codex-edu\src\App.tsx`
- Test: `D:\rustwork\cn-codex-edu\src\components\education\EducationSidebar.test.tsx`

- [ ] **Step 1: 写出失败测试**

测试任务栏显示今日任务、即将开始、材料库、学习小工具和周期回顾；断言不显示“项目”“终端”“代码”等开发入口。

- [ ] **Step 2: 运行测试确认失败**

运行目标测试。预期：组件不存在或断言失败。

- [ ] **Step 3: 定义学习领域数据与组件**

定义 `LearningTask`、`KnowledgeSkill`、`LearningEvidence` 数据类型；以本地演示数据渲染任务状态、纸质材料页码、预计时长、学习小工具入口。

- [ ] **Step 4: 运行测试并人工检查**

运行目标测试，并在浏览器或桌面预览中检查无开发入口。

## Task 4: 实现右侧成长面板与可读图表

**Files:**
- Create: `D:\rustwork\cn-codex-edu\src\components\education\GrowthPanel.tsx`
- Create: `D:\rustwork\cn-codex-edu\src\components\education\GrowthPanel.test.tsx`
- Modify: `D:\rustwork\cn-codex-edu\src\App.tsx`

- [ ] **Step 1: 写出失败测试**

测试成长面板显示今日状态、技能与知识点、学习热力图、掌握趋势、材料进度、计划节奏与证据来源。

- [ ] **Step 2: 运行测试确认失败**

运行目标测试。预期：组件不存在或缺少标题。

- [ ] **Step 3: 最小实现卡片与 SVG/CSS 图表**

实现无需新增大型图表依赖的可访问卡片与轻量 SVG/CSS 视图；每个颜色状态提供文字说明；证据抽屉默认折叠并可展开。

- [ ] **Step 4: 运行测试、构建与视觉检查**

运行测试与 `pnpm build`；检查右栏在常见窗口宽度下不挤压聊天输入区。

## Task 5: 裁剪开发入口并保留 MiniApp 能力

**Files:**
- Modify: `D:\rustwork\cn-codex-edu\src\components\layout\Sidebar.tsx` 或停止引用
- Modify: `D:\rustwork\cn-codex-edu\src\components\layout\RightPanel.tsx` 或停止引用
- Modify: `D:\rustwork\cn-codex-edu\src\i18n\zh-CN\common.json`
- Modify: `D:\rustwork\cn-codex-edu\src\components\layout\MiniAppSidePanel.tsx`
- Test: `D:\rustwork\cn-codex-edu\src\__tests__\educationNavigation.test.tsx`

- [ ] **Step 1: 写出失败测试**

测试教育版导航保留学习小工具入口，不加载工作区、文件树、终端、Git 和代码工具入口。

- [ ] **Step 2: 运行测试确认失败**

运行目标测试。预期：旧开发入口仍存在。

- [ ] **Step 3: 实现教育版导航裁剪**

删除或断开开发面板的装配路径，保留并重命名 MiniApp 为“学习小工具”；限制其默认说明为闪卡、计时器、错题和复习工具。

- [ ] **Step 4: 运行测试与搜索检查**

运行测试；搜索应用入口确保没有项目选择、文件树、终端、Git、补丁或代码审查 UI 被加载。

## Task 6: 实现 Android 配套端 MVP 与数据契约

**Files:**
- Create/Modify: `D:\rustwork\cn-codex-edu\mobile-web\src\*`
- Create: `D:\rustwork\cn-codex-edu\mobile-web\src\learningCapture.ts`
- Create: `D:\rustwork\cn-codex-edu\mobile-web\src\learningCapture.test.ts`

- [ ] **Step 1: 写出失败测试**

测试姿态序列只产生非学习候选片段摘要；连续 `focused_like` 不产生上行事件。测试试卷帧仅选择满足清晰度阈值的少量最佳帧。

- [ ] **Step 2: 运行测试确认失败**

运行移动端目标测试。预期：契约函数不存在。

- [ ] **Step 3: 实现纯函数数据契约**

定义 `PostureSample`、`NonLearningCandidate`、`ExamFrameQuality`、`ExamCapturePayload`，实现端侧聚合和关键帧排序函数；不实现身份识别、不上传逐帧视频。

- [ ] **Step 4: 实现 Android 对话式界面**

实现任务状态顶部栏、圆形语音球、低清本地前置预览占位、语音/文本/相机入口和后置扫描模式；明确展示采集开关与隐私状态。

- [ ] **Step 5: 运行测试与移动端构建**

运行移动端测试与构建。预期：数据契约测试通过，移动端编译成功。

## Task 7: 后端与打包最小化检查

**Files:**
- Modify: `D:\rustwork\cn-codex-edu\src-tauri\Cargo.toml`
- Modify: `D:\rustwork\cn-codex-edu\src-tauri\tauri.conf.json`
- Test: `D:\rustwork\cn-codex-edu\src-tauri\tests\*`

- [ ] **Step 1: 识别被教育版前端引用的 Tauri 命令**

搜索前端 API 调用，列出教育 MVP 启动、窗口、配置、对话、学习小工具所需命令。

- [ ] **Step 2: 仅移除未引用的开发插件与后端入口**

每次删除一个插件/模块后运行 `cargo check`；不对无法独立裁剪的核心对话后端进行重构。

- [ ] **Step 3: 运行 Rust 检查**

运行 `pnpm rust:check:all`。预期：Cargo 检查通过。

## Task 8: 全量验证与交付说明

**Files:**
- Modify: `D:\rustwork\cn-codex-edu\README.md`
- Modify: `D:\rustwork\cn-codex-edu\docs\superpowers\specs\2026-07-27-education-edition-product-design.md`

- [ ] **Step 1: 运行前端、移动端与 Rust 验证**

运行桌面端测试、桌面端构建、移动端测试/构建、Rust 检查和 `git diff --check`。

- [ ] **Step 2: 在 README 记录运行方式和范围**

说明教育版为独立工程、仅淡色主题、保留学习小工具、Android 端采集最小化策略，以及不包含的开发能力。

- [ ] **Step 3: 检查复制目录不含构建缓存**

确认新工程不包含 `node_modules`、`target`、`dist` 等可再生构建目录；说明依赖安装与构建后这些目录会重新出现。
