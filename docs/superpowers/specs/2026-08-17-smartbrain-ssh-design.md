# 本地知识库 SSH 服务器配置与远程执行设计

日期：2026-08-17  
状态：待审阅

## 1. 背景

设置 → 本地知识库已经支持多数据库配置：左侧列表、右侧表单、连接测试，以及把配置保存到 SQLite `app_state`。对话开启本地知识库后，AI 可通过 `smartbrain_sql_query` 使用这些已保存连接，不必再向用户索要密码。

当前没有对等的 SSH 能力。用户无法在设置里保存多台服务器，也无法让 AI 使用已保存凭据做远程探测或执行。

本功能在数据库页签后面增加 SSH 页签，复用同一套多连接管理、测试和 SQLite 持久化模式，并在对话开启本地知识库后提供内置远程执行工具。

## 2. 目标

1. 用户可在「设置 → 本地知识库 → SSH」中配置多台服务器。
2. 每台服务器支持密码登录或私钥登录，私钥可填文件路径或直接粘贴内容，口令可选。
3. 用户可对当前表单（含未保存改动）执行连接测试，并看到成功或失败原因。
4. 配置保存到现有 SQLite `app_state`，key 为 `smartbrain.ssh.sources`。
5. 对话开启本地知识库后，AI 可通过 `smartbrain_ssh_exec` 使用已保存且已启用的服务器。
6. 每台服务器独立控制是否允许执行任意命令；默认只允许只读探测。
7. AI 不得再向用户索要已保存的密码或私钥，也不得改用 Python/shell 自行拼接 `ssh`/`plink`。

## 3. 非目标

1. 不新建独立业务表；不把 SSH 配置写入 `config.toml`。
2. 不做系统钥匙串或额外加密；凭据存储级别与数据库密码相同，明文 JSON 存本地 SQLite。
3. 不做跳板机链、端口转发、SFTP 文件浏览、交互式 PTY、长时间会话复用。
4. 不做每台服务器的命令白名单编辑器，也不做全局 SSH 权限设置页。
5. 不支持键盘交互、证书登录、`ssh-agent`、硬件密钥。
6. 本期不为 Skill 实验室单独增加 SSH 开关；它跟随主对话的本地知识库总开关。
7. 不把私钥或密码写入日志、提示词或工具回显。

## 4. 入口与信息架构

在 `SmartbrainPanel` 的页签顺序中，于「数据库」后插入「SSH」：

`知识` → `经验` → `数据库` → `SSH` → `数据库设置` → `小程序`

布局对齐 `SmartbrainDatabasePanel`：

- 左侧：已保存服务器列表，点击切换。
- 右侧：当前服务器编辑表单。
- 顶部操作：新增、保存、删除、连接测试。
- 未保存草稿只存在于设置页内存；AI 只读取已保存配置。

## 5. 数据模型与存储

### 5.1 存储位置

继续使用现有 SQLite `usage.db` 中的 `app_state` 表：

| 项 | 值 |
|---|---|
| 读写命令 | `app_state_get` / `app_state_set` |
| Key | `smartbrain.ssh.sources` |
| Value | `SmartbrainSshSource[]` 的 JSON |
| 后端读取路径 | `workspace_config_dir/usage.db` |

不新增 `smartbrain.ssh.settings` 全局配置。超时和输出限制使用代码内默认值。

### 5.2 服务器对象

```ts
type SmartbrainSshAuthMethod = "password" | "privateKey";

interface SmartbrainSshSource {
  id: string;
  name: string;
  enabled: boolean;
  host: string;
  port: number | null;
  username: string;
  authMethod: SmartbrainSshAuthMethod;
  password: string;
  privateKey: string;
  privateKeyPath: string;
  passphrase: string;
  allowExec: boolean;
  updatedAt: number;
}
```

字段约定：

- `id`：前端生成的稳定 UUID；更新时保持不变。
- `name`：显示名。空名称保存时回退为 `username@host`，再空则显示「未命名服务器」。
- `enabled`：默认 `true`。关闭后 AI 不可见、不可用。
- `host`：必填。
- `port`：空值按 `22` 处理。
- `username`：必填。
- `authMethod`：`password` 使用 `password`；`privateKey` 使用 `privateKey` 或 `privateKeyPath`。
- `privateKey` 与 `privateKeyPath` 同时存在时，优先使用 `privateKey` 内容。
- `allowExec`：默认 `false`。
- `updatedAt`：Unix 秒。

### 5.3 校验

保存前必须满足：

1. `host` 非空。
2. `username` 非空。
3. `authMethod === "password"` 时，`password` 非空。
4. `authMethod === "privateKey"` 时，`privateKey` 或 `privateKeyPath` 至少一个非空。
5. `port` 若填写，必须是 `1-65535` 的整数。

连接测试使用同一套校验，但针对当前草稿，不要求先保存。

## 6. 设置页行为

### 6.1 列表

每项显示：

- 名称
- `username@host:port`
- 启用/未启用，以及是否允许执行

没有服务器时显示空状态，并引导点击「新增服务器」。

### 6.2 表单字段

固定字段：

- 名称
- 启用此服务器
- Host
- Port，占位默认 22
- 用户名
- 认证方式：密码 / 私钥
- 允许远程执行（`allowExec`）

认证方式切换后只显示对应输入：

- 密码：密码框，可显示/隐藏。
- 私钥：私钥文件路径、私钥内容、可选口令。口令同样可显示/隐藏。

不提供连接串智能解析。

### 6.3 连接测试

前端调用 `smartbrain_test_ssh_connection`，传入当前草稿，不要求已保存。

后端流程：

1. 按草稿完成认证。
2. 超时默认 10 秒。
3. 执行只读探测：`uname -a || echo ok`。
4. 返回 `{ ok, message, host, port, username, timeoutSec }`。

成功文案示例：`连接成功 · deploy@192.168.1.10:22 · Linux web-01`

失败必须可读，且不回显密钥内容。典型原因：

- 主机不可达
- 超时
- 认证失败
- 私钥无效或口令错误
- 必填字段缺失

设置页连接测试不受 `enabled` 或 `allowExec` 限制。

### 6.4 保存与删除

- 保存：规范化字段后整表写回 `smartbrain.ssh.sources`。
- 删除：二次确认后从数组移除并写回。
- 新增：清空右侧表单，不立即写库；用户点击保存后才落盘。

## 7. 权限模型

每台服务器独立控制，不设全局 SSH 权限页。

| 状态 | AI 可用性 | 可执行命令 |
|---|---|---|
| `enabled=false` | 不可用 | 无 |
| `enabled=true` 且 `allowExec=false` | 可用 | 仅只读探测命令 |
| `enabled=true` 且 `allowExec=true` | 可用 | 单条非交互命令，但仍拦截高危命令 |

只读探测允许的完整命令集合：

```text
uname -a
hostname
whoami
pwd
uptime
df -h
free -h
id
echo ok
```

匹配规则：去掉首尾空白后精确匹配，或匹配上述命令加无害空格。带管道、重定向、`;`、`&&`、`||`、命令替换的变体一律视为非只读，必须 `allowExec=true`。

设置页测试命令 `uname -a || echo ok` 只用于测试接口，不属于 AI 只读白名单。

### 7.1 高危命令拦截

即使 `allowExec=true`，出现以下模式时拒绝执行：

- `rm -rf /`、`rm -rf /*`、`rm -rf ~`、`rm -rf $HOME`
- `mkfs`、`dd if=`
- `shutdown`、`reboot`、`halt`、`poweroff`、`init 0`、`init 6`
- 改写 `/etc/passwd`、`/etc/shadow`、`/etc/sudoers`
- `curl|wget ... | sh` 以及等价的远程脚本直接执行

拒绝时返回明确错误，不尝试降级执行。

本期不做用户可编辑的命令白名单。若后续需要，另开需求。

## 8. 后端执行层

新增模块 `src-tauri/src/smartbrain/ssh.rs`，由设置页测试命令和 AI 工具共用。

依赖：在 `src-tauri/Cargo.toml` 增加原生 SSH 客户端库，优先 `russh` + `russh-keys`。不调用本机 `ssh`/`plink`。

执行约束：

- 每次调用新建短连接，用完即关。
- 不分配 PTY。
- 不支持交互式输入。
- 默认超时 15 秒，最大 60 秒。
- 合并 stdout/stderr，按 UTF-8 有损解码。
- 输出超过 8000 字符时截断，并标记 `truncated=true`。
- 日志只记录 host、port、username、authMethod、是否成功；不记录 password、privateKey、passphrase。

认证顺序：

1. `authMethod=password`：用户名 + 密码。
2. `authMethod=privateKey`：先读 `privateKey`，否则读 `privateKeyPath`；若有 `passphrase` 则用于解锁。

## 9. AI 工具与提示词

### 9.1 工具暴露条件

仅当当前对话开启本地知识库时，才把 `smartbrain_ssh_exec` 加入工具列表，条件与 `smartbrain_sql_query` 相同。

未开启本地知识库时：

- 不暴露该工具。
- 不注入 SSH 服务器列表。
- 若被直接调用，返回本地知识库未开启的错误。

### 9.2 工具定义

```text
name: smartbrain_ssh_exec
description: Execute a command on a Local Knowledge Base-configured SSH server using saved credentials. Prefer this over Python/shell ssh/plink scripts. Do not ask the user for password or private key when the server is already configured.
parameters:
  server: string, optional when only one enabled server exists. Display name, host, or username@host.
  command: string, required. A single non-interactive remote command.
  timeout_sec: integer, optional, 1-60, default 15.
```

解析服务器时按以下顺序匹配已启用配置：

1. 精确匹配 `name`
2. 精确匹配 `host`
3. 精确匹配 `username@host`
4. 忽略大小写的上述匹配

0 个启用服务器：提示前往「设置 → 本地知识库 → SSH」配置。  
多个启用服务器且 `server` 为空或无法唯一匹配：列出可用服务器名后拒绝。

### 9.3 系统提示

在 `render_smartbrain_runtime_prompt` 中，数据库提示后追加 SSH 段落。列出全部已保存服务器的非敏感字段，但按可用性分组，风格对齐数据库提示：

```text
### 已配置且当前可用的 SSH 服务器
- `生产跳板机`: 目标=`deploy@192.168.1.10:22`；认证=`privateKey`；执行=`只读探测`
- `测试机-1`: 目标=`root@10.0.0.8:22`；认证=`password`；执行=`允许远程执行`

### 已配置但当前应跳过的 SSH 服务器
- `下线机`: 目标=`ops@10.0.0.9:22`；认证=`password`；执行=`已禁用`
```

解析 `server` 时只匹配 `enabled=true` 的服务器。禁用项仅用于提示“已配置但不可用”，避免模型误调用。

提示规则：

- 查看主机状态、磁盘、当前用户时，使用只读探测命令。
- 安装软件、改配置、重启服务等必须该服务器已开启 `allowExec`。
- 必须使用 `smartbrain_ssh_exec`，不得手写 SSH 脚本，不得再要密码或私钥。
- 提示词中禁止出现 password、privateKey、passphrase。

## 10. 错误处理

| 场景 | 行为 |
|---|---|
| 设置页字段不完整 | 阻止测试/保存，显示对应中文错误 |
| 测试连接失败 | 显示失败原因，不关闭表单，不自动清空密码 |
| 保存失败 | 保留草稿，显示错误 |
| 对话未开本地知识库 | 工具不可用；误调用则返回未开启错误 |
| 无启用服务器 | 提示去设置页配置 |
| 服务器名不唯一或不存在 | 列出可用名称 |
| 只读模式执行了非白名单命令 | 拒绝，并说明需要开启「允许远程执行」 |
| 高危命令 | 拒绝 |
| 认证/网络失败 | 返回原因，不中断整个对话 |

SSH 失败不得覆盖或丢弃当前对话中的其他工具结果。

## 11. 测试计划

### 11.1 前端状态

- 空列表加载为 `[]`。
- 保存后再次加载字段完整，包括 `allowExec=false` 默认值。
- 旧数据缺字段时回填默认值，不崩溃。
- 密码模式下缺密码、私钥模式下缺密钥时拒绝保存。

### 11.2 权限

- 只读模式允许 `uname -a`、`df -h`。
- 只读模式拒绝 `ls /tmp`、`systemctl restart nginx`、`uname -a && rm -rf /tmp/a`。
- `allowExec=true` 仍拒绝 `rm -rf /`、`shutdown now`。

### 11.3 服务器解析

- 仅一台启用服务器时可省略 `server`。
- 名称、host、`user@host` 都能匹配。
- 重名时要求更精确的标识。

### 11.4 命令与 UI

- `smartbrain_test_ssh_connection` 使用草稿，不要求先保存。
- `smartbrain_ssh_exec` 只读已保存配置。
- 设置页文案中英都有对应 key。
- 凭据不出现在工具回显和提示词中。

真实网络登录可作为手动验收，不作为默认单测依赖。

## 12. 主要改动文件

前端：

- `src/components/settings/SmartbrainPanel.tsx`
- `src/components/settings/SmartbrainSshPanel.tsx`（新增）
- `src/components/settings/smartbrainSshState.ts`（新增）
- `src/__tests__/smartbrainSshState.test.ts`（新增）
- `src/i18n/zh-CN/common.json`
- `src/i18n/en-US/common.json`

后端：

- `src-tauri/Cargo.toml`
- `src-tauri/src/smartbrain/mod.rs`
- `src-tauri/src/smartbrain/ssh.rs`（新增）
- `src-tauri/src/smartbrain/commands.rs`
- `src-tauri/src/lib.rs`
- `src-tauri/src/agent/prompt_context.rs`
- `src-tauri/src/tool_executor/tool_specs_support.rs`
- `src-tauri/src/tool_executor/smartbrain_support.rs`
- `src-tauri/src/tool_executor.rs`
- 对应 Rust 单测

## 13. 验收标准

1. 设置 → 本地知识库中，数据库后面出现 SSH 页签。
2. 用户可新增、编辑、删除、启用/禁用多台服务器，刷新后配置仍在。
3. 密码和私钥两种认证都可测试连接，成功和失败都有明确反馈。
4. 配置写入 SQLite `app_state.smartbrain.ssh.sources`。
5. 对话开启本地知识库后，AI 能看到已启用服务器，并用 `smartbrain_ssh_exec` 执行命令。
6. 未开启 `allowExec` 时，非只读命令被拒绝。
7. 高危命令被拒绝。
8. 未开启本地知识库时，SSH 工具不可用。
9. 密码和私钥不会出现在提示词、日志或工具输出中。
