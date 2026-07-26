# 现在就给 IDE 里的 Grok 4.5 打分

目标：得到可与 [Artificial Analysis](https://artificialanalysis.ai/) 对照的分数：

| 你的对象 | AA 对照对象 |
|----------|-------------|
| **Grok 4.5 (high) @ CN-Codex** | Grok 4.5 @ Grok Build（Coding Agent Index **76**） |
| （模型本体参考） | Grok 4.5 Intelligence Index **54** |

---

## 0. 先记住 3 条规则

1. AA 评的是 **模型 + harness**，不是裸模型。  
2. 本地 12 题是 **Proxy Index**，口径对齐但任务集更小，**不能直接宣称等于官方 76**。  
3. 每题尽量 **3 次 attempt**，pass=0/1，禁止人工改代码救场。

---

## 1. 初始化（1 分钟）

在仓库根目录执行：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File benchmarks/aa-aligned/runner.ps1 -Action init -Model grok-4.5-high
```

会生成：

`benchmarks/aa-aligned/workspaces/<task_id>/`

---

## 2. CN-Codex 设置

| 项 | 建议 |
|----|------|
| 模型 | Grok 4.5 |
| Reasoning | **high**（对齐 AA high） |
| 模式 | Goal 模式（推荐）或 Agent 模式，全程固定 |
| 权限 | 允许 shell / 读文件 / 写文件 / apply_patch |
| 人工 | 除必要审批外不要插手实现 |

---

## 3. 逐题跑（主流程）

对每个任务：

1. 把工作区切到  
   `benchmarks/aa-aligned/workspaces/<task_id>`
2. 将 `PROMPT.md` 全文发给 Grok 4.5  
3. 等它自己完成后，立刻判分：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File benchmarks/aa-aligned/runner.ps1 -Action grade -TaskId <task_id> -Attempt 1 -Model grok-4.5-high
```

4. 同一题再开新会话跑 Attempt 2、3（可先 `init` 重置该工作区，或手动清空后重来）

### 任务列表（12）

**repo_qa（对齐 SWE-Atlas-QnA）**

- `qa-01-find-entrypoint`
- `qa-02-config-provider`
- `qa-03-tool-pipeline`
- `qa-04-test-command`

**terminal（对齐 Terminal-Bench v2）**

- `term-01-json-transform`
- `term-02-log-etl`
- `term-03-batch-rename`
- `term-04-mini-pipeline`

**swe_edit（对齐 DeepSWE）**

- `swe-01-fix-off-by-one`
- `swe-02-add-feature`
- `swe-03-refactor-api`
- `swe-04-bug-and-regression`

---

## 4. 汇总分数

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File benchmarks/aa-aligned/runner.ps1 -Action summarize -Model grok-4.5-high
```

输出：

- `benchmarks/aa-aligned/results/latest-scorecard.md`
- `benchmarks/aa-aligned/results/latest-summary.json`

### 分数怎么读

```text
Proxy Index = mean(repo_qa, terminal, swe_edit)
```

| Proxy Index | 解读（相对 AA Coding Agent 语境） |
|-------------|-------------------------------------|
| ≥ 0.80 | 很强，接近头部 coding agent 带 |
| 0.60–0.79 | 强，可作主力编码 agent |
| 0.40–0.59 | 中等 |
| < 0.40 | 弱，优先查 harness/工具闭环 |

对照句式：

> 本地 **Grok 4.5 @ CN-Codex** Proxy Index = **X.XX**  
> AA 公开 **Grok 4.5 @ Grok Build** Coding Agent Index = **76**  
> 因任务集不同，仅用于相对比较；若本地显著偏低，优先怀疑 CN-Codex harness 设置，而非否定模型官方智商分。

---

## 5. 最快烟雾测试（可选，只跑 3 题）

若时间紧，先各轴 1 题：

1. `qa-01-find-entrypoint`  
2. `term-01-json-transform`  
3. `swe-01-fix-off-by-one`  

仍用同样 grade/summarize；注意这只是烟雾，不是完整 Proxy。

---

## 6. 工具自检（不调用模型）

验证判分器本身：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File benchmarks/aa-aligned/runner.ps1 -Action selftest
```

期望：12/12 pass，Proxy Index = 1。

---

## 7. 和 AA 排行页怎么并排看

打开：

1. Coding Agents 榜：https://artificialanalysis.ai/agents/coding-agents  
2. Grok 4.5 模型页：https://artificialanalysis.ai/models/grok-4-5  
3. 你的本地：`results/latest-scorecard.md`

并排记录：

| 维度 | AA | 本地 |
|------|----|------|
| 名称 | Grok 4.5 @ Grok Build | Grok 4.5 @ CN-Codex |
| 主分 | Coding Agent Index 76 | Proxy Index ? |
| 成本 | ~$2.5/task | 你的估算 |
| 特长 | terminal / agentic | 看三分量 |

---

## 8. 常见坑

- 用 Chat 随便聊两句就打分 → 无效  
- Attempt 不足 3 次 → 方差大  
- 人工修 bug 后再 grade → 虚高  
- 把 Intelligence 54 和 Coding Agent 76 直接比大小 → 口径不同  
- reasoning 开 low 却对比 AA high → 不公平  

更多细节见：

- `README.md`
- `AA-COMPARISON.md`
- `scorecard.template.md`
