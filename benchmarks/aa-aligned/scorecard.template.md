# Grok 4.5 @ CN-Codex 评分卡（模板）

- 日期：
- CN-Codex 版本 / commit：
- 模型：Grok 4.5
- Reasoning：high / medium / low
- 模式：Goal / Agent
- approval_policy：
- 评测人：

## 官方对照（AA）

| 指标 | 分数 | 来源 |
|------|------|------|
| Intelligence Index | 54 | https://artificialanalysis.ai/models/grok-4-5 |
| Coding Agent Index @ Grok Build | 76 | AA article 2026-07-08 |

## 本地 Proxy 结果

| 组件 | 对齐 | pass@1 | 备注 |
|------|------|--------|------|
| repo_qa | SWE-Atlas-QnA |  |  |
| terminal | Terminal-Bench v2 |  |  |
| swe_edit | DeepSWE |  |  |
| **Proxy Index** | Coding Agent Index 代理 |  | mean of 3 |

## 任务明细

| Task | A1 | A2 | A3 | pass@1 | 失败原因 |
|------|----|----|----|--------|----------|
| qa-01-find-entrypoint |  |  |  |  |  |
| qa-02-config-provider |  |  |  |  |  |
| qa-03-tool-pipeline |  |  |  |  |  |
| qa-04-test-command |  |  |  |  |  |
| term-01-json-transform |  |  |  |  |  |
| term-02-log-etl |  |  |  |  |  |
| term-03-batch-rename |  |  |  |  |  |
| term-04-mini-pipeline |  |  |  |  |  |
| swe-01-fix-off-by-one |  |  |  |  |  |
| swe-02-add-feature |  |  |  |  |  |
| swe-03-refactor-api |  |  |  |  |  |
| swe-04-bug-and-regression |  |  |  |  |  |

## 效率（可选）

| Task | 平均耗时(s) | 平均工具调用 | 估算成本 |
|------|-------------|--------------|----------|
|  |  |  |  |

## 结论

- 相对 AA `Grok 4.5 @ Grok Build (76)`：更高 / 接近 / 更低
- 主要短板：repo_qa / terminal / swe_edit
- harness 改进建议：
- 是否值得作为默认编码模型：
