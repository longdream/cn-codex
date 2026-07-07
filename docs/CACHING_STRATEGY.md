# CN-Codex 缓存优化指南：如何实现高命中率

> 目标：弄清楚「不同供应商缓存策略到底差在哪」，并基于 **CN-Codex 当前代码结构**判断「具体怎么做命中率最高」。
>
> 本文档的供应商参数均来自**官方文档**（OpenAI / Anthropic / DeepSeek / Google Gemini，核对日期 2026-07），并与 `src-tauri` 实际代码做了对比，标出**现状**与**真实缺口**。
>
> 适用代码版本：`src-tauri` 当前实现。

---

## 1. 先说结论（太长不看版）

1. **CN-Codex 目前只「统计」缓存命中数，从不主动设置缓存断点。** 4 个 adapter（`chat` / `anthropic` / `responses` / `gemini`）的 `build_body` 都没有写 `cache_control` 之类的标记。
2. 供应商分两类：
   - **自动前缀缓存（无需标记，前缀一致即命中）**：`chat`（OpenAI 兼容 / DeepSeek / 中转站）、`responses`（OpenAI Responses）、`gemini`（Gemini 2.5+ 隐式缓存）。
   - **需主动打断点**：`anthropic`（Claude）。Claude 有两种模式，但**不打任何 `cache_control` 就完全不缓存**——CN-Codex 当前对 Claude 付全价。
3. **高命中率的唯一核心原则**：让 prompt 前缀（system 提示词 + tools + 历史开头）在多次请求间**逐字节一致**。
4. **对比结构后发现的 3 个真实代码缺口**（详见第 7 节）：
   - ⚠️ **DeepSeek 命中数读不到**：DeepSeek 返回 `prompt_cache_hit_tokens`（usage 顶层），而 CN-Codex 的 `chat` adapter 读的是 `prompt_tokens_details.cached_tokens`（OpenAI 格式）→ 用 DeepSeek 时前端 ⚡ 徽章永远显示命中 0。
   - ⚠️ **Gemini 命中数读不到**：Gemini 返回 `usageMetadata`，CN-Codex 的 `google` adapter 完全没解析 → 显示 0。
   - ⚠️ **Claude 完全没缓存**：`anthropic` adapter 没写 `cache_control`，系统提示词这一最大头每次都重新计费。

---

## 2. CN-Codex 当前的缓存现状（实测）

代码事实（来自 `src-tauri`）：

- `adapter/types.rs` 的 `UsageInfo` 已含 `cached_tokens / cache_creation_tokens / reasoning_tokens`，能从响应读出部分字段——**只读取与展示，不主动请求缓存**。
- 四个 adapter 的 `build_body(...)` 都只拼 `model / messages / tools / max_tokens / stream`，**无 `cache_control` / `cacheControl`**。
- 供应商选择靠配置 `provider.wire_api`（默认 `"chat"`），映射到 `adapter::get_adapter(&wire_api)`。合法值：
  - `"chat"` → `ChatCompletionsAdapter`（OpenAI 兼容：DeepSeek、国产模型、中转站）
  - `"anthropic"` → `AnthropicAdapter`（Claude 原生 Messages API）
  - `"responses"` → `ResponsesAdapter`（OpenAI Responses API）
  - `"gemini"` → `GoogleAdapter`（Gemini 原生 REST；多数用户经 Google 的 OpenAI 兼容端点，实际走 `chat`）

结论：**能跑出命中数字的，只有 `chat`（OpenAI 格式供应商）与 `responses`**；且前提是前缀稳定。DeepSeek 经 `chat` 走时因字段不匹配读不到（见第 7.1 节），Gemini 原生端点读不到，Claude 则完全没启用。

---

## 3. 四种供应商缓存机制对比（官方文档核对）

| 维度 | `chat`（OpenAI 兼容） | `anthropic`（Claude） | `responses`（OpenAI Responses） | `gemini`（Gemini 原生） |
|---|---|---|---|---|
| 触发方式 | 自动（前缀一致即命中） | **需 `cache_control` 断点**（顶层自动模式或块级显式） | 自动（前缀一致即命中） | 自动（Gemini 2.5+ 隐式缓存默认开） |
| 是否需改代码 | 否（稳前缀即可） | **需加 `cache_control`** | 否 | 否 |
| 最小可缓存 token | **1024**（统一阈值） | **按模型**：Opus 4.8/Sonnet 4.5/Opus 4.1 = 1024；Opus 4.7/Mythos Preview = 2048；Opus 4.6/4.5/Haiku 4.5 = 4096；Fable 5/Mythos 5 = 512 | 1024 | **2048**（2.5 Flash/Pro）/ **4096**（3.5 Flash、3.1 Pro） |
| TTL | 内存 5–10 分钟（高峰最长 1h）；部分新模型仅 24h 扩展保留 | 标准 5 分钟（命中免费刷新）；扩展 1 小时（2× 价） | 同 OpenAI | 由系统自动处理（隐式，无用户可控 TTL） |
| 写入价 | **免费**（无写入费） | 5m 写入 = 1.25×；1h 写入 = 2× | 免费 | 隐式无额外写入费 |
| 读取价 | 约 **0.1×**（最高省 90% 输入） | **0.1×** | 0.1× | 大幅折扣（自动传递） |
| 返回字段 | `usage.prompt_tokens_details.cached_tokens` | `cache_read_input_tokens` / `cache_creation_input_tokens` | `usage.input_tokens_details.cached_tokens` | `usageMetadata`（如 `cachedContentTokenCount`） |
| CN-Codex 读取支持 | ✅ 已读（OpenAI 格式） | ✅ 已读 | ✅ 已读 | ❌ 未解析（置 0） |
| 备注 | DeepSeek 走此格式但**字段不同**（见 7.1） | 最多 4 个断点；层级 `tools→system→messages` | 同 chat | 仅 Interactions API 支持隐式；原生 `cachedContent` 需另版文档 |

> 关键差异（来自官方文档）：
> - **OpenAI / DeepSeek / Gemini 是「免改代码、前缀不变就自动省」**；其中 OpenAI 写入免费、读取 0.1×。
> - **Claude 是「不打断点就不缓存」**，且写入还要 1.25×~2× 价、读取 0.1×——所以 Claude 既需要代码改动，又要权衡写入成本（频繁 5 分钟内不重用的话，写缓存反而更贵）。

---

## 4. 高命中率的核心原理：稳定前缀

CN-Codex 每次请求结构（以 `chat` 为例）：

```
[system 提示词]   ← 最大、最该被缓存的一段
[tools 工具定义]  ← 第二该被缓存的一段
[历史对话...]     ← 每轮都在变，必须放在最后
[当前用户输入]
```

缓存按**从开头算起的连续前缀**匹配：前缀不变 → 整段命中（0.1× 价）；历史放在前缀之后，其变化**不会**让前面缓存失效（正是想要的）。一旦**前缀本身**变了（哪怕一字符），整段失效、重新计全价并写新缓存。

### 4.1 会破坏前缀稳定性的「坑」（按 CN-Codex 代码核对）

`build_system_prompt()`（`agent.rs`）会把以下内容拼进 system 前缀：`effective_cwd`、OS/ARCH、`user-rules.md`、`render_available_skills_prompt()`、`render_plugin_apps_prompt()`、web 工具说明等——**同一会话内基本稳定，利于缓存**。但要警惕：

- ❌ 在 system 前缀塞**时间戳 / 当前日期 / 随机 session id / 递增计数器** → 前缀逐字节不同，缓存全废。
- ❌ 每次请求**随机重排 tools** 或变动 tool 列表 → tools 紧跟 system，属前缀一部分。
- ❌ 中途**改 `codey/user-rules.md`** 或启停插件 → system 文本变。
- ❌ **compaction（上下文压缩）** 会重写历史并把 `last_single_prompt_tokens` 回推 → context 变了，命中率下降（见第 6.3 节）。
- ❌ **Claude 的缓存层级顺序为 `tools → system → messages`**：改了 tools 会让其下 system、messages 全部失效。因此 tools 定义必须固定。

### 4.2 该做的

- ✅ 不变的大段内容（角色设定、工具规范、长期指令）固定放在 system 开头。
- ✅ tools 定义**固定顺序、固定 schema**，不每轮重排/增删。
- ✅ 同会话内不改 `user-rules.md`、不动插件开关。
- ✅ 把「会变的信息」（当前时间、最近结果）放**历史对话 / 当前用户输入**，而非 system 前缀。

---

## 5. 分供应商实操建议

### 5.1 `chat`（OpenAI 兼容 / DeepSeek / 中转站）
- **无需改代码**即可自动前缀缓存（≥1024 token）。
- 只需保证 system + tools 前缀稳定（第 4 节）。
- 注意：连续对话间隔别太长，超出 TTL（5–10 分钟）缓存过期，下次需重新写入（写入那次计 `cache_creation` 全价，见 ⚡ 徽章「缓存写入」）。
- ⚠️ **DeepSeek 例外**：它虽走 `chat` 格式，但返回字段是 `prompt_cache_hit_tokens`（见 7.1），当前代码读不到。
- 验证：对话几轮后看 ⚡ 徽章「缓存命中」是否 > 0（DeepSeek 用户会看到 0，属 bug 非未命中）。

### 5.2 `anthropic`（Claude）—— 必须加 `cache_control`（当前缺口，最大优化点）
Claude **不打断点就不缓存**。官方提供两种模式：
- **顶层自动模式**：请求顶层放单个 `{"type":"ephemeral"}`，系统自动把它应用到「最后一个可缓存块」。对 CN-Codex 而言最后一个块是每轮变化的当前输入 → **缓存块几乎每次都变，等于没缓存**。所以此模式无效。
- **块级显式断点（推荐）**：在 system 块（最大头）末尾显式打 `cache_control`，才能稳定缓存系统提示词。

具体建议（CN-Codex 语境）：
- 在 `AnthropicAdapter::build_body` 中，把 `body["system"]` 从纯字符串改为带 `cache_control` 的结构，并在 tools 数组末项加断点。
- 断点前的块需 ≥ 对应模型的**最小 token**（Opus 4.8/Sonnet 4.5 = 1024；若用 Opus 4.6 则需 4096）。CN-Codex 的系统提示词通常远超此值。
- 最多 4 个断点；层级 `tools→system→messages`，改上层会使下层全失效——**所以 tools 必须固定**。
- TTL：默认 5 分钟（命中免费刷新）；若对话间隔常 >5 分钟，可用 `{"type":"ephemeral","ttl":"1h"}`（2× 写入价）权衡。
- 成本核算：写入 1.25×（5m）/ 2×（1h），读取 0.1×。**若同一缓存 5 分钟内被复用 ≥2 次才划算**；否则不如不缓存。CN-Codex 多轮对话通常满足。

```rust
// 示意：anthropic build_body 中给 system 打断点
body["system"] = serde_json::json!([
    { "type": "text", "text": system_text, "cache_control": { "type": "ephemeral" } }
]);
```
> 读取侧已就绪：`anthropic.rs` 的 `message_start` 已读 `cache_read_input_tokens` / `cache_creation_input_tokens`，前端 ⚡ 徽章可直接展示，只需补齐「主动打断点」。

### 5.3 `responses`（OpenAI Responses）
- 与 `chat` 同理，前缀一致即自动命中，无需改代码。
- 区别：Requests API 把 system/历史建模为 `input` 里的 item，前缀稳定性原则一致——不变内容放 input 最前。
- `ResponsesAdapter::build_body` 已能从 `input_tokens_details.cached_tokens` 读命中数，无需额外改动。

### 5.4 `gemini`（Gemini 原生）
- Gemini 2.5+ 隐式缓存默认开，无需配置；前缀一致 + 达到最小 token（2048/4096）即自动命中。
- 高阶玩法：`cachedContent` 显式上传长期不变的大段上下文（适合超大 system 提示词），但 Interactions API 当前只支持隐式。
- ⚠️ **CN-Codex 当前 `google` adapter 没解析 Gemini 的 `usageMetadata`** → 前端看不到命中数字（实际缓存仍生效）。需在 `google.rs` 补解析（见 7.2）。
- 多数用户经 Google 的 OpenAI 兼容端点（走 `chat` adapter），直接享受 5.1 的自动缓存即可。

---

## 6. CN-Codex 代码层面的建议改动清单（按投入产出比）

### 6.1 【高收益·必做】给 `anthropic` adapter 加 `cache_control` 断点
- 文件：`src-tauri/src/adapter/anthropic.rs` 的 `build_body`。
- 动作：system 文本与 tools 末尾打 `cache_control: { "type": "ephemeral" }`（必要时 `ttl:"1h"`）。
- 收益：Claude 用户从此享受 0.1× 输入价（读取侧已就绪）。

### 6.2 【高收益·必做】修复 DeepSeek 命中字段读取（当前 bug）
- 文件：`src-tauri/src/adapter/chat_completions.rs` 的 usage 解析。
- 事实：DeepSeek 在 `usage` **顶层**返回 `prompt_cache_hit_tokens` / `prompt_cache_miss_tokens`，而代码读的是 `prompt_tokens_details.cached_tokens`（OpenAI 格式）→ DeepSeek 用户命中数恒为 0。
- 动作：在 `ChunkUsage` 解析中**同时兼容**两种来源：
  ```rust
  // 优先 OpenAI 格式，回退 DeepSeek 格式
  let cached = usage.prompt_tokens_details.as_ref().and_then(|d| d.cached_tokens)
      .or(usage.prompt_cache_hit_tokens)
      .unwrap_or(0);
  ```
  并相应扩展 `ChunkUsage` 增加 `prompt_cache_hit_tokens: Option<u64>`。

### 6.3 【中收益】保证 system 前缀绝对稳定
- 文件：`agent.rs` 的 `build_system_prompt`。
- 动作：审计是否拼入「每次请求都变」的内容（当前无时间戳/随机值，良好；防止后续误加）。可变信息移到历史/用户输入。
- 收益：决定 `chat / responses / gemini` 三类命中率上限。

### 6.4 【中收益】理解并驯服 compaction 对缓存的影响
- 文件：`compaction.rs`、`agent.rs` 的 goal-loop。
- 事实：compaction 重写历史 → 前缀变化 → 缓存失效一次（下次重写缓存，计 `cache_creation` 全价）。
- 建议：compaction 阈值别设过小（太频繁压缩 → 缓存反复失效）；压缩后保留「不变的大段 system 前缀」在开头，让压缩只影响历史部分，从而**保住 system 断点**（对 Claude 尤其重要）。

### 6.5 【低收益·可选】给 `gemini` 补缓存命中统计
- 文件：`src-tauri/src/adapter/google.rs` 的 usage 解析。
- 动作：读取 `usageMetadata` 中的缓存命中 token，填入 `UsageInfo.cached_tokens`。
- 收益：仅前端展示更完整，不影响实际缓存行为。

### 6.6 【可选】Claude 多断点 / 长 TTL
- 在 6.1 基础上，可对「历史开头若干轮」再打一个断点进一步提升长上下文缓存；权衡：历史变动会让该断点失效。间隔 >5 分钟用 `ttl:"1h"`。

---

## 7. 当前结构与官方文档对比后的「真实缺口」汇总

| # | 供应商 | 官方行为 | CN-Codex 现状 | 影响 | 修复 |
|---|---|---|---|---|---|
| 1 | DeepSeek（`chat`） | 返回 `prompt_cache_hit_tokens`（usage 顶层） | 读 `prompt_tokens_details.cached_tokens` → 0 | 命中数恒显示 0（误以为没命中） | 6.2 兼容两种字段 |
| 2 | Gemini（`gemini`） | `usageMetadata` 含命中数 | 未解析 → 0 | 看不到命中数字 | 6.5 补解析 |
| 3 | Claude（`anthropic`） | 需 `cache_control` 才缓存 | 完全没打断点 | 完全不缓存，付全价 | 6.1 加断点 |
| 4 | OpenAI / Responses | 自动前缀缓存 + `cached_tokens` | 已正确读取 | 正常（前提前缀稳定） | 无需改，保稳定即可 |
| 5 | 所有 | 前缀稳定才命中 | system 前缀稳定，但 compaction/tools 变动会破坏 | 命中率波动 | 6.3 / 6.4 |

---

## 8. 如何验证命中率（用顶部 ⚡ 徽章）

聊天页顶部已有带 ⚡ 图标的 **Token 用量徽章**，悬停看本次对话累计明细：

- **缓存命中**（`cachedTokens`）：被缓存命中的输入 token 数。
- **缓存写入**（`cacheCreationTokens`）：本次新写入缓存的 token 数（过期后重写那次会涨）。
- **缓存未命中** = 输入 − 缓存命中 − 缓存写入。
- **缓存命中率** = 缓存命中 / 输入。

验证步骤：
1. 用目标供应商跑**多轮**同会话对话。
2. 看「缓存命中」是否随轮次增长、「缓存命中率」是否上升。
3. 用 **Claude** 且未做 6.1 → 命中率恒 0（全价），即该优化的信号。
4. 用 **DeepSeek** 且未做 6.2 → 命中率也显示 0（但实际可能已命中，是字段 bug）。
5. 中途改 `user-rules.md` / 插件 / 触发 compaction → 「缓存写入」涨、「缓存命中」回落，属预期。

---

## 9. 一页速查表

| 我想… | 该做 |
|---|---|
| 用 OpenAI / 中转站 省 token | 不改代码；保持 system+tools 前缀稳定 |
| 用 DeepSeek 省 token | 不改也能自动缓存，但**先修 6.2** 才能看到命中数 |
| 用 Claude 省 token | **必须**给 `anthropic` adapter 加 `cache_control`（6.1） |
| 用 Gemini 省 token | 走 OpenAI 兼容端点即自动缓存；原生端点可选 `cachedContent`（6.5 仅补展示） |
| 命中率最高 | 前缀（system+tools+历史开头）逐字节稳定；别放时间/随机/变动列表 |
| 看命中了多少 | 顶部 ⚡ 徽章 → 悬停看「缓存命中 / 写入 / 命中率」 |
| 理解为什么命中掉了 | 改了 user-rules / 插件 / 触发 compaction / 跨 TTL → 缓存失效一次 |

---

*参考官方文档（核对于 2026-07）：*
- *OpenAI Prompt Caching：`developers.openai.com/api/docs/guides/prompt-caching`*
- *Anthropic Prompt Caching：`platform.claude.com/docs/en/build-with-claude/prompt-caching`*
- *DeepSeek 上下文硬盘缓存：`api-docs.deepseek.com/zh-cn/guides/kv_cache/`*
- *Gemini Context Caching：`ai.google.dev/gemini-api/docs/caching`*

*CN-Codex 代码索引：*
- *缓存统计字段：`src-tauri/src/adapter/types.rs`（`UsageInfo`）*
- *四个 adapter 请求构造：`src-tauri/src/adapter/{chat_completions,anthropic,responses,google}.rs` 的 `build_body`*
- *系统提示词构建：`src-tauri/src/agent.rs` 的 `build_system_prompt`*
- *上下文压缩：`src-tauri/src/compaction.rs`*
- *前端明细展示：`src/components/chat/TokenUsageBadge.tsx`*
