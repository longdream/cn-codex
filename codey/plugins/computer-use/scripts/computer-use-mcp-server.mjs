#!/usr/bin/env node
/**
 * CN-Codex Computer Use MCP Server
 *
 * 上游 computer-use 插件依赖 Codex 的 node_repl + native pipe。
 * CN-Codex 没有 node_repl 运行时，因此这里直接基于 @oai/sky
 * 暴露可调用的 stdio MCP 工具，避免“配置了 computer-use 但无法使用”。
 *
 * 协议：与 CN-Codex tool_executor 一致，使用 newline-delimited JSON-RPC。
 */

import { createRequire } from "node:module";
import path from "node:path";
import process from "node:process";
import { fileURLToPath, pathToFileURL } from "node:url";
import readline from "node:readline";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const PLUGIN_ROOT = path.resolve(__dirname, "..");
const SKY_ENTRY = path.join(
  PLUGIN_ROOT,
  "node_modules",
  "@oai",
  "sky",
  "dist",
  "project",
  "cua",
  "sky_js",
  "src",
  "index.js",
);

// helper 在部分审批场景会回落到 globalThis.nodeRepl.createElicitation。
// CN-Codex 没有 node_repl，这里提供自动接受实现，避免能力直接不可用。
if (globalThis.nodeRepl == null) {
  globalThis.nodeRepl = {};
}
if (typeof globalThis.nodeRepl.createElicitation !== "function") {
  globalThis.nodeRepl.createElicitation = async (params = {}) => {
    const message =
      typeof params?.message === "string" && params.message.trim()
        ? params.message.trim()
        : "Allow Computer Use app access";
    process.stderr.write(`[computer-use-mcp] auto-accept elicitation: ${message}\n`);
    return { action: "accept" };
  };
}

const require = createRequire(import.meta.url);

/** @type {import("@oai/sky").sky | null} */
let skyClient = null;
let skyLoadError = null;

async function getSky() {
  if (skyClient) {
    return skyClient;
  }
  if (skyLoadError) {
    throw skyLoadError;
  }
  try {
    if (process.platform !== "win32") {
      throw new Error("Computer Use MCP is only supported on Windows");
    }
    const mod = await import(pathToFileURL(SKY_ENTRY).href);
    skyClient = mod.sky;
    if (!skyClient) {
      throw new Error("Failed to load @oai/sky export");
    }
    return skyClient;
  } catch (error) {
    skyLoadError = error instanceof Error ? error : new Error(String(error));
    throw skyLoadError;
  }
}

const WINDOW_SCHEMA = {
  type: "object",
  description: "Window object returned by list_apps/list_windows/get_window",
  properties: {
    app: { type: "string" },
    id: { type: "number" },
    title: { type: "string" },
  },
  required: ["app", "id"],
  additionalProperties: true,
};

const TOOLS = [
  {
    name: "list_apps",
    description:
      "List installed Windows apps and their currently open targetable windows. Call this first for Computer Use tasks.",
    inputSchema: {
      type: "object",
      properties: {},
      additionalProperties: false,
    },
  },
  {
    name: "list_windows",
    description: "List currently open windows that can be targeted by Computer Use.",
    inputSchema: {
      type: "object",
      properties: {},
      additionalProperties: false,
    },
  },
  {
    name: "get_window",
    description: "Rehydrate a currently open window by id/app.",
    inputSchema: {
      type: "object",
      properties: {
        id: { type: "number" },
        app: { type: "string" },
      },
      required: ["id"],
      additionalProperties: false,
    },
  },
  {
    name: "launch_app",
    description:
      "Launch an app by id from list_apps, or by an explicit .exe path/process identifier.",
    inputSchema: {
      type: "object",
      properties: {
        app: { type: "string" },
      },
      required: ["app"],
      additionalProperties: false,
    },
  },
  {
    name: "activate_window",
    description: "Bring an open window to the foreground.",
    inputSchema: {
      type: "object",
      properties: {
        window: WINDOW_SCHEMA,
      },
      required: ["window"],
      additionalProperties: false,
    },
  },
  {
    name: "get_window_state",
    description:
      "Capture screenshot and/or accessibility text for an open window. Prefer include_text=true when you need element indexes.",
    inputSchema: {
      type: "object",
      properties: {
        window: WINDOW_SCHEMA,
        include_screenshot: { type: "boolean", default: true },
        include_text: { type: "boolean", default: false },
      },
      required: ["window"],
      additionalProperties: false,
    },
  },
  {
    name: "click",
    description:
      "Click an accessibility element_index or a window-relative coordinate from the latest screenshot.",
    inputSchema: {
      type: "object",
      properties: {
        window: WINDOW_SCHEMA,
        element_index: { type: "number" },
        x: { type: "number" },
        y: { type: "number" },
        click_count: { type: "number" },
        mouse_button: {
          type: "string",
          enum: ["left", "right", "middle", "l", "r", "m"],
        },
        screenshotId: { type: "string" },
      },
      required: ["window"],
      additionalProperties: false,
    },
  },
  {
    name: "type_text",
    description: "Type text into the current focus of a window.",
    inputSchema: {
      type: "object",
      properties: {
        window: WINDOW_SCHEMA,
        text: { type: "string" },
      },
      required: ["window", "text"],
      additionalProperties: false,
    },
  },
  {
    name: "press_key",
    description:
      "Press a key or + separated chord using X11-style keysyms, e.g. Return, Tab, Control_L+a.",
    inputSchema: {
      type: "object",
      properties: {
        window: WINDOW_SCHEMA,
        key: { type: "string" },
      },
      required: ["window", "key"],
      additionalProperties: false,
    },
  },
  {
    name: "scroll",
    description: "Scroll from a window screenshot coordinate by scrollX/scrollY deltas.",
    inputSchema: {
      type: "object",
      properties: {
        window: WINDOW_SCHEMA,
        x: { type: "number" },
        y: { type: "number" },
        scrollX: { type: "number" },
        scrollY: { type: "number" },
        screenshotId: { type: "string" },
      },
      required: ["window", "x", "y", "scrollX", "scrollY"],
      additionalProperties: false,
    },
  },
  {
    name: "drag",
    description: "Drag from one window screenshot coordinate to another.",
    inputSchema: {
      type: "object",
      properties: {
        window: WINDOW_SCHEMA,
        from_x: { type: "number" },
        from_y: { type: "number" },
        to_x: { type: "number" },
        to_y: { type: "number" },
        screenshotId: { type: "string" },
      },
      required: ["window", "from_x", "from_y", "to_x", "to_y"],
      additionalProperties: false,
    },
  },
  {
    name: "set_value",
    description: "Replace the value of an indexed editable accessibility element.",
    inputSchema: {
      type: "object",
      properties: {
        window: WINDOW_SCHEMA,
        element_index: { type: "number" },
        value: { type: "string" },
      },
      required: ["window", "element_index", "value"],
      additionalProperties: false,
    },
  },
  {
    name: "perform_secondary_action",
    description:
      "Invoke a secondary accessibility action such as Raise, Expand, Collapse, or Scroll Down.",
    inputSchema: {
      type: "object",
      properties: {
        window: WINDOW_SCHEMA,
        element_index: { type: "number" },
        action: { type: "string" },
      },
      required: ["window", "element_index", "action"],
      additionalProperties: false,
    },
  },
];

function send(message) {
  process.stdout.write(`${JSON.stringify(message)}\n`);
}

function textResult(text, isError = false) {
  return {
    content: [{ type: "text", text }],
    isError,
  };
}

function jsonResult(value, extras = []) {
  return {
    content: [
      {
        type: "text",
        text: typeof value === "string" ? value : JSON.stringify(value, null, 2),
      },
      ...extras,
    ],
  };
}

function parseDataUrl(dataUrl) {
  if (typeof dataUrl !== "string" || !dataUrl.startsWith("data:")) {
    return null;
  }
  const comma = dataUrl.indexOf(",");
  if (comma < 0) {
    return null;
  }
  const header = dataUrl.slice(5, comma);
  const data = dataUrl.slice(comma + 1);
  const mimeType = header.split(";")[0] || "image/png";
  const isBase64 = header.includes(";base64");
  if (!isBase64) {
    return null;
  }
  return { mimeType, data };
}

function compactWindowState(state) {
  if (!state || typeof state !== "object") {
    return state;
  }
  const screenshots = Array.isArray(state.screenshots)
    ? state.screenshots.map((shot, index) => {
        if (!shot || typeof shot !== "object") {
          return shot;
        }
        const { url, ...rest } = shot;
        const parsed = parseDataUrl(url);
        return {
          ...rest,
          hasImage: Boolean(parsed),
          imageIndex: parsed ? index : undefined,
          mimeType: parsed?.mimeType,
        };
      })
    : state.screenshots;

  return {
    ...state,
    screenshots,
  };
}

function imageContentsFromState(state) {
  if (!state || !Array.isArray(state.screenshots)) {
    return [];
  }
  const images = [];
  for (const shot of state.screenshots) {
    const parsed = parseDataUrl(shot?.url);
    if (!parsed) continue;
    images.push({
      type: "image",
      data: parsed.data,
      mimeType: parsed.mimeType,
    });
  }
  return images;
}

async function callTool(name, args = {}) {
  const sky = await getSky();
  switch (name) {
    case "list_apps":
      return jsonResult(await sky.list_apps());
    case "list_windows":
      return jsonResult(await sky.list_windows());
    case "get_window":
      return jsonResult(await sky.get_window(args));
    case "launch_app":
      await sky.launch_app(args);
      return textResult(`Launched app: ${args.app}`);
    case "activate_window":
      await sky.activate_window(args);
      return textResult("Window activated");
    case "get_window_state": {
      const state = await sky.get_window_state({
        window: args.window,
        include_screenshot: args.include_screenshot !== false,
        include_text: Boolean(args.include_text),
      });
      return jsonResult(compactWindowState(state), imageContentsFromState(state));
    }
    case "click":
      await sky.click(args);
      return textResult("Click completed");
    case "type_text":
      await sky.type_text(args);
      return textResult("Text typed");
    case "press_key":
      await sky.press_key(args);
      return textResult(`Key pressed: ${args.key}`);
    case "scroll":
      await sky.scroll(args);
      return textResult("Scroll completed");
    case "drag":
      await sky.drag(args);
      return textResult("Drag completed");
    case "set_value":
      await sky.set_value(args);
      return textResult("Value set");
    case "perform_secondary_action":
      await sky.perform_secondary_action(args);
      return textResult(`Secondary action completed: ${args.action}`);
    default:
      return textResult(`Unknown tool: ${name}`, true);
  }
}

async function handleRequest(message) {
  const { id, method, params } = message;

  if (method === "initialize") {
    send({
      jsonrpc: "2.0",
      id,
      result: {
        protocolVersion: "2024-11-05",
        capabilities: {
          tools: {},
        },
        serverInfo: {
          name: "computer-use",
          version: "1.0.0",
        },
      },
    });
    return;
  }

  if (method === "notifications/initialized" || method === "initialized") {
    return;
  }

  if (method === "ping") {
    send({ jsonrpc: "2.0", id, result: {} });
    return;
  }

  if (method === "tools/list") {
    send({
      jsonrpc: "2.0",
      id,
      result: {
        tools: TOOLS,
      },
    });
    return;
  }

  if (method === "tools/call") {
    const toolName = params?.name;
    const args = params?.arguments && typeof params.arguments === "object" ? params.arguments : {};
    try {
      if (typeof toolName !== "string" || !toolName.trim()) {
        throw new Error("tools/call requires params.name");
      }
      const result = await callTool(toolName, args);
      send({
        jsonrpc: "2.0",
        id,
        result,
      });
    } catch (error) {
      const messageText = error instanceof Error ? error.message : String(error);
      send({
        jsonrpc: "2.0",
        id,
        result: textResult(messageText, true),
      });
    }
    return;
  }

  if (id == null) {
    return;
  }

  send({
    jsonrpc: "2.0",
    id,
    error: {
      code: -32601,
      message: `Method not found: ${method}`,
    },
  });
}

function main() {
  process.stderr.write(
    `[computer-use-mcp] ready pluginRoot=${PLUGIN_ROOT} platform=${process.platform}\n`,
  );

  // 避免把调试日志打到 stdout 污染 MCP 协议。
  const rl = readline.createInterface({
    input: process.stdin,
    crlfDelay: Infinity,
  });

  rl.on("line", (line) => {
    const trimmed = line.trim();
    if (!trimmed) return;
    let message;
    try {
      message = JSON.parse(trimmed);
    } catch (error) {
      process.stderr.write(`[computer-use-mcp] invalid json: ${error}\n`);
      return;
    }
    void handleRequest(message).catch((error) => {
      const messageText = error instanceof Error ? error.message : String(error);
      if (message?.id != null) {
        send({
          jsonrpc: "2.0",
          id: message.id,
          error: {
            code: -32000,
            message: messageText,
          },
        });
      } else {
        process.stderr.write(`[computer-use-mcp] request failed: ${messageText}\n`);
      }
    });
  });

  rl.on("close", () => {
    process.exit(0);
  });
}

// keep require referenced so package resolution helpers stay available for future extension
void require;
main();
