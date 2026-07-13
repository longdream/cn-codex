# CN-Codex 自动更新设计（方案 A）

## 目标

为便携版 `CN-Codex.exe` 提供自动更新能力：

1. 主程序启动后自动拉起同目录 `updater.exe` 检查线上版本。
2. 线上服务仅提供最新版本元数据与 exe 下载地址。
3. 用户确认更新后，由 `updater.exe` 结束主程序、显示下载进度、覆盖 exe 并重启。

## 已确认决策

- 启动方式：主程序启动时自动拉起 `updater.exe --check`。
- 更新服务：轻量静态服务 + `latest.json`，监听 `47.113.221.244:5005`。
- 进度条：独立 `updater.exe` 小窗口。
- 小版本更新：仅替换主程序 exe。

## 架构

```text
CN-Codex.exe
  └─ 启动后 invoke: update_check
        └─ spawn updater.exe --check --current-version 0.1.0
              └─ GET http://47.113.221.244:5005/latest.json
                    └─ 返回 UpdateCheckResult 给主程序
                          └─ 有新版本时弹窗
                                └─ 用户确认后 invoke: update_start
                                      └─ spawn updater.exe --update ...
                                            └─ 等待主进程退出
                                            └─ 下载 exe 并显示进度
                                            └─ 覆盖 CN-Codex.exe
                                            └─ 启动新主程序
```

## 组件

### 1. update-server

目录：

```text
update-server/
  server.py              # 纯 Python 标准库，无需编译
  requirements.txt       # 无第三方依赖
  public/
    index.html
    latest.json
    files/
  deploy/
    cn-codex-update.service
    deploy.sh
  README.md
```

职责：

- 静态托管 `latest.json` 与 `files/*`
- 提供中文首页，展示当前最新版本与下载 URL
- 默认监听 `0.0.0.0:5005`
- 部署方式：直接 `python3 server.py`，不依赖 Rust 编译

`latest.json` 字段：

```json
{
  "version": "0.1.1",
  "url": "http://47.113.221.244:5005/files/CN-Codex-0.1.1.exe",
  "sha256": "",
  "notes": "修复若干问题",
  "force": false,
  "publishedAt": "2026-03-21T12:00:00Z"
}
```

### 2. updater.exe

实现位置：`src-tauri/src/bin/updater.rs`

模式：

| 模式 | 参数 | 行为 |
|------|------|------|
| check | `--check --current-version <v> [--manifest-url <url>]` | 请求 latest.json，比较版本，stdout 输出 JSON 后退出 |
| update | `--update --url <url> --target <path> --pid <pid> [--sha256 <hash>] [--launch <path>]` | 等待主进程退出，下载到 `.new`，可选校验，覆盖目标，启动新主程序 |

输出（check）：

```json
{
  "updateAvailable": true,
  "currentVersion": "0.1.0",
  "latestVersion": "0.1.1",
  "url": "http://47.113.221.244:5005/files/CN-Codex-0.1.1.exe",
  "sha256": "",
  "notes": "修复若干问题",
  "force": false
}
```

### 3. 主程序接入

Rust 命令：

- `update_check()`：启动 updater 检查并解析结果
- `update_start(url, sha256?)`：启动 updater 更新并退出主程序

前端：

- 应用初始化完成后后台检查更新
- 发现新版本时弹窗展示版本号与 notes
- 用户点击更新后调用 `update_start`

发布脚本：

- `scripts/release-portable.ps1` 将 `updater.exe` 复制到发布目录

## 版本比较

- 使用 `major.minor.patch` 数值比较
- 仅当远端版本严格大于本地版本时提示更新
- 网络失败静默忽略，不影响主程序启动

## 安全与容错

1. 下载到 `CN-Codex.exe.new`
2. 可选 sha256 校验
3. 覆盖前等待主进程退出（超时重试）
4. 覆盖失败保留旧 exe，并提示错误
5. 成功后删除临时文件并启动新主程序

## 非目标

- 不做完整管理后台
- 不做大版本差分包
- 不在主程序内嵌下载进度条
