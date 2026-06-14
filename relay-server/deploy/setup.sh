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

# [1] 安装 Rust（如需）
if ! command -v cargo &> /dev/null; then
    echo "[1/6] Installing Rust toolchain..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    source "$HOME/.cargo/env"
else
    echo "[1/6] Rust already installed: $(rustc --version)"
fi

# [2] 编译
echo "[2/6] Building relay server (release mode)..."
if [ -d "$SCRIPT_DIR/relay-server" ]; then
    cd "$SCRIPT_DIR/relay-server"
elif [ -f "$SCRIPT_DIR/Cargo.toml" ]; then
    cd "$SCRIPT_DIR"
else
    echo "[ERROR] Cannot find Cargo.toml"
    exit 1
fi

cargo build --release
BINARY="$(pwd)/target/release/cn-codex-relay"

if [ ! -f "$BINARY" ]; then
    echo "[ERROR] Build succeeded but binary not found at $BINARY"
    exit 1
fi

# [3] 安装
echo "[3/6] Installing to $INSTALL_DIR..."
mkdir -p "$INSTALL_DIR"
cp "$BINARY" "$INSTALL_DIR/cn-codex-relay"
chmod +x "$INSTALL_DIR/cn-codex-relay"

# 复制 mobile-dist
if [ -d "$SCRIPT_DIR/mobile-dist" ]; then
    rm -rf "$INSTALL_DIR/mobile-dist"
    cp -r "$SCRIPT_DIR/mobile-dist" "$INSTALL_DIR/mobile-dist"
    echo "  - mobile-dist installed."
else
    echo "  [WARN] mobile-dist not found, creating empty directory."
    mkdir -p "$INSTALL_DIR/mobile-dist"
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
if command -v firewall-cmd &> /dev/null; then
    firewall-cmd --add-port=8080/tcp --permanent 2>/dev/null || true
    firewall-cmd --reload 2>/dev/null || true
    echo "  - firewall-cmd: port 8080 opened."
elif command -v ufw &> /dev/null; then
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
