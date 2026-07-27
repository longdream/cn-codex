# Skill 实验室运行模型与本地知识库实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 为 Skill 实验室自动生成和迭代进化增加独立的临时供应商、模型与本地知识库选择，同时确保这些选择不进入 Skill 元数据或部署产物。

**Architecture:** 前端新增纯函数管理实验室最近选择与命令级供应商快照，并在 SkillLabPanel 中呈现两个独立配置区。后端复用主对话内存配置覆盖结构，为生成和迭代命令应用不可持久化的 ConfigToml 克隆；生成走主 Agent 的知识库能力，迭代继续走现有非流式闭环并注入请求级知识召回。

**Tech Stack:** React 18、TypeScript、Zustand、Vitest、Tauri 2、Rust、Serde、现有 SmartBrain BM25 检索。

---

## 文件边界

- Create: `src/utils/skillLabRuntime.ts` — 实验室阶段偏好读取、校验、回退与运行参数类型。
- Create: `src/components/shared/ProviderModelPicker.tsx` — 供应商/模型联动选择核心 UI。
- Create: `src/__tests__/skillLabRuntime.test.ts` — 前端偏好和运行快照行为测试。
- Modify: `src/stores/appStore.ts` — 导出可复用的供应商运行快照构造方法类型，不改变线程行为。
- Modify: `src/components/chat/ChatInput.tsx` — 使用共享选择核心或共享解析能力，保持主对话行为。
- Modify: `src/components/settings/SkillLabPanel.tsx` — 两阶段配置区、偏好保存、命令参数和执行锁定。
- Modify: `src/i18n/zh-CN/common.json` and `src/i18n/en-US/common.json` — 配置区、知识库和失效提示文案。
- Modify: `src-tauri/src/standalone.rs` — 将现有线程覆盖结构及内存合并函数开放给 crate 内复用。
- Modify: `src-tauri/src/commands/skill_lab.rs` — 运行配置参数、生成覆盖、迭代覆盖、知识召回和上下文注入。
- Modify: `docs/skill-lab-autogen-regression.md` — 增加人工回归项。

### Task 1: 前端运行偏好纯函数

- [ ] 在 `src/__tests__/skillLabRuntime.test.ts` 写失败测试：无偏好时两个阶段继承当前供应商/模型且知识库关闭；已保存阶段彼此独立；失效模型回退；序列化结果不包含 API Key。
- [ ] 运行 `pnpm test -- src/__tests__/skillLabRuntime.test.ts`，确认因模块不存在而失败。
- [ ] 在 `src/utils/skillLabRuntime.ts` 实现 `loadSkillLabPreferences`、`saveSkillLabPreferences`、`resolveSkillLabStagePreference` 和类型定义，使用独立 localStorage key，仅保存 `providerId/modelId/smartbrainEnabled`。
- [ ] 再运行目标测试并确认通过。

### Task 2: 通用供应商/模型选择 UI

- [ ] 为选择状态解析补充失败测试：供应商切换时选中该供应商首个模型，空供应商返回无效状态，搜索按标签和 ID 匹配。
- [ ] 运行目标测试确认新增断言失败。
- [ ] 在 `src/components/shared/ProviderModelPicker.tsx` 实现受控组件；在 `skillLabRuntime.ts` 实现可测试的 `selectProviderDefaultModel`、`filterProviderModels`。
- [ ] 在 `ChatInput.tsx` 复用共享过滤函数，保持现有弹层视觉和交互不变。
- [ ] 运行 `pnpm test -- src/__tests__/skillLabRuntime.test.ts src/__tests__/chatModelSelection.test.ts`。

### Task 3: Skill 实验室双配置区和命令快照

- [ ] 更新前端测试，覆盖两个阶段命令参数构造不包含草稿元数据字段，并验证偏好切换不依赖 skillId。
- [ ] 运行测试确认失败。
- [ ] 修改 `SkillLabPanel.tsx`：从 store 获取 providers、当前供应商/模型和 `buildThreadChatProviderOverride`；初始化/恢复两阶段偏好；渲染两个 `ProviderModelPicker` 与知识库开关；生成和迭代调用分别传 `runtimeConfig`；对应运行期间锁定控件。
- [ ] 增加中英文 i18n 文案。
- [ ] 运行前端目标测试与 `pnpm build`。

### Task 4: 后端临时配置覆盖

- [ ] 在 `skill_lab.rs` 测试模块写失败测试：运行配置覆盖模型和知识库但不修改原配置；Serde 参数兼容 camelCase；运行配置不会出现在 SkillLabMeta JSON。
- [ ] 运行 `cargo test --manifest-path src-tauri/Cargo.toml skill_lab_runtime_config --lib`，确认失败。
- [ ] 将 `standalone.rs` 的 `ThreadChatProviderOverride`、端点类型和 `apply_thread_chat_overrides` 调整为 `pub(crate)`；在 `skill_lab.rs` 定义 `SkillLabRuntimeConfig` 和 `apply_skill_lab_runtime_config`，复用该函数。
- [ ] 扩展 `SkillLabGenerateFromGoalParams` 和新的 `SkillLabRunTestParams`；生成与迭代均使用配置克隆，不写回全局配置。
- [ ] 运行目标 Rust 测试。

### Task 5: 生成链路知识库参与

- [ ] 写失败测试验证生成提示词在知识库开启时包含“按目标判断是否写入 smartbrain_search 规范”，关闭时不包含该要求。
- [ ] 运行目标测试确认失败。
- [ ] 抽取 `build_generation_prompt` 并根据开关注入要求；使用覆盖后的 ConfigToml 创建线程和运行 Agent，让现有请求级预召回与工具开关生效。
- [ ] 运行目标测试和现有 skill_lab Rust 测试。

### Task 6: 迭代链路知识召回与三阶段注入

- [ ] 写失败测试验证知识上下文包装为“不可信参考资料”，并分别进入测试、评估、改写消息；无上下文时保持原提示结构。
- [ ] 运行目标测试确认失败。
- [ ] 实现基于 `goal + test_prompt + skill 摘要` 的 BM25 请求级召回；索引不存在、无命中或检索错误时返回空上下文并继续。
- [ ] 抽取测试、评估、改写消息构造函数并注入同一轮召回上下文；进度事件只报告命中数量，不输出正文或密钥。
- [ ] 运行 Rust 目标测试及 `cargo test --manifest-path src-tauri/Cargo.toml commands::skill_lab::tests --lib`。

### Task 7: 回归、审查与文档

- [ ] 更新 `docs/skill-lab-autogen-regression.md`，增加双阶段模型选择、知识库开关、切换草稿不改变偏好、部署产物无配置和运行中快照稳定性检查。
- [ ] 运行 `pnpm test -- src/__tests__/skillLabRuntime.test.ts src/__tests__/chatModelSelection.test.ts`。
- [ ] 运行 `pnpm build`。
- [ ] 运行 `cargo test --manifest-path src-tauri/Cargo.toml commands::skill_lab::tests --lib`。
- [ ] 运行 `cargo check --manifest-path src-tauri/Cargo.toml --lib`。
- [ ] 运行 `git diff --check` 和代码审查，修复发现的问题后重跑受影响验证。
