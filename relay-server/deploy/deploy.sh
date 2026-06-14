#!/bin/bash
set -e

echo "============================================"
echo "  CN-Codex Relay Server - Deploy"
echo "============================================"
echo ""

INSTALL_DIR="/opt/cn-codex-relay"
SERVICE_NAME="cn-codex-relay"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"

# [1] 创建安装目录
echo "[1/5] Creating install directory: $INSTALL_DIR"
mkdir -p "$INSTALL_DIR"

# [2] 复制文件
echo "[2/5] Copying files..."
cp "$SCRIPT_DIR/cn-codex-relay" "$INSTALL_DIR/cn-codex-relay"
chmod +x "$INSTALL_DIR/cn-codex-relay"

if [ -d "$SCRIPT_DIR/mobile-dist" ]; then
    rm -rf "$INSTALL_DIR/mobile-dist"
    cp -r "$SCRIPT_DIR/mobile-dist" "$INSTALL_DIR/mobile-dist"
    echo "  - mobile-dist copied."
else
    echo "  [WARN] mobile-dist not found, skipping static files."
    mkdir -p "$INSTALL_DIR/mobile-dist"
fi

# [3] 安装 systemd 服务
echo "[3/5] Installing systemd service..."
cp "$SCRIPT_DIR/cn-codex-relay.service" "/etc/systemd/system/$SERVICE_NAME.service"
systemctl daemon-reload

# [4] 启动服务
echo "[4/5] Starting service..."
systemctl enable "$SERVICE_NAME"
systemctl restart "$SERVICE_NAME"

# [5] 检查状态
echo "[5/5] Checking service status..."
sleep 1
systemctl status "$SERVICE_NAME" --no-pager || true

echo ""
echo "============================================"
echo "  Deploy complete!"
echo ""
echo "  Service: $SERVICE_NAME"
echo "  Binary:  $INSTALL_DIR/cn-codex-relay"
echo "  Port:    8080"
echo ""
echo "  Commands:"
echo "    systemctl status $SERVICE_NAME"
echo "    systemctl restart $SERVICE_NAME"
echo "    journalctl -u $SERVICE_NAME -f"
echo ""
echo "  Firewall (if needed):"
echo "    firewall-cmd --add-port=8080/tcp --permanent"
echo "    firewall-cmd --reload"
echo ""
echo "    Or for iptables:"
echo "    iptables -I INPUT -p tcp --dport 8080 -j ACCEPT"
echo ""
echo "  CN-Codex PC config:"
echo "    Set relay_server_url to: http://<your-server-ip>:8080"
echo "============================================"
