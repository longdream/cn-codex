# CN-Codex 对话机制审查与改进方案

日期：2026-07-18

## 1. 审查范围与基线

本次审查覆盖以下链路：

- 模型供应商协议选择与能力匹配
- Chat Completions / Responses / DSML 流式解析
- assistant 文本、reasoning、tool call 的事件分流
- agent 与 subagent 的工具循环、取消、重试和终止
- tool call / tool result 历史完整性
- 上下文裁剪、压缩与恢复
- Tauri 事件、JSONL 持久化与前端流式状态一致性

对照基线：

- OpenAI Codex：`origin/main@56395bddaf26eb2829387ca6a417bf9128e5b239`
- Grok Build：`main@98c3b2438aa922fbbe6178a5c0a4c48f85edc8ce`
- CN-Codex：当前工作区，包含本轮开始前的未提交功能改动

`D:\rustwork\codex` 已执行 `git fetch origin --prune`。其本地 `main` 有 2 个自有提交并落后 `origin/main` 1156 个提交，因此审查直接读取最新 `origin/main`，没有强制改写本地分支。

## 2. 结论摘要

本次故障不是前端工具卡渲染问题，而是供应商协议能力不匹配：`grok-4.5` 经当前网关使用 `/chat/completions` 时忽略外部 function tools，把伪工具协议写入 assistant `content`；同一模型改用 `/responses` 后能正确返回结构化 `function_call`。

CN-Codex 原链路还有三个放大器：

1. 文本伪工具协议会直接流向 UI，没有失配检测。
2. SSE 收到 `response.completed` 后仍等待 TCP 连接关闭。
3. 单轮模型迭代、单响应工具数量、工具索引和流字节数缺少统一硬上限。

因此界面出现了 `shell2 shell3 ... shell225`，但会话 JSONL 中没有真实 tool call。它们是模型输出的协议文本，不是 225 次已执行命令。

## 3. 与 Codex / Grok Build 的关键差异

| 领域 | 最新 Codex | Grok Build | CN-Codex 审查结果 |
| --- | --- | --- | --- |
| Responses 完成语义 | 以 `response.output_item.done` 作为完整 item，以 `response.completed` 结束流 | sampler 产生明确终态 | 原先依赖连接关闭；本轮已修 |
| 失败事件 | 解析 `response.failed`、`response.incomplete` 并分类 | sampler error 进入明确 TurnOutcome | 原先静默忽略；本轮已修 |
| 工具历史完整性 | 补齐缺失 output，删除 orphan output，使用稳定合成 ID | 压缩和恢复不拆散 tool call/result | 原先只删除 orphan，不补 dangling call；本轮已修 |
| 循环保护 | 依赖终态、取消 token、严格流错误 | `max_turns`、doom-loop 检测、stall fingerprint | 主 agent 原先无硬上限；本轮已修 64 次上限 |
| 流资源上限 | SSE idle timeout，终态缺失即报错 | 流式捕获 8 MB 上限 | 原先主要依赖 HTTP 600 秒 read timeout；本轮已加 8 MB 上限 |
| Chat 非标准兼容 | 结构化协议优先 | 由 sampler 统一规范化 | 完整 `message` 曾被当增量拼接；本轮已修 |
| 取消 | cancellation token 可中断等待 | 任务取消有独立 TurnOutcome | 仍主要在收到 chunk 后轮询，需继续改进 |
| 上下文 | 增量历史、稳定 prompt cache、严格 call/output 规范化 | 两阶段压缩并吸附 tool 边界 | system 重排和多处临时注入仍可能造成缓存抖动 |

## 4. 风险清单

### Skill、MCP 与知识库加载审查

- Skill：当前采用“元数据小列表 + `tool_search` 发现 + `read_file` 按需读取”的分层加载，插件禁用状态和 manifest 路径逃逸校验已生效；不会默认把全部 `SKILL.md` 注入上下文。
- MCP：当前采用“目录发现 + 线程级激活 + 下一次模型请求挂载 schema”的机制，直接 MCP/Playwright schema 不默认暴露；stdio/HTTP 请求有 20 秒超时，配置变化会清理旧 session。
- 知识库：`smartbrain_search` 仅在 SmartBrain 启用时暴露，SQL 等非核心工具按需发现。原先自动召回会把结果写入持久线程历史，已改为本轮请求级、不可信资料上下文，避免重复累积和指令重放。
- `smartbrain.knowledge_enabled` 现在作为知识功能的统一闸门；关闭后自动召回、知识搜索和知识库 SQL 都不会继续运行，但 SmartBrain 的经验提取等其他能力不受影响。
- 知识库索引返回的文件路径现在必须 canonicalize 后仍位于 `codey/memories` 内，防止被篡改索引引导读取目录外文件。
- MCP 触发策略已优化：普通 turn 只构建核心工具与配置级离线索引，不启动、不连接也不 `tools/list` MCP Server。仅当模型通过 `tool_search` 命中某个 MCP Server 后，才挂载通用 MCP 工具；随后由 `mcp_list_tools(server)` 或 `mcp_call_tool` 触发对应 server 的实际连接。这样闲聊不显示 MCP loading，显式 MCP 调用仍保留 20 秒执行超时。

### P0：已直接修复

#### P0-1 供应商协议失配导致伪工具调用泄漏

- 触发：模型不支持 Chat Completions 外部工具，却返回 recipient/tool 文本协议。
- 影响：工具不执行、UI 被大量伪工具标签污染、意图重试继续消耗 token。
- 修复：检测 recipient 标记和连续编号工具标签，立即返回可操作的协议错误；故障便携版改为 `wire_api = "responses"`。

#### P0-2 SSE 终态与连接生命周期耦合

- 触发：网关发送 `response.completed` 后保持 HTTP 连接。
- 影响：任务看似卡死，直到 read timeout 或用户中断。
- 修复：主 agent 和 subagent 收到 `Done` 或 `[DONE]` 后主动结束读取。

#### P0-3 工具索引、数量和流大小无界

- 触发：恶意或异常模型返回超大 `output_index`、大量 DSML 调用或无限流文本。
- 影响：向量扩容导致内存耗尽，或批量执行非预期工具。
- 修复：单响应最多 64 个工具调用；索引必须小于 64；单响应最多读取 8 MB；主 agent 单轮最多 64 次模型迭代。

#### P0-4 崩溃后遗留 dangling tool call

- 触发：assistant tool call 已写入 JSONL，但进程在 tool result 写入前退出。
- 影响：下一轮 Chat/Responses 请求被供应商拒绝，线程永久无法继续。
- 修复：构建模型历史时删除 orphan/重复 result，并为缺失 result 注入稳定的 `aborted` tool 消息。

#### P0-5 Responses 失败事件被静默丢弃

- 触发：上游发出 `response.failed` 或 `response.incomplete`。
- 影响：错误被误报为“空响应”，无法正确重试和诊断。
- 修复：转换为内部 `StreamEvent::Error`，主 agent/subagent 立即进入现有错误与重试链路。

#### P0-6 Chat 完整 message 被当作 delta

- 触发：非标准网关在每个流帧发送累计 `message`。
- 影响：文本和 function arguments 重复拼接，JSON 参数损坏。
- 修复：只有终止帧才消费完整 `message`；标准 `delta` 仍实时消费。

#### P0-7 未执行工具却强制生成工具总结

- 触发：模型只输出行动意图且未产生 tool call。
- 影响：系统注入错误的 “All tool executions have completed”，进一步误导模型。
- 修复：只有本轮确实执行过工具时才进入工具总结补偿路径。

#### P0-8 DeepSeek 空 `finish_reason` 被误判为终态

- 触发：DeepSeek V4 Flash 在生成中的 SSE 分片返回 `finish_reason: ""`，正文位于后续分片，并通过 `reasoning_content` 独立输出推理。
- 影响：新终态逻辑在首个推理分片后提前结束，产生 `1 calls / 0 tokens`、无 Assistant 正文的伪成功 turn。
- 修复：空白 `finish_reason` 不再产生 Done；原生 reasoning 使用独立事件；Chat 继续读取 usage-only 分片直到 `[DONE]`；真正无正文且无工具的响应统一进入 Failed；公网 Provider 恢复系统代理，私网/本地地址才直连。

### P1：下一阶段必须完成

#### P1-1 可中断 HTTP 与 SSE idle watchdog（已完成）

当前 cancel flag 主要在请求返回或收到下一个 chunk 后检查。若连接建立或流读取长期静默，停止操作不能立即生效。

改进：

- 用 cancellation token 或 `tokio::select!` 同时等待网络、取消与 idle timeout。
- 连接阶段和每次 SSE poll 分别设置超时。
- 取消必须产生唯一、稳定的 turn terminal event。

验收：模拟永不返回 header 和永不返回下一 chunk 的服务，点击停止后 500 ms 内结束任务。

本轮实现：

- 新增共享请求控制器，以 `tokio::select!` 同时等待网络结果、取消信号和超时。
- 主 Agent 与 Subagent 的响应头、错误体、非流式响应体和 SSE poll 全部接入取消控制。
- 响应头等待上限为 60 秒；SSE/响应体 idle timeout 为 300 秒，与最新 Codex 和 Grok Build 默认口径一致。
- 取消标志每 25ms 检测一次；重试退避同样可取消，不再出现停止后继续等待 30 秒退避的问题。
- Subagent 新增明确的 `Cancelled` 完成分支，流中取消不再误判为 `Completed`。
- idle timeout 为非重试错误，避免对已有部分输出的请求进行盲目重放。

#### P1-2 非流式 Responses JSON 解析（已完成）

主 agent 和 subagent 在 `Content-Type: application/json` 时仍主要按 Chat `choices[].message` 解析。部分兼容网关会忽略 `stream=true` 并返回完整 Responses 对象。

改进：按 `wire_api` 分派非流式解析器，统一解析 `output[]`、usage、function/custom/tool-search call 和 incomplete/error。

验收：同一 Responses fixture 分别以 SSE 和完整 JSON 返回，最终内部消息完全一致。

本轮实现：

- `ProviderAdapter` 增加完整 JSON 解析入口，Responses adapter 解析 `output_text` 和 `output[]`。
- 支持 `function_call`、`custom_tool_call`、`tool_search_call` 及 `call_id` / arguments 正规化。
- 支持 `input_tokens`、`output_tokens`、缓存 token、reasoning token 和 total usage。
- `failed`、`incomplete`、`cancelled` 状态转为明确错误，不再按 Chat `choices[].message` 静默得到空响应。
- 主 Agent 与 Subagent 共用 adapter 结果转换，保持文本、工具调用和 usage 语义一致。

工具状态可见性修复：

- `tool-calls-end` 收敛工具卡后同步刷新 `streamingLabel`，旧的“正在读取文件”不会覆盖 3/3 完成状态。
- `turn-completed` 仍负责最终清空 streaming 状态；工具组完成但模型仍在生成时显示“正在处理请求...”。

#### P1-3 供应商能力探测（已完成）

当前“测试连接”主要验证模型列表，不能证明该模型/端点支持外部工具。

改进：增加无副作用 capability probe，至少验证：

- structured function tool
- streaming terminal event
- reasoning channel
- usage
- parallel tool calls

探测结果应缓存到 provider+model 指纹，并在设置页给出推荐 `wire_api`，不应静默自动改协议。

本轮实现：

- 新增 `probe_model_capabilities` Tauri 命令，使用专用虚拟函数名进行探测，不执行真实工具。
- Chat / Responses 分别发送结构化工具、流式终止和 reasoning 请求；探测 usage 与工具调用结果。
- 结果按 `provider + base_url + model + wire_api` 指纹缓存，显式点击设置页按钮时强制刷新。
- `ProviderModel.capabilities` 持久化探测结果及时间戳，模型行显示 tools、stream、reasoning、usage、parallel 状态。
- 探测会在当前 Chat 工具能力不足时给出 `responses` 建议，但不自动修改 `wire_api`；尚未探测或无法判定的能力显示为 `?`。

#### P1-4 统一 turn 状态机与事件日志（进行中）

当前线程 JSONL、Tauri 增量事件、前端 streaming runtime 分别维护状态，结束顺序依赖多个事件处理器。

改进：定义单一 turn 状态机：`Created -> Sampling -> ToolRunning -> Sampling -> Completed/Failed/Cancelled`。所有 UI 事件由持久化事件投影生成，并携带单调序号以去重。

本轮实现第一阶段：

- Agent 广播事件统一携带全局单调 `eventSeq` 和可追踪 `eventId`。
- 前端按线程记录最后序号，重复或倒序事件直接丢弃，兼容没有序号的旧事件。
- 关键 turn 投影明确记录 `Sampling`、`ToolRunning`、`Completed` 阶段，终态事件不会重复收敛。
- 后续仍需将阶段持久化到线程事件日志，并补充 Failed/Cancelled 的显式终态事件。

本轮继续实现：

- Skill catalog、MCP catalog、`tool_search` 激活统一发出 `turn-loading`，包含 kind、phase、status、数量、激活工具和耗时。
- Agent 取消时发出 `turn-cancelled`；命令层异常时关闭未完成 turn 并发出 `turn-failed`。
- 前端监听两类终态，收敛流式文本、运行中工具和 live usage，避免异常后界面继续显示读取中。
- 取消发生在 turn 建立前也会补发 `turn-cancelled`，覆盖 pre-turn compaction 等早退路径。

#### P1-5 请求重试幂等性

为每次 sampling request 增加稳定 request/attempt ID。只有“尚未观察到任何输出 item”时允许透明重试；已有 tool call 或文本后只能恢复/终止，不能盲目重放。

#### P1-6 长任务上下文与压缩稳定性

当前上下文分为三层：

- 持久化历史：线程消息持续写入 `codey/sessions/{thread_id}.jsonl`，普通模型请求不会删除原始消息。
- 模型请求历史：规范化 call/result 配对后，最近 100 条工具结果保持完整；读取、搜索、失败等高价值结果扩展到 150 条，超过窗口后才生成带关键行、头尾内容的限长摘要。
- checkpoint compaction：达到显式阈值，或默认达到模型上下文窗口 80% 时触发；覆盖 pre-turn、mid-turn、goal continuation 和手动 `/compact`。80% 为单次大输出和下一轮请求预留安全余量。

长任务保护：

- 压缩请求包含普通对话、工具请求、工具结果、路径和错误，不再跳过 `tool` 消息。
- 单条工具结果与参数在压缩输入中限长，压缩输入设置软上限，防止大量命令输出反向撑爆摘要请求。
- 压缩完成后的新历史继续保留最近 100 条结构化命令上下文（名称、参数和结果），而不是只留下单段摘要。
- 单个 turn 的模型迭代保护上限为 128，避免单命令迭代超过 64 次时被过早终止；重复循环仍会被上限截断。
- 空摘要视为压缩失败，禁止覆盖原历史。
- 重写线程 JSONL 前复制到 `codey/sessions/archive/`；备份失败则取消压缩。
- mid-turn 压缩失败时保留原历史和 token 计数，不再错误重置为 0；同一 turn 不循环重试失败的压缩请求。

### P2：质量与性能优化

- 将 `agent.rs` 的协议、历史规范化、turn 状态机拆成独立模块，降低中央文件修改冲突。
- 对只读、无依赖工具提供受控并发；写文件、shell 和审批工具默认串行。
- 压缩切点必须吸附到完整 assistant-tool/result 组，不能只按消息数裁剪。
- system/runtime 上下文使用稳定分层结构，减少每轮重排导致的 prompt cache miss。
- conversation logger 记录 provider、wire API、终态、attempt ID、首字节时间、最后事件类型和截断原因；禁止记录凭据。
- 前端按 call ID 和 turn sequence 幂等合并，不依赖事件到达顺序。

## 5. 本轮代码变更

主要修改文件：

- `src-tauri/src/adapter/types.rs`
- `src-tauri/src/adapter/chat_completions.rs`
- `src-tauri/src/adapter/responses.rs`
- `src-tauri/src/adapter/mod.rs`
- `src-tauri/src/agent.rs`
- `src-tauri/src/subagent_engine.rs`
- `src-tauri/src/standalone.rs`
- `src-tauri/src/request_control.rs`
- `src-tauri/src/tool_executor.rs`
- `src-tauri/src/tool_executor/patch_support.rs`
- `src/components/chat/ChatInput.tsx`
- `src/components/chat/PlanExecutionProgress.tsx`
- `src/utils/planExecutionProgress.ts`

执行可见性补强：

- 从最近一轮用户消息中解析最新 `update_plan`，统一呈现已完成、进行中和待处理步骤。
- `apply_patch` 成功写盘后携带每个文件的 `additions` / `deletions`，运行中实时累计文件数和增删行。
- 任务完成后优先使用 `changedFileSnapshots` 计算最终差异，避免同一文件多次修改造成重复统计。
- 输入框上方增加紧凑进度入口，展开后显示完整计划；支持点击外部与 `Escape` 关闭，并适配窄屏换行。

便携版运行配置：

- `D:\rustwork\CN-Codex-portable-x64-142.0.3595.80\codey\config.toml`

## 6. 验证策略

本轮已执行：

- `cargo check --lib`
- `cargo fmt --check`
- `cargo test --lib --no-run`
- `pnpm test`（24 个测试文件、161 项测试）
- `pnpm build`
- Chat 累计 message、Responses done/failed/incomplete、协议泄漏、dangling result 的针对性测试编译
- 永不完成请求的快速取消、SSE idle timeout 和可取消重试退避测试
- 非流式 Responses 文本、function/custom/tool-search、usage 和 incomplete fixture 测试
- 计划解析、补丁统计、完成快照覆盖和用户轮次隔离测试
- 1280px 桌面与 360px 窄屏视觉检查；弹层无溢出，页面无横向滚动

仍需在动态库环境修复后执行实际测试二进制。当前机器启动 Rust 测试程序时存在既有 `STATUS_ENTRYPOINT_NOT_FOUND`，不属于本轮断言失败。

发布前还应执行端到端场景：

1. Chat 原生 tool_calls 正常往返。
2. Responses function/custom/tool-search 正常往返。
3. `response.completed` 后连接不关闭，任务仍立即结束。
4. `response.failed`、`response.incomplete` 显示准确错误。
5. 进程在 tool call 与 result 之间退出，恢复线程后可继续。
6. 超大 output index、65 个工具调用和超过 8 MB 的流均被拒绝，且没有工具执行。
7. 错误 wire API 返回文本伪协议时，在少量标签内中止并提示切换 Responses。
8. 服务端永不返回响应头或下一 SSE chunk 时，停止操作在 500ms 内完成。

## 7. 建议实施顺序

1. 合入本轮 P0 与 P1-1 修复，并完成真实运行时回归。
2. 增加 P1-3 capability probe，阻止同类配置错误再次进入生产。
3. 为 P1-5 增加稳定 request/attempt ID，收紧透明重试条件。
4. 分阶段推进 turn 状态机和事件日志统一，最后再做工具并发与上下文缓存优化。
