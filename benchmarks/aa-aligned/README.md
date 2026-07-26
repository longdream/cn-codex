# CN-Codex × Artificial Analysis 对齐评分套件

本目录用于把 **CN-Codex + 指定模型（例如 Grok 4.5）** 按 [Artificial Analysis](https://artificialanalysis.ai/) 的 **Agent / Coding Agent** 口径做本地可比评分，并对照 AA 公开排行。

> 重要：AA 官方榜评的是 **模型 + harness（运行环境）**，不是纯模型。  
> 所以你在 IDE 配 Grok 4.5，本地分数应记为：  
> **`Grok 4.5 @ CN-Codex`**，而不是直接写成 “Grok 4.5 官方分”。

---

## 1. AA 官方在评什么

### 1.1 Coding Agent Index（更适合你当前 IDE Agent 场景）

官方页：https://artificialanalysis.ai/agents/coding-agents  
方法论：https://artificialanalysis.ai/methodology/coding-agents-benchmarking

| 组件 | 任务类型 | 任务数 | 尝试 | 评分 |
|------|----------|--------|------|------|
| DeepSWE | 长程软件工程 / 仓库改动 | 113 | 3 | 程序验证器 pass/fail，pass@1 |
| Terminal-Bench v2 | 终端 Agent 任务 | 84* | 3 | 测试套件 pass/fail，pass@1 |
| SWE-Atlas-QnA | 仓库问答 | 124 | 3 | 二元 pass/fail，pass@1 |

- **Index 总分** = 三个组件 pass@1 的 **简单平均**
- 每任务先对 3 次 attempt 取平均，再对所有任务等权平均
- 同时报告：cost/task、tokens/task、wall-time/task

公开参考（AA 文章，2026-07-08）：

| 变体 | Coding Agent Index | 备注 |
|------|--------------------|------|
| Grok 4.5 @ Grok Build | **76** | 与 GPT-5.5@Codex 接近，低于 Fable 5@Claude Code |
| 成本 | ~$2.49–2.59 / task | 明显低于 Claude Code / Codex 同类 |
| 总 token | ~1.9M / task | 很省 token |

### 1.2 Intelligence Index v4.1（全能力综合，不只是 coding agent）

官方页：https://artificialanalysis.ai/evaluations/artificial-analysis-intelligence-index  
方法论：https://artificialanalysis.ai/methodology/intelligence-benchmarking

| 类别 | 权重 | 主要评测 |
|------|------|----------|
| Agents | 34% | GDPval-AA v2 (20%) + τ³-Banking (14%) |
| Coding | 24% | Terminal-Bench v2.1 (16%) + SciCode (8%) |
| Scientific Reasoning | 24% | HLE (12%) + GPQA Diamond (6%) + CritPt (6%) |
| General | 18% | AA-Omniscience (12%) + AA-LCR (6%) |

Grok 4.5（high）公开参考：

| 指标 | 数值 |
|------|------|
| Intelligence Index | **54**（约第 4 梯队 / 前十量级，随榜单刷新会变） |
| 价格 | Input $2 / 1M，Output $6 / 1M |
| Context | 500k |
| 速度 | ~61 tok/s（相对中等偏慢） |

---

## 2. 你该怎么给 “IDE + Grok 4.5” 打分

### 原则

1. **固定 harness**：CN-Codex 当前版本 + 默认工具权限 + 同一套系统提示 / Goal 模式设置  
2. **固定模型设置**：Grok 4.5 + reasoning effort（建议 high，对齐 AA 的 high）  
3. **pass@1 二元判定**：任务全过 = 1，否则 = 0（不要用“感觉还行”）  
4. **每任务 3 次 attempt**，任务分 = 3 次平均；组件分 = 任务分平均  
5. **记录效率**：耗时、工具调用数、估算 token / 费用  
6. **写清对照对象**：对比的是 AA 的 `Grok 4.5 @ Grok Build` 等公开变体，不是“裸模型”

### 推荐评分路径（由易到难）

| 级别 | 名称 | 说明 | 何时用 |
|------|------|------|--------|
| L0 | 官方对照分 | 直接引用 AA 公开榜 | 快速了解模型天花板 |
| L1 | 本地代理套件（本目录） | 12 题迷你 AA 风格任务，可自动验分 | **现在就该跑** |
| L2 | 开源子集复现 | Terminal-Bench / SWE 类公开任务子集 | 要更接近官方数字 |
| L3 | 官方全量 | AA / Mercor / Laude 全量 harness | 研究级，成本高 |

> 本仓库已内置 **L1**。L2/L3 需要额外下载官方数据集与沙箱，不在默认流程内。

---

## 3. 本地 L1 套件结构

```text
benchmarks/aa-aligned/
  README.md                 # 本说明
  suite.json                # 任务清单与权重
  tasks/                    # 每个任务的题目与自动判分
  fixtures/                 # 任务初始文件
  runner.ps1                # 一键初始化 / 验分 / 汇总
  scorecard.template.md     # 人工评分卡模板
  results/                  # 运行结果输出（gitignore 可忽略）
  AA-COMPARISON.md          # 与 AA 排行对照说明
```

### L1 组件映射（对齐 Coding Agent Index 三轴）

| 本地组件 | 对齐 AA | 任务数 | 权重 |
|----------|---------|--------|------|
| `repo_qa` | SWE-Atlas-QnA | 4 | 1/3 |
| `terminal` | Terminal-Bench v2 | 4 | 1/3 |
| `swe_edit` | DeepSWE | 4 | 1/3 |

本地 **CN-Codex Coding Agent Proxy Index** = 三个组件 pass@1 的平均。

---

## 4. 快速开始

### 4.1 初始化任务工作区

```powershell
pwsh -File benchmarks/aa-aligned/runner.ps1 -Action init
```

会在 `benchmarks/aa-aligned/workspaces/` 生成每个任务的独立工作目录。

### 4.2 在 CN-Codex 中跑任务（Grok 4.5）

1. IDE 模型切到 **Grok 4.5**，reasoning 尽量 **high**  
2. 打开 Goal 模式（或普通 Agent 模式，二选一并固定）  
3. 对每个任务：
   - 工作目录切到对应 `workspaces/<task_id>`
   - 把 `tasks/<task_id>/PROMPT.md` 完整粘贴给 Agent
   - 允许必要工具：shell / 读写文件 / apply_patch
   - **不要**人工帮忙改代码（否则分数无效）
4. Agent 说完成后，运行判分：

```powershell
pwsh -File benchmarks/aa-aligned/runner.ps1 -Action grade -TaskId <task_id> -Attempt 1
```

### 4.3 三次 attempt 与总分

每个 task 建议跑 3 次（可新开会话），attempt=1/2/3。  
全部跑完后：

```powershell
pwsh -File benchmarks/aa-aligned/runner.ps1 -Action summarize
```

会生成：

- `results/latest-summary.json`
- `results/latest-scorecard.md`

### 4.4 只做自动自检（不调用模型）

用于验证判分脚本本身是否正确：

```powershell
pwsh -File benchmarks/aa-aligned/runner.ps1 -Action selftest
```

---

## 5. 如何对照 AA 排行解读本地分

| 本地 Proxy Index | 粗解读（相对 AA Coding Agent 公开分） |
|------------------|----------------------------------------|
| ≥ 0.80 | 非常强：接近/达到 AA 头部 coding agent 带（70–80+） |
| 0.60–0.79 | 强：具备稳定多工具编码能力 |
| 0.40–0.59 | 中等：能做常见仓库任务，长程/终端易翻车 |
| < 0.40 | 弱：工具使用或工程闭环不足 |

**不要**把本地 12 题分数直接等同 AA 的 76。  
正确写法：

> `Grok 4.5 @ CN-Codex` 本地 Coding Agent Proxy Index = **X.XX**  
> AA 公开 `Grok 4.5 @ Grok Build` Coding Agent Index = **76**  
> 二者口径相近但任务集不同，仅可做 **相对强弱与 harness 差距** 分析。

若你本地分显著低于 AA 的 76，优先怀疑：

1. CN-Codex 工具链/权限/提示词弱于 Grok Build  
2. reasoning effort 不是 high  
3. 审批打断过多（approval_policy）  
4. 上下文/截断策略不同  
5. 任务语言/仓库风格差异

---

## 6. 建议的正式实验记录字段

每个 attempt 至少记录：

| 字段 | 说明 |
|------|------|
| model | 如 `grok-4.5-high` |
| harness | `CN-Codex <version>` |
| task_id | 任务 ID |
| attempt | 1..3 |
| pass | 0/1 |
| wall_time_sec | 墙钟秒数 |
| tool_calls | 工具调用次数 |
| est_input_tokens / est_output_tokens | 可估 |
| notes | 失败原因：权限/幻觉/死循环/测试不过等 |

---

## 7. 下一步（可选升级到更接近官方）

1. **Terminal-Bench 公开子集**：接 Laude Institute Terminal-Bench  
2. **SWE 类 patch 任务**：用带测试的真实仓库 issue  
3. **APEX-Agents 子集**：长程跨应用专业任务（pass@1 更难，头部也常 <50%）  
4. 把 CN-Codex 的会话日志自动导入 `results/`，减少人工抄分

---

## 参考链接

- AA Coding Agents：https://artificialanalysis.ai/agents/coding-agents  
- Coding Agent 方法论：https://artificialanalysis.ai/methodology/coding-agents-benchmarking  
- Intelligence Index 方法论：https://artificialanalysis.ai/methodology/intelligence-benchmarking  
- APEX-Agents-AA：https://artificialanalysis.ai/evaluations/apex-agents-aa  
- Grok 4.5 AA 文章：https://artificialanalysis.ai/articles/grok-4-5-brings-spacexai-to-the-the-intelligence-frontier  
- Grok 4.5 模型页：https://artificialanalysis.ai/models/grok-4-5  
