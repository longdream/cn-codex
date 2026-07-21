//! Create a minimal Node.js MCP + Web miniapp package under codey/miniapps/<slug>.

use std::fs;
use std::path::Path;

use super::{
    app_dir, ensure_miniapps_dir, now_secs, resolve_bundled_node, write_manifest, MiniAppMcpConfig,
    MiniAppPage, MiniAppRecord, MiniAppStatus, MiniAppToolMeta,
};

#[derive(Debug, Clone)]
pub struct ScaffoldRequest {
    pub name: String,
    pub slug: String,
    pub description: String,
    pub database_id: String,
    pub database_name: String,
}

pub fn scaffold_miniapp(
    workspace_config_dir: &Path,
    req: ScaffoldRequest,
) -> Result<MiniAppRecord, String> {
    ensure_miniapps_dir(workspace_config_dir)?;
    let root = app_dir(workspace_config_dir, &req.slug);
    if root.exists() && root.join("miniapp.json").exists() {
        return Err(format!("小程序 `{}` 已存在", req.slug));
    }
    fs::create_dir_all(root.join("server")).map_err(|e| format!("创建 server 目录失败: {e}"))?;
    fs::create_dir_all(root.join("web")).map_err(|e| format!("创建 web 目录失败: {e}"))?;

    let node = resolve_bundled_node(workspace_config_dir)?;
    let server_entry = root.join("server").join("index.mjs");
    let package_json = root.join("package.json");
    let mcp_json = root.join(".mcp.json");
    let web_index = root.join("web").join("index.html");
    let readme = root.join("README.md");

    fs::write(&package_json, package_json_content(&req.slug, &req.name))
        .map_err(|e| format!("写入 package.json 失败: {e}"))?;
    fs::write(&server_entry, server_index_mjs())
        .map_err(|e| format!("写入 server/index.mjs 失败: {e}"))?;
    fs::write(&web_index, web_index_html(&req.name, &req.slug, &req.database_name))
        .map_err(|e| format!("写入 web/index.html 失败: {e}"))?;
    fs::write(
        &mcp_json,
        mcp_json_content(&node, &server_entry, &root, &req.database_id),
    )
    .map_err(|e| format!("写入 .mcp.json 失败: {e}"))?;
    fs::write(
        &readme,
        format!(
            "# {}\n\nSlug: `{}`\n\nDatabase: `{}` ({})\n\n## Run\n\n```bash\n{} server/index.mjs\n```\n\n{}",
            req.name,
            req.slug,
            req.database_name,
            req.database_id,
            node.display(),
            req.description
        ),
    )
    .map_err(|e| format!("写入 README.md 失败: {e}"))?;

    let now = now_secs();
    let record = MiniAppRecord {
        id: format!("miniapp-{}", req.slug),
        name: req.name,
        slug: req.slug.clone(),
        description: req.description,
        database_id: req.database_id,
        database_name: req.database_name,
        status: MiniAppStatus::Generated,
        port: None,
        root_path: root.to_string_lossy().to_string(),
        mcp: MiniAppMcpConfig {
            command: node.to_string_lossy().to_string(),
            args: vec![server_entry.to_string_lossy().to_string()],
            cwd: Some(root.to_string_lossy().to_string()),
        },
        pages: vec![MiniAppPage {
            id: "home".into(),
            title: "首页".into(),
            path: "/".into(),
            description: "默认示例页".into(),
        }],
        tools: vec![
            MiniAppToolMeta {
                name: "list_pages".into(),
                description: "列出页面".into(),
            },
            MiniAppToolMeta {
                name: "get_status".into(),
                description: "健康检查".into(),
            },
            MiniAppToolMeta {
                name: "open_page".into(),
                description: "打开页面".into(),
            },
        ],
        last_error: String::new(),
        created_at: now,
        updated_at: now,
    };
    write_manifest(&record)?;
    Ok(record)
}

fn package_json_content(slug: &str, name: &str) -> String {
    format!(
        r#"{{
  "name": "miniapp-{slug}",
  "version": "0.1.0",
  "private": true,
  "type": "module",
  "description": "{name}",
  "main": "server/index.mjs",
  "scripts": {{
    "start": "node server/index.mjs"
  }}
}}
"#
    )
}

fn mcp_json_content(
    node: &Path,
    server_entry: &Path,
    cwd: &Path,
    database_id: &str,
) -> String {
    serde_json::json!({
        "mcpServers": {
            "miniapp": {
                "command": node.to_string_lossy(),
                "args": [server_entry.to_string_lossy()],
                "cwd": cwd.to_string_lossy(),
                "env": {
                    "MINIAPP_DATABASE_ID": database_id
                }
            }
        }
    })
    .to_string()
}

fn web_index_html(name: &str, slug: &str, database_name: &str) -> String {
    format!(
        r#"<!doctype html>
<html lang="zh-CN">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>{name}</title>
    <style>
      :root {{
        color-scheme: light dark;
        font-family: "Segoe UI", system-ui, sans-serif;
      }}
      body {{
        margin: 0;
        min-height: 100vh;
        background: #0f172a;
        color: #e2e8f0;
        display: grid;
        place-items: center;
      }}
      .card {{
        width: min(520px, calc(100vw - 32px));
        border: 1px solid #334155;
        border-radius: 16px;
        background: #111827;
        padding: 24px;
        box-shadow: 0 20px 50px rgba(0,0,0,.35);
      }}
      h1 {{ margin: 0 0 8px; font-size: 22px; }}
      p {{ margin: 0 0 12px; color: #94a3b8; line-height: 1.5; }}
      .meta {{ font-size: 12px; color: #64748b; }}
      code {{ color: #93c5fd; }}
    </style>
  </head>
  <body>
    <div class="card">
      <h1>{name}</h1>
      <p>这是小程序 <code>{slug}</code> 的示例页面。</p>
      <p class="meta">绑定数据库：{database_name}</p>
      <p class="meta">后续可由主链路 Agent 在模板上生成业务录入/查询页。</p>
    </div>
  </body>
</html>
"#
    )
}

fn server_index_mjs() -> &'static str {
    r#"#!/usr/bin/env node
/**
 * MiniApp MCP + static web server.
 * - HTTP: serves ./web on MINIAPP_PORT / auto free port
 * - stdio: minimal JSON-RPC MCP tools/list + tools/call
 */
import http from "node:http";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const ROOT = path.resolve(__dirname, "..");
const WEB_ROOT = path.join(ROOT, "web");
const MANIFEST_PATH = path.join(ROOT, "miniapp.json");

function readManifest() {
  try {
    return JSON.parse(fs.readFileSync(MANIFEST_PATH, "utf8"));
  } catch {
    return {
      name: process.env.MINIAPP_NAME || "MiniApp",
      slug: process.env.MINIAPP_SLUG || "miniapp",
      databaseId: process.env.MINIAPP_DATABASE_ID || "",
      pages: [{ id: "home", title: "首页", path: "/" }],
    };
  }
}

const manifest = readManifest();
const preferredPort = Number(process.env.MINIAPP_PORT || process.env.PORT || 0);

function contentType(filePath) {
  if (filePath.endsWith(".html")) return "text/html; charset=utf-8";
  if (filePath.endsWith(".js")) return "text/javascript; charset=utf-8";
  if (filePath.endsWith(".css")) return "text/css; charset=utf-8";
  if (filePath.endsWith(".json")) return "application/json; charset=utf-8";
  return "application/octet-stream";
}

function sendJson(res, status, body) {
  const raw = JSON.stringify(body);
  res.writeHead(status, {
    "Content-Type": "application/json; charset=utf-8",
    "Content-Length": Buffer.byteLength(raw),
  });
  res.end(raw);
}

const server = http.createServer((req, res) => {
  const url = new URL(req.url || "/", "http://127.0.0.1");
  if (url.pathname === "/api/status") {
    sendJson(res, 200, {
      ok: true,
      code: "OK",
      data: {
        name: manifest.name,
        slug: manifest.slug,
        databaseId: manifest.databaseId || process.env.MINIAPP_DATABASE_ID || "",
        port: Number(process.env.MINIAPP_BOUND_PORT || 0),
        pages: manifest.pages || [],
      },
    });
    return;
  }

  let rel = decodeURIComponent(url.pathname);
  if (rel === "/") rel = "/index.html";
  const filePath = path.normalize(path.join(WEB_ROOT, rel.replace(/^\/+/, "")));
  if (!filePath.startsWith(WEB_ROOT)) {
    res.writeHead(403).end("Forbidden");
    return;
  }
  fs.readFile(filePath, (err, data) => {
    if (err) {
      res.writeHead(404, { "Content-Type": "text/plain; charset=utf-8" });
      res.end("Not Found");
      return;
    }
    res.writeHead(200, { "Content-Type": contentType(filePath) });
    res.end(data);
  });
});

function listen(port) {
  return new Promise((resolve, reject) => {
    const onError = (err) => reject(err);
    server.once("error", onError);
    server.listen(port, "127.0.0.1", () => {
      server.off("error", onError);
      const address = server.address();
      resolve(typeof address === "object" && address ? address.port : port);
    });
  });
}

const boundPort = await listen(Number.isFinite(preferredPort) ? preferredPort : 0);
process.env.MINIAPP_BOUND_PORT = String(boundPort);
// Host runtime can scrape this line for the allocated port.
console.error(`[miniapp] listening on 127.0.0.1:${boundPort}`);

const TOOLS = [
  {
    name: "list_pages",
    description: "List miniapp pages",
    inputSchema: { type: "object", properties: {} },
  },
  {
    name: "get_status",
    description: "Return miniapp health/port/database binding",
    inputSchema: { type: "object", properties: {} },
  },
  {
    name: "open_page",
    description: "Open a miniapp page (returns URL for host to display)",
    inputSchema: {
      type: "object",
      properties: {
        page: { type: "string", description: "page id or path" },
        known: { type: "object", description: "optional prefill values" },
      },
    },
  },
];

function toolResult(payload) {
  return {
    content: [{ type: "text", text: JSON.stringify(payload, null, 2) }],
    structuredContent: payload,
  };
}

function handleToolCall(name, args = {}) {
  if (name === "list_pages") {
    return toolResult({
      ok: true,
      code: "OK",
      data: { pages: manifest.pages || [] },
    });
  }
  if (name === "get_status") {
    return toolResult({
      ok: true,
      code: "OK",
      data: {
        name: manifest.name,
        slug: manifest.slug,
        port: boundPort,
        databaseId: manifest.databaseId || process.env.MINIAPP_DATABASE_ID || "",
        status: "running",
      },
    });
  }
  if (name === "open_page") {
    const pageId = typeof args.page === "string" ? args.page : "home";
    const pages = Array.isArray(manifest.pages) ? manifest.pages : [];
    const page =
      pages.find((p) => p.id === pageId || p.path === pageId || p.path === `/${pageId}`) ||
      pages[0] ||
      { id: "home", path: "/" };
    const pathPart = page.path?.startsWith("/") ? page.path : `/${page.path || ""}`;
    return toolResult({
      ok: true,
      code: "OK",
      message: "page ready",
      data: {
        pageId: page.id || pageId,
        url: `http://127.0.0.1:${boundPort}${pathPart || "/"}`,
        known: args.known || {},
      },
      ui: { action: "open_page", pageId: page.id || pageId },
    });
  }
  return toolResult({
    ok: false,
    code: "UNKNOWN_TOOL",
    message: `Unknown tool: ${name}`,
  });
}

function sendRpc(message) {
  const raw = JSON.stringify(message);
  process.stdout.write(`Content-Length: ${Buffer.byteLength(raw, "utf8")}\r\n\r\n${raw}`);
}

function handleRpc(msg) {
  if (!msg || typeof msg !== "object") return;
  const { id, method, params } = msg;
  if (method === "initialize") {
    sendRpc({
      jsonrpc: "2.0",
      id,
      result: {
        protocolVersion: "2024-11-05",
        capabilities: { tools: {} },
        serverInfo: { name: manifest.slug || "miniapp", version: "0.1.0" },
      },
    });
    return;
  }
  if (method === "notifications/initialized" || method === "initialized") {
    return;
  }
  if (method === "tools/list") {
    sendRpc({ jsonrpc: "2.0", id, result: { tools: TOOLS } });
    return;
  }
  if (method === "tools/call") {
    const name = params?.name;
    const args = params?.arguments || {};
    sendRpc({ jsonrpc: "2.0", id, result: handleToolCall(name, args) });
    return;
  }
  if (method === "ping") {
    sendRpc({ jsonrpc: "2.0", id, result: {} });
    return;
  }
  if (id !== undefined) {
    sendRpc({
      jsonrpc: "2.0",
      id,
      error: { code: -32601, message: `Method not found: ${method}` },
    });
  }
}

// Support both Content-Length framed MCP and newline-delimited JSON.
let buffer = Buffer.alloc(0);
process.stdin.on("data", (chunk) => {
  buffer = Buffer.concat([buffer, chunk]);
  while (true) {
    const headerEnd = buffer.indexOf("\r\n\r\n");
    if (headerEnd >= 0) {
      const header = buffer.slice(0, headerEnd).toString("utf8");
      const match = /Content-Length:\s*(\d+)/i.exec(header);
      if (!match) {
        buffer = buffer.slice(headerEnd + 4);
        continue;
      }
      const len = Number(match[1]);
      const total = headerEnd + 4 + len;
      if (buffer.length < total) break;
      const body = buffer.slice(headerEnd + 4, total).toString("utf8");
      buffer = buffer.slice(total);
      try {
        handleRpc(JSON.parse(body));
      } catch (err) {
        console.error("[miniapp] invalid rpc", err);
      }
      continue;
    }
    // newline delimited fallback
    const nl = buffer.indexOf("\n");
    if (nl < 0) break;
    const line = buffer.slice(0, nl).toString("utf8").trim();
    buffer = buffer.slice(nl + 1);
    if (!line) continue;
    try {
      handleRpc(JSON.parse(line));
    } catch {
      // ignore non-json noise
    }
  }
});

process.stdin.on("end", () => {
  server.close(() => process.exit(0));
});

// Keep process alive even if stdin is not a TTY pipe yet.
setInterval(() => {}, 1 << 30);
"#
}
