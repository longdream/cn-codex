# Skill Lab AutoGen 回归清单

## 目标生成链路
- [ ] 无 Python 环境时，点击“自动生成”应提示安装引导，不发起生成。
- [ ] 有 Python 环境时，输入目标后可自动生成 `SKILL.md` 与 `scripts/*.py`。
- [ ] 生成完成后，草稿列表和详情页中的 `name/content/testPrompt` 自动刷新。
- [ ] 非法脚本路径（如 `../x.py`）或空脚本应被后端拒绝并返回可读错误。

## 自动进化链路
- [ ] 点击“运行测试”后，阶段顺序保持 `testing -> evaluating -> rewriting -> done`。
- [ ] 评估结果应包含多维分数（clarity/robustness/executability/maintainability/total）。
- [ ] 达到高分且进入平台期后自动停止；未达标时最多迭代到上限。
- [ ] 最终落盘内容应使用最佳分版本，而非最后一轮版本。

## 推广链路
- [ ] 推广后 `codey/skills/<id>/SKILL.md` 存在。
- [ ] 推广后 `codey/skills/<id>/scripts/` 与实验室草稿目录保持同步。

## LLM 传输一致性
- [ ] provider 的 `http_headers` 能注入到请求头。
- [ ] provider 的 `query_params` 能追加到请求 URL。
- [ ] 主循环外流式解析在 `responses/anthropic/gemini` 下能正常提取文本。
