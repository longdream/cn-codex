# Auto Update Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 为 CN-Codex 便携版增加自动更新：线上 latest.json 服务 + updater.exe + 主程序启动检查/确认更新。

**Architecture:** 主程序启动后静默调用同目录 `updater.exe --check` 拉取 `http://47.113.221.244:5005/latest.json`；若有新版本则弹窗；用户确认后由 updater 结束主程序、下载并覆盖 `CN-Codex.exe`，再重启。

**Tech Stack:** Python 标准库更新服务 + Rust 独立 updater bin、Tauri commands、React 更新弹窗

---

### Task 1: update-server 轻量更新站（Python）

**Files:**
- Create: `update-server/server.py`
- Create: `update-server/requirements.txt`
- Create: `update-server/public/index.html`
- Create: `update-server/public/latest.json`
- Create: `update-server/public/files/.gitkeep`
- Create: `update-server/deploy/cn-codex-update.service`
- Create: `update-server/deploy/deploy.sh`
- Create: `update-server/README.md`

### Task 2: updater.exe

**Files:**
- Create: `src-tauri/src/bin/updater.rs`
- Modify: `src-tauri/Cargo.toml` 增加 `[[bin]] name = "updater"` 与必要依赖（clap/sha2/hex/self_update 风格手写下载）

### Task 3: 主程序命令

**Files:**
- Create: `src-tauri/src/commands/update.rs`
- Modify: `src-tauri/src/commands/mod.rs`
- Modify: `src-tauri/src/lib.rs` 注册 command

### Task 4: 前端更新弹窗

**Files:**
- Create: `src/api/update.ts`
- Create: `src/components/common/UpdateModal.tsx`
- Modify: `src/api/index.ts`
- Modify: `src/App.tsx`
- Modify: `src/i18n/zh-CN/common.json`
- Modify: `src/i18n/en-US/common.json`

### Task 5: 发布脚本

**Files:**
- Modify: `scripts/release-portable.ps1` 复制 `updater.exe`
