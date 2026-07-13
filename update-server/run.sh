#!/usr/bin/env bash
# Resolve a modern Python and start the update server.
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PUBLIC_DIR="${PUBLIC_DIR:-$ROOT_DIR/public}"
BIND="${BIND:-0.0.0.0:5005}"
MIN_MAJOR=3
MIN_MINOR=6

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

  local candidates=(
    python3.12
    python3.11
    python3.10
    python3.9
    python3.8
    python3.7
    python3.6
    python3
    python
  )

  # Prefer PATH resolution first (covers conda/env python).
  local name
  for name in "${candidates[@]}"; do
    if is_usable_python "$name"; then
      command -v "$name"
      return 0
    fi
  done

  # Absolute fallbacks commonly used on servers.
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
  echo "error: no usable Python ${MIN_MAJOR}.${MIN_MINOR}+ found" >&2
  echo "hint: export PYTHON_BIN=/path/to/python3.10 and retry" >&2
  exit 1
fi

echo "using python: $PYTHON_PATH ($("$PYTHON_PATH" --version 2>&1))"
exec "$PYTHON_PATH" "$ROOT_DIR/server.py" --bind "$BIND" --public-dir "$PUBLIC_DIR" "$@"
