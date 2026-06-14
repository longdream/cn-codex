#!/bin/bash
set -e

echo "============================================"
echo "  CN-Codex Relay - Build on Server"
echo "============================================"
echo ""
echo "This script builds the relay server directly"
echo "on the Linux server (for when cross-compile"
echo "is not available on Windows)."
echo ""

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"

# 检查 Rust 是否安装
if ! command -v cargo &> /dev/null; then
    echo "[1/3] Installing Rust..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    source "$HOME/.cargo/env"
else
    echo "[1/3] Rust already installed."
fi

# 编译
echo "[2/3] Building release binary..."
cd "$SCRIPT_DIR"

if [ -f "Cargo.toml" ]; then
    cargo build --release
    BINARY="$SCRIPT_DIR/target/release/cn-codex-relay"
elif [ -f "src/main.rs" ]; then
    cargo build --release
    BINARY="$SCRIPT_DIR/target/release/cn-codex-relay"
else
    echo "[ERROR] Cargo.toml not found. Please run this from the relay-server source directory."
    exit 1
fi

echo "[3/3] Binary built: $BINARY"
echo ""
echo "Next steps:"
echo "  1. Copy binary to deploy folder:"
echo "     cp $BINARY /tmp/cn-codex-relay/"
echo "  2. Run deploy.sh"
