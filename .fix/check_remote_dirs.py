#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""Check remote update-server directories.

Use env vars:
  UPDATE_SERVER_HOST
  UPDATE_SERVER_USER
  UPDATE_SERVER_PASSWORD
"""

from __future__ import annotations

import os
import sys

try:
    import paramiko
except ImportError:
    import subprocess

    subprocess.check_call([sys.executable, "-m", "pip", "install", "paramiko"])
    import paramiko


HOST = os.environ.get("UPDATE_SERVER_HOST", "47.113.221.244")
USER = os.environ.get("UPDATE_SERVER_USER", "root")
PASSWORD = os.environ.get("UPDATE_SERVER_PASSWORD", "")


def main() -> int:
    if not PASSWORD:
        print("ERROR: set UPDATE_SERVER_PASSWORD env var first")
        return 2

    ssh = paramiko.SSHClient()
    ssh.set_missing_host_key_policy(paramiko.AutoAddPolicy())
    ssh.connect(HOST, username=USER, password=PASSWORD, timeout=30)

    def run(cmd: str) -> str:
        _, stdout, stderr = ssh.exec_command(cmd, timeout=60)
        out = stdout.read().decode("utf-8", errors="replace")
        err = stderr.read().decode("utf-8", errors="replace")
        return (out + (("\n[stderr]\n" + err) if err else "")).rstrip()

    cmds = [
        "systemctl cat cn-codex-update.service",
        "ss -lntp | grep 5005 || true",
        "ps -ef | grep server.py | grep -v grep || true",
        "echo '=== /opt/cn-codex-update/public ==='; ls -lah /opt/cn-codex-update/public; cat /opt/cn-codex-update/public/latest.json",
        "echo '=== /www/wwwroot/cn-codex-update-server ==='; ls -lah /www/wwwroot/cn-codex-update-server; cat /www/wwwroot/cn-codex-update-server/latest.json 2>/dev/null || true",
        "curl -sS http://127.0.0.1:5005/latest.json",
    ]
    for cmd in cmds:
        print("\n$", cmd)
        print(run(cmd))

    ssh.close()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
