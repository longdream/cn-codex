import type { ChatMessage, ThreadRuntimeState } from "../stores/appStore";

const COMPUTER_USE_NAME_RE = /computer[-_]?use/i;
const SKY_API_RE =
  /\bsky\.(list_apps|list_windows|get_window|get_window_state|click|type_text|press_key|drag|scroll|activate_window|launch_app|set_value|perform_secondary_action)\b/i;
const SETUP_RE = /setupComputerUseRuntime|@oai\/sky/i;

function textLooksLikeComputerUseCode(value: string): boolean {
  const text = value.trim();
  if (!text) return false;
  // 仅匹配明确的运行时代码/API，避免把路径、插件目录名里的 computer-use 误判为控制中。
  if (SETUP_RE.test(text)) return true;
  if (SKY_API_RE.test(text)) return true;
  return false;
}

function inspectPayload(value: unknown, depth = 0): boolean {
  if (depth > 4 || value == null) return false;
  if (typeof value === "string") {
    return textLooksLikeComputerUseCode(value);
  }
  if (Array.isArray(value)) {
    return value.some((item) => inspectPayload(item, depth + 1));
  }
  if (typeof value === "object") {
    const record = value as Record<string, unknown>;
    const server = record.server;
    if (typeof server === "string" && COMPUTER_USE_NAME_RE.test(server.trim())) {
      return true;
    }
    // 只检查可能承载脚本/调用意图的字段，避免深扫任意字符串路径导致误判。
    for (const key of ["code", "function", "cmd", "command", "prompt", "text", "script", "tool", "name"]) {
      const field = record[key];
      if (typeof field === "string") {
        if (key === "tool" || key === "name") {
          if (COMPUTER_USE_NAME_RE.test(field)) {
            return true;
          }
          continue;
        }
        if (textLooksLikeComputerUseCode(field) || COMPUTER_USE_NAME_RE.test(field)) {
          // command/code 里出现 computer-use MCP 工具名时也算
          if (key === "code" || key === "function" || key === "script" || key === "text" || key === "prompt") {
            if (textLooksLikeComputerUseCode(field)) {
              return true;
            }
            // 避免仅因路径包含 plugins/computer-use 就命中
            continue;
          }
          if (key === "cmd" || key === "command") {
            // shell 命令路径含 computer-use 目录不等于正在操控桌面
            if (textLooksLikeComputerUseCode(field)) {
              return true;
            }
            continue;
          }
        }
      } else if (inspectPayload(field, depth + 1)) {
        return true;
      }
    }
  }
  return false;
}

/** 判断一次工具调用是否属于 Computer Use 控制链路。 */
export function isComputerUseToolCall(name: string, args = ""): boolean {
  const toolName = name.trim();
  if (COMPUTER_USE_NAME_RE.test(toolName)) {
    return true;
  }
  // 直接 MCP 命名空间：mcp__computer-use__*
  if (/^mcp__[^_]*computer[-_]?use__/i.test(toolName)) {
    return true;
  }
  try {
    return inspectPayload(JSON.parse(args));
  } catch {
    // 非 JSON 参数：只认 sky/setup，不认裸字符串里的 computer-use 路径
    return textLooksLikeComputerUseCode(args);
  }
}

function messagesHaveRunningComputerUse(messages: ChatMessage[] | undefined): boolean {
  if (!messages?.length) return false;
  for (const message of messages) {
    if (!message.toolCalls?.length) continue;
    for (const toolCall of message.toolCalls) {
      if (toolCall.status !== "running") continue;
      if (isComputerUseToolCall(toolCall.name, toolCall.arguments)) {
        return true;
      }
    }
  }
  return false;
}

/** 仅检查「当前回合」（最后一条 user 消息之后）是否出现过 Computer Use。 */
function messagesHaveComputerUseActivityInCurrentTurn(
  messages: ChatMessage[] | undefined,
): boolean {
  if (!messages?.length) return false;
  let start = 0;
  for (let i = messages.length - 1; i >= 0; i -= 1) {
    if (messages[i].role === "user") {
      start = i + 1;
      break;
    }
  }
  for (let i = start; i < messages.length; i += 1) {
    const message = messages[i];
    if (!message.toolCalls?.length) continue;
    for (const toolCall of message.toolCalls) {
      if (isComputerUseToolCall(toolCall.name, toolCall.arguments)) {
        return true;
      }
    }
  }
  return false;
}

/**
 * 任意线程出现 Computer Use 运行中工具时立即激活；
 * 若当前回合仍在流式输出，且本回合（非历史）已发生过 Computer Use，则保持激活，避免工具间隙闪断。
 * 流式结束后必须关闭，避免「控制中」标记一直无法消失。
 */
export function resolveComputerUseActive(params: {
  messages: ChatMessage[];
  isStreaming: boolean;
  threadRuntimeStates: Record<string, ThreadRuntimeState>;
}): boolean {
  const { messages, isStreaming, threadRuntimeStates } = params;

  if (messagesHaveRunningComputerUse(messages)) {
    return true;
  }

  for (const runtime of Object.values(threadRuntimeStates)) {
    if (messagesHaveRunningComputerUse(runtime.messages)) {
      return true;
    }
    // 仅本回合 + 仍在流式时保持，避免历史会话再次点亮遮罩
    if (runtime.isStreaming && messagesHaveComputerUseActivityInCurrentTurn(runtime.messages)) {
      return true;
    }
  }

  if (isStreaming && messagesHaveComputerUseActivityInCurrentTurn(messages)) {
    return true;
  }

  return false;
}
