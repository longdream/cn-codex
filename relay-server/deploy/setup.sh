#!/bin/bash
set -e

echo "============================================"
echo "  CN-Codex Relay - Full Setup"
echo "  (Build + Deploy on Alibaba Cloud Linux)"
echo "============================================"
echo ""

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
INSTALL_DIR="/opt/cn-codex-relay"
SERVICE_NAME="cn-codex-relay"

resolve_relay_root() {
    script_dir="$1"
    cwd="$2"
    for candidate in \
        "$script_dir" \
        "$script_dir/.." \
        "$script_dir/relay-server" \
        "$script_dir/../relay-server" \
        "$cwd" \
        "$cwd/relay-server"
    do
        if [ -f "$candidate/Cargo.toml" ]; then
            echo "$candidate"
            return 0
        fi
    done

    return 1
}

# [1] 安装 Rust（如需）
if ! command -v cargo >/dev/null 2>&1; then
    echo "[1/6] Installing Rust toolchain..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    . "$HOME/.cargo/env"
else
    echo "[1/6] Rust already installed: $(rustc --version)"
fi

# [2] 编译
echo "[2/6] Building relay server (release mode)..."
RELAY_ROOT="$(resolve_relay_root "$SCRIPT_DIR" "$(pwd)")"
if [ -z "$RELAY_ROOT" ]; then
    echo "[ERROR] Cannot find Cargo.toml for relay-server."
    echo "        Please run setup.sh from one of these locations:"
    echo "        - relay-server/deploy"
    echo "        - relay-server"
    echo "        - repository root containing relay-server/"
    exit 1
fi
cd "$RELAY_ROOT"
echo "  - Relay source root: $RELAY_ROOT"

# 将构建产物写到外部目录，避免 deploy 目录出现庞大的 target。
BUILD_TARGET_DIR="${CARGO_TARGET_DIR:-/tmp/cn-codex-relay-target}"
echo "  - Cargo target dir: $BUILD_TARGET_DIR"
cargo build --release --manifest-path "$RELAY_ROOT/Cargo.toml" --target-dir "$BUILD_TARGET_DIR"
BINARY="$BUILD_TARGET_DIR/release/cn-codex-relay"

if [ ! -f "$BINARY" ]; then
    echo "[ERROR] Build succeeded but binary not found at $BINARY"
    exit 1
fi

# [3] 安装
echo "[3/6] Installing to $INSTALL_DIR..."
mkdir -p "$INSTALL_DIR"

# 在覆盖可执行文件前，先停止已有服务并等待进程完全退出。
# 目的：
# 1) 避免 `cp: ... Text file busy`（旧进程仍占用二进制）
# 2) 防止替换过程中出现“新旧版本混用”的不一致状态
if command -v systemctl >/dev/null 2>&1; then
    if systemctl is-active --quiet "$SERVICE_NAME"; then
        echo "  - Service is running, stopping $SERVICE_NAME first..."
        systemctl stop "$SERVICE_NAME"

        # 最多等待 20 秒，确保 systemd 状态进入 inactive。
        # 如果超时，直接失败退出，避免在进程仍占用文件时继续安装。
        wait_seconds=0
        while systemctl is-active --quiet "$SERVICE_NAME"; do
            sleep 1
            wait_seconds=$((wait_seconds + 1))
            if [ "$wait_seconds" -ge 20 ]; then
                echo "[ERROR] Failed to stop $SERVICE_NAME within 20s."
                echo "        Please check service state and retry."
                systemctl status "$SERVICE_NAME" --no-pager -l || true
                exit 1
            fi
        done
        echo "  - Service stopped."
    else
        echo "  - Service is not active, continue installing."
    fi
fi

# 先写入临时文件再原子替换，降低安装过程中文件不完整的风险。
cp "$BINARY" "$INSTALL_DIR/cn-codex-relay.new"
chmod +x "$INSTALL_DIR/cn-codex-relay.new"
mv -f "$INSTALL_DIR/cn-codex-relay.new" "$INSTALL_DIR/cn-codex-relay"

# 复制 mobile-dist（优先 relay 根目录，其次脚本目录）
# 强制要求存在 index.html，避免服务启动成功但 /m 页面恒定 404。
if [ -d "$RELAY_ROOT/mobile-dist" ] && [ -f "$RELAY_ROOT/mobile-dist/index.html" ]; then
    rm -rf "$INSTALL_DIR/mobile-dist"
    cp -r "$RELAY_ROOT/mobile-dist" "$INSTALL_DIR/mobile-dist"
    echo "  - mobile-dist installed from relay root."
elif [ -d "$SCRIPT_DIR/mobile-dist" ] && [ -f "$SCRIPT_DIR/mobile-dist/index.html" ]; then
    rm -rf "$INSTALL_DIR/mobile-dist"
    cp -r "$SCRIPT_DIR/mobile-dist" "$INSTALL_DIR/mobile-dist"
    echo "  - mobile-dist installed from deploy directory."
else
    echo "  [ERROR] mobile-dist/index.html not found."
    echo "          This will cause /m/<room_id> to return 404 (index.html not found)."
    echo "          Please upload mobile-dist together with deploy files."
    echo "          If building locally, run: cd mobile-web && pnpm build"
    exit 1
fi

# [4] 安装 systemd 服务
echo "[4/6] Installing systemd service..."
cat > /etc/systemd/system/$SERVICE_NAME.service << 'EOF'
[Unit]
Description=CN-Codex Relay Server
After=network.target

[Service]
Type=simple
User=root
WorkingDirectory=/opt/cn-codex-relay
ExecStart=/opt/cn-codex-relay/cn-codex-relay --port 8080 --static-dir /opt/cn-codex-relay/mobile-dist
Restart=always
RestartSec=3
Environment=RUST_LOG=cn_codex_relay=info

[Install]
WantedBy=multi-user.target
EOF

systemctl daemon-reload

# [5] 启动服务
echo "[5/6] Starting service..."
systemctl enable "$SERVICE_NAME"
systemctl restart "$SERVICE_NAME"

# [6] 开放端口
echo "[6/6] Configuring firewall..."
if command -v firewall-cmd >/dev/null 2>&1; then
    firewall-cmd --add-port=8080/tcp --permanent 2>/dev/null || true
    firewall-cmd --reload 2>/dev/null || true
    echo "  - firewall-cmd: port 8080 opened."
elif command -v ufw >/dev/null 2>&1; then
    ufw allow 8080/tcp 2>/dev/null || true
    echo "  - ufw: port 8080 allowed."
else
    echo "  - No firewall tool found, skipping."
    echo "    If using Alibaba Cloud security group, add inbound rule for port 8080."
fi

# 检查状态
echo ""
sleep 1
echo "============================================"
echo "  Service status:"
echo "--------------------------------------------"
systemctl status "$SERVICE_NAME" --no-pager -l || true
echo ""
echo "============================================"
echo "  Deploy complete!"
echo ""
echo "  Service: $SERVICE_NAME (port 8080)"
echo "  Binary:  $INSTALL_DIR/cn-codex-relay"
echo "  Static:  $INSTALL_DIR/mobile-dist/"
echo ""
echo "  Useful commands:"
echo "    systemctl status $SERVICE_NAME"
echo "    systemctl restart $SERVICE_NAME"
echo "    systemctl stop $SERVICE_NAME"
echo "    journalctl -u $SERVICE_NAME -f"
echo ""
echo "  IMPORTANT: Also open port 8080 in Alibaba"
echo "  Cloud security group (ECS console)."
echo ""
echo "  Then in CN-Codex PC settings, set:"
echo "    relay_server_url = http://<server-ip>:8080"
echo "============================================"
