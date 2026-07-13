#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
INSTALL_DIR="${INSTALL_DIR:-/opt/cn-codex-update}"
SERVICE_NAME="cn-codex-update.service"

is_usable_python() {
  local bin="$1"
  command -v "$bin" >/dev/null 2>&1 || return 1
  "$bin" - <<'PY' >/dev/null 2>&1
import sys
raise SystemExit(0 if sys.version_info >= (3, 6) else 1)
PY
}

resolve_python() {
  if [[ -n "${PYTHON_BIN:-}" ]] && is_usable_python "$PYTHON_BIN"; then
    command -v "$PYTHON_BIN"
    return 0
  fi

  local name
  for name in python3.12 python3.11 python3.10 python3.9 python3.8 python3.7 python3.6 python3 python; do
    if is_usable_python "$name"; then
      command -v "$name"
      return 0
    fi
  done

  local path
  for path in \
    /usr/local/bin/python3 \
    /usr/bin/python3 \
    /opt/conda/bin/python \
    /root/miniconda3/bin/python \
    /root/anaconda3/bin/python
  do
    if [[ -x "$path" ]] && is_usable_python "$path"; then
      echo "$path"
      return 0
    fi
  done

  return 1
}

PYTHON_PATH="$(resolve_python || true)"
if [[ -z "${PYTHON_PATH}" ]]; then
  echo "error: no usable Python 3.6+ found" >&2
  echo "hint: export PYTHON_BIN=/path/to/python3.10 and retry" >&2
  exit 1
fi

echo "==> selected python: $PYTHON_PATH ($("$PYTHON_PATH" --version 2>&1))"
echo "==> install pure-python update server to $INSTALL_DIR"
sudo mkdir -p "$INSTALL_DIR/public/files"
sudo cp "$ROOT_DIR/server.py" "$INSTALL_DIR/server.py"
sudo cp "$ROOT_DIR/run.sh" "$INSTALL_DIR/run.sh"
sudo cp "$ROOT_DIR/requirements.txt" "$INSTALL_DIR/requirements.txt"
sudo cp -r "$ROOT_DIR/public/." "$INSTALL_DIR/public/"
sudo cp "$ROOT_DIR/deploy/$SERVICE_NAME" "/etc/systemd/system/$SERVICE_NAME"
sudo chmod +x "$INSTALL_DIR/server.py" "$INSTALL_DIR/run.sh"

# Pin discovered python into systemd unit so conda/env python is used reliably.
if [[ -n "${PYTHON_BIN:-}" ]] || [[ "$PYTHON_PATH" != "/usr/bin/python3" ]]; then
  echo "==> pin PYTHON_BIN=$PYTHON_PATH in systemd unit"
  sudo sed -i "s|^# Optional override.*|Environment=PYTHON_BIN=${PYTHON_PATH}|" \
    "/etc/systemd/system/$SERVICE_NAME" || true
  if ! grep -q "^Environment=PYTHON_BIN=" "/etc/systemd/system/$SERVICE_NAME"; then
    # Insert after PUBLIC_DIR environment line.
    sudo sed -i "/Environment=PUBLIC_DIR=.*/a Environment=PYTHON_BIN=${PYTHON_PATH}" \
      "/etc/systemd/system/$SERVICE_NAME"
  fi
fi

echo "==> syntax check"
"$PYTHON_PATH" -m py_compile "$INSTALL_DIR/server.py"

echo "==> enable service"
sudo systemctl daemon-reload
sudo systemctl enable "$SERVICE_NAME"
sudo systemctl restart "$SERVICE_NAME"
sleep 1
sudo systemctl --no-pager --full status "$SERVICE_NAME" || true

echo
echo "health check:"
if curl -fsS "http://127.0.0.1:5005/healthz"; then
  echo
  echo "OK"
else
  echo
  echo "health check failed, recent logs:"
  sudo journalctl -u "$SERVICE_NAME" -n 50 --no-pager || true
  exit 1
fi

echo "done."
echo "python: $("$PYTHON_PATH" --version 2>&1)"
