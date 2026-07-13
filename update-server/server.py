#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""CN-Codex lightweight update server (Python stdlib only).

Compatible with Python 3.6+.
No third-party dependencies.

Provides:
  GET /            latest version page
  GET /latest.json update manifest
  GET /files/*     executable downloads
  GET /healthz     health check
"""

from __future__ import print_function

import argparse
import html
import json
import mimetypes
import os
import socket
import sys
from http import HTTPStatus
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import unquote, urlparse

try:
    # Python 3.9+
    from typing import Any, Dict, Optional, Tuple
except ImportError:  # pragma: no cover
    Any = object  # type: ignore
    Dict = dict  # type: ignore
    Optional = object  # type: ignore
    Tuple = tuple  # type: ignore


DEFAULT_BIND = "0.0.0.0"
DEFAULT_PORT = 5005
MIN_PY = (3, 6)


def ensure_python_version():
    if sys.version_info < MIN_PY:
        raise SystemExit(
            "Python {}.{}+ is required, current is {}.{}.{}".format(
                MIN_PY[0],
                MIN_PY[1],
                sys.version_info[0],
                sys.version_info[1],
                sys.version_info[2],
            )
        )


def parse_args():
    parser = argparse.ArgumentParser(
        description="CN-Codex lightweight update server (no third-party deps)"
    )
    parser.add_argument(
        "--host",
        default=DEFAULT_BIND,
        help="bind host (default: {})".format(DEFAULT_BIND),
    )
    parser.add_argument(
        "--port",
        type=int,
        default=DEFAULT_PORT,
        help="bind port (default: {})".format(DEFAULT_PORT),
    )
    parser.add_argument(
        "--bind",
        default="",
        help="optional host:port form, e.g. 0.0.0.0:5005 (overrides --host/--port)",
    )
    parser.add_argument(
        "--public-dir",
        default="public",
        help="public directory containing latest.json / files (default: public)",
    )
    return parser.parse_args()


def resolve_bind(args):
    # type: (argparse.Namespace) -> Tuple[str, int]
    if args.bind:
        host, sep, port_text = args.bind.rpartition(":")
        if not sep or not port_text.isdigit():
            raise SystemExit("invalid --bind value: {}".format(args.bind))
        return (host or DEFAULT_BIND), int(port_text)
    return args.host, int(args.port)


def load_latest(public_dir):
    # type: (Path) -> Dict[str, Any]
    path = public_dir / "latest.json"
    with path.open("r", encoding="utf-8") as fh:
        raw = fh.read()
    value = json.loads(raw)
    if not isinstance(value, dict):
        raise ValueError("latest.json root must be an object")

    return {
        "version": str(value.get("version") or "").strip(),
        "url": str(value.get("url") or "").strip(),
        "sha256": str(value.get("sha256") or "").strip(),
        "notes": str(value.get("notes") or "").strip(),
        "force": bool(value.get("force") or False),
        "publishedAt": str(
            value.get("publishedAt") or value.get("published_at") or ""
        ).strip(),
    }


def render_index_html(info):
    # type: (Dict[str, Any]) -> str
    version = html.escape(info.get("version") or "")
    url = html.escape(info.get("url") or "")
    notes = html.escape(info.get("notes") or "")
    published_at = html.escape(info.get("publishedAt") or "")
    return """<!doctype html>
<html lang="zh-CN">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <title>CN-Codex 更新服务</title>
  <style>
    :root {{
      color-scheme: dark;
      --bg: #0b1220;
      --card: #121a2b;
      --text: #e8eefc;
      --muted: #9fb0d0;
      --accent: #4f8cff;
      --border: rgba(255,255,255,0.08);
    }}
    * {{ box-sizing: border-box; }}
    body {{
      margin: 0;
      min-height: 100vh;
      font-family: "Segoe UI", "PingFang SC", "Microsoft YaHei", sans-serif;
      background: radial-gradient(circle at top, #18233a, var(--bg));
      color: var(--text);
      display: grid;
      place-items: center;
      padding: 24px;
    }}
    .card {{
      width: min(720px, 100%);
      background: #121a2b;
      border: 1px solid var(--border);
      border-radius: 18px;
      padding: 28px;
      box-shadow: 0 20px 60px rgba(0,0,0,0.35);
    }}
    h1 {{ margin: 0 0 8px; font-size: 28px; }}
    p {{ margin: 0 0 12px; color: var(--muted); line-height: 1.6; }}
    .meta {{
      display: grid;
      gap: 10px;
      margin: 20px 0;
      padding: 16px;
      border-radius: 12px;
      background: rgba(255,255,255,0.03);
      border: 1px solid var(--border);
    }}
    .row {{ display: flex; gap: 12px; flex-wrap: wrap; }}
    .label {{ min-width: 88px; color: var(--muted); }}
    .value {{ word-break: break-all; color: var(--text); }}
    a.button {{
      display: inline-flex;
      align-items: center;
      justify-content: center;
      margin-top: 8px;
      padding: 12px 18px;
      border-radius: 10px;
      background: var(--accent);
      color: white;
      text-decoration: none;
      font-weight: 600;
    }}
    code {{
      font-family: Consolas, "Courier New", monospace;
      background: rgba(255,255,255,0.06);
      padding: 2px 6px;
      border-radius: 6px;
    }}
  </style>
</head>
<body>
  <main class="card">
    <h1>CN-Codex 自动更新服务</h1>
    <p>本站点只提供最新版本信息与下载地址。客户端通过 <code>/latest.json</code> 检查更新。</p>
    <div class="meta">
      <div class="row"><span class="label">最新版本</span><span class="value">{version}</span></div>
      <div class="row"><span class="label">发布时间</span><span class="value">{published_at}</span></div>
      <div class="row"><span class="label">更新说明</span><span class="value">{notes}</span></div>
      <div class="row"><span class="label">下载地址</span><span class="value">{url}</span></div>
    </div>
    <a class="button" href="{url}">下载最新 CN-Codex.exe</a>
    <p style="margin-top:18px">接口：<code>GET /latest.json</code></p>
  </main>
</body>
</html>
""".format(
        version=version,
        url=url,
        notes=notes,
        published_at=published_at,
    )


class UpdateRequestHandler(BaseHTTPRequestHandler):
    server_version = "CN-Codex-UpdateServer/1.0"
    public_dir = None  # type: Optional[Path]

    def end_headers(self):
        self.send_header("Cache-Control", "no-cache")
        self.send_header("Access-Control-Allow-Origin", "*")
        self.send_header("Access-Control-Allow-Methods", "GET, HEAD, OPTIONS")
        self.send_header("Access-Control-Allow-Headers", "*")
        BaseHTTPRequestHandler.end_headers(self)

    def do_OPTIONS(self):  # noqa: N802
        self.send_response(HTTPStatus.NO_CONTENT)
        self.end_headers()

    def do_GET(self):  # noqa: N802
        self._handle_read(include_body=True)

    def do_HEAD(self):  # noqa: N802
        self._handle_read(include_body=False)

    def _handle_read(self, include_body):
        # type: (bool) -> None
        parsed = urlparse(self.path)
        path = unquote(parsed.path or "/")
        public_dir = self.public_dir
        if public_dir is None:
            self._send_bytes(
                HTTPStatus.INTERNAL_SERVER_ERROR,
                b"public_dir is not configured",
                "text/plain; charset=utf-8",
                include_body=include_body,
            )
            return

        if path == "/healthz":
            self._send_bytes(
                HTTPStatus.OK,
                b"ok",
                "text/plain; charset=utf-8",
                include_body=include_body,
            )
            return

        if path in ("/", "/index.html"):
            try:
                info = load_latest(public_dir)
                body = render_index_html(info).encode("utf-8")
            except Exception as exc:
                self._send_bytes(
                    HTTPStatus.INTERNAL_SERVER_ERROR,
                    "failed to render index: {}".format(exc).encode("utf-8"),
                    "text/plain; charset=utf-8",
                    include_body=include_body,
                )
                return
            self._send_bytes(
                HTTPStatus.OK,
                body,
                "text/html; charset=utf-8",
                include_body=include_body,
            )
            return

        if path == "/latest.json":
            try:
                info = load_latest(public_dir)
                body = json.dumps(info, ensure_ascii=False, indent=2).encode("utf-8")
            except Exception as exc:
                self._send_bytes(
                    HTTPStatus.INTERNAL_SERVER_ERROR,
                    "failed to read latest.json: {}".format(exc).encode("utf-8"),
                    "text/plain; charset=utf-8",
                    include_body=include_body,
                )
                return
            self._send_bytes(
                HTTPStatus.OK,
                body,
                "application/json; charset=utf-8",
                include_body=include_body,
            )
            return

        self._serve_public_file(public_dir, path, include_body=include_body)

    def _serve_public_file(self, public_dir, request_path, include_body):
        # type: (Path, str, bool) -> None
        relative = request_path.lstrip("/")
        if not relative:
            self.send_error(HTTPStatus.NOT_FOUND, "File not found")
            return

        candidate = (public_dir / relative).resolve()
        public_root = public_dir.resolve()
        try:
            # Python 3.9+: Path.is_relative_to; fallback for older versions.
            if hasattr(candidate, "is_relative_to"):
                if not candidate.is_relative_to(public_root):  # type: ignore[attr-defined]
                    self.send_error(HTTPStatus.FORBIDDEN, "Forbidden")
                    return
            else:
                common = os.path.commonpath([str(public_root), str(candidate)])
                if common != str(public_root):
                    self.send_error(HTTPStatus.FORBIDDEN, "Forbidden")
                    return
        except Exception:
            self.send_error(HTTPStatus.FORBIDDEN, "Forbidden")
            return

        if not candidate.is_file():
            self.send_error(HTTPStatus.NOT_FOUND, "File not found")
            return

        content_type = mimetypes.guess_type(str(candidate))[0] or "application/octet-stream"
        size = candidate.stat().st_size
        self.send_response(HTTPStatus.OK)
        self.send_header("Content-Type", content_type)
        self.send_header("Content-Length", str(size))
        if relative.startswith("files/"):
            self.send_header(
                "Content-Disposition",
                'attachment; filename="{}"'.format(candidate.name),
            )
        else:
            self.send_header("Content-Disposition", "inline")
        self.end_headers()
        if include_body:
            with candidate.open("rb") as fh:
                while True:
                    chunk = fh.read(1024 * 64)
                    if not chunk:
                        break
                    self.wfile.write(chunk)

    def _send_bytes(self, status, body, content_type, include_body):
        # type: (HTTPStatus, bytes, str, bool) -> None
        self.send_response(status)
        self.send_header("Content-Type", content_type)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        if include_body:
            self.wfile.write(body)

    def log_message(self, fmt, *args):
        message = "%s - %s" % (self.address_string(), fmt % args)
        print(message, flush=True)


class DualStackServer(ThreadingHTTPServer):
    address_family = socket.AF_INET
    daemon_threads = True
    allow_reuse_address = True


def make_handler(public_dir):
    # type: (Path) -> type
    class BoundHandler(UpdateRequestHandler):
        pass

    BoundHandler.public_dir = public_dir
    return BoundHandler


def main():
    ensure_python_version()
    args = parse_args()
    host, port = resolve_bind(args)
    public_dir = Path(args.public_dir).expanduser().resolve()
    if not public_dir.is_dir():
        raise SystemExit("public_dir does not exist: {}".format(public_dir))

    handler = make_handler(public_dir)
    server = DualStackServer((host, port), handler)
    print(
        "CN-Codex update server listening on http://{}:{}".format(host, port),
        flush=True,
    )
    print("python: {}".format(sys.version.replace("\n", " ")), flush=True)
    print("public dir: {}".format(public_dir), flush=True)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        print("\nshutting down...", flush=True)
    finally:
        server.server_close()


if __name__ == "__main__":
    main()
