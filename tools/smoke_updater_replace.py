import json
import os
import subprocess
import tempfile
import threading
import time
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path


def main() -> None:
    repo = Path(__file__).resolve().parents[1]
    updater = repo / "src-tauri" / "target" / "debug" / "updater.exe"
    if not updater.exists():
        raise SystemExit(f"updater missing: {updater}")

    with tempfile.TemporaryDirectory(prefix="cn-codex-update-smoke-") as tmp_raw:
        tmp = Path(tmp_raw)
        www = tmp / "www"
        files = www / "files"
        files.mkdir(parents=True)

        target = tmp / "CN-Codex.exe"
        payload = files / "CN-Codex-0.1.1.exe"
        # Use a real PE binary so launch after replace does not hang on MessageBox.
        seed_exe = Path(os.environ.get("SystemRoot", r"C:\Windows")) / "System32" / "cmd.exe"
        payload_bytes = seed_exe.read_bytes()
        payload.write_bytes(payload_bytes)
        target.write_bytes(b"OLD-BINARY-CONTENT")
        launch = tmp / "launch-ok.cmd"
        launch.write_text("@echo off\r\nexit /b 0\r\n", encoding="ascii")

        manifest = {
            "version": "0.1.1",
            "url": "http://127.0.0.1:5015/files/CN-Codex-0.1.1.exe",
            "sha256": "",
            "notes": "smoke",
            "force": False,
            "publishedAt": "2026-03-21T12:00:00Z",
        }
        (www / "latest.json").write_text(json.dumps(manifest), encoding="utf-8")

        class Handler(SimpleHTTPRequestHandler):
            def __init__(self, *args, **kwargs):
                super().__init__(*args, directory=str(www), **kwargs)

            def log_message(self, format, *args):
                return

        server = ThreadingHTTPServer(("127.0.0.1", 5015), Handler)
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        time.sleep(0.2)

        dummy = subprocess.Popen(
            ["powershell", "-NoProfile", "-Command", "Start-Sleep -Seconds 1"]
        )
        cmd = [
            str(updater),
            "update",
            "--url",
            "http://127.0.0.1:5015/files/CN-Codex-0.1.1.exe",
            "--target",
            str(target),
            "--pid",
            str(dummy.pid),
            "--launch",
            str(launch),
            "--wait-secs",
            "10",
        ]
        proc = subprocess.run(cmd, capture_output=True, text=True)
        try:
            dummy.wait(timeout=5)
        except Exception:
            dummy.kill()
        server.shutdown()

        content = target.read_bytes()
        print(
            json.dumps(
                {
                    "exitCode": proc.returncode,
                    "stdout": (proc.stdout or "")[-500:],
                    "stderr": (proc.stderr or "")[-500:],
                    "contentLen": len(content),
                    "replaced": content == payload_bytes,
                    "tmpdir": str(tmp),
                },
                ensure_ascii=False,
            )
        )


if __name__ == "__main__":
    main()
