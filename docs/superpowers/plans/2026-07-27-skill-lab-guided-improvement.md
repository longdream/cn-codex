# Skill 实验室引导式一键改进实施计划

> 实施时按 `test-driven-development` 执行：每项行为先写失败测试，再写最小实现，最后回归验证。

**目标：** 为评估未通过且有最近评估意见的 Skill 增加“需要改进”标识、可编辑改进要求和“一键改进”入口，并复用现有测试—评估—改写闭环。

**架构：** 后端列表返回计算字段 `needsImprovement`；前端纯函数解析最近评估并生成改进要求；运行命令新增请求级 `improvementRequest`，同一固定目标注入测试、评估、改写三阶段，但不写入元数据、Skill 文件或部署产物。

## Task 1：前端改进内容纯函数

**文件：**
- Create: `src/utils/skillLabImprovement.ts`
- Create: `src/__tests__/skillLabImprovement.test.ts`

1. 先写测试，覆盖 camelCase/snake_case JSON、Markdown JSON 代码块、自然语言回退、空输入，以及仅 `failed + 非空评估` 判定为需要改进。
2. 运行单测并确认因模块缺失而失败。
3. 实现 `formatSkillLabImprovementRequest` 与 `needsSkillLabImprovement`。
4. 重跑测试至通过。

## Task 2：后端列表字段与请求规范化

**文件：**
- Modify: `src-tauri/src/commands/skill_lab.rs`

1. 先添加 Rust 单元测试：列表判定、空白请求归一化、字符数上限。
2. 为 `SkillLabSummary` 增加 `needs_improvement` 计算字段。
3. 为 `SkillLabRunTestParams` 增加可选 `improvement_request`。
4. 实现 trim、空白转 `None`、按字符截断的规范化函数。
5. 使用 `cargo check --tests` 验证测试代码和实现可编译。

## Task 3：固定改进目标注入三阶段

**文件：**
- Modify: `src-tauri/src/commands/skill_lab.rs`

1. 先写 Rust 单元测试，证明同一要求进入测试、评估、改写消息，且 `None` 保持原消息结构。
2. 抽取消息构造函数，使用明确的“本次固定改进目标”边界。
3. 在测试阶段要求模型按目标执行，在评估阶段要求检查目标是否满足，在改写阶段要求优先修复目标且保留有效能力。
4. 保持现有关键问题澄清、知识库上下文、资源池运行快照和普通运行行为不变。

## Task 4：列表与详情交互

**文件：**
- Modify: `src/components/settings/SkillLabPanel.tsx`
- Modify: `src/i18n/zh-CN/common.json`
- Modify: `src/i18n/en-US/common.json`

1. 扩展前端摘要类型并在列表显示“需要改进”标识。
2. 详情加载后，仅对符合条件的 Skill 根据最近评估初始化可编辑改进要求；点击列表不自动调用模型。
3. 将现有运行函数重构为接收可选改进要求：普通“运行测试”传 `null`，“一键改进”传 trim 后文本。
4. 增加改进面板、textarea、说明和禁用态；复用现有保存、运行快照、进度、完成刷新逻辑。
5. 运行结束仍失败时按最新评估刷新改进文本；通过后隐藏改进面板和列表标识。

## Task 5：回归文档与验证

**文件：**
- Modify: `docs/skill-lab-autogen-regression.md`

1. 补充列表标识、编辑确认、一键改进、普通运行兼容和不持久化要求。
2. 运行：
   - `pnpm test -- src/__tests__/skillLabImprovement.test.ts src/__tests__/skillLabRuntime.test.ts src/__tests__/chatModelSelection.test.ts`
   - `pnpm build`
   - `cargo check --manifest-path src-tauri/Cargo.toml --tests`
   - `cargo check --manifest-path src-tauri/Cargo.toml --lib`
   - `cargo test --manifest-path src-tauri/Cargo.toml commands::skill_lab::tests --lib`
   - `rustfmt --edition 2024 --check src-tauri/src/commands/skill_lab.rs`
   - `git diff --check`
3. 执行代码审查，修复实质问题后再提交。

## 验收条件

- 仅评估失败且有最近评估的 Skill 显示“需要改进”。
- 打开详情不调用模型，改进要求可编辑。
- “一键改进”将固定要求注入完整自动迭代闭环。
- 普通“运行测试”行为不变。
- 改进要求不进入元数据、Skill 文件或部署产物。
- 前端测试、构建、Rust 编译检查和格式检查通过；如 Windows Rust 测试二进制仍出现入口点环境错误，保留编译证据并如实记录。
