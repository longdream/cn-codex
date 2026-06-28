---

## 22. 本地资源池（Local Pool）

### 22.1 架构概述

本地资源池（Local Pool）是 CN-Codex 的多端点 LLM 供应商管理系统。它为同一供应商配置多个 API 端点，支持自动故障转移和健康检测，确保在某个端点不可用时自动切换到可用端点。

```
ConfigToml
  ├─ model_providers.local-pool    ← 本地资源池供应商
  ├─ model_endpoints[]             ← 端点列表
  │   ├─ url                       ← API URL
  │   ├─ label                     ← 显示名称
  │   ├─ model                     ← 该端点使用的模型名
  │   ├─ api_key                   ← API Key
  │   └─ wire_api                  ← API 格式
  └─ active_endpoint_index         ← 当前活跃端点索引（持久化）
```

### 22.2 PoolResolver 核心实现

```rust
pub struct PoolResolver {
    inner: Arc<Mutex<HashMap<String, PoolState>>>,
    initial_index: usize,
}

struct PoolState {
    current_index: usize,
    health: HashMap<usize, EndpointHealth>,
}

struct EndpointHealth {
    fail_count: u32,
    last_failure: Option<Instant>,
}
```

- **线程安全**：使用 `Arc<Mutex<HashMap<String, PoolState>>>`，按 `provider_id:model` 键隔离不同模型的状态
- **顺序轮询**：从 `current_index` 开始依次尝试，跳过故障端点
- **60 秒恢复期**：故障端点标记后 60 秒自动恢复健康状态
- **全故障重置**：所有端点均不可用时，重置全部健康状态，强制从第一个端点重试

### 22.3 端点选择策略

```
resolve_endpoint(key, endpoints):
  1. 获取上次使用的 index
  2. 从该 index 开始顺序遍历
  3. 跳过 health.is_healthy() = false 的端点
  4. 返回第一个健康端点的 ResolvedEndpoint
  5. 如果全部不可用 → 重置健康状态 → 返回 endpoints[0]
```

### 22.4 故障标记流程

```
1. HTTP 请求返回 5xx/超时 → mark_failed(key, index)
2. fail_count += 1
3. last_failure = Instant::now()
4. 后续 resolve_endpoint 调用自动跳过该端点
5. 60 秒后 is_healthy() 恢复为 true
```

### 22.5 前端配置

前端通过 `ProviderPanel.tsx` 中的端点管理 UI 进行配置：
- 添加/编辑端点（URL、标签、模型、API Key）
- 设置活跃端点索引
- 端点配置持久化到 `config.toml` 的 `model_endpoints` 数组

---

## 23. OCR 离线识别

### 23.1 技术架构

CN-Codex 内置基于 **PaddleOCR v5 Mobile** 的离线文字识别引擎，使用 ONNX Runtime 进行推理。

```
图片输入
  ↓
decode_data_url() / load_image()    ← Base64 解码或文件加载
  ↓
downscale_large_image()             ← 缩放至 1600px 以内
  ↓
PP-OCRv5 Pipeline                   ← 检测 + 识别
  ├─ TextDetection (DBnet)          ← 文本框检测
  └─ TextRecognition (CRNN)         ← 文字识别
  ↓
collect_result_text()               ← 结果收集（信度 > 0.15）
  ↓
纯文本输出
```

### 23.2 模型文件

| 文件 | 说明 | 打包位置 |
|------|------|---------|
| `ppocrv5_mobile_det.onnx` | 文本检测模型 | `src-tauri/resources/ocr/` |
| `ppocrv5_mobile_rec.onnx` | 文字识别模型 | `src-tauri/resources/ocr/` |
| `ppocrv5_mobile_vocab.txt` | 识别词表 | `src-tauri/resources/ocr/` |
| `onnxruntime.dll` | ONNX Runtime 动态库 | `src-tauri/resources/ocr/` |

### 23.3 检测配置

```rust
TextDetectionConfig {
    score_threshold: 0.5,    // 文本框得分阈值
    box_threshold: 0.7,      // 框选阈值
    unclip_ratio: 1.8,       // 框扩展比例
    max_candidates: 40,      // 最大候选框数
    limit_side_len: Some(960), // 限制边长
    limit_type: Some(Max),   // 限制方式
    max_side_len: Some(2000), // 最大边长
}
```

### 23.4 图片预处理

```rust
const MAX_OCR_IMAGE_SIDE: u32 = 1600;

fn downscale_large_image(image: RgbImage) -> RgbImage {
    // 如果最长边 > 1600px，按比例缩小
    // 使用 Triangle 滤波保证缩放质量
}
```

### 23.5 使用场景

| 场景 | 入口 | 说明 |
|------|------|------|
| 用户 `view_image` 后 | `extract_text_from_data_urls()` | 自动提取图片中的文字 |
| 附件图片识别 | `OcrImageInput` → data URL 解码 | 多图批量识别 |
| 本地图片文件 | `extract_text_from_image_file()` | 直接识别文件系统中的图片 |

### 23.6 资源加载策略

```rust
fn resolve_ocr_resource_dir(project_root: &Path) -> AppResult<PathBuf> {
    // 查找顺序：
    // 1. CN_CODEX_OCR_RESOURCE_DIR 环境变量指定路径
    // 2. project_root/src-tauri/resources/ocr/
    // 3. project_root/resources/ocr/
    // 4. 可执行文件同目录 resources/ocr/
}
```

---

## 24. 终端模拟器

### 24.1 架构

```
┌─────────────────────────────────────────────────────┐
│                   前端 (React)                       │
│  ┌──────────────────────────────────────────────┐   │
│  │           TerminalPanel.tsx                   │   │
│  │  ┌────────────────────────────────────────┐  │   │
│  │  │         @xterm/xterm                   │  │   │
│  │  │  (xterm.js 终端模拟器)                  │  │   │
│  │  └────────────────────────────────────────┘  │   │
│  └──────────────────────────────────────────────┘   │
└──────────────────────┬──────────────────────────────┘
                       │ Tauri IPC
┌──────────────────────┴──────────────────────────────┐
│                   后端 (Rust)                        │
│  ┌──────────────────────────────────────────────┐   │
│  │           TerminalManager                      │   │
│  │  sessions: Mutex<HashMap<String, PtySession>> │   │
│  │                                                │   │
│  │  PtySession {                                  │   │
│  │    writer: Box<dyn Write + Send>,              │   │
│  │    _master: Box<dyn MasterPty + Send>,         │   │
│  │    _child: Box<dyn Child + Send + Sync>,       │   │
│  │  }                                             │   │
│  └──────────────────────────────────────────────┘   │
│                    │                                │
│  ┌─────────────────┴──────────────────────────┐    │
│  │          portable-pty                       │    │
│  │  打开 PTY (伪终端) → 衍生系统 Shell          │    │
│  └────────────────────────────────────────────┘    │
└────────────────────────────────────────────────────┘
```

### 24.2 命令集

| 命令 | 参数 | 说明 |
|------|------|------|
| `terminal_create` | `cwd?: string` | 创建新终端会话，返回 session_id |
| `terminal_write` | `session_id, data` | 向终端写入数据（Shell 输入） |
| `terminal_resize` | `session_id, cols, rows` | 调整终端尺寸 |
| `terminal_close` | `session_id` | 关闭终端会话 |

### 24.3 数据流

```
1. terminal_create → 
   - native_pty_system().openpty() 创建 PTY 对
   - spawn_command() 衍生默认 Shell
   - 启动后台线程读取 PTY 输出
   - 通过 Tauri emit("terminal-output", payload) 推送到前端

2. 后台读取线程：
   - reader.read(buf) → 编码为 UTF-8
   - emit("terminal-output", { sessionId, data })
   - 读到 EOF 时 emit { closed: true }

3. xterm.js 接收 terminal-output 事件：
   - terminal.write(data) 渲染到终端界面
```

### 24.4 技术特点

- **跨平台**：`portable-pty` 在 Windows 使用 WinPTY，macOS/Linux 使用 Unix PTY
- **零拷贝**：PTY 数据直接从系统内核到 xterm.js，无需中间缓冲
- **会话管理**：支持多终端标签页，每个标签独立 PTY 会话
- **尺寸同步**：前端 resize 事件实时同步到 PTY 尺寸

---

## 25. 窗口系统

### 25.1 窗口架构

CN-Codex 采用 **无边框多窗口架构**，包含三个核心窗口：

```
主窗口 (main)
├─ 标题：CN-Codex
├─ 尺寸：1200×800（默认），最小 800×600
├─ decorations: false（无边框）
├─ 全自绘标题栏 + 拖拽移动
└─ 主工作区：侧边栏 + 聊天 + 右侧面板

文档详情窗口 (document-detail)
├─ 标题：文档详情
├─ 尺寸：1060×760，最小 760×520
├─ 单实例复用（先查找已存在窗口）
├─ 显示文件内容（Markdown 渲染）
└─ 关闭时清空 active_path 缓存

文件差异窗口 (runsummary-diff)
├─ 标题：RunSummary Diff
├─ 尺寸：自动（基于 diff 内容）
├─ 单实例复用
├─ 显示文件变更前后对比
└─ payload 从 AppState 缓存读取
```

### 25.2 主窗口初始化

```rust
// tauri.conf.json
{
  "app": {
    "windows": [{
      "label": "main",
      "title": "CN-Codex",
      "width": 1200,
      "height": 800,
      "minWidth": 800,
      "minHeight": 600,
      "visible": false,         // 先隐藏，后端就绪后再显示
      "resizable": true,
      "decorations": false,     // 无边框
      "dragDropEnabled": false
    }]
  }
}
```

### 25.3 窗口控制命令

| 命令 | 功能 |
|------|------|
| `window_start_dragging` | 标题栏拖拽移动窗口 |
| `window_minimize` | 最小化 |
| `window_toggle_maximize` | 切换最大化/还原 |
| `window_close` | 关闭主窗口 |
| `window_show_main` | 显示主窗口（初始化完成后） |
| `window_open_browser` | 打开浏览器子窗口（400×600 浮动窗） |
| `window_resize_browser` | 调整浏览器子窗口尺寸 |
| `window_navigate_browser` | 浏览器子窗口导航 |
| `window_close_browser` | 关闭浏览器子窗口 |
| `window_open_document_detail` | 打开文档详情窗口 |
| `window_close_document_detail` | 关闭文档详情窗口 |
| `window_get_document_detail_path` | 获取当前详情窗文件路径 |
| `window_open_runsummary_diff` | 打开 Diff 对比窗口 |
| `window_close_runsummary_diff` | 关闭 Diff 对比窗口 |
| `window_toggle_devtools` | 切换开发者工具 |

### 25.4 8 秒强制显示机制

```rust
// 后台兜底：若 8 秒后前端仍未显示主窗口，强制显示
tokio::time::sleep(Duration::from_secs(8)).await;
if let Ok(false) = main_window.is_visible() {
    main_window.show()?;  // 强制显示
}
```

### 25.5 浏览器子窗口（Embedded Browser）

- 使用 Tauri WebviewWindow 作为嵌入式浏览器
- 支持 CDP 调试端口（9242）用于远程调试
- URL 验证：只允许 http/https 协议，拒绝 file://
- 端点元数据持久化到 `visible-browser.json`

---

## 26. 用户规则系统

### 26.1 规则类型

| 类型 | 文件路径 | 作用范围 |
|------|---------|---------|
| **全局规则** | `codey/rules/user.md` | 所有项目 |
| **项目规则** | 项目目录下的 `.rule.md` | 当前项目 |

### 26.2 加载机制

```
App 初始化
  ├─ rules_read() → 读取 codey/rules/user.md（全局）
  └─ rules_read_project() → 读取项目/.rule.md（项目级）
       ↓
内容合并后注入 Agent 的 system prompt
```

### 26.3 规则格式

```markdown
# 用户规则

## 编码规范
- 使用 TypeScript 严格模式
- 函数命名使用 camelCase
- 组件使用 PascalCase

## 项目约束
- 不要使用 any 类型
- 所有公共 API 必须有 JSDoc 注释
```

### 26.4 前端管理

- `SettingsPanel.tsx` → `RulesPanel`：编辑全局规则
- 编辑器中实时预览 Markdown
- 保存后自动注入到下一次 Agent 调用

### 26.5 注入流程

```rust
// 在 AgentEngine.run_turn() 中：
// 1. 读取全局规则文件
// 2. 读取项目规则文件
// 3. 合并到 system_prompt_builder
// 4. 作为 system message 的第一部分发送给 LLM
```

---

## 27. 协议层（Protocol）

### 27.1 架构

CN-Codex 内部实现了轻量级的 **JSON-RPC 风格协议**，用于以下通信场景：

```
前端 ←→ Rust 后端（Tauri IPC）
  ├─ approval (审批通道)
  ├─ notification (通知推送)
  └─ request/response (请求-响应)

Rust 后端内部
  ├─ approval_tx/rx → 审批队列
  └─ file_review_sessions → 文件审阅缓存
```

### 27.2 审批通道（Approval）

```rust
pub enum ApprovalAction {
    Resolve {
        request_id: RequestId,
        result: serde_json::Value,
    },
    Reject {
        request_id: RequestId,
        error: JSONRPCErrorError,
    },
}

// 通道创建
let (approval_tx, approval_rx) = mpsc::channel(64);
```

**流程**：
```
1. Agent 执行需要审批的操作（如 apply_patch、shell 命令）
2. approval_tx.send(ApprovalAction) → 前端收到审批请求
3. 前端显示 ApprovalModal
4. 用户批准/拒绝 → commands::resolve_approval / reject_approval
5. Agent 继续/中断执行
```

### 27.3 文件审阅（File Review）

`apply_patch` 前置确认机制：

```rust
pub struct PendingPatchReview {
    pub thread_id: String,
    pub call_id: String,
    pub raw_patch: String,
    pub files: Vec<FileReviewFile>,
    pub keep_all: bool,
    pub status: ReviewStatus,
}
```

**流程**：
```
1. Agent 调用 apply_patch
2. ToolExecutor 解析 patch，生成 PendingPatchReview
3. 写入 file_review_sessions cache (key: threadId + callId)
4. 前端弹出 PatchDiffModal 显示变更预览
5. 用户逐个文件确认/拒绝 → file_review_update()
6. 确认后 → file_review_apply() 执行写入
7. 取消 → file_review_cancel() 丢弃
```

### 27.4 请求-响应模块

```rust
pub mod jsonrpc {
    // JSON-RPC 风格的请求/响应数据结构
    pub struct Request {
        pub id: RequestId,
        pub method: String,
        pub params: serde_json::Value,
    }
    pub struct Response {
        pub id: RequestId,
        pub result: Option<serde_json::Value>,
        pub error: Option<JSONRPCErrorError>,
    }
}

pub mod notifications {
    // 纯通知（单向，无需响应）
    pub struct Notification {
        pub method: String,
        pub params: serde_json::Value,
    }
}

pub mod requests {
    // 前端请求封装
    pub struct FrontendRequest<T: Deserialize> {
        pub id: String,
        pub payload: T,
    }
}
```

### 27.5 事件推送

所有后端事件通过 Tauri `emit` 推送到前端：

```rust
fn emit_and_broadcast(app_handle: &AppHandle, event: &str, payload: serde_json::Value) {
    app_handle.emit(event, payload.clone()).ok();
    crate::mobile_server::broadcast(event, payload);  // 同时广播到移动端
}
```

### 27.6 主要事件列表

| 事件名 | 触发时机 | 载荷 |
|--------|---------|------|
| `turn-start` | turn 开始 | `{threadId, mode, model}` |
| `turn-completed` | turn 完成 | `{threadId, usage, changedFiles}` |
| `streaming-text` | LLM 流式输出 | `{delta}` |
| `tool-call-start` | 工具调用开始 | `{id, name, arguments}` |
| `tool-call-output` | 工具调用输出 | `{id, output}` |
| `context-compacted` | 上下文压缩完成 | `{threadId, savedTokens}` |
| `approval-request` | 需要用户审批 | `{requestId, action, reason}` |
| `terminal-output` | 终端输出 | `{sessionId, data}` |
| `goal-completed` | Goal 完成 | `{threadId, summary}` |
| `file-change` | 文件变更 | `{path, action}` |

---

## 更新：前端组件与状态管理补充

### 4.7 前端组件树完整结构

```
App.tsx
├── ErrorBoundary
├── IntlProvider (react-intl)
├── AppContent (useTauriEvents 注册)
├── app-frame
│   ├── AppBackground
│   ├── app-shell-content
│   │   ├── TitleBar
│   │   │   ├── 窗口控制按钮（最小化/最大化/关闭）
│   │   │   ├── 标题 + 二维码按钮
│   │   │   └── 拖拽区域
│   │   ├── app-workbench
│   │   │   ├── Sidebar (左侧)
│   │   │   │   ├── 标签切换（会话/项目）
│   │   │   │   ├── 会话列表（按项目分组）
│   │   │   │   ├── 搜索框
│   │   │   │   └── 底部功能按钮
│   │   │   ├── app-resizer-left (拖拽分隔条)
│   │   │   ├── app-main (聊天区)
│   │   │   │   ├── MessageList
│   │   │   │   │   ├── ChatMessage (用户/助手)
│   │   │   │   │   ├── CodeBlock (代码高亮)
│   │   │   │   │   ├── ToolCallCard (工具调用)
│   │   │   │   │   ├── RunSummaryCard (回合总结)
│   │   │   │   │   └── PlanCard (计划)
│   │   │   │   ├── ChatInput
│   │   │   │   │   ├── 文本输入区
│   │   │   │   │   ├── 模式切换（chat/plan/goal）
│   │   │   │   │   ├── 附件上传
│   │   │   │   │   └── SlashCommandPanel
│   │   │   │   └── StreamingIndicator
│   │   │   ├── app-resizer-right (拖拽分隔条)
│   │   │   └── RightPanel (右侧)
│   │   │       ├── Tab 切换
│   │   │       ├── BrowserPanel (应用内浏览器)
│   │   │       ├── TerminalPanel (终端)
│   │   │       ├── GitPanel (Git 操作)
│   │   │       └── FileTree (文件树)
│   │   └── StatusBar
│   │       ├── 模型名
│   │       ├── 活跃端点
│   │       └── 状态指示
│   ├── SettingsPanel (模态层)
│   │   ├── ProviderPanel
│   │   ├── HooksPanel
│   │   ├── SkillsPanel
│   │   ├── PluginsPanel
│   │   ├── RobotsPanel
│   │   ├── WorkflowsPanel
│   │   ├── KnowledgePanel
│   │   ├── ExperiencePanel
│   │   ├── IntegrationPanel
│   │   └── UsageDashboard
│   ├── ApprovalModal
│   ├── WorkflowExtractModal
│   ├── FortuneBubble
│   └── RecordingToggle
```

### 4.8 多页面构建

项目实现了三入口多页面架构：

```typescript
// vite.config.ts
rollupOptions: {
  input: {
    main: path.resolve(__dirname, "index.html"),    // 主窗口
    detail: path.resolve(__dirname, "detail.html"), // 文档详情窗口
    diff: path.resolve(__dirname, "diff.html"),     // Diff 对比窗口
  },
}
```

每个入口独立加载，独立窗口独立渲染，通过 Tauri IPC 通信。

### 4.9 国际化实现

使用 `react-intl` 实现中英文双语：

```typescript
// src/i18n/
├── zh-CN/
│   └── common.json           // 中文翻译
└── en-US/
    └── common.json           // 英文翻译

// App.tsx
<IntlProvider
  locale={locale}
  messages={messages[locale] ?? zhCN}
  defaultLocale="zh-CN"
>
```

- **默认语言**：zh-CN（中文）
- **切换**：设置面板中的语言选择
- **支持**：所有 UI 文案、错误提示、状态描述

### 4.10 Zustand 状态管理详解

**appStore.ts**（~1300 行）管理：

| 状态分组 | 关键字段 |
|---------|---------|
| **会话** | threads, currentThreadId, messages, streamingText |
| **项目** | projects, currentProjectId, workspaceCwd |
| **模型** | providers, activeProviderId, configuredModels, activeModelId |
| **布局** | sidebarWidth, rightPanelWidth, rightPanelVisible |
| **UI** | showSettings, sidebarTab, currentChatMode |
| **运行** | isStreaming, currentTurnId, isInitialized |
| **审批** | pendingFileReviews, pendingApproval |
| **性能** | performanceMetrics, initError |

**settingsStore.ts** 管理：
- `locale`：语言（zh-CN / en-US）
- `theme`：主题（dark / light / system）

---

## 更新：ToolExecutor 完整工具集

### 7.5 完整工具清单（40+ 工具）

#### 核心开发工具

| 工具名 | 功能 | 参数示例 |
|--------|------|---------|
| `shell` / `shell_command` | 执行 Shell 命令 | `{command, workdir, timeout_ms}` |
| `exec_command` | 持久化命令会话 | `{cmd, workdir, yield_time_ms}` |
| `write_stdin` | 写入命令 stdin | `{session_id, chars}` |
| `close_exec_session` | 关闭命令会话 | `{session_id}` |
| `read_file` | 读取文件内容 | `{path}` |
| `write_file` | 写入/覆盖文件 | `{path, content}` |
| `apply_patch` | 多文件补丁 | `{patch}` |
| `list_directory` | 列出目录 | `{path}` |

#### AI 辅助工具

| 工具名 | 功能 | 参数示例 |
|--------|------|---------|
| `update_plan` | 更新任务计划 | `{plan: [{step, status}]}` |
| `request_user_input` | 请求用户输入 | `{questions: [...]}` |
| `request_permissions` | 请求额外权限 | `{permissions, reason}` |
| `tool_search` | 搜索可用工具/技能 | `{query, limit}` |
| `code_review` | 代码审查 | `{base_ref, paths}` |

#### 浏览器工具

| 工具名 | 功能 | 参数示例 |
|--------|------|---------|
| `browser_run` | 浏览器自动化 | `{actions, url, viewport}` |
| `view_image` | 查看图片 | `{path}` |
| `ocr_image` | OCR 图片识别 | `{path}` |
| `image_generate` | AI 生成图片 | `{prompt, size, n}` |

#### MCP 工具

| 工具名 | 功能 | 参数示例 |
|--------|------|---------|
| `mcp_list_servers` | 列出 MCP 服务器 | — |
| `mcp_status` | 查询 MCP 状态 | `{server, probe}` |
| `mcp_list_tools` | 列出 MCP 工具 | `{server}` |
| `mcp_list_resources` | 列出 MCP 资源 | `{server}` |
| `mcp_list_prompts` | 列出 MCP 提示词 | `{server}` |
| `mcp_call_tool` | 调用 MCP 工具 | `{server, tool, arguments}` |
| `mcp_read_resource` | 读取 MCP 资源 | `{server, uri}` |
| `mcp_get_prompt` | 获取 MCP 提示词 | `{server, prompt, arguments}` |
| `mcp__*` (direct tools) | 直接调用 MCP 工具 | 按工具 schema |

#### 记忆/知识工具

| 工具名 | 功能 | 参数示例 |
|--------|------|---------|
| `memory_list` | 列出记忆 | `{path}` |
| `memory_read` | 读取记忆 | `{path}` |
| `memory_write` | 写入记忆 | `{path, content}` |
| `memory_update` | 更新记忆 | `{path, old_text, new_text}` |
| `memory_forget` | 删除记忆 | `{path, match_text}` |
| `memory_search` | 搜索记忆 | `{query, path}` |

#### 子代理工具

| 工具名 | 功能 | 参数示例 |
|--------|------|---------|
| `spawn_agent` | 生成子代理 | `{prompt, role, wait}` |
| `wait_agent` | 等待子代理 | `{agent_id, timeout_ms}` |
| `send_input` | 发送输入给子代理 | `{target, message}` |
| `resume_agent` | 恢复子代理 | `{id, timeout_ms, wait}` |
| `list_agents` | 列出子代理 | — |
| `close_agent` | 关闭子代理 | `{target}` |

#### 网络工具

| 工具名 | 功能 | 参数示例 |
|--------|------|---------|
| `web_search` | 网页搜索 | `{query, max_results}` |
| `web_fetch` | 抓取网页内容 | `{url, max_chars}` |

#### 机器人工具

| 工具名 | 功能 | 参数示例 |
|--------|------|---------|
| `robot_save` | 保存机器人配置 | `{id, config}` |
| `plugin_manage` | 插件管理 | `{action, plugin_id}` |
| `apps_list` | 列出应用连接器 | `{connector_id}` |

### 7.6 工具执行模式

```rust
// 同步模式：等待结果返回
pub enum ToolExecuteMode {
    Sync,     // 默认，等待执行完成
    Approve,  // 需要用户审批
}

// 执行结果
pub struct ToolExecuteResult {
    pub output: String,
    pub error: Option<String>,
    pub file_changes: Vec<FileChange>,
    pub needs_approval: bool,
}
```

---

## 更新：API 层完整命令列表

### 5.5 完整 Tauri Command 清单

#### 核心引擎命令

| 命令 | 类别 | 说明 |
|------|------|------|
| `standalone_init` | Core | 初始化 Standalone 引擎 |
| `standalone_config_read` | Core | 读取配置 |
| `standalone_config_write` | Core | 写入配置 |
| `standalone_thread_create` | Core | 创建新会话 |
| `standalone_thread_list` | Core | 列出会话 |
| `standalone_thread_read` | Core | 读取会话内容 |
| `standalone_chat` | Core | 发送聊天消息 |
| `standalone_turn_interrupt` | Core | 中断当前 turn |
| `standalone_thread_goal_set` | Core | 设置 Goal |
| `standalone_thread_goal_status` | Core | 查询 Goal 状态 |
| `standalone_thread_goal_edit` | Core | 编辑 Goal |
| `standalone_thread_goal_clear` | Core | 清除 Goal |
| `standalone_mcp_enable_playwright` | Core | 启用 Playwright MCP |
| `standalone_smartbrain_enable` | Core | 启用 SmartBrain |
| `fortune_llm_call` | Core | 运势 LLM 调用 |
| `fortune_detail_stream_start` | Core | 运势详情流式加载 |

#### 文件操作命令

| 命令 | 说明 |
|------|------|
| `read_directory` | 读取目录 |
| `read_file_for_attach` | 读取文件作为附件 |
| `read_text_file_preview` | 读取文本文件预览 |
| `write_text_file_preview` | 写入文本文件预览 |
| `reveal_in_explorer` | 在文件管理器中显示 |

#### Git 命令

| 命令 | 说明 |
|------|------|
| `git_status` | Git 状态 |
| `git_diff` | Git 差异 |
| `git_log` | Git 日志 |
| `git_branch_list` | 分支列表 |
| `git_stage` | 暂存文件 |
| `git_unstage` | 取消暂存 |
| `git_commit` | 提交 |
| `git_checkout` | 切换分支 |
| `git_pull` | 拉取 |
| `git_push` | 推送 |
| `git_reset` | 重置 |
| `git_revert` | 还原 |
| `git_cherry_pick` | Cherry Pick |

#### SmartBrain 命令

| 命令 | 说明 |
|------|------|
| `smartbrain_list_experiences` | 列出经验 |
| `smartbrain_read_experience` | 读取经验 |
| `smartbrain_delete_experience` | 删除经验 |
| `smartbrain_list_knowledge` | 列出知识 |
| `smartbrain_read_knowledge` | 读取知识 |
| `smartbrain_delete_knowledge` | 删除知识 |
| `smartbrain_upload_knowledge` | 上传知识 |
| `smartbrain_search` | 搜索 |
| `smartbrain_rebuild_index` | 重建索引 |
| `smartbrain_migrate_to_okf` | 迁移到 OKF 格式 |

#### 工作流命令

| 命令 | 说明 |
|------|------|
| `workflow_extract` | 从会话提取工作流 |
| `workflow_save` | 保存工作流 |
| `workflow_list` | 列出工作流 |
| `workflow_read` | 读取工作流 |
| `workflow_delete` | 删除工作流 |

---

> 本文档由 Codey 自动整理生成，基于 CN-Codex 项目源码（v0.1.0）全面分析。
> 涵盖 27 个技术章节，约 50,000 字符，覆盖项目所有核心模块。