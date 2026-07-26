# 与 Artificial Analysis 排行对照手册

## 一句话结论

要给 **IDE 配 Grok 4.5** 打分，正确对象是：

> **`Grok 4.5 (high) @ CN-Codex`**

对照对象通常是：

> **`Grok 4.5 (high) @ Grok Build`**（AA Coding Agent Index ≈ **76**）  
> 以及模型本体 **Intelligence Index ≈ 54**

不能把本地 IDE 体感直接写成 “Grok 4.5 官方分”。

---

## AA 官方分数（公开参考）

来源：

- https://artificialanalysis.ai/models/grok-4-5
- https://artificialanalysis.ai/articles/grok-4-5-brings-spacexai-to-the-the-intelligence-frontier
- https://artificialanalysis.ai/agents/coding-agents
- https://artificialanalysis.ai/methodology/coding-agents-benchmarking
- https://artificialanalysis.ai/methodology/intelligence-benchmarking

| 榜单 | Grok 4.5 参考分 | 含义 |
|------|-----------------|------|
| Intelligence Index v4.1 | **54** | 综合智能（Agents+Coding+Science+General） |
| Coding Agent Index（Grok Build harness） | **76** | 编码 Agent 复合分（DeepSWE + Terminal-Bench v2 + SWE-Atlas-QnA） |
| 价格 | $2 / $6 per 1M in/out | 成本维度 |
| Context | 500k | 上下文 |

> 排行名次会随新模型上榜波动；**分数口径**比瞬时名次更重要。

---

## 你应该采用的打分公式

### A. 官方 Coding Agent 口径（推荐对齐）

```text
task_score = mean(pass of attempt1..attempt3)   # 每次 0/1
component_score = mean(task_score over tasks)
CodingAgentIndex ≈ mean(DeepSWE, Terminal-Bench, SWE-Atlas-QnA)
```

本地代理实现：

```text
ProxyIndex = mean(repo_qa, terminal, swe_edit)
```

### B. 效率副指标（AA 也会报）

对每次 attempt 记录：

- wall_time_sec
- tool_calls
- est tokens / cost

AA 公开里 Grok 4.5@Grok Build 很省：约 **1.9M tokens/task**、约 **$2.5/task**（Coding Agent 套件）。

---

## IDE 实操清单（CN-Codex + Grok 4.5）

1. **固定设置**
   - 模型：Grok 4.5
   - reasoning：high（若可配）
   - 模式：Goal 模式或 Agent 模式（二选一，全程不变）
   - 审批：尽量减少人工打断（否则不是 agent 自主分）
2. **初始化**
   ```powershell
   pwsh -File benchmarks/aa-aligned/runner.ps1 -Action init -Model grok-4.5-high
   ```
3. **逐题跑**
   - 打开 `benchmarks/aa-aligned/workspaces/<task_id>/`
   - 把 `PROMPT.md` 交给 Grok 4.5
   - 只允许 agent 自己改文件/跑命令
4. **判分**
   ```powershell
   pwsh -File benchmarks/aa-aligned/runner.ps1 -Action grade -TaskId <id> -Attempt 1 -Model grok-4.5-high
   ```
5. **每题 3 次**后汇总
   ```powershell
   pwsh -File benchmarks/aa-aligned/runner.ps1 -Action summarize -Model grok-4.5-high
   ```
6. **读** `results/latest-scorecard.md`

---

## 如何把本地分“挂到” AA 排行语境

### 写法模板

```markdown
### Grok 4.5 @ CN-Codex（本地 AA-aligned proxy）
- Proxy Index: 0.72
- repo_qa: 0.83
- terminal: 0.67
- swe_edit: 0.67
- attempts: 3 per task

### AA 公开对照
- Grok 4.5 Intelligence Index: 54
- Grok 4.5 @ Grok Build Coding Agent Index: 76

### 结论
本地 harness 在 terminal/SWE 上弱于 Grok Build 约 X%；
模型本体处于 AA 近前沿梯队，当前瓶颈更可能在 CN-Codex 工具链/权限/提示词。
```

### 对照解释规则

| 现象 | 更可能原因 |
|------|------------|
| 本地 proxy 高，AA 也高 | 模型强且 CN-Codex 工具闭环够用 |
| AA 高、本地明显低 | **harness 差距**（最常见） |
| 本地 Q&A 高、SWE/terminal 低 | 多步工具/补丁/终端执行弱 |
| 三次 attempt 方差很大 | 不稳定性高，AA 也会用 3 次平均压噪声 |

---

## 不要做的事

1. 不要只聊 2 个需求就给“85 分”  
2. 不要把 Chat 模式分数和 Agent 模式混谈  
3. 不要人工救场后再算 pass  
4. 不要拿 Intelligence Index 的 54 和 Coding Agent 的 76 直接比大小后说“矛盾”——它们不是同一套任务  
5. 不要声称“已复现 AA 官方分”，除非你跑的是同一数据集与 harness

---

## 升级路径（更接近官方）

| 阶段 | 内容 | 成本 |
|------|------|------|
| L1 本套件 | 12 题 proxy，已就绪 | 低 |
| L2 Terminal-Bench 子集 | 公开终端任务 + 自动测试 | 中 |
| L2 SWE 子集 | 真实仓 patch + 单测 | 中高 |
| L3 AA/Mercor 全量 | APEX / DeepSWE 等 | 高 |

---

## 当前仓库已提供

- `benchmarks/aa-aligned/suite.json`：任务与权重
- `benchmarks/aa-aligned/tasks/*`：题面 + 自动 grade
- `benchmarks/aa-aligned/runner.ps1`：init/grade/summarize/selftest
- `benchmarks/aa-aligned/results/`：跑分输出
