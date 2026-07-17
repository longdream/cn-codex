# 单机双客户端局域网协作测试指南

> 适用场景：只有一台电脑，需要模拟两个 CN-Codex 客户端互通  
> 依赖能力：LAN 协作（弱中心 Owner + P2P）、模型共享、知识库共享  
> 日期：2026-07-17

---

## 1. 结论先看

**可以在一台电脑上测。** 不需要第二台机器，也不需要中心服务器。

关键点只有 3 个：

1. **两个进程**（两个 CN-Codex 窗口）
2. **两套隔离数据目录**（不同 `node_id` / 配置 / 知识库）
3. **用回环地址连接**：`127.0.0.1:<A 的监听端口>`

当前实现已支持：

| 能力 | 行为 |
|------|------|
| 监听端口 | 从 `47800` 起自动尝试到 `47820`，第二实例会自动错开 |
| 节点身份 | 存在 `{projectRoot}/codey/lan_collab/`，按工作区隔离 |
| 工作区覆盖 | 环境变量 `CN_CODEX_PROJECT_ROOT` |
| 无单实例锁 | 未启用 single-instance 插件，可多开 |
| 集成冒烟 | `runtime.rs` 内双节点测试覆盖连接/入组/聊天/模型/知识 |

---

## 2. 为什么不能直接开两个相同窗口

如果两个进程都指向**同一个项目根目录**：

- 会共用同一个 `codey/lan_collab/identity.json`
- 两个窗口变成“同一个节点”跟自己说话
- 配置、知识库、组状态也会互相覆盖

如果 WebView2 用户数据目录不隔离：

- 第二个窗口可能启动异常、白屏、或抢同一缓存目录

因此必须：

```text
实例 A: CN_CODEX_PROJECT_ROOT = ...\node-a
        WEBVIEW2_USER_DATA_FOLDER = ...\webview-a

实例 B: CN_CODEX_PROJECT_ROOT = ...\node-b
        WEBVIEW2_USER_DATA_FOLDER = ...\webview-b
```

---

## 3. 推荐方式：一键双实例脚本

仓库已提供：

```powershell
scripts\lan-dual-client-test.ps1
```

### 3.1 前置

任选其一：

1. **已有可执行文件**（发布包 / `src-tauri\target\release\cn-codex.exe` / `debug`）
2. 或先构建：

```powershell
npm run tauri build
# 或
cargo build --manifest-path src-tauri/Cargo.toml --release
```

### 3.2 启动

在仓库根目录执行：

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\lan-dual-client-test.ps1
```

脚本会：

1. 自动寻找 `cn-codex.exe`
2. 创建隔离目录：
   - `.lan-test\node-a`
   - `.lan-test\node-b`
   - `.lan-test\webview-a`
   - `.lan-test\webview-b`
3. 给 A 写入一份示例知识文档（便于测知识共享）
4. 拉起两个独立进程
5. 在控制台打印下一步手工操作清单

可选参数：

```powershell
# 指定 exe
.\scripts\lan-dual-client-test.ps1 -ExePath "D:\path\to\cn-codex.exe"

# 指定测试根目录
.\scripts\lan-dual-client-test.ps1 -TestRoot "D:\tmp\cn-lan-test"

# 只准备目录，不启动
.\scripts\lan-dual-client-test.ps1 -PrepareOnly

# 清理旧测试目录后重建
.\scripts\lan-dual-client-test.ps1 -Clean
```

---

## 4. 手工双实例（不用脚本）

### 4.1 准备目录

```powershell
$root = "E:\tmp\cn-lan-test"
New-Item -ItemType Directory -Force -Path "$root\node-a","$root\node-b","$root\webview-a","$root\webview-b" | Out-Null
```

### 4.2 启动 A

```powershell
$env:CN_CODEX_PROJECT_ROOT = "E:\tmp\cn-lan-test\node-a"
$env:WEBVIEW2_USER_DATA_FOLDER = "E:\tmp\cn-lan-test\webview-a"
Start-Process -FilePath "路径\cn-codex.exe"
```

### 4.3 启动 B（新开一个 PowerShell 窗口）

```powershell
$env:CN_CODEX_PROJECT_ROOT = "E:\tmp\cn-lan-test\node-b"
$env:WEBVIEW2_USER_DATA_FOLDER = "E:\tmp\cn-lan-test\webview-b"
Start-Process -FilePath "路径\cn-codex.exe"
```

> 注意：两个 `Start-Process` 必须分别带上自己的环境变量。  
> 不要在同一会话先设 A 再设 B 后连续启动，除非用 `Start-Process -Environment`（PowerShell 7+）或脚本方式注入。

---

## 5. UI 联调步骤（A ↔ B）

### 5.1 打开面板

两个窗口都进入：

**右侧边栏 → 网络/协作图标（`lan`）→ 局域网协作面板**

### 5.2 改显示名（便于区分）

- A：`OwnerA`
- B：`MemberB`

### 5.3 开启协作

两边都打开「启用局域网协作」。

观察状态里的：

- `localAddress` / 监听端口  
  - 通常 A = `47800`  
  - B = `47801`（A 占用后自动顺延）

### 5.4 建立连接

在 **B** 的“手动连接”输入：

```text
127.0.0.1:47800
```

（端口以 **A 面板实际显示** 为准）

成功标志：

- 两边 `connectedPeerCount >= 1`
- 对端出现在附近节点列表，状态为已连接

### 5.5 建组 + 入组 + 聊天

1. **A** 创建协作组，例如 `Smoke Team`
2. 复制邀请码
3. **B** 粘贴邀请码加入
4. 任一侧发送消息，另一侧应看到

### 5.6 模型共享（零 Key 使用）

1. **A** 先配置好可用模型（A 本机有 Key / 本地模型）
2. A 在 LAN 面板选择模型 → 共享
3. **B** 在“远端共享模型”点「使用」
4. B **不需要**配置上游 Key，直接用该共享模型发对话

说明：

- API Key 始终只留在 A
- B 实际访问的是 A 本机模型代理端口（约 `47900-47920`）

### 5.7 知识库共享

1. **A** 选择要共享的文档 → 发布共享
2. **B** 看到远端知识清单
3. B 搜索 / 拉取文档正文
4. A 取消共享后，B 再拉取应失败

---

## 6. 无 UI 自动化冒烟（开发者）

代码内已有双节点集成测试（不弹窗）：

```text
src-tauri/src/lan_collab/runtime.rs  #[cfg(test)]
```

覆盖：

- 双节点 connect
- 建组 / 邀请码入组
- 组内聊天
- 模型代理调用
- 知识 fetch / 未共享拒绝 / unshare

运行：

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --lib lan_collab -- --nocapture
```

### 已知环境坑（本机 AppInit）

若测试进程启动即失败，错误类似：

```text
0xc0000139
C:\Windows\LVUAAgentInstBaseRoot\system32\Vozokopot.dll
LoadAppInit_DLLs=1
```

这是 **系统 AppInit DLL 注入** 问题，不是业务代码编译失败。

可选处理：

1. 管理员临时关闭 `LoadAppInit_DLLs` 后再跑测试
2. 或优先用 **双 UI 实例 + 127.0.0.1** 做功能验证
3. `cargo check` / `cargo test --no-run` 仍可用于确认能编译

---

## 7. 开发模式（`tauri dev`）注意

`npm run tauri dev` 默认：

- 前端固定 `http://localhost:1420`
- 同一仓库通常只适合跑 **一个** dev 实例

因此：

| 目标 | 建议 |
|------|------|
| 日常开发联调 LAN | 1 个 dev + 1 个 release/debug exe（隔离目录） |
| 纯双端产品验收 | 两个 exe + 隔离目录（脚本） |
| 回归协议/权限 | `cargo test` 双节点冒烟 |

示例（dev 当 A，exe 当 B）：

```powershell
# 终端 1：正常开发实例（当前仓库 = A）
npm run tauri dev

# 终端 2：隔离 B
$env:CN_CODEX_PROJECT_ROOT = "E:\tmp\cn-lan-test\node-b"
$env:WEBVIEW2_USER_DATA_FOLDER = "E:\tmp\cn-lan-test\webview-b"
& "E:\work\RustWorks\cn-codex\src-tauri\target\debug\cn-codex.exe"
```

然后 B 连接 A 显示的 `127.0.0.1:<port>`。

---

## 8. 验收清单

| # | 用例 | 期望 |
|---|------|------|
| 1 | 双实例启动 | 两个窗口，显示名不同，nodeId 不同 |
| 2 | 端口 | 两边都能 enable；端口不冲突（自动顺延） |
| 3 | 手动连接 | B 连 `127.0.0.1:A_port` 成功 |
| 4 | 建组/入组 | B 能凭邀请码加入 A 的组 |
| 5 | 聊天 | 双向消息可见 |
| 6 | 模型共享 | B 无上游 Key 可调用 A 共享模型 |
| 7 | 知识共享 | B 可搜/拉已共享文档；未共享/已撤销失败 |
| 8 | 关闭协作 | disable 后连接与共享目录清理符合预期 |

---

## 9. 故障排查

| 现象 | 排查 |
|------|------|
| 第二个窗口起不来 / 白屏 | 是否设置了不同的 `WEBVIEW2_USER_DATA_FOLDER` |
| 两边显示同一个名字/同一节点 | 是否误用了同一个 `CN_CODEX_PROJECT_ROOT` |
| 连接失败 | A 是否已 enable；端口是否抄成 B 的；是否写成了局域网 IP 但防火墙拦截（单机优先 `127.0.0.1`） |
| 端口绑定失败 | `47800-47820` 是否全被占用 |
| 模型“使用”后调用失败 | A 上游是否可用；A 是否仍在线；代理端口是否被防火墙拦截 |
| 知识拉不到 | 是否已同组/已连接；文档是否在共享清单；A 是否 unshare |
| `cargo test` 直接 0xc0000139 | AppInit DLL 注入，见第 6 节 |

---

## 10. 与架构的对应关系

单机双实例测的是同一套协议，只是把“两台电脑”换成“两个进程”：

```text
┌──────────────┐         TCP 127.0.0.1:47800        ┌──────────────┐
│  Node A      │◄──────────────────────────────────►│  Node B      │
│  Owner       │   chat / invite / kb / model meta  │  Member      │
│  :47800      │                                    │  :47801      │
│  proxy:479xx │◄──── B 以 access_token 调模型 ─────│  无上游 Key  │
└──────────────┘                                    └──────────────┘
```

- **控制面**：A 作为组 Owner 权威
- **业务面**：聊天、知识拉取、模型代理仍是 P2P
- **无中心服务器**

---

## 11. 下一步（可选）

1. mDNS 自动发现（免手输 IP:端口）
2. Agent 工具直接检索远端共享知识库
3. TLS/QUIC 与更细 ACL
4. 测试环境提供 “AppInit-safe” 的 CI runner 说明
