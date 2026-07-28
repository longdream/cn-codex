---
title: AI自主学习教练系统 · 产品技术 SPEC
type: spec
author: CN-Codex AI Assistant
date: 2026-07-27
version: 1.4
status: review
audience: 研发、产品、赛事评委
---

# AI 自主学习教练系统 · 产品技术 SPEC

## 0. 文档目的

本 SPEC 将既有设计文档收敛为**可评审、可实现、可验收**的技术规格，服务：

1. GOAI 初赛方案深度说明；
2. 复赛工程实现与测试对齐；
3. 团队内部对范围、接口、数据契约与非目标的统一认知。

**上游文档**：

- `education/docs/教育IDE_需求文档_可参赛版.md`
- `education/docs/AI-教学引擎设计.md`
- GOAI Boundless Agents 参赛手册（初赛/复赛要求）

---

## 1. Overview

### 1.1 产品一句话

以**会生长的学生数字孪生**为决策中枢，以**纸质主学 + 短时屏幕教练**为执行形态：沟通建档、材料盘点、动态计划、作业拍照取证与错因策略；**不以系统内置题库长时间刷屏**为主路径；可选姿态辅助。

### 1.2 系统形态

- 运行于 CN-Codex 教育 IDE 能力之上的学习陪伴与教学编排系统
- 左侧任务会话 + 中间引导对话 + 上层 Education Modal + 右侧情境面板
- 本地优先数据与事件账本；模型能力可插拔

### 1.3 范围版本

| 版本 | 范围 |
|---|---|
| SPEC v1.0 / 初赛 | 完整设计规格 + MVP 验收标准；不要求全量实现 |
| MVP（复赛目标） | 单学科、完整 1 条任务闭环、L0/L1 为主、L2 异步可选 |
| 决赛增强 | 多学科包、语音完善、教师/监护人授权视图、完整审计证明 |

---

## 2. Goals / Non-goals

### 2.1 Goals

1. 建立可解释、可修正的学生数字孪生  
2. 通过**心理学友好的多次交流与多源证据**让孪生持续生长，而非一次问卷/测评定终身  
3. 以孪生状态驱动**有依据的反馈**与**动态计划调整**（学生确认）  
4. 在授权时间窗主动发起可拒绝的学习沟通  
5. **纸质优先**：任务默认落到学生已有卷子/作业页码；屏幕会话短时、有目的  
6. 支持材料盘点与可选购书建议（可拒绝）；作业拍照/反馈取证  
7. 可选手机姿态类别增强投入/疲劳相关线索  
8. 培养自主调节；关键路径即时，深度分析异步可降级  

### 2.2 Non-goals

1. 心理疾病诊断 / 治疗 / 危机干预替代  
2. 替代学校正式评价、考试结论、升学决策  
3. 自动化惩罚、排名、人格标签  
4. 身份人脸识别、面部微表情情绪读心、监控式常开录像与惩罚告状  
5. 将姿态信号直接等同于心理诊断或学业人品评价  
6. 完整商业题库/教材分发平台  
7. 远程第三方任意代码学科包商店（第一期）  
8. **以系统内置题库作为学生长时间屏上作答主路径**  
9. 强制导购/商城闭环；未经确认的材料购买压力话术  

---

## 3. Users & Permissions

| 角色 | 核心权限 | 边界 |
|---|---|---|
| 学生 | 画像查看/更正、任务执行、反馈、提醒设置 | 高影响计划变更需本人确认 |
| 家长/监护人 | 授权范围内周报与风险提示 | 默认不可看私密对话全文 |
| 教师/辅导者 | 聚合学情、授权任务线索 | 最终教育判断由人作出 |
| 管理员 | 账户、内容策略、审计配置 | 不可任意浏览学生内容 |

未成年人场景：监护人授权为启用学习状态采集、语音、附件等能力的前置条件（按功能分级）。
姿态辅助为**独立分级授权**，默认关闭。

---

## 4. System Architecture

### 4.1 逻辑架构

```text
UI Layer
  Sidebar Tasks · Chat Stream · EducationModalHost · Twin Panel · Consent Panel
                │
Orchestration Layer
  Event Router (L0/L1/L2) · Supervisor · Job Queue · Template Engine
                │
Agent Layer
  Planning · Companion · Growth Analysis · Intervention · Safety
                │
Twin & Knowledge Layer
  StudentTwin · Evidence Ledger · Subject Packs · Summary Cache
                │
Data & Security Layer
  Events · Consent · Audit · Local Storage · Optional Voice STT · Optional Posture
```

### 4.2 执行分层

| 路径 | 时延 | 执行者 | 允许 | 禁止 |
|---|---|---|---|---|
| L0 | <200ms 本机 | 状态机 + 规则 | 写事件、切状态、开弹窗、频控、授权检查 | 完整多 Agent 链、阻塞 UI |
| L1 | 通常 <2s | 模板/轻量规则或短调用 | 话术、3–4 选项、简单洞察 | 重写长期目标、长文分析 |
| L2 | 异步数秒~数十秒 | CN-Codex Agent 轮次 | 周计划、复杂诊断、长解释 | 阻塞输入；自动强制改计划 |

### 4.3 顺畅性硬规则

1. 学生点击后必须先有即时反馈  
2. 一次用户动作最多触发一次 L2 `orchestration_job`  
3. 默认不串行跑齐全部子 Agent  
4. 主动提醒优先预渲染模板 + 孪生摘要缓存  
5. `awaiting_user_input` 只等人，不等人模型  
6. 温度话术用模板骨架，不因共情额外增加慢轮次  

---

## 5. Core Domain Model

### 5.1 实体

| 实体 | 说明 |
|---|---|
| `student_twin` | 孪生主记录 |
| `learning_goal` | 长/中/短目标 |
| `learning_plan` | 阶段与周计划 |
| `learning_task` | 线上/线下学习单元 |
| `interaction_request` | 弹窗结构化请求 |
| `interaction_record` / `interaction_event` | 交互与事件 |
| `learning_evidence` | 作答/反馈/附件证据 |
| `state_observation` | 五维原子观测 |
| `state_series_snapshot` | 时间窗趋势快照 |
| `state_inference` | 待确认/非事实推断 |
| `student_correction` | 学生更正 |
| `intervention_proposal` | 可选干预 |
| `consent_record` | 授权 |
| `orchestration_job` | L2 异步任务 |
| `twin_summary_cache` | 最小摘要缓存 |
| `subject_pack` / `enrollment` | 学科包与启用关系 |
| `posture_observation` | 可选手机姿态类别观测（短有效期） |
| `plan_adjustment_proposal` | 动态计划调整建议（待学生确认） |
| `task_item` | 学科包实例化后的可评分题/步骤 |
| `learning_material` | 学生已有卷子/教辅/作业来源档案 |
| `homework_photo_evidence` | 作业/错题照片证据（可删原图） |
| `mastery_observation` | 知识点掌握观测（分层、带证据与覆盖度） |

### 5.2 LearningTask 最小字段

```json
{
  "id": "task_function_basics_001",
  "plan_id": "plan_math_term_1",
  "title": "二次函数基础：图像与解析式转换",
  "subject": "math",
  "learning_objective": "能说出开口、对称轴和顶点与解析式的对应关系",
  "scheduled_window": "2026-08-01 20:00-20:30",
  "estimated_minutes": 30,
  "mode": "offline_practice",
  "status": "scheduled",
  "knowledge_nodes": ["math.function.graph_mapping"],
  "completion_evidence": [],
  "feedback_required": true,
  "adaptation_policy": "reduce_scope_then_retry"
}
```

任务状态机：

```text
scheduled → proactive_prompted → confirmed → in_progress
  → feedback_pending → analyzing → adjusted|completed
  ↘ postponed | paused_today | cancelled
```

### 5.3 StudentTwin 结构

```text
StudentTwin
├── Profile（相对稳定）
│   identity_profile / goal_model / knowledge_model
│   interest_preference_model / support_preferences
├── LearningStateSeries（五维动态，按 scope+window）
│   goal_planning
│   engagement_execution
│   cognition_strategy
│   monitoring_reflection
│   motivation_affect
├── evidence_ledger
├── intervention_history
├── long_term_memory（仅学生确认且学习相关）
└── growth_timeline
```

### 5.4 五维状态规则

| 维度 | 可观察证据 | 禁止 |
|---|---|---|
| goal_planning | 目标确认、计划调整、时间窗匹配 | 未完成=自律差 |
| engagement_execution | 开始/完成/改期、用时、主动暂停 | 用在线时长推断努力/情绪 |
| engagement_execution（可选增强） | 授权下的姿态类别（趴伏/过近等） | 用姿态直接判懒惰/情绪病 |
| cognition_strategy | 作答、错因、策略选择 | 一次错题=能力低 |
| monitoring_reflection | 难度预测对比、反思、主动调整 | 不愿长反思=无能力 |
| motivation_affect | 主动量表/自愿表述 | 摄像头/副语言隐式情绪识别 |

写入原则：

- 事实 / 自评 / 推断分层  
- 动态状态必须有 `expires_at`  
- 无 `scope.subject` 不得写跨学科总分  
- 未确认推断不得进入正式状态与共享  

补充：

- 可选 `posture_category` 仅作为 `engagement_execution` 的短效线索，**不得**直接写入 motivation 诊断结论  
- 孪生必须维护 `coverage`（证据覆盖度）；覆盖不足时只允许轻问或保持计划，不允许武断重排  

### 5.5 StateObservation 契约

```json
{
  "observation_id": "obs_20260727_001",
  "student_id": "stu_001",
  "dimension": "motivation_affect",
  "subdimension": "energy_level",
  "scope": {
    "subject": "math",
    "goal_id": "goal_math",
    "task_id": "task_function_basics_001"
  },
  "value": "a_little_tired",
  "source_type": "student_self_report",
  "source_event_id": "modal_submit_20260727_021",
  "captured_at": "2026-07-27T20:32:00+08:00",
  "expires_at": "2026-07-27T23:59:59+08:00",
  "confidence": "high",
  "consent_ref": "consent_learning_state_v1",
  "student_visible": true,
  "student_editable": true
}
```

### 5.6 孪生生长机制（多次交流）

| 阶段 | 输入 | 孪生变化 | 产品约束 |
|---|---|---|---|
| 冷启动 | 愿望/时间优先的短对话 | 最小 Profile + 低 coverage | 非测评口吻；可跳过；不假装全知 |
| 日常生长 | 任务节点轻问 + 反馈 | 追加 observation/evidence | 一次一问；问前说明用途 |
| 可选姿态 | 手机姿态类别事件 | 短效 engagement 线索 | 默认可关；不存原视频；可忽略 |
| 冲突修正 | 学生“这不准确” | correction + supersede | 致谢修正，不争辩 |
| 过期回落 | expires_at 到达 | 状态失效 | 防止永久标签 |
| 跳过 | skip / later | 仅记未提供 | **禁止**把跳过写成负向状态 |

### 5.6.1 心理学友好采集规格（强制）

| 规则 ID | 规则 | 验收 |
|---|---|---|
| PSY-01 | 问前必须有一句用途说明（为何问、如何帮计划） | 冷启动/反馈模板审计 |
| PSY-02 | 单轮仅 1 个主问题；优先 choice/modal | UI 与协议约束 |
| PSY-03 | 所有非关键项可跳过；跳过不写负向默认 | 事件契约测试 |
| PSY-04 | 禁用评估/诊断/羞辱措辞词表 | 话术 lint / 模板审核 |
| PSY-05 | 选项语言中性或成长向（策略/节奏，非人格） | 选项文案审查 |
| PSY-06 | 学生更正后旧状态 supersede | 状态机测试 |
| PSY-07 | 答后即时展示“这对计划意味着什么” | 交互回显 |
| PSY-08 | 自主三出口：跳过 / 稍后 / 关闭此类询问 | Modal 必含 |
| PSY-09 | 禁止“完成测评才能使用核心功能”的硬门禁（最小目标确认除外） | 流程验收 |
| PSY-10 | 姿态/语音等敏感通道默认关，开启用邀请句而非监控句 | 授权文案 |

**禁用措辞示例（非穷尽）**：测评、诊断、差生、态度差、必须填写、监控你、举报/告知家长惩罚。  
**推荐框架**：一起看看、你更想、如果方便、也可以先不说、改了也没关系。

### 5.7 动态计划模型

```text
learning_plan (骨架)
  → learning_task[]
  → evidence / twin snapshot
  → plan_adjustment_proposal (reason, options, confidence)
  → student confirm | reject | modify
  → write-back plan + twin intervention_history
```

| 字段（建议） | 说明 |
|---|---|
| `proposal_id` | 调整建议 ID |
| `trigger_evidence_ids` | 触发证据 |
| `twin_snapshot_ref` | 所依据的孪生摘要版本 |
| `actions` | reduce_scope / reschedule / change_strategy / pause… |
| `requires_student_confirmation` | 高影响变更必须 true |
| `status` | proposed / accepted / rejected / expired |

**与传统计划的规格差异**：传统计划无 `proposal` 层与孪生快照绑定；本系统任何自动“变难/加量/改目标”路径默认禁止。

### 5.8 姿态观测契约（可选）

```json
{
  "posture_id": "pos_20260727_01",
  "student_id": "stu_001",
  "task_id": "task_function_basics_001",
  "category": "slouching",
  "category_enum": ["upright", "slouching", "leaning_too_close", "away_from_seat", "unknown"],
  "confidence": 0.78,
  "source": "mobile_camera_ondevice",
  "raw_media_retained": false,
  "captured_at": "2026-07-27T20:18:00+08:00",
  "expires_at": "2026-07-27T22:18:00+08:00",
  "consent_ref": "consent_posture_v1",
  "student_visible": true,
  "allowed_actions": ["gentle_reminder", "offer_shorten_task", "ignore"],
  "forbidden_actions": ["punish", "notify_guardian_for_discipline", "identity_recognition", "emotion_diagnosis"]
}
```

---

## 6. Interaction Protocol（Modal-first）

### 6.1 硬规则

1. 正式选择/填写/确认必须弹窗  
2. 弹窗打开后主链路 `awaiting_user_input`  
3. 仅提交（或策略允许的明确跳过）后推进  
4. 未提交关闭 ≠ 答错  
5. 同时优先一个主弹窗（可排队）  
6. 提交结果：对话回显 + 事件账本  

### 6.2 InteractionRequest

```json
{
  "request_id": "req_step3_unit1",
  "task_id": "task.milk_tea_half_price",
  "thread_id": "thr_...",
  "chain_step_key": "find_unit1",
  "blocking": true,
  "ui": {
    "mode": "modal",
    "auto_open": true,
    "allow_dismiss": true
  },
  "content": {
    "title": "找出单位1",
    "prompt": "第二杯半价时，这里的“1/2”是相对于谁？",
    "help": "单位1通常是被当作全部/原价参照的量",
    "knowledge_hint": ["小学_数学_单位1"]
  },
  "fields": [
    {
      "id": "choice",
      "type": "choice",
      "required": true,
      "options": [
        {"value": "A", "label": "半价后的第二杯价格"},
        {"value": "B", "label": "一杯奶茶原价"},
        {"value": "C", "label": "两杯实付总价"}
      ]
    }
  ],
  "actions": [
    {"id": "hint", "role": "secondary"},
    {"id": "submit", "role": "primary"}
  ]
}
```

### 6.3 Modal 类型（MVP）

| type | 用途 | MVP |
|---|---|---|
| choice | 单选 | 是 |
| form | 多字段反馈/分析 | 是 |
| text_input / textarea | 填空/简答 | 是 |
| confirm | 确认 | 建议 |
| multi_choice | 多选知识点 | Phase 2 |
| knowledge_pick | 知识点点选 | Phase 2 |
| voice_compose | 语音填窗 | Phase 2 |
| number / expression / mixed | 数值/列式/混合 | 按任务需要 |

### 6.4 阻塞状态机

```text
step_ready
  → open EducationModal(request)
  → awaitStatus = awaiting_user_input
  → user edit / voice partial
  → submit → validate
  → close modal
  → echo + event
  → grade/update
  → next or remediate
```

---

## 7. Multi-Agent Design

### 7.1 角色与 IO

| Agent | 输入 | 输出 | 禁区 |
|---|---|---|---|
| Supervisor | 事件、孪生摘要、规则 | 路由计划、确认项 | 绕过确认 |
| Planning | 目标、时间、知识证据、学科包 | 阶段/周计划/任务 | 提分承诺 |
| Companion | 任务、偏好、提醒策略 | 主动消息、反馈请求 | 羞辱/催命 |
| Growth Analysis | 证据、反馈、趋势 | 洞察+置信度 | 伪事实/诊断 |
| Intervention | 洞察、历史策略 | 可选策略 | 强制改计划 |
| Safety | 内容、年龄、权限 | 允许/阻断/转介 | 替代专业救助 |

### 7.2 决策流程

```text
事件进入
  → L0 落库/状态/必要时开 Modal
  → 读 twin_summary_cache
  → 路由 none | L1 | L2 | safety
  → L1：建议+理由+选项
  → L2：orchestration_job 异步
  → 学生确认/拒绝/稍后/自改
  → 写 InteractionEvent / TwinUpdate
  → 下一任务或周回顾
```

### 7.3 路由预算

| 事件 | 默认路径 |
|---|---|
| 时间窗到达 | L0+L1 |
| 开始/稍后/暂停 | L0 |
| 反馈提交 | L0+L1，连续困难可升 L2 |
| 首周/下周计划 | L2（可先骨架） |
| 高风险表述 | L0 安全模板 |

`orchestration_job` 必填：`job_id`、触发事件、预算（max_turns/timeout）、可取消、学生可见进度文案。  
超时/失败 → 降级 L1，并记 `path_degraded_to_l1`。

---

## 8. Teaching Main Chain

任务内主链路：

```text
材料确认 → 今日纸质任务（页码/题号）→ 离屏练习 → 拍照/反馈 → 错因策略 → 明日协商 → 反思
```

原则：

- **纸质主学**：正式解题离屏完成  
- 屏幕只做短时确认、材料建议、拍照复盘  
- 题目主来源=学生已有材料，不是系统题库  
- 不直接以标准答案作为主反馈；先策略与错因  
- 拍照与附件授权、可删、最小必要  

### 8.1 学科内容闭环（强制说清出处）

```text
友好沟通 + 材料盘点（learning_material_inventory）
  → 纸质任务（material_ref + 页码/题号范围）
  → 离屏纸笔完成
  → homework_photo / offline_feedback（短时回传）
  → diagnose（OCR/视觉理解 + 学生确认卡点）
  → 答案线索：材料答案页 / 师批 / 自批 / AI 辅助（标置信）
  → 解析：策略提示 + 错因模板 + 可选分步讲解
  → mastery_observation + 动态计划（学生确认）
```

学科包角色变更：提供 **知识图谱 + 错因/策略脚手架**，用于归类与建议；**不是**把学生拉进内置屏上题库长时间作答。

### 8.2 题目来源（Item Provenance）

| 来源 | 优先级 | 约束 |
|---|---|---|
| 学生已有纸质卷子/教辅/学校作业 | **P0 主来源** | 沟通盘点；可选拍封面/目录建档 |
| 每日真实作业页/错题 | **P0 练习载体** | 纸笔完成后再回传 |
| AI 材料建议（用现有/可选补购） | P1 顾问 | 可拒绝；无商城强制闭环 |
| 学科包知识节点与任务脚手架 | P1 诊断 | 帮映射知识点与策略，非刷题主库 |
| 系统 demo 样例题 | P3 兜底 | 仅 Demo/无材料冷启动；标注 sample |
| 完整商业题库屏上分发 | 禁止 | 非目标 |

`PaperTask`（纸质任务）最小字段：

```json
{
  "task_id": "task_paper_20260727_01",
  "material_id": "mat_school_workbook_math",
  "material_title": "学校数学练习册",
  "page_range": "P18-P19",
  "estimated_minutes": 35,
  "subject": "math",
  "knowledge_node_ids": ["math.function.graph_mapping"],
  "mode": "offline_paper",
  "screen_budget_minutes": 5,
  "evidence_expected": ["homework_photo_or_feedback"]
}
```

### 8.3 答案来源与评分

| 作答类型 | 答案来源 | 评分路径 |
|---|---|---|
| 纸质客观题 | 材料答案页 / 师批勾画 / 学生自批 | 优先人类批改线索；AI 辅助核对并标置信度 |
| 纸质解答题 | 要点 rubric（脚手架）+ 学生过程照片 | L1/L2 辅助诊断，非替写答案 |
| 仅文字反馈无图 | 学生自述完成与卡点 | 记事实+自评；掌握置信偏低 |

约束：

- **禁止**把“给完整标准答案”当主路径  
- AI 判定不确定时必须问学生，不得装全知  
- 证据写入 `learning_evidence`，含 `material_ref`、`page_range?`、`photo_ref?`、`error_code?`、`confidence`

### 8.4 学生输入路径

| 通道 | API/UI | 正式作答？ |
|---|---|---|
| 纸笔离屏完成 | 物理作业 | **是（主路径）** |
| 作业/错题拍照 | `edu_submit_homework_photo` | 是（证据） |
| 短时反馈 Modal/表单 | `edu_submit_offline_feedback` | 是（完成/难点） |
| 材料盘点 Modal | `edu_inventory_learning_materials` | 是（材料档案） |
| 计划确认 Modal | `edu_confirm_plan_adjustment` | 是（决策） |
| 屏上内置题库连刷 | — | **否（非目标）** |
| 聊天自由输入 | ChatInput | 辅助澄清 |

未提交反馈 / 取消拍照：不得记错误；可仅记“任务进行中/稍后回传”。

### 8.5 解析来源

| 层级 | 来源字段 | 何时 |
|---|---|---|
| 先问卡点 | 友好单题 | 拍照后 |
| 策略/错因模板 | `error-taxonomy.json` | 命中 error_code |
| 材料自带解析 | 纸质教辅页 | 提示去看对应页，不全文搬运版权解析 |
| L2 分步讲解 | LLM 绑定照片区域+知识点假设 | 学生要“讲一步”；可取消 |

禁止：无确认的完整答案倾倒；把作业拍照做成监控式常开拍摄。

### 8.6 知识点掌握模型

```json
{
  "mastery_observation_id": "mst_20260727_01",
  "student_id": "stu_001",
  "knowledge_node_id": "math.function.graph_mapping",
  "subject": "math",
  "window": "2026-W31",
  "level": "emerging",
  "level_enum": ["unknown", "emerging", "practicing", "consolidating", "demonstrated"],
  "evidence_ids": ["ev_item_fn_map_001", "ev_offline_001"],
  "error_codes": ["confuse_axis_with_vertex"],
  "coverage": 0.34,
  "confidence": "low",
  "source_types": ["homework_photo", "teacher_mark", "self_mark", "offline_feedback", "error_taxonomy"],
  "student_visible": true,
  "student_editable": true,
  "expires_at": null,
  "notes": "完成≠掌握；单次拍照不错不升 demonstrated；主证据来自纸质作业"
}
```

升级规则（MVP 可规则化）：

1. 无证据 → `unknown`  
2. 仅 1 次相关证据或仅自评 → `emerging`  
3. 同节点 ≥2 次过程证据且错因下降 → `practicing`  
4. 间隔多日同类题订正成功 / 师批改善 → `consolidating` / `demonstrated`  
5. 任何升级若 `coverage` 不足或学生更正，不得静默生效；高影响用于改计划前须确认  

### 8.7 掌握判定与孪生写入

掌握状态必须**可解释、可更正、可撤回**；**覆盖度不足时只允许轻问或保持计划，不允许武断判掌握并改计划**。  
写入 `cognition_strategy` 相关观测时：事实（作答结果）/ 自评 / 推断分层；推断须带 `evidence_ids` 与 `confidence`。

### 8.8 与上游文档一致性

本章与方案书 §3.4 **纸质优先内容闭环**对齐；学科包降级为诊断脚手架。相对早期“屏上 Modal 刷题”设计，以护眼与真实材料为更高优先级产品约束。

---

## 9. Offline Learning Loop

```text
制定线下任务
  → 学生线下完成
  → 结构化反馈（完成/部分/未开始、用时、难点、难度、信心、可选附件）
  → 更新证据与孪生
  → 提出调整（学生确认）
```

约束：

- 完成 ≠ 掌握  
- 未完成 ≠ 缺乏意愿  
- 附件可选、脱敏提醒、默认可不上传  
- **本路径为学科练习默认路径**（高于屏上题库）  
- 推荐证据：作业/错题照片 或 等价结构化反馈  
- 单次教练屏时建议有预算（如确认+回传合计数分钟级，可配置）  

---

## 10. Subject Pack Plugin Spec

### 10.1 目标

教育核心引擎学科无关；教学内容以学科包提供。

### 10.2 目录

```text
education/subject-packs/<pack-id>/
  manifest.json
  knowledge-graph.json
  task-templates.json
  error-taxonomy.json
  demo/   # 可选
```

### 10.3 生命周期

- 选择学科 → 校验 → 启用 → enrollment  
- 停用：不再生成新任务，**保留**历史任务与证据  
- 第一期：内置官方包，无远程任意代码包  

### 10.4 MVP 包

建议 `junior-math`：至少 3 个真实任务模板 + 基础知识节点 + 错因分类。

### 10.5 内容资产字段（与 §8 对齐）

| 文件 | 必须能支撑 |
|---|---|
| `knowledge-graph.json` | `knowledge_node_id`、先修、可观测指标 |
| `task-templates.json` | 场景题干、步骤、绑定节点、`input_schema`、`answer_key`/`scoring_rubric`、`hint_ladder` |
| `error-taxonomy.json` | `error_code`、反馈话术、建议策略 |
| `demo/` | 样例 TaskItem 与演示标注 |

---

## 11. APIs / Tool Surface（概念）

| 接口 | 作用 |
|---|---|
| `edu_initialize_student_twin` | 创建并确认最小孪生 |
| `edu_generate_learning_plan` | 生成阶段/周计划 |
| `edu_schedule_task` | 创建/调整任务 |
| `edu_start_proactive_session` | 主动沟通 |
| `edu_enqueue_interaction` | 推入弹窗并等待 |
| `edu_submit_interaction` | 回传弹窗结果 |
| `edu_submit_offline_feedback` | 线下反馈 |
| `edu_inventory_learning_materials` | 盘点已有卷子/教辅/作业来源 |
| `edu_propose_material_plan` | 建议用现有材料页码或可选补购（可拒） |
| `edu_submit_homework_photo` | 提交作业/错题照片证据 |
| `edu_diagnose_homework_evidence` | 对照照片+卡点做错因诊断 |
| `edu_grade_and_diagnose` | 对照 answer_key/rubric 评分并映射错因 |
| `edu_mark_knowledge_invoked` | 记录知识点显式/隐式调用 |
| `edu_update_mastery_observation` | 更新知识点掌握观测（分层、可更正） |
| `edu_analyze_growth` | 成长洞察 |
| `edu_propose_intervention` | 干预选项 |
| `edu_confirm_plan_adjustment` | 确认写回计划 |
| `edu_manage_consent` | 授权/撤回/导出/删除 |
| `edu_route_event` | L0/L1/L2 路由 |
| `edu_enqueue_orchestration_job` | 创建 L2 job |
| `edu_get_orchestration_job` | 查询 job |
| `edu_cancel_orchestration_job` | 取消/supersede |
| `edu_refresh_twin_summary_cache` | 刷新摘要 |
| `edu_render_template_message` | 本地模板话术 |
| `edu_list_subject_packs` | 列包 |
| `edu_enable_subject_pack` | 启用包 |
| `edu_disable_subject_pack` | 停用包 |
| `edu_get_subject_pack_context` | 取包最小上下文 |
| `edu_ingest_posture_observation` | 接收手机端姿态类别（校验授权与有效期） |
| `edu_propose_plan_adjustment` | 基于孪生快照生成动态计划建议 |

---

## 12. Data Events

关键事件（节选）：

```text
goal_confirmed
plan_generated
task_scheduled
proactive_message_sent
student_response_received
offline_feedback_submitted
learning_evidence_added
state_self_reported
state_observation_recorded
state_snapshot_calculated
twin_state_inference_created
twin_correction_submitted
twin_updated
intervention_proposed
plan_adjustment_confirmed
plan_adjustment_proposed
plan_adjustment_rejected
posture_observation_recorded
posture_reminder_shown
posture_consent_granted / posture_consent_withdrawn
weekly_review_completed
consent_granted / consent_withdrawn
voice_transcription_confirmed / voice_raw_audio_deleted
orchestration_job_enqueued / completed / failed / cancelled
path_degraded_to_l1
```

---

## 13. Frontend Modules（建议）

```text
src/components/education/
  StudentTwinPanel.tsx
  GoalOnboarding.tsx
  LearningPlanBoard.tsx
  TaskTimeline.tsx
  EducationModalHost.tsx
  modals/*
  OfflineFeedbackForm.tsx
  GrowthReviewPanel.tsx
  InterventionProposal.tsx
  ConsentAndPrivacyPanel.tsx
  OrchestrationJobBanner.tsx
  AwaitingModalBanner.tsx
```

与 CN-Codex 对齐：

- `ApprovalModal` / `request_user_input` → 等待范式蓝本  
- `Sidebar` → 任务入口  
- `ChatMessage` / 回显卡 → 解释与提交回显  
- 会话写盘：高频事件先写教育账本，再按需摘要进会话  

---

## 14. Security, Privacy & Compliance

1. 最小采集、用途限定、权限隔离  
2. 监护人分级授权；可撤回  
3. 学生可查看/更正/导出/删除授权范围数据  
4. 禁止身份人脸库、微表情情绪识别、声纹人格推断进入状态接口  
5. 语音：仅转写文本可用；原始音频最短必要保留后删除并审计  
6. 姿态：默认关闭；仅类别标签；默认不保留原视频；不得用于惩罚/告状  
7. 高风险内容：停止常规教育编排，提示现实支持渠道  
8. 输出定位为辅助建议，不替代专业教育/心理/医疗决策  
9. 第三方模型/API 必须披露范围、费用假设与可替代性  
10. 动态计划高影响变更必须学生确认，禁止静默改计划  

详见 `04-数据来源与合规说明.md`。

---

## 15. Acceptance Criteria（可测试）

1. 未提交结构化反馈时，不得假设完成或自动推进  
2. “今天暂停”后当日不得继续任务催促  
3. 每次计划调整可回溯事件/反馈/规则并展示解释  
4. 学生更正状态后，后续沟通使用新状态；旧快照 `superseded`  
5. 撤回语音/附件授权后，前后端停止新采集  
6. 连续困难干预至少提供一个非惩罚、可拒绝选项  
7. 高风险表述不得输出诊断或替代专业救助结论  
8. 跳过自评不得写入默认负向状态  
9. 改期/未登录/错误增多不得生成 `motivation_affect` 观测  
10. 生物特征类字段入状态接口必须拒绝并审计  
11. 关键点击走 L0/L1 即时反馈，不得空白等待 L2  
12. 到点提醒可不启动完整五 Agent 串行  
13. 同一 `source_event_id` 不重复创建同类 job  
14. L2 失败降级 L1 并记事件  
15. Agent 上下文仅为 `twin_summary_cache` + 最近证据窗口  
16. 未启用学科包不得生成该学科新计划/任务  
17. 正式 checkpoint 弹窗提交率目标 100%（MVP 核心路径）  
18. 无有效孪生覆盖度时，不得自动生成高影响重排；仅允许骨架计划或轻问  
19. 动态计划调整必须绑定 `twin_snapshot_ref` 与证据 ID，并可被拒绝  
20. 姿态能力默认关；未授权时 `edu_ingest_posture_observation` 必须拒绝  
21. 姿态事件不得触发惩罚、排名或默认通知家长纪律处理  
22. 姿态原始媒体默认 `raw_media_retained=false`，测试需断言  
23. 采集问前必须有用途说明；单轮仅一主问（PSY-01/02）  
24. 跳过不得写入负向默认状态（PSY-03）  
25. 禁用评估/羞辱措辞词表命中时，模板不得上线（PSY-04）  
26. 学生更正后旧状态不得继续驱动计划（PSY-06）  
27. 默认学习任务 `mode=offline_paper` 或等价，且绑定 `material_ref`（无材料时走盘点流）  
28. 不得提供“连续屏上刷系统题库”作为主学习路径  
29. 作业照片需授权；默认最短必要保留，支持删除原图仅留结构化证据  
30. AI 诊断须标 `confidence`；低置信必须请求学生确认，不得当确定掌握  
31. 材料购买建议可一键拒绝，且不得重复施压  
32. 单次正确/单次拍照不错不得直接升 `demonstrated`；`coverage` 不足不得自动高影响改计划  

---

## 16. Phased Delivery

### Phase 1 · MVP（复赛）

- [ ] 孪生初始化与确认  
- [ ] 孪生多次生长可见（证据列表/覆盖度）  
- [ ] 首周计划与任务  
- [ ] 动态计划建议—确认—回写  
- [ ] 主动启动/反馈/改期/暂停（L0+L1）  
- [ ] 线下反馈  
- [ ] L1 规则洞察 + 干预确认  
- [ ] Modal：choice/form/text  
- [ ] orchestration_job 基础队列  
- [ ] junior-math 包 + 3 任务 + Demo 数据  
- [ ] （加分）手机姿态最小闭环：授权→类别→提醒→可选缩时  

### Phase 2

- [ ] 语音填窗、TTS 可选  
- [ ] 更完整知识图谱与错因  
- [ ] 周/月回顾  
- [ ] 教师/监护人授权视图  
- [ ] 姿态端侧模型稳定化与多端孪生同步  

### Phase 3

- [ ] 多学科包  
- [ ] 更丰富线下证据（仍可选最小化）  
- [ ] 教师协同与完整审计演练证明  

---

## 17. Risks & Mitigations

| 风险 | 应对 |
|---|---|
| 画像不准 | 来源可见、置信度、更正、过期 |
| 姿态误检 | 低置信忽略、可关、不惩罚、不唯一依据 |
| 计划过频调整 | 合并窗口、学生确认、覆盖度门槛 |
| 打扰过多 | 频控、静默、免打扰、拒绝路径 |
| 线下反馈失真 | 多源证据、低置信、不单次定性 |
| 模型幻觉 | 建议化、规则校验、学生确认 |
| 未成年人数据 | 最小采集、授权、删除、审计 |
| Agent 慢 | L0/L1/L2、异步、降级、预算 |
| 边界模糊 | 明确辅助定位与转介 |

---

## 18. Open Questions（实现前确认）

1. 正式作答是否强制仅弹窗提交（推荐是）  
2. 弹窗默认自动弹出 vs 手动“开始作答”  
3. MVP 语音是否同迭代  
4. MVP 是否包含手机姿态最小 Demo（推荐作为加分项同期）  
5. 姿态推理端侧模型选型与性能基线  
6. 默认模型提供方与离线降级策略  
7. 数据默认本地路径与导出格式  

---

## 19. Traceability to Competition Rubric

| 评审维度 | SPEC 落点 |
|---|---|
| 行业场景价值 25% | §1–3 用户与闭环 |
| Agent 能力与任务闭环 25% | §7–9 |
| 产品体验与 Demo 完成度 20% | §4.2、§6、§16 |
| 技术实现深度 15% | §4、§5、§11–13 |
| 安全合规可追溯 10% | §5.4、§14、§15 |
| 开放/复用 5% | 学科包协议、交互协议、示例数据 |

---

## 20. Change Log

| 版本 | 日期 | 说明 |
|---|---|---|
| 1.0 | 2026-07-27 | 初赛 SPEC：整合需求文档与教学引擎设计，对齐 GOAI 初赛材料 |
| 1.1 | 2026-07-27 | 强化孪生多次生长、动态计划对比、手机姿态可选能力与验收 |
| 1.2 | 2026-07-27 | 增加心理学友好采集规格（自主/能力/关联、话术与跳过规则） |
| 1.3 | 2026-07-27 | 补齐学科内容闭环：题目/答案/输入/解析/掌握证据契约与验收 |
| 1.4 | 2026-07-27 | 转向纸质主学：材料盘点、作业拍照、屏时克制；内置题库非主路径 |
