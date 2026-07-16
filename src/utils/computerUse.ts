import type { ChatMessage, ThreadRuntimeState } from "../stores/appStore";

const COMPUTER_USE_NAME_RE = /computer[-_]?use/i;
const SKY_API_RE =
  /\bsky\.(list_apps|list_windows|get_window|get_window_state|click|type_text|press_key|drag|scroll|activate_window|launch_app|set_value|perform_secondary_action)\b/i;
const SETUP_RE = /setupComputerUseRuntime|computer-use-client\.mjs|@oai\/sky/i;

function textLooksLikeComputerUse(value: string): boolean {
  const text = value.trim();
  if (!text) return false;
  if (COMPUTER_USE_NAME_RE.test(text)) return true;
  if (SETUP_RE.test(text)) return true;
  if (SKY_API_RE.test(text)) return true;
  return false;
}

function inspectPayload(value: unknown, depth = 0): boolean {
  if (depth > 4 || value == null) return false;
  if (typeof value === "string") {
    return textLooksLikeComputerUse(value);
  }
  if (Array.isArray(value)) {
    return value.some((item) => inspectPayload(item, depth + 1));
  }
  if (typeof value === "object") {
    const record = value as Record<string, unknown>;
    const server = record.server;
    if (typeof server === "string" && COMPUTER_USE_NAME_RE.test(server)) {
      return true;
    }
    for (const key of ["code", "function", "cmd", "command", "prompt", "text", "script"]) {
      if (inspectPayload(record[key], depth + 1)) {
        return true;
      }
    }
    return Object.values(record).some((item) => inspectPayload(item, depth + 1));
  }
  return false;
}

/** 判断一次工具调用是否属于 Computer Use 控制链路。 */
export function isComputerUseToolCall(name: string, args = ""): boolean {
  if (textLooksLikeComputerUse(name)) {
    return true;
  }
  if (textLooksLikeComputerUse(args)) {
    return true;
  }
  try {
    return inspectPayload(JSON.parse(args));
  } catch {
    return false;
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

function messagesHaveComputerUseActivity(messages: ChatMessage[] | undefined): boolean {
  if (!messages?.length) return false;
  for (let i = messages.length - 1; i >= 0; i -= 1) {
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
 * 若当前回合仍在流式输出，且本回合已发生过 Computer Use，则保持激活，避免工具间隙闪断。
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
    if (runtime.isStreaming && messagesHaveComputerUseActivity(runtime.messages)) {
      return true;
    }
  }

  if (isStreaming && messagesHaveComputerUseActivity(messages)) {
    return true;
  }

  return false;
}
