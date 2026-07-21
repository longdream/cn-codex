# 小程序体系 + 数据库灵活权限 改造方案（待确认）

> 状态：**草案，待产品/开发确认后实施**  
> 创建日期：2026-07-21  
> 目标：  
> 1）数据库权限改为**可灵活配置、供 AI 使用**，禁止写死业务逻辑；  
> 2）**完整移除** `build_entry_form` / `save_form_data` 表单入库主链路；  
> 3）用「设置中用自然语言生成小程序 + 挂数据库 + 主链路 AI 以 MCP 方式调用」替代原结构化入库形态。

---

## 1. 背景与现状

### 1.1 当前表单入库链路（需移除）

当前实现于提交 `73730fd`（`temp`）引入，主路径如下：

```
用户自然语言
  → LLM 判断库/表 + 提取 known_values
  → build_entry_form
  → 前端 ApprovalModal 渲染表单
  → 用户补全并保存
  → save_form_data（校验 + writeData 权限 + INSERT）
```

主要代码位置：

| 区域 | 路径 | 说明 |
|------|------|------|
| 后端核心 | `src-tauri/src/smartbrain/form_entry.rs` | **整文件删除**（该提交新增） |
| 工具注册/执行 | `src-tauri/src/tool_executor.rs` | 去掉 `build_entry_form` / `save_form_data` schema、dispatch、exec、相关测试 |
| Prompt | `src-tauri/src/agent/prompt_context.rs`、`agent.rs` | 去掉结构化入库工具说明 |
| 前端弹窗 | `src/components/approval/ApprovalModal.tsx` | 回退表单入库 UI（对照 git 历史，勿误删其他审批能力） |
| 事件/Store | `src/hooks/useTauriEvents.ts`、`src/stores/appStore.ts` | 去掉 build_entry_form 文案/状态分支 |
| 测试 SQL | `docs/sql/sb_entry_form_type_test*.sql` | 随功能删除或归档 |
| 模块导出 | `src-tauri/src/smartbrain/mod.rs` | 去掉 `form_entry` 模块 |

**重要：回退边界（对照 git，禁止改错）**

- 以 `73730fd` 中与 **entry form** 相关的 diff 为回退主参考。
- **不要**误回退同提交中无关能力，例如：
  - Git 增强（`commands/git.rs`、`GitPanel`、`api/git.ts`）
  - adapter 其他改动
  - 数据库连接/列表刷新等非 form 的 `SmartbrainDatabasePanel` 改进（需按 diff 细拆）
- 当前工作区对 `tool_executor.rs` 可能有额外未提交修改，实施时需先 `git diff` 再精准剥离 form 相关块。

### 1.2 当前数据库权限模型（需增强）

现状（库级三开关）：

```ts
permissions: {
  readSchema: boolean;
  readData: boolean;
  writeData: boolean;
}
```

- 配置 UI：`SmartbrainDatabasePanel.tsx`
- 执行校验：`db_query.rs` → `validate_sql_against_permissions`
- AI 可见：`prompt_context.rs` 注入「权限=readSchema, readData, writeData」
- 全局策略：`SmartbrainDatabaseSettingsPanel`（行数、超时、deny DDL/DROP 等）

问题：

- 权限粒度粗，仅库级布尔，**无法**表达「仅某表可读 / 某表可写 / 禁止 DELETE」等灵活策略；
- 规则 Markdown 偏提示，**未结构化强制执行**；
- 与「权限给 AI 用、不能写死」的目标不匹配。

### 1.3 可复用基础设施

| 能力 | 现状 | 复用方式 |
|------|------|----------|
| 内置 Node | `codey/node/`（node/npm/npx） | 小程序 MCP 服务启动命令 |
| 插件 MCP | `codey/plugins/*/ .mcp.json` | 小程序生成物对齐该形态 |
| 附件下拉 | `ChatInput` AttachMenu：`skill/plugin/mcp` | 增加「小程序」入口 |
| 右栏 | `RightPanel`：browser / project / terminal / git / lan | 增加「小程序」分类 Tab |
| 端口分配 | 已有 `127.0.0.1:0` 绑定模式 | 小程序 HTTP 页自动端口 |
| SQL 工具 | `smartbrain_sql_query` | 小程序后端/AI 仍可走统一 DB 执行与权限 |

---

## 2. 目标架构（改造后）

### 2.1 总体流程

```
【设置】用户用自然语言描述业务
        + 选择挂载的数据库
        + 填写小程序名称 / 英文名(slug)
        ↓
   【主链路 AI】在设置场景中触发生成任务（与对话主链路同一套 Agent/工具能力）
        - 不是独立旁路 LLM 接口，而是走主链路：读 schema、写文件、装依赖、自检
        - 强制产出 **Node.js 小程序包**（MCP-like）
        - 包内容：
            · MCP tools（多方法：open_page / submit / query ...）—— Node 实现
            · 预置 Web 页面（录入/列表/详情等）—— 由 Node 静态服务或同包前端资源提供
            · package.json / 依赖清单（仅 Node 生态）
            · 包元数据、权限声明、挂载 databaseId
        - 运行时统一使用内置 `codey/node`（node / npm / npx），禁止 Python/其他运行时作为小程序主体
        ↓
【右侧项目栏】「小程序」分类中出现该小程序
        - 可启动/停止/打开页面/查看日志
        ↓
【对话】附件下拉增加「小程序」
        - 用户点选 → 注入上下文，主链路 AI 可调用该小程序 MCP 方法
        ↓
AI 调用小程序方法（如 open_page:contract_entry）
        → 弹出/打开预生成页面，用户输入
        → 小程序接口处理并返回结构化结果
        → AI 判断成功与否，继续对话
```

### 2.2 与旧链路对比

| 项 | 旧：build_entry_form | 新：小程序 MCP |
|----|----------------------|----------------|
| 表单来源 | 运行时按 schema 动态拼 Form JSON | 生成时固化为页面 + tools |
| 用户交互 | ApprovalModal 通用表单 | 小程序自有页面（窗口/内嵌） |
| 写入方式 | `save_form_data` 固定 INSERT | 小程序方法内逻辑 + 受权限约束的 SQL/API |
| AI 工具 | 内置 2 个固定 tool | 每个小程序独立 MCP 多方法 |
| 扩展性 | 仅入库 | 查询、录入、审批、报表等可扩展 |

### 2.3 核心原则

1. **权限可配置、可注入 AI、可执行时强制**，业务规则不写死在代码分支里。  
2. **小程序 = MCP 形态包**：必须暴露 methods；UI 是包内资源，不是主应用硬编码表单。  
3. **主链路 AI 只通过 MCP/工具协议调用小程序**；小程序返回结果给 AI 判定，不绕过对话。  
4. **小程序必须用 Node.js 开发与运行**（MCP server + 页面服务均为 Node）；使用内置 `codey/node`；依赖按生成规则 `npm install`；端口自动分配。  
5. **小程序代码由主链路 AI 生成**：设置页发起生成时进入主链路 Agent 流程（可带专用 system 约束与工作目录），利用写文件/读 schema/shell 等工具落盘，而不是单独封装一个「只返回文本」的旁路 LLM。  
6. **权限模型不做旧三开关兼容**：直接以灵活策略配置为准，移除/替换原 `readSchema/readData/writeData` 库级三勾选 UX 与依赖它的写死逻辑。  
7. **移除旧 form 链路时严格对照 git**，避免误伤 Git/数据库连接等无关改动。

---

## 3. 需求拆解

### 3.1 数据库权限增强（灵活配置 + AI 可用）

#### 3.1.1 权限模型（建议）

**不做旧库级三开关兼容。** 权限配置直接采用可灵活编排的**结构化策略**（供 AI 读取 + 运行时强制执行），示例 JSON：

```json
{
  "version": 2,
  "defaults": {
    "readSchema": true,
    "readData": true,
    "writeData": false,
    "allowDelete": false,
    "allowUpdate": false,
    "allowInsert": false,
    "allowDdl": false
  },
  "tables": {
    "contract": {
      "read": true,
      "insert": true,
      "update": true,
      "delete": false,
      "columns": {
        "id": { "read": true, "write": false },
        "amount": { "read": true, "write": true }
      }
    }
  },
  "rules": [
    {
      "id": "no-drop",
      "effect": "deny",
      "match": { "sqlKinds": ["ddl", "drop"] },
      "message": "禁止 DDL/删库删表"
    }
  ],
  "aiNotes": "该库用于合同业务；录入优先走小程序 contract_entry。"
}
```

说明：

- **defaults**：未单独声明表时的默认能力（可配置，非写死业务）。  
- **tables / columns**：可选细粒度覆盖。  
- **rules**：通用规则列表（allow/deny + 匹配条件），**运行时解释执行**，而非代码 if-else 写死业务名。  
- **aiNotes**：自然语言备注，注入 prompt，供 AI 理解边界。  
- **不兼容旧模型**：移除 `permissions.readSchema / readData / writeData` 三勾选作为主配置形态；实现时直接切换到新策略结构。若本地已存旧 JSON，加载时可**一次性映射后丢弃旧字段**（仅迁移数据，不在 UI/API 层保留双轨兼容）。

#### 3.1.2 设置 UI

- 位置：本地知识库 → 数据库编辑页（**替换**原权限三勾选区域）。  
- 能力：
  - 默认策略编辑（defaults：读结构/读数据/插入/更新/删除/DDL 等可选项）
  - 表级策略：从表结构同步后勾选/编辑（read/insert/update/delete）
  - 规则列表编辑器（allow/deny + 匹配条件；可 Markdown 辅助说明 + JSON 结构化）
  - 「同步表结构」：读取 schema 后生成可选表白名单（**仅生成配置，不写死业务**）
  - 「AI 可读说明」：`aiNotes` 文本框
- 全局安全策略（deny DDL 等）继续保留，与库策略取**更严交集**。

#### 3.1.3 执行层

- `smartbrain_sql_query` / 小程序内部 SQL 通道：统一走 `validate_sql_against_permissions` 升级版。  
- 校验顺序建议：
  1. 全局 settings（denyDdl / denyDrop / timeout / rowLimit）
  2. 库 defaults
  3. 表/列覆盖
  4. rules 列表
- 失败时返回**结构化错误**（code + message），便于 AI 转述，而不是 silent skip。

#### 3.1.4 Prompt 注入

- 继续在 `prompt_context` 注入可用库列表。  
- 权限摘要改为人类+机器可读，例如：
  - `权限=readSchema,readData；表contract允许insert/update；禁止delete/ddl`
  - 附带 `aiNotes` 截断注入  
- **禁止**在 prompt 中写死「必须用 build_entry_form」；改为「业务 UI/写入优先通过已挂载小程序；SQL 必须遵守权限策略」。

---

### 3.2 移除 build_entry_form 链路

#### 3.2.1 删除/回退清单

| 动作 | 目标 |
|------|------|
| 删除 | `src-tauri/src/smartbrain/form_entry.rs` |
| 修改 | `smartbrain/mod.rs` 去掉 `pub mod form_entry` |
| 修改 | `tool_executor.rs` 去掉工具名、schema、exec、测试断言 |
| 修改 | `agent.rs` / `prompt_context.rs` 去掉 form 工具说明 |
| 回退相关 hunk | `ApprovalModal.tsx` 的 entry form 渲染/保存逻辑 |
| 回退相关 hunk | `useTauriEvents.ts`、`appStore.ts`、i18n 中 form 相关 key |
| 删除或归档 | `docs/sql/sb_entry_form_type_test*.sql` |
| 检查 | `ChatInput` / `SmartbrainDatabasePanel` 中若仅为 form 服务的增量，按 diff 剥离 |

#### 3.2.2 验收

- 工具列表在开启本地知识库时**不再**暴露 `build_entry_form` / `save_form_data`。  
- 对话不再触发通用入库表单弹窗。  
- 既有 `smartbrain_sql_query`、数据库设置、连接测试等功能正常。  
- `cargo test` / 前端相关单测无 form 残留失败。

---

### 3.3 小程序体系（新增）

#### 3.3.1 概念

- **小程序（MiniApp）**：由自然语言生成的本地业务包，形态类似 MCP Server + Web UI。  
- **必须字段**：
  - `name`：显示名称（中文可）
  - `slug` / `nameEn`：英文标识（目录名、MCP server 名、端口注册键）
  - `databaseId`：挂载的本地知识库数据库 id（必填）
  - `description`：生成时的业务描述
  - `status`：draft / generated / running / error
  - `port`：运行时分配（可为空=未启动）
  - `mcp`：stdio 启动配置（对齐 `.mcp.json`）
  - `pages`：页面清单（path、title、用途）
  - `tools`：MCP 方法清单（name、description、inputSchema）

#### 3.3.2 目录与存储

建议路径（项目级 `codey` 下）：

```
codey/miniapps/
  <slug>/
    miniapp.json          # 元数据
    package.json
    .mcp.json             # MCP 入口
    server/
      index.mjs           # MCP server（多 tools）
    web/
      index.html
      assets/
    pages/                # 预生成页面源码
    README.md             # 生成说明
  index.json              # 全量注册表（可选，或扫目录）
```

状态持久化 key 建议：`smartbrain.miniapps` 或独立 `codey/miniapps/registry.json`。

#### 3.3.3 设置页：自然语言生成

- 入口：设置 → 本地知识库 下新增子 Tab **「小程序」**，或独立设置分区（待确认，默认挂 Smartbrain 下）。  
- 创建表单字段：
  1. 中文名称 *  
  2. 英文名称/slug *（校验：`[a-z][a-z0-9_-]*`）  
  3. 挂载数据库 *（下拉已配置库）  
  4. 自然语言需求 *（多行）  
  5. 可选：是否允许写库、预置页面类型（录入/列表/查询）  
- 操作：
  - **生成**：走**主链路 Agent**（同一套工具调用能力），在小程序工作目录内写 Node.js 代码并自检  
  - **重新生成 / 增量修改**  
  - **启动 / 停止**  
  - **打开页面**  
  - **删除**

生成规则（强制）：

1. **技术栈强制 Node.js**：MCP server、页面 HTTP 服务、业务逻辑均用 Node.js（`.mjs`/`.js`）；禁止以 Python/其他语言作为小程序运行主体。  
2. **生成通道强制主链路**：设置页点击生成 = 启动一次受约束的主链路任务（专用 prompt：必须生成 Node MCP 包、挂 databaseId、写到 `codey/miniapps/<slug>`），Agent 使用 write/shell 等工具落盘，**不是**单独 HTTP 调模型只吐文本。  
3. 包结构固定为 **MCP-like**：`tools/list` 可发现多方法；入口由 `.mcp.json` 指向 `codey/node` + `server/index.mjs`。  
4. 至少包含：
   - `open_page`：打开指定页面（录入时弹窗/内嵌）  
   - `list_pages`：列出页面  
   - `get_status`：健康检查/端口/DB 绑定  
   - 业务方法：如 `submit_form`、`search`（由需求生成）  
5. 业务方法返回**统一 envelope**：

```json
{
  "ok": true,
  "code": "OK",
  "message": "合同已保存",
  "data": { "id": 123 },
  "ui": { "action": "close_page", "pageId": "contract_entry" }
}
```

6. 涉及数据库操作必须走宿主提供的安全通道（或受权限校验的本地 API），**禁止**把密码写进生成代码；连接信息用 `databaseId` 引用配置。  
7. 运行时强制 `codey/node`（`node` / `npm` / `npx`）；生成后 `npm install`（离线优先，失败再联网，可配置）。  
8. 页面服务监听 **自动分配端口**（`127.0.0.1:0` 或端口池），写入 registry。  

主链路生成示意：

```
设置页「生成」
  → 创建 codey/miniapps/<slug> 目录 + 最小 Node 脚手架
  → 启动主链路 Agent（工作目录=该目录；system 约束=Node.js MCP 小程序规范）
  → Agent：smartbrain_sql_query 读 schema / write 写 server+web / shell npm install / 自检启动
  → 注册表写入成功状态 → 右侧栏可见
```

#### 3.3.4 右侧项目栏

- `RightPanel` 增加 Tab：**小程序**（与 project / terminal / git 并列）。  
- 列表展示：名称、slug、状态（运行中/已停止）、端口、绑定库。  
- 操作：启动、停止、打开默认页、复制 MCP 名、查看生成日志。  
- 点击打开页面：内嵌 WebView 或系统窗口打开 `http://127.0.0.1:<port>/...`（实现方式待确认，默认优先内嵌/受控窗口）。

#### 3.3.5 对话附件下拉

- `ChatInput` `AttachMenuView` 扩展：`"root" | "skill" | "plugin" | "mcp" | "miniapp"`。  
- 根菜单增加「小程序」。  
- 选择某小程序后行为（对齐 MCP/插件）：
  - 向输入框注入提示文本（如：`请使用小程序 <name>(<slug>) 完成...`）
  - 并将该小程序 MCP **挂入本轮/本线程可用工具**（激活 server 或写入 thread 上下文）
- AI 主链路：
  - 通过既有 MCP 调用栈 `mcp_call_tool` / 直连 tool schema 调用小程序方法  
  - 对 `open_page`：宿主弹出小程序页面窗口，等待用户操作后把接口结果回传 tool result  
  - AI 根据 `ok/code/message` 继续对话

#### 3.3.6 AI 主链路对接示意

```
用户: 帮我录入今天和华为签的 12000 采购合同
  → AI 发现已挂载小程序 contract-app
  → mcp_call_tool(server=contract-app, tool=open_page, { page: "entry", known: {...} })
  → 宿主打开录入页并预填
  → 用户确认提交
  → tool result: { ok:true, data:{id:..} }
  → AI: 「已写入合同 #id」
```

无小程序或未挂载库时：AI 可提示去设置生成/挂载，**不再**走 build_entry_form。

---

## 4. 模块设计（实施层）

### 4.1 后端（Rust）

| 模块 | 职责 |
|------|------|
| `smartbrain/permissions.rs`（新） | 权限模型解析、SQL 校验、prompt 摘要 |
| `smartbrain/db_query.rs` | 接入新校验；加载时可选一次性迁移旧三字段后丢弃 |
| `miniapp/mod.rs`（新） | 注册表 CRUD、生成编排、进程/端口生命周期 |
| `miniapp/generator.rs` | 主链路生成任务编排 + Node 脚手架模板 |
| `miniapp/runtime.rs` | 用 `codey/node` 启动 MCP+Web、端口分配、停止清理 |
| `miniapp/host_bridge.rs` | open_page 弹窗、结果回传、DB 代理 API |
| `tool_executor` / MCP 集成 | 将运行中 miniapp 注册为 MCP server |
| `commands/*` | 前端 invoke：list/create/generate/start/stop/open |
| 删除 | `form_entry` 全链路 |

### 4.2 前端

| 模块 | 职责 |
|------|------|
| `settings/MiniAppPanel.tsx`（新） | 生成/管理 UI |
| `settings/SmartbrainDatabasePanel.tsx` | 权限 UI 升级 |
| `settings/smartbrainDatabaseState.ts` | 权限类型扩展与持久化 |
| `layout/RightPanel.tsx` + 新 `MiniAppPanel` | 右侧分类 |
| `chat/ChatInput.tsx` | 附件「小程序」 |
| `stores/*` | miniapp 列表/运行状态 |
| `approval/*` 或独立 `MiniAppWindow` | open_page 交互（**不再**通用 entry form） |
| i18n | 中英文文案 |

### 4.3 生成模板（内置）

仓库内置脚手架模板，例如：

```
codey/templates/miniapp-mcp/
  package.json
  server/index.mjs.tpl
  web/...
  .mcp.json.tpl
  miniapp.json.tpl
```

主链路 Agent 在模板基础上编写业务 tools/pages；**不得改变** MCP 协议外壳与 Node 运行方式。  
模板与生成结果必须可被 `codey/node .../server/index.mjs` 直接启动。

---

## 5. 分阶段实施计划

### Phase 0 — 清理旧 form 链路（优先、可单独合并）

1. 按 git 历史精确删除/回退 form 相关代码。  
2. 清理 prompt/i18n/测试。  
3. 验证 SQL 查询与数据库设置仍可用。  

**产出**：无 `build_entry_form` / `save_form_data` 的干净基线。

### Phase 1 — 数据库灵活权限

1. 数据模型切换为结构化策略；旧三字段仅做一次性加载迁移（不保留双轨 UX/API）。  
2. 执行校验升级。  
3. 设置 UI **替换**原三勾选 + AI prompt 注入。  
4. 单测：表级 deny、列级只读、DDL 拦截。  

### Phase 2 — 小程序脚手架与运行时

1. 目录/registry/端口/Node 启动。  
2. 模板 MCP server + 示例页面。  
3. Tauri commands + 右侧栏列表。  
4. 手动用样例包验证 MCP tools 可被主链路调用。  

### Phase 3 — 自然语言生成

1. 设置页生成向导（名称/英文名/数据库/需求）。  
2. **主链路 Agent 生成流水线**（Node.js 包）与失败重试。  
3. 依赖安装策略（强制内置 `codey/node`）。  

### Phase 4 — 主链路对接与附件入口

1. AttachMenu「小程序」。  
2. open_page 宿主桥接（弹窗 + 结果回传）。  
3. Prompt 指导 AI 优先使用已挂载小程序。  
4. 端到端场景：合同录入对话。  

### Phase 5 — 打磨

- 日志、错误码、权限拒绝文案  
- 停止/崩溃清理端口  
- 文档与 i18n  

---

## 6. 非目标（本阶段不做）

- 不做云端小程序市场。  
- 不替代通用 MCP/插件体系（小程序是专用子集）。  
- 不恢复/兼容 `build_entry_form` API。  
- **不保留**旧库级三开关（readSchema/readData/writeData）作为正式权限 UX。  
- 小程序**不支持**非 Node.js 运行时。  
- 不在无用户确认的情况下自动对生产库执行高危 SQL。  

---

## 7. 风险与对策

| 风险 | 对策 |
|------|------|
| 回退 form 误伤 Git/DB 其他改动 | 按文件 hunk 对照 `73730fd`，review diff 后再提交 |
| LLM 生成代码质量不稳 | 固定模板外壳 + 仅生成业务片段 + 启动自检 |
| 主链路生成耗时/中断 | 可取消、可续跑；状态机 draft→generating→ready/error |
| 端口冲突/僵尸进程 | 注册表 + drop 时 kill；绑定 `127.0.0.1` |
| 权限模型过复杂难用 | UI 提供「从 schema 生成默认策略」一键填充；规则可视化编辑 |
| 安全：生成代码直连 DB | 禁止写密码；强制 databaseId + 宿主代理 + 权限引擎 |
| 性能：每个小程序一个 Node 进程 | 按需启动；会话结束可停；限制并发数 |

---

## 8. 验收标准

### 8.1 权限

- [x] 可在设置中配置库/表/规则，并持久化  
- [x] AI prompt 能看到权限摘要与 aiNotes  
- [x] 越权 SQL 被拒绝且返回明确错误  
- [x] 旧三字段配置加载后映射为新策略（无双轨 UI）  

### 8.2 移除旧链路

- [x] 工具列表无 build_entry_form / save_form_data（源码已无，仅计划文档保留历史描述）  
- [x] 无通用入库表单弹窗  
- [x] 对照 git 无无关功能回退（实现落在 miniapp/权限相关文件，未回退连接配置）  

### 8.3 小程序

- [x] 设置中可创建：名称、英文名、挂数据库、自然语言生成（**主链路 Agent**）  
- [x] 生成物为 **Node.js MCP 包**，由 `codey/node` 启动  
- [x] 右侧项目栏「小程序」分类可见并可启停  
- [x] 附件下拉可选小程序并进入对话上下文  
- [x] AI 可调用 open_page 等 MCP 方法；宿主监听 `miniapp-open-page` 打开右侧浏览器；业务提交结果经 MCP tool result 回主链路  
- [x] 端口自动分配（runtime `127.0.0.1:0` / `MINIAPP_PORT`）  

### 8.4 实现锚点（落地后）

| 能力 | 主要路径 |
|------|----------|
| 权限引擎 | `src-tauri/src/smartbrain/permissions.rs` + `db_query.rs` + `prompt_context.rs` |
| 小程序后端 | `src-tauri/src/miniapp/{mod,scaffold,runtime,commands}.rs` |
| 设置生成 UI | `src/components/settings/MiniAppSettingsPanel.tsx` |
| 右侧栏 | `MiniAppSidePanel.tsx` + `RightPanel` tab `miniapp` |
| 附件挂载 | `ChatInput` attach view `miniapp` |
| MCP 注入 | `agent.rs` `list_miniapp_mcp_servers` + system prompt |
| open_page 桥 | `tool_executor` emit `miniapp-open-page` → `useTauriEvents` → `windowOpenBrowser` |

---

## 9. 待你确认的问题

请重点确认以下决策（确认后按此实施）：

1. **权限模型范围**  
   - A. 仅增强库级 + rulesMarkdown 强制化（改动小）  
   - **B. 库 defaults + 表级 + rules 列表（推荐）**  
   - C. 再加列级控制  
   - 已确认：**不做旧三开关 UX 兼容**  

2. **小程序设置入口位置**  
   - **A. 本地知识库 Smartbrain 下新 Tab「小程序」（推荐）**  
   - B. 设置顶层独立 Tab  
   - C. 与 Skill 实验室并列  

3. **open_page 展示方式**  
   - A. 独立小窗  
   - **B. 右侧栏内嵌（推荐，与现有 browser 类似）**  
   - C. 模态弹窗  

4. **生成时机**  
   - **A. 设置页一键生成完整包（推荐）**  
   - B. 对话中也可生成（二期）  

5. **Phase 0 是否先单独合并**  
   - **是：先去掉 form 链路再做小程序（推荐）**  
   - 否：同 PR 一起做  

6. **自然语言生成通道**（已按反馈收敛）  
   - **强制：主链路 Agent 生成**（非旁路单次补全）  
   - 模型选用：默认跟随设置中的默认编程模型（仍可再确认是否允许单独指定）  
   - **技术栈强制：Node.js 小程序**（`codey/node`）  

---

## 10. 建议实施顺序（确认后）

1. 确认第 9 节选项  
2. Phase 0 清理 form  
3. Phase 1 权限  
4. Phase 2–4 小程序运行时 → 生成 → 主链路  
5. 文档更新：`docs/requirements.md` 增补 FR 条目  

---

## 11. 附录：旧链路相关 git 锚点

- 引入提交：`73730fd` `temp`  
  - 新增：`src-tauri/src/smartbrain/form_entry.rs`  
  - 大改：`tool_executor.rs`、`ApprovalModal.tsx`  
  - 提示：`prompt_context.rs` 中「结构化入库请使用 build_entry_form…」  
- 数据库设置相关：`b4cbc13` `数据库设置修改`（连接配置等保留；**权限三开关 UI/模型按本方案替换为灵活策略**，不要整提交回退连接相关改动）  

---

**请确认本方案是否按此推进，或标注需修改的章节/选项。确认后我再开始改代码。**
