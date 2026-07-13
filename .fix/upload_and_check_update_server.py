#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""Upload update artifacts to the remote update server.

Credentials must be provided via environment variables, never hard-coded:

  UPDATE_SERVER_HOST
  UPDATE_SERVER_USER
  UPDATE_SERVER_PASSWORD

Example:

  set UPDATE_SERVER_HOST=47.113.221.244
  set UPDATE_SERVER_USER=root
  set UPDATE_SERVER_PASSWORD=***
  python .fix/upload_and_check_update_server.py
"""

from __future__ import annotations

import json
import os
import sys
import traceback
from pathlib import Path

try:
    import paramiko
except ImportError:
    import subprocess

    subprocess.check_call([sys.executable, "-m", "pip", "install", "paramiko"])
    import paramiko


HOST = os.environ.get("UPDATE_SERVER_HOST", "47.113.221.244")
PORT = int(os.environ.get("UPDATE_SERVER_PORT", "22"))
USER = os.environ.get("UPDATE_SERVER_USER", "root")
PASSWORD = os.environ.get("UPDATE_SERVER_PASSWORD", "")

# The live service public dir used by cn-codex-update.service
REMOTE_PUBLIC = os.environ.get(
    "UPDATE_SERVER_PUBLIC_DIR", "/opt/cn-codex-update/public"
)
# Optional website mirror dir
REMOTE_WWW_MIRROR = os.environ.get(
    "UPDATE_SERVER_WWW_DIR", "/www/wwwroot/cn-codex-update-server"
)

ROOT = Path(__file__).resolve().parents[1]
LOCAL_LATEST = ROOT / "build" / "update-artifacts" / "update-upload" / "latest.json"
LOCAL_ZIP = (
    ROOT
    / "build"
    / "update-artifacts"
    / "update-upload"
    / "files"
    / "CN-Codex-1.0.0.zip"
)


def run(ssh: paramiko.SSHClient, cmd: str, timeout: int = 60) -> tuple[int, str, str]:
    _, stdout, stderr = ssh.exec_command(cmd, timeout=timeout)
    out = stdout.read().decode("utf-8", errors="replace")
    err = stderr.read().decode("utf-8", errors="replace")
    code = stdout.channel.recv_exit_status()
    return code, out, err


def main() -> int:
    if not PASSWORD:
        print("ERROR: set UPDATE_SERVER_PASSWORD env var first")
        return 2
    if not LOCAL_LATEST.is_file() or not LOCAL_ZIP.is_file():
        print("ERROR: missing local artifacts under build/update-artifacts/update-upload")
        return 1

    manifest = json.loads(LOCAL_LATEST.read_text(encoding="utf-8"))
    print("local version:", manifest.get("version"))

    ssh = paramiko.SSHClient()
    ssh.set_missing_host_key_policy(paramiko.AutoAddPolicy())
    ssh.connect(HOST, port=PORT, username=USER, password=PASSWORD, timeout=30)
    sftp = ssh.open_sftp()

    targets = [REMOTE_PUBLIC]
    if REMOTE_WWW_MIRROR:
        targets.append(REMOTE_WWW_MIRROR)
        targets.append(f"{REMOTE_WWW_MIRROR}/public")

    for public_dir in targets:
        run(ssh, f"mkdir -p '{public_dir}/files'")
        latest = f"{public_dir}/latest.json"
        zip_path = f"{public_dir}/files/{LOCAL_ZIP.name}"
        print(f"upload -> {latest}")
        sftp.put(str(LOCAL_LATEST), latest)
        print(f"upload -> {zip_path}")
        sftp.put(str(LOCAL_ZIP), zip_path)

    sftp.close()

    print("\nverify live service:")
    for cmd in [
        "systemctl cat cn-codex-update.service | sed -n '1,40p'",
        f"cat {REMOTE_PUBLIC}/latest.json",
        "curl -sS http://127.0.0.1:5005/latest.json",
        f"curl -sS -I http://127.0.0.1:5005/files/{LOCAL_ZIP.name} | head -n 12",
    ]:
        code, out, err = run(ssh, cmd)
        print(f"\n$ {cmd}")
        if out.strip():
            print(out.rstrip())
        if err.strip():
            print("[stderr]", err.rstrip())
        print(f"[exit={code}]")

    ssh.close()
    print("\nDONE")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except Exception:
        traceback.print_exc()
        raise SystemExit(1)
