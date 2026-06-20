# Novel-Claude CLI 命令文档

## 概述

Novel-Claude V3 是一个微内核插件化网文生成系统，提供以下核心功能：
- 世界观初始化
- 分卷大纲规划
- 多智能体场景写作
- RAG 动态记忆管理
- Batch API 大规模并发码字

---

## 全局命令

### `python cli.py --help`

查看所有可用命令。

---

## 核心写作流程

### 1. 初始化世界观

```bash
uv run python cli.py init "核心创意描述"
```

**功能：** 根据一句核心创意，生成完整的的世界观设定，包括：
- 势力分布 (`factions.json`)
- 战力体系 (`power_levels.json`)
- 主要角色 (`main_characters.json`)
- 世界规则 (`world_rules.json`)
- 可读手册 (`world_manual.md`)

**参数：**
- `logline`: 一句话描述小说核心创意

---

### 2. 生成卷大纲

#### 2.1 生成全10卷宏观大纲

```bash
uv run python cli.py plan
```

**功能：** 生成全书10卷的宏观框架，包含核心冲突和战力上限。

#### 2.2 生成指定卷的章节细纲

```bash
uv run python cli.py plan --volume 1
```

**功能：** 为指定卷生成50章的微观打点(Scene Beats)，每章自动归一化为5000字。

**参数：**
- `--volume`: 指定卷号(1-10)

---

### 3. 实时码字（标准API）

```bash
uv run python cli.py write --volume 1 --chapters "1-10"
```

**功能：** 使用标准Chat API进行实时码字，支持流式输出和动态RAG记忆更新。自动检查Checkpoint，跳过已生成章节。

**参数：**
- `--volume`: 目标卷号
- `--chapters`: 章节范围，格式如 "1-5" 或单章 "1"

---

## Batch API 批量写作

Batch API 使用智谱GLM-4模型，可享受5折优惠，适合大规模并发码字。

### 3.1 构建JSONL请求文件

```bash
uv run python cli.py batch-build --volume 1 --chapters "1-50"
```

**功能：** 读取指定章节的Beats数据，注入RAG记忆，打包成智谱Batch API格式。生成的请求文件保存在配置的 `BATCH_DIR` 目录中。

**参数：**
- `--volume`: 目标卷号
- `--chapters`: 章节范围

---

### 3.2 提交异步任务

```bash
uv run python cli.py batch-submit <jsonl_path>
```

**功能：** 上传JSONL文件并提交异步任务。返回Batch ID，请妥善保存用于后续同步。

**参数：**
- `jsonl_path`: 第一步生成的.jsonl文件路径

---

### 3.3 同步合并成稿

```bash
uv run python cli.py batch-sync <batch_id>
```

**功能：** 轮询任务状态，完成后自动下载结果、调用Editor Agent平滑合并场景、自动更新RAG向量记忆库。

**特点：**
- 自动重试与轮询（每分钟一次）
- 任务完成后自动下载结果文件
- 自动调用Editor Agent进行场景合并
- 自动更新RAG向量记忆库

**参数：**
- `batch_id`: 第二步返回的任务ID

---

## 工具命令

### RAG记忆重建

```bash
uv run python cli.py reindex --volume 1 --chapters "1-10"
```

**功能：** 手动将已生成的成稿重新导入RAG记忆库。如果由于网络报错等原因导致章节没有成功入库，使用此命令通过Aho-Corasick自动机重新提取实体并入库。

**参数：**
- `--volume`: 目标卷号
- `--chapters`: 章节范围，格式如 "1-5"

---

### AI多文件审阅修改

```bash
uv run python cli.py review -f "factions.json" -f "power_levels.json" -i "将金丹期统一改为结丹期"
```

**功能：** 多文件AI辅助审阅并自动修改文件内容。

**参数：**
- `-f, --files`: 需要修改的目标文件，支持多次使用指定多个文件
- `-i, --instruction`: 修改意见与指令

---

## 插件管理

### 查看插件列表

```bash
uv run python cli.py skills list
```

**功能：** 列出所有已加载的V3插件和skills/目录下可发现的插件文件夹。显示：
- 🟢 已加载的插件
- 🔴 已禁用的插件
- ⚠️ 未成功加载的插件（可能缺少skill.py或有报错）

---

### 启用插件

```bash
uv run python cli.py skills enable <name>
```

**功能：** 启用指定插件。

**参数：**
- `name`: skills/目录下的文件夹名（如 ext_gold_finger）

---

### 禁用插件

```bash
uv run python cli.py skills disable <name>
```

**功能：** 禁用指定插件。

**参数：**
- `name`: skills/目录下的文件夹名

---

### 热重载插件

```bash
# 重载全部插件
uv run python cli.py skills reload

# 重载指定插件
uv run python cli.py skills reload <name>
```

**功能：** 热重载插件，不指定NAME则重载全部。

**参数：**
- `name`: 可选，skills/目录下的文件夹名

---

### 自动生成插件

```bash
uv run python cli.py skills build "帮我写一个Skill，在每次生成前注入一句主角很帅"
```

**功能：** 让SkillBuilder Agent根据自然语言需求自动生成插件代码。

**参数：**
- `request`: 用自然语言描述你想要的插件功能

---

## 工作流程总结

```
1. init          -> 初始化世界观
2. plan          -> 生成宏观大纲（全10卷）
3. plan --volume -> 生成指定卷章节细纲
4. write         -> 实时码字（标准API）
   或
4. batch-build   -> 构建Batch请求
5. batch-submit  -> 提交Batch任务
6. batch-sync    -> 同步合并成稿
```

---

## 环境配置

系统配置通过 `.env` 文件管理，主要配置项：

| 配置项 | 说明 |
|--------|------|
| `ANTHROPIC_BASE_URL` | API地址 |
| `ANTHROPIC_API_KEY` | API密钥 |
| `MODEL_ID` | 主模型 |
| `FLASH_MODEL_ID` | 备用快速模型 |
| `NOVEL_NAME` | 当前项目名称 |
| `CHAPTER_TARGET_WORDS` | 每章目标字数（默认5000） |
| `TOTAL_VOLUMES` | 总卷数（默认10） |
| `CHAPTERS_PER_VOLUME` | 每卷章节数（默认50） |
| `LLM_PROVIDER` | LLM提供商 |

---

## 内置插件（skills/）

| 插件 | 说明 |
|------|------|
| `ext_gold_finger` | 金手指插件 |
| `ext_world_highlight_system` | 世界高亮系统 |
| `ext_handsome_protagonist` | 主角光环插件 |
| `core_memory_rag` | 核心记忆RAG系统 |