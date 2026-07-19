# 统一 Share Origin + Workflow/Skill/Knowledge 共享版本设计

> 状态：已确认并进入实现  
> 日期：2026-03-22  
> 决策：**方案 A — Unified Share Origin layer**  
> 范围：Workflow / Skill / Knowledge 导出副本的局域网共享、安装、版本区分与再次拉取

---

## 1. 目标

把 **Workflow / Skill / Knowledge 导出副本** 统一到与现有 Skill/Knowledge 类似的局域网共享体验，并保证接收方能区分：

1. **原始共享版本**（远端来源内容哈希 `originContentHash`）
2. **本机已安装下载版本**（上次成功拉取哈希 `installedContentHash`）
3. **本机已修改**（安装后被用户编辑 → 更新时冲突）

接收方必须能：

- 看到资源“来自谁”
- 判断“已是最新 / 有更新 / 本地已修改”
- 再次拉取最新共享版本，且不与本机原创资源混淆

---

## 2. 已锁定决策

| 决策 | 选择 |
|------|------|
| 架构 | 方案 A：统一 Share Origin 层 |
| 范围 | Workflow + Skill + Knowledge 导出副本 |
| 版本 | **内容哈希（SHA-256）**，不用手写 semver |
| 安装命名 | **保留原名** + sidecar 元数据（不改名为副本） |
| Sidecar | `origin.share.json`（知识库可用 origins 目录） |
| 本机原创 | **不写** origin 文件 |
| 同名冲突 | 提示覆盖/取消；若 `localModified` 需二次确认 |
| 更新 UX | 已是最新 / 有更新 / 本地已修改；冲突可选：保留本地 / 覆盖为远端 / 另存副本 |

非目标（本期不做）：

- 方案 C 中心化 share store
- 每类资源独立版本号体系
- 强制改名安装
- 跨公网中继

---

## 3. Share Origin 元数据

### 3.1 `origin.share.json`（schemaVersion = 1）

```json
{
  "schemaVersion": 1,
  "kind": "workflow",
  "resourceId": "deploy-checklist",
  "title": "发版检查清单",
  "sourceShareId": "wshare_...",
  "sourceHostNodeId": "node_A",
  "sourceHostDisplayName": "Alice-PC",
  "sourceGroupId": null,
  "originContentHash": "sha256:abcd...",
  "installedContentHash": "sha256:abcd...",
  "installedAt": 1710000000,
  "lastPulledAt": 1710000000,
  "lastCheckedAt": 1710000100,
  "localModified": false
}
```

字段语义：

| 字段 | 含义 |
|------|------|
| `originContentHash` | 首次安装时远端共享版本哈希；用于“相对原始来源”对照 |
| `installedContentHash` | 最近一次成功安装/拉取的内容哈希 |
| `localModified` | 当前本地内容哈希 ≠ `installedContentHash` |
| `lastCheckedAt` | 最近一次与远端目录/哈希对照时间 |

> 实现约定：读取 origin 时实时重算当前内容哈希，刷新 `localModified`（可不立即回写磁盘，仅在 pull/install 时持久化）。

### 3.2 磁盘布局

```text
codey/skills/<id>/
  SKILL.md
  origin.share.json          # 仅共享下载副本存在

codey/workflows/<name>/
  workflow.json
  SKILL.md
  scripts/...
  scripts-manifest.json      # 可选
  origin.share.json          # 仅共享下载副本存在

codey/memories/knowledge/
  docs/<doc-id>.md
  origins/<doc-id>.origin.share.json   # 知识导出副本
```

本机原创资源：无 origin 文件 → UI 不显示“来自 xxx / 检查更新”。

### 3.3 内容哈希输入

统一输出：`sha256:` + 小写 hex。

| kind | 哈希输入 |
|------|----------|
| skill | `SKILL.md` 原始 UTF-8 字节 |
| workflow | `workflow.json` 规范化（pretty 序列化后的稳定文本） + 脚本快照：按相对路径字典序拼接 `path\\0content\\0` |
| knowledge | 文档全文（含 frontmatter）UTF-8 字节 |

Workflow 哈希不包含 `origin.share.json` 自身，避免元数据污染版本。

---

## 4. 更新状态机

设：

- `remoteHash` = 当前远端 offer 的 `contentHash`
- `installedHash` = origin.installedContentHash
- `currentHash` = 本地当前内容哈希
- `localModified` = `currentHash != installedHash`

| 条件 | 状态 | 动作 |
|------|------|------|
| 无 origin | `local_original` 或未安装 | 安装时若同名冲突则确认覆盖 |
| 有 origin 且 `remoteHash == installedHash` 且 !localModified | `up_to_date` | 显示“已是最新” |
| 有 origin 且 `remoteHash != installedHash` 且 !localModified | `has_update` | 一键拉取覆盖 |
| 有 origin 且 localModified 且 `remoteHash != installedHash` | `conflict` | 保留本地 / 覆盖远端 / 另存副本 |
| 有 origin 且 localModified 且 `remoteHash == installedHash` | `local_modified` | 仅提示本地已改，无远端更新 |

安装/覆盖规则：

1. 目标不存在 → 直接安装并写 origin
2. 目标存在且无 origin → 提示将覆盖本机原创；确认后覆盖并写 origin（该资源变为“来自共享”）
3. 目标存在且有 origin 且 `localModified` → 二次确认后才允许 `forceOverwrite`
4. 选择“另存副本” → 使用 `name-from-<host>` 或 `name-copy-N` 安装（本期前端可选；后端预留 `installAs`）

---

## 5. 协议扩展

沿用现有长度前缀 JSON 帧；新增字段一律 `#[serde(default)]`，兼容旧节点。

### 5.1 Skill（增量）

`SharedSkillOffer` 增加：

```json
{
  "contentHash": "sha256:...",
  "updatedAt": 1710000000
}
```

`SkillFetchResponse` 增加可选 `contentHash`。

### 5.2 Workflow（新增，镜像 Skill）

WireMessage：

- `WorkflowShareAdvert { offers }`
- `WorkflowShareQuery {}`
- `WorkflowFetchRequest { request_id, share_id }`
- `WorkflowFetchResponse { request_id, name?, title?, content_hash?, workflow_json?, skill_md?, scripts?, error? }`

`SharedWorkflowOffer`：

```json
{
  "shareId": "wshare_...",
  "hostNodeId": "node_A",
  "hostDisplayName": "Alice-PC",
  "workflowName": "deploy-checklist",
  "title": "发版检查清单",
  "description": "...",
  "nodeCount": 3,
  "contentHash": "sha256:...",
  "groupId": null,
  "online": true
}
```

`SharedWorkflowPayload`：

```json
{
  "shareId": "wshare_...",
  "workflowName": "deploy-checklist",
  "title": "...",
  "description": "...",
  "contentHash": "sha256:...",
  "workflowJson": "{...}",
  "skillMd": "...",
  "scripts": [
    { "path": "scripts/tools/build.py", "content": "..." }
  ],
  "scriptsManifestJson": "[...]" 
}
```

体积限制（首期）：

- 单帧仍受 `MAX_FRAME_BYTES`（1 MiB）约束
- workflow 正文 + scripts 总字符上限：800_000
- 单脚本最大：200_000 字符
- 超限拒绝共享/拉取并提示

### 5.3 Knowledge 导出副本（增量）

保持现有在线检索/按需读；新增可选能力：

- 前端“导出到本地”调用现有 fetch 后写入 `docs/` + `origins/*.origin.share.json`
- offer/doc meta 可附 `contentHash`（若可得）以便后续检查更新

本期优先打通 **Workflow 端到端** 与 **Skill origin/更新**；Knowledge 导出 origin 提供后端 helper，UI 可后续接。

---

## 6. Tauri 命令

### 6.1 Workflow

| 命令 | 作用 |
|------|------|
| `lan_collab_share_workflow` | 共享本机 workflow |
| `lan_collab_unshare_workflow` | 取消共享 |
| `lan_collab_list_local_shared_workflows` | 本机已共享 |
| `lan_collab_list_remote_shared_workflows` | 远端可用 |
| `lan_collab_install_remote_workflow` | 安装/覆盖拉取 |
| `lan_collab_list_share_origins` | 列出本机已安装共享副本状态（可选 kind 过滤） |
| `lan_collab_inspect_share_origin` | 查看单个资源 origin + 实时 localModified |

`install_remote_workflow` 参数：

```ts
{
  hostNodeId: string;
  shareId: string;
  overwrite?: boolean;
  forceOverwrite?: boolean; // localModified 时必须 true
  installAs?: string | null; // 另存副本名
}
```

### 6.2 Skill（行为增强）

`lan_collab_install_remote_skill` 增加 `forceOverwrite`；安装成功后写 origin。

### 6.3 状态

`LanCollabStatus` 增加：

- `localSharedWorkflows`
- `remoteSharedWorkflows`

事件：

- `lan-collab-workflow-share`

---

## 7. 模块划分

```text
src-tauri/src/lan_collab/
  share_origin.rs      # origin 读写、哈希、更新状态
  skill_share.rs       # 既有 + contentHash + origin 写入
  workflow_share.rs    # 新增
  knowledge_share.rs   # helper：export 本地副本 + origin（可后置）
  protocol.rs / types.rs / runtime.rs / commands.rs
```

前端：

```text
src/components/settings/LanSkillShareSection.tsx     # 更新徽章 / 拉取
src/components/settings/LanWorkflowShareSection.tsx  # 新增
src/components/settings/WorkflowsPanel.tsx           # 挂载共享区 + 来源徽章
src/api/lanCollab.ts                                 # 类型与命令
```

---

## 8. 前端 UX

### 8.1 共享区（对齐 LanSkillShareSection）

- 选择本机资源 → 共享
- 本机已共享列表 → 取消
- 局域网可用列表 → 安装 / 更新

### 8.2 列表徽章

- `来自 Alice · 已是最新`
- `来自 Alice · 有更新`
- `来自 Alice · 本地已修改`
- 本机原创：无“来自”文案

### 8.3 冲突对话框文案

> 本地已修改，且远端有新版本。  
> - 保留本地  
> - 用远端覆盖  
> - 另存为副本（可选）

---

## 9. 实现顺序

1. `share_origin.rs` + 单元测试（哈希 / 读写 / localModified）
2. `workflow_share.rs` + protocol/runtime/commands 端到端
3. Skill：offer 带 hash、安装写 origin、forceOverwrite
4. 前端 Workflow 共享区 + Skill 更新状态
5. Knowledge export origin helper（可与 4 并行）
6. 集成测试：双节点共享/安装/改本地/再拉更新

---

## 10. 验收标准

- [ ] 共享方共享 Workflow 后，对端可见 offer 且含 contentHash
- [ ] 对端安装后生成 `codey/workflows/<name>/origin.share.json`
- [ ] 本机原创 Workflow 无 origin 文件
- [ ] 远端未变时显示“已是最新”
- [ ] 远端内容变化后显示“有更新”，一键拉取后哈希对齐
- [ ] 本地修改后显示“本地已修改”；强行覆盖需 forceOverwrite
- [ ] Skill 安装同样写入 origin，可再次识别更新
- [ ] 旧节点无 contentHash 字段时不崩溃（default 空 → 仅支持安装，不支持可靠更新检测）

---

## 11. 风险

| 风险 | 处理 |
|------|------|
| Workflow + scripts 超过 1MiB 帧 | 共享前校验；超限报错；后续可分片 |
| 同名覆盖误伤本机原创 | 明确确认；UI 区分原创/共享副本 |
| 规范化 JSON 导致哈希漂移 | 统一用 `serde_json::to_string_pretty` 基于解析后的 WorkflowDef |
| 旧协议兼容 | 新字段 default；缺 hash 时降级为“无法检测更新” |

---

**文档结束。** 按第 9 节开始编码。
