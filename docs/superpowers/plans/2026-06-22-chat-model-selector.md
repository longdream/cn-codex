# Chat Model Selector Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 新建对话继承已启动供应商，并提供供应商下拉框与可搜索模型下拉框。

**Architecture:** 将供应商内模型解析和筛选提取为纯函数，供 `ChatInput` 使用并直接单元测试。`ChatInput` 只负责菜单状态、两段联动和受限高度布局；全局与对话级状态仍由 Zustand store 管理。

**Tech Stack:** React 18、TypeScript、Zustand、Vitest、Tailwind CSS

---

### Task 1: 模型解析与筛选规则

**Files:**
- Create: `src/utils/chatModelSelection.ts`
- Create: `src/__tests__/chatModelSelection.test.ts`

- [ ] **Step 1: 写失败测试**

覆盖以下行为：当前供应商为 Z-API 时忽略属于 Grok 的旧版 active model；优先使用属于当前供应商的对话覆盖和 `currentModel`；按模型 label/id 不区分大小写筛选。

- [ ] **Step 2: 验证测试失败**

Run: `pnpm test src/__tests__/chatModelSelection.test.ts`
Expected: FAIL，因为纯函数模块尚不存在。

- [ ] **Step 3: 实现最小纯函数**

提供 `resolveProviderModelId(provider, overrideModelId, currentModel, legacyActiveModel)` 和 `filterProviderModels(models, query)`，所有候选模型都必须在当前 provider 的模型列表中验证。

- [ ] **Step 4: 验证测试通过**

Run: `pnpm test src/__tests__/chatModelSelection.test.ts`
Expected: PASS。

### Task 2: 双下拉联动界面

**Files:**
- Modify: `src/components/chat/ChatInput.tsx`
- Modify: `src/i18n/zh-CN/common.json`
- Modify: `src/i18n/en-US/common.json`

- [ ] **Step 1: 接入统一模型解析**

使用 Task 1 的纯函数替代 `activeEntry?.model ?? currentModel`，确保旧 Grok 模型不能覆盖当前 Z-API。

- [ ] **Step 2: 添加菜单局部状态**

维护待选供应商 ID、模型搜索词和模型候选框开关；打开菜单时从当前有效供应商同步，关闭或切换供应商时清理搜索状态。

- [ ] **Step 3: 替换平铺列表**

供应商使用 `<select>`；模型使用文本输入和受限高度候选列表。切换供应商时为当前对话选择该供应商的首个有效模型；点击模型时调用 `setThreadModelOverride(providerId, modelId)`。

- [ ] **Step 4: 添加响应式和防遮挡样式**

菜单锚定输入区上方，宽度受限；控件在小屏纵向排列；候选列表 `max-height` + `overflow-y-auto`；整体高度使用视口上限。

- [ ] **Step 5: 添加中英文文案**

增加供应商、搜索模型、无结果等键，并运行 i18n 测试。

### Task 3: 回归验证

**Files:**
- Verify: `src/__tests__/appStore.test.ts`
- Verify: all frontend source

- [ ] **Step 1: 运行定向测试**

Run: `pnpm test src/__tests__/chatModelSelection.test.ts src/__tests__/appStore.test.ts src/__tests__/i18n.test.ts`
Expected: PASS。

- [ ] **Step 2: 运行全部测试**

Run: `pnpm test`
Expected: 所有测试通过。

- [ ] **Step 3: 运行生产构建**

Run: `pnpm build`
Expected: TypeScript 检查和 Vite 构建成功。

- [ ] **Step 4: 检查差异**

Run: `git diff --check`
Expected: 无空白或补丁格式错误，并确认未改动无关工作区文件。
