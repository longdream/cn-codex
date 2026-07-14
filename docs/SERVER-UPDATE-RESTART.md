# 服务器更新与重启说明

适用服务器：`47.113.221.244`

如果你改了本机仓库里的 `latest.json`，但访问：

```text
http://47.113.221.244:5005/latest.json
```

还是旧内容，原因通常是：**只改了本地文件，没有上传到服务器真实目录**。

服务实际读取的是服务器上的：

```text
/opt/cn-codex-update/public/latest.json
```

不是你电脑里的：

```text
D:\rustwork\cn-codex\update-server\public\latest.json
D:\rustwork\cn-codex\build\latest.json
```

> 重要：服务器上可能同时存在
>
> - `/www/wwwroot/cn-codex-update-server`（网站目录 / 人工上传目录）
> - `/opt/cn-codex-update/public`（**5005 端口真正读取的目录**）
>
> 只改 `/www/wwwroot/...` **不会**让 `http://IP:5005/latest.json` 生效。
> 真正对外服务进程是：
>
> ```text
> python3 ... /opt/cn-codex-update/server.py --bind 0.0.0.0:5005 --public-dir /opt/cn-codex-update/public
> ```

---

## 1. 服务器上有哪些服务

| 服务 | 端口 | 安装目录 | systemd 服务名 | 作用 |
|------|------|----------|----------------|------|
| 自动更新服务 | `5005` | `/opt/cn-codex-update` | `cn-codex-update.service` | 提供 `/latest.json` 和更新包下载 |
| 手机中转服务 | `8080` | `/opt/cn-codex-relay` | `cn-codex-relay.service` | 手机远程控制 WebSocket / 静态页 |
| 官网/文档站 | `8081` | 视部署方式而定 | 可能是 nginx / 其他静态服务 | 官网与使用指南 |

### 自动更新服务有两个目录，别搞混

| 目录 | 是否被 5005 使用 | 说明 |
|------|------------------|------|
| `/opt/cn-codex-update/public` | **是** | systemd 服务 `cn-codex-update` 真正读取这里 |
| `/www/wwwroot/cn-codex-update-server` | **否** | 宝塔/网站目录，方便人工放文件；默认**不对外提供 5005** |

当前线上服务启动参数等价于：

```bash
/www/server/pyporject_evn/versions/3.10.14/bin/python3.10 \
  /opt/cn-codex-update/server.py \
  --bind 0.0.0.0:5005 \
  --public-dir /opt/cn-codex-update/public
```

所以：

- 改 `/www/wwwroot/cn-codex-update-server/latest.json` → **5005 不会变**
- 改 `/opt/cn-codex-update/public/latest.json` → **5005 立即生效（无需重启）**

当前自动更新相关接口：

- 健康检查：`http://47.113.221.244:5005/healthz`
- 最新版本：`http://47.113.221.244:5005/latest.json`
- 下载目录：`http://47.113.221.244:5005/files/...`

---

## 2. 你这次“改了 latest.json 没生效”该怎么处理

### 结论

- **只改本地文件：线上不会变**
- **只改服务器 `latest.json`：版本号会变，但下载包也必须一起上传**
- **只上传 zip 不改 `latest.json`：线上仍会指向旧版本**
- **更新包 / latest.json 上传后：通常不需要重启更新服务**

### 正确发布目录（推荐）

本地构建后优先用这个目录上传：

```text
build/update-artifacts/update-upload/
  latest.json
  files/CN-Codex-<version>.zip
```

例如当前 1.0.1：

```text
build/update-artifacts/update-upload/latest.json
build/update-artifacts/update-upload/files/CN-Codex-1.0.1.zip
```

对应服务器目录：

```text
/opt/cn-codex-update/public/latest.json
/opt/cn-codex-update/public/files/CN-Codex-1.0.1.zip
```

---

## 3. 更新“自动更新服务”内容（最常见）

### 方式 A：从 Windows 直接上传（推荐）

在 **PowerShell** 中执行：

```powershell
# 1) 进入仓库根目录
cd D:\rustwork\cn-codex

# 2) 上传 latest.json + 更新包
scp build\update-artifacts\update-upload\latest.json root@47.113.221.244:/opt/cn-codex-update/public/latest.json
scp build\update-artifacts\update-upload\files\CN-Codex-1.0.1.zip root@47.113.221.244:/opt/cn-codex-update/public/files/
```

如果本机没有 `scp`，可用 WinSCP / FinalShell / 宝塔面板，把这两个文件拖到对应目录。

### 方式 B：先上传整个 update-upload 目录

```powershell
scp -r build\update-artifacts\update-upload\* root@47.113.221.244:/opt/cn-codex-update/public/
```

### 方式 C：先 SSH 到服务器再手工改

```bash
ssh root@47.113.221.244
```

然后：

```bash
# 查看当前线上内容
cat /opt/cn-codex-update/public/latest.json
ls -lah /opt/cn-codex-update/public/files/

# 编辑 latest.json
vi /opt/cn-codex-update/public/latest.json
```

示例内容：

```json
{
  "version": "1.0.1",
  "url": "http://47.113.221.244:5005/files/CN-Codex-1.0.1.zip",
  "sha256": "6c9f19ac07769d9ca63a932c8c02d7485fde9971c4d5898aa6a085cb3799c441",
  "notes": "CN-Codex 1.0.1",
  "force": false,
  "publishedAt": "2026-07-13T13:27:23Z",
  "packageType": "zip"
}
```

注意：

1. `url` 必须指向服务器上真实存在的文件
2. 推荐使用 zip 包，不要再写旧的 `.exe`
3. `sha256` 建议填真实哈希，避免客户端校验失败
4. **只更新静态文件时，一般不需要 restart**

### 验证是否生效

本地或服务器都可验证：

```bash
curl http://47.113.221.244:5005/latest.json
curl -I http://47.113.221.244:5005/files/CN-Codex-1.0.1.zip
curl http://47.113.221.244:5005/healthz
```

期望：

- `latest.json` 的 `version` 已变成你上传的版本
- zip 返回 `200`
- `healthz` 返回 `ok`

浏览器再打开：

```text
http://47.113.221.244:5005/latest.json
```

如果浏览器还显示旧内容，先强制刷新（`Ctrl+F5`）。服务本身已返回 `Cache-Control: no-cache`。

---

## 4. 什么时候需要重启服务

### 4.1 自动更新服务 `cn-codex-update`

#### 只更新 `latest.json` / zip 包

**不需要重启。**

服务每次请求都会重新读取 `public/latest.json`。

#### 更新了服务程序本身

以下任一情况需要重启：

- 改了 `update-server/server.py`
- 改了 `update-server/run.sh`
- 改了 `update-server/deploy/cn-codex-update.service`
- 首次部署 / 服务挂了

上传代码后重启：

```bash
ssh root@47.113.221.244

# 方式 1：直接重启
systemctl restart cn-codex-update.service
systemctl status cn-codex-update.service --no-pager

# 方式 2：用仓库部署脚本（会覆盖程序文件并重启）
cd /path/to/cn-codex/update-server
bash deploy/deploy.sh
```

常用排查：

```bash
systemctl status cn-codex-update.service --no-pager
journalctl -u cn-codex-update.service -n 100 --no-pager
ss -lntp | grep 5005
curl http://127.0.0.1:5005/healthz
curl http://127.0.0.1:5005/latest.json
```

### 4.2 手机中转服务 `cn-codex-relay`

更新这些内容后需要重启：

- `cn-codex-relay` 二进制
- `mobile-dist` 静态资源

部署示例：

```bash
# 在服务器上，进入已准备好的 deploy 目录后
bash deploy.sh
```

或手动：

```bash
systemctl restart cn-codex-relay.service
systemctl status cn-codex-relay.service --no-pager
journalctl -u cn-codex-relay.service -n 100 --no-pager
curl http://127.0.0.1:8080/
```

### 4.3 官网 `8081`

如果官网是独立 nginx / 静态目录：

- 只换 HTML / 静态文件：通常 **reload nginx** 或不重启
- 换了站点程序：按对应服务重启

常见 nginx：

```bash
nginx -t
systemctl reload nginx
# 或
systemctl restart nginx
```

---

## 5. 该更新哪些东西到服务器

按目标选择，不要无传。

### 场景 A：只想让客户端检查更新看到新版本

上传：

```text
build/update-artifacts/update-upload/latest.json
build/update-artifacts/update-upload/files/CN-Codex-<version>.zip
```

到：

```text
/opt/cn-codex-update/public/
```

是否重启：

- **否**

### 场景 B：更新自动更新服务程序（Python 服务逻辑变了）

上传/覆盖：

```text
update-server/server.py
update-server/run.sh
update-server/deploy/cn-codex-update.service   # 若 unit 有改动
update-server/public/*                         # 如需同步默认静态内容
```

到：

```text
/opt/cn-codex-update/
```

然后：

```bash
systemctl daemon-reload   # 仅 unit 有改动时
systemctl restart cn-codex-update.service
```

或直接跑：

```bash
bash update-server/deploy/deploy.sh
```

### 场景 C：更新手机中转 / 扫码控制

更新：

```text
relay-server 编译出的 cn-codex-relay 二进制
mobile-dist 静态资源
```

到：

```text
/opt/cn-codex-relay/
```

然后：

```bash
systemctl restart cn-codex-relay.service
```

### 场景 D：更新官网/使用指南

更新 `8081` 对应静态站点目录或站点源码，然后按 nginx/站点服务要求 reload/restart。

---

## 6. 推荐的完整发布流程（从本地构建到线上）

### 步骤 1：本地生成更新产物

如果你已经有发布包，可用现成目录：

```text
build/update-artifacts/update-upload/
```

如果需要重新生成，走完整发布脚本（仓库根目录）：

```bat
scripts\release-portable.bat
```

或已有便携目录时，用：

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\prepare-update-artifacts.ps1 `
  -Version 1.0.1 `
  -OutDir .\build\update-artifacts `
  -PortableDir .\build\你的便携目录 `
  -BaseUrl http://47.113.221.244:5005 `
  -Notes "CN-Codex 1.0.1"
```

成功后应看到：

```text
build/update-artifacts/update-upload/latest.json
build/update-artifacts/update-upload/files/CN-Codex-1.0.1.zip
```

### 步骤 2：上传到服务器

```powershell
scp build\update-artifacts\update-upload\latest.json root@47.113.221.244:/opt/cn-codex-update/public/latest.json
scp build\update-artifacts\update-upload\files\CN-Codex-1.0.1.zip root@47.113.221.244:/opt/cn-codex-update/public/files/
```

### 步骤 3：线上验证

```bash
curl http://47.113.221.244:5005/latest.json
curl -I http://47.113.221.244:5005/files/CN-Codex-1.0.1.zip
```

### 步骤 4：客户端验证

1. 打开已安装的旧版 CN-Codex
2. 触发检查更新
3. 应提示新版本并可下载安装

---

## 7. 服务常用命令速查

### 自动更新服务

```bash
systemctl status cn-codex-update.service
systemctl restart cn-codex-update.service
systemctl stop cn-codex-update.service
systemctl start cn-codex-update.service
journalctl -u cn-codex-update.service -f
```

### 手机中转服务

```bash
systemctl status cn-codex-relay.service
systemctl restart cn-codex-relay.service
systemctl stop cn-codex-relay.service
systemctl start cn-codex-relay.service
journalctl -u cn-codex-relay.service -f
```

### 查看端口占用

```bash
ss -lntp | grep -E '5005|8080|8081'
```

### 开机自启

```bash
systemctl enable cn-codex-update.service
systemctl enable cn-codex-relay.service
```

---

## 8. 常见问题排查

### Q1. 我改了本地 `latest.json`，线上还是旧的

正常。必须上传到：

```text
/opt/cn-codex-update/public/latest.json
```

### Q2. 线上 `latest.json` 已改，但下载 404

检查：

```bash
ls -lah /opt/cn-codex-update/public/files/
cat /opt/cn-codex-update/public/latest.json
```

确认 `url` 中的文件名与 `files/` 目录完全一致。

### Q3. 服务重启后 5005 不通

```bash
systemctl status cn-codex-update.service --no-pager
journalctl -u cn-codex-update.service -n 100 --no-pager
ss -lntp | grep 5005
```

常见原因：

1. Python 版本太旧
2. `PUBLIC_DIR` 路径不对
3. 端口被占用
4. 防火墙未放行 5005

### Q4. 防火墙放行示例

```bash
# firewalld
firewall-cmd --add-port=5005/tcp --permanent
firewall-cmd --add-port=8080/tcp --permanent
firewall-cmd --reload

# 或 iptables
iptables -I INPUT -p tcp --dport 5005 -j ACCEPT
iptables -I INPUT -p tcp --dport 8080 -j ACCEPT
```

### Q5. 当前线上仍显示 0.1.0 时怎么快速修好

在开发机执行：

```powershell
cd D:\rustwork\cn-codex
scp build\update-artifacts\update-upload\latest.json root@47.113.221.244:/opt/cn-codex-update/public/latest.json
scp build\update-artifacts\update-upload\files\CN-Codex-1.0.1.zip root@47.113.221.244:/opt/cn-codex-update/public/files/
curl http://47.113.221.244:5005/latest.json
```

若 `curl` 已显示 `1.0.1`，说明更新成功，**不需要重启服务**。

### Q6. 我明明改了 `/www/wwwroot/cn-codex-update-server`，为什么还是旧的？

因为 **5005 端口服务没有读这个目录**。

请检查服务真实 public 目录：

```bash
systemctl cat cn-codex-update.service
ss -lntp | grep 5005
ps -ef | grep server.py | grep -v grep
cat /opt/cn-codex-update/public/latest.json
```

正确上传目标永远是：

```text
/opt/cn-codex-update/public/latest.json
/opt/cn-codex-update/public/files/CN-Codex-<version>.zip
```

如果你希望统一到宝塔目录，需要改 systemd 的 `PUBLIC_DIR` 并重启服务；否则请始终更新 `/opt/cn-codex-update/public`。

---

## 9. 一句话记忆

- **改版本信息 / 换更新包**：上传 `public/latest.json` + `public/files/*.zip`，一般 **不用重启**
- **改更新服务代码**：更新 `/opt/cn-codex-update` 后 `systemctl restart cn-codex-update`
- **改手机中转**：更新 `/opt/cn-codex-relay` 后 `systemctl restart cn-codex-relay`
- **本地改文件 ≠ 线上生效**，必须以服务器目录为准
