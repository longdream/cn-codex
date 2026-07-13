# CN-Codex Update Server

轻量自动更新服务（**纯 Python 标准库**，无需编译、无需 pip 安装第三方包）。

默认监听 `0.0.0.0:5005`。

## 提供接口

- `GET /`：展示当前最新版本与下载地址
- `GET /latest.json`：客户端检查更新
- `GET /files/*`：更新包下载（推荐 zip 便携包）
- `GET /healthz`：健康检查

> 客户端流程：下载 zip → 解压 → 覆盖安装目录（保留用户配置/会话等本地数据）。

## 环境要求

- Python **3.6+**（推荐 3.8+ / 3.10）
- 无需 `cargo` / 无需编译
- 无需安装第三方依赖

> 注意：很多 Linux 服务器上 `/usr/bin/python3` 可能是很旧的版本。  
> 部署脚本会自动选择可用 Python（含 conda 环境），也可用 `PYTHON_BIN` 手动指定。

## 本地运行

```bash
cd update-server
python3 server.py --bind 0.0.0.0:5005 --public-dir ./public
```

或：

```bash
cd update-server
bash run.sh
```

Windows PowerShell：

```powershell
cd update-server
python .\server.py --bind 0.0.0.0:5005 --public-dir .\public
```

## 服务器部署

```bash
cd update-server
# 若系统 python3 太旧，可先指定你的 3.10：
# export PYTHON_BIN=$(command -v python3.10)
# 或：export PYTHON_BIN=/root/miniconda3/bin/python
bash deploy/deploy.sh
```

验证：

```bash
curl http://127.0.0.1:5005/healthz
curl http://127.0.0.1:5005/latest.json
systemctl status cn-codex-update.service
```

## 线上更新 / 重启（必读）

如果你改了本机 `latest.json`，但 `http://47.113.221.244:5005/latest.json` 没变，说明**只改了本地，没有上传到服务器**。

服务真实读取目录：

```text
/opt/cn-codex-update/public/latest.json
/opt/cn-codex-update/public/files/
```

注意：服务器上可能还有 `/www/wwwroot/cn-codex-update-server`，那是网站目录，**不是** 5005 服务当前读取的目录。只改 wwwroot 不会生效。

推荐直接上传构建产物：

```powershell
scp build\update-artifacts\update-upload\latest.json root@47.113.221.244:/opt/cn-codex-update/public/latest.json
scp build\update-artifacts\update-upload\files\CN-Codex-1.0.0.zip root@47.113.221.244:/opt/cn-codex-update/public/files/
```

规则：

- 只更新 `latest.json` 和 zip 包：**不需要重启服务**
- 更新了 `server.py` / `run.sh` / systemd unit：**需要重启**

```bash
systemctl restart cn-codex-update.service
systemctl status cn-codex-update.service --no-pager
```

完整说明（含中转服务、排查命令、发布流程）见：

- [docs/SERVER-UPDATE-RESTART.md](../docs/SERVER-UPDATE-RESTART.md)

## 发布新版本

本地构建会自动生成服务器覆盖包：

```text
build/update-artifacts/update-upload/
  latest.json
  files/CN-Codex-<version>.zip
```

或使用完整发布：

```text
publish/update-artifacts/update-upload/
  latest.json
  files/CN-Codex-<version>.zip
```

上传覆盖服务器目录即可（无需重启服务）：

```bash
# 从 Windows 开发机上传示例
scp -r build/update-artifacts/update-upload/* root@47.113.221.244:/opt/cn-codex-update/public/
```

手工发布时也可以：

1. 上传新便携 zip 包到 `public/files/`，例如：
   - `public/files/CN-Codex-0.1.1.zip`
2. 修改 `public/latest.json`：

```json
{
  "version": "0.1.1",
  "url": "http://47.113.221.244:5005/files/CN-Codex-0.1.1.zip",
  "sha256": "",
  "notes": "修复自动更新与若干问题",
  "force": false,
  "publishedAt": "2026-03-21T12:00:00Z",
  "packageType": "zip"
}
```

3. 无需重启服务：每次请求都会重新读取 `latest.json`。

## 目录结构

```text
update-server/
  server.py
  run.sh
  requirements.txt
  public/
    index.html
    latest.json
    files/
  deploy/
    cn-codex-update.service
    deploy.sh
  README.md
```
