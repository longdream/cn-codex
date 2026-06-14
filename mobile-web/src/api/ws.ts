import { useMobileStore } from "../stores/mobileStore";
import { fetchThreadMessages, genId } from "./http";

let socket: WebSocket | null = null;
let reconnectTimer: ReturnType<typeof setTimeout> | null = null;

export function connectWebSocket() {
  if (socket?.readyState === WebSocket.OPEN) return;

  const wsUrl = `ws://${window.location.host}/ws`;
  socket = new WebSocket(wsUrl);

  socket.onopen = () => {
    useMobileStore.getState().setConnected(true);
    if (reconnectTimer) {
      clearTimeout(reconnectTimer);
      reconnectTimer = null;
    }
  };

  socket.onclose = () => {
    useMobileStore.getState().setConnected(false);
    scheduleReconnect();
  };

  socket.onerror = () => {
    socket?.close();
  };

  socket.onmessage = (event) => {
    try {
      const data = JSON.parse(event.data) as {
        event: string;
        payload: Record<string, unknown>;
      };
      handleEvent(data.event, data.payload);
    } catch {
      // ignore parse errors
    }
  };
}

function scheduleReconnect() {
  if (reconnectTimer) return;
  reconnectTimer = setTimeout(() => {
    reconnectTimer = null;
    connectWebSocket();
  }, 3000);
}

function handleEvent(event: string, payload: Record<string, unknown>) {
  const store = useMobileStore.getState();

  switch (event) {
    case "turn-started": {
      const threadId = payload.threadId as string | undefined;
      if (threadId) {
        store.setActiveThreadId(threadId);
      }
      if (!threadId || threadId === store.currentThreadId) {
        store.setStreaming(true);
        store.clearStreamingText();
      }
      break;
    }

    case "agent-message-delta": {
      const threadId = payload.threadId as string | undefined;
      if (threadId && threadId !== store.currentThreadId) {
        break;
      }
      const delta = payload.delta as string;
      if (delta) {
        store.appendStreamingText(delta);
      }
      break;
    }

    case "turn-completed": {
      const tcThreadId = payload.threadId as string | undefined;
      if (tcThreadId && tcThreadId !== store.currentThreadId) {
        break;
      }
      const text = store.streamingText;
      if (text) {
        store.addMessage({
          id: genId(),
          role: "assistant",
          content: text,
          timestamp: Date.now(),
        });
      }
      store.clearStreamingText();
      store.setStreaming(false);
      break;
    }

    case "tool-calls-start": {
      const tcsThreadId = payload.threadId as string | undefined;
      if (tcsThreadId && tcsThreadId !== store.currentThreadId) {
        break;
      }
      const calls = payload.calls as Array<{ name: string; arguments: string }> | undefined;
      if (calls?.length) {
        const toolNames: Record<string, string> = {
          shell: "执行命令",
          file_write: "写入文件",
          file_read: "读取文件",
          file_edit: "编辑文件",
          browser: "浏览器操作",
          list_directory: "查看目录",
        };
        const desc = calls.map((c) => toolNames[c.name] || c.name).join("、");
        store.addMessage({
          id: genId(),
          role: "tool",
          content: `🔧 ${desc}`,
          timestamp: Date.now(),
        });
      }
      break;
    }

    case "tool-exec-end": {
      const teeThreadId = payload.threadId as string | undefined;
      if (teeThreadId && teeThreadId !== store.currentThreadId) {
        break;
      }
      const tool = payload.tool as string;
      const exitCode = payload.exitCode as number | undefined;
      if (exitCode !== undefined && exitCode !== 0) {
        store.addMessage({
          id: genId(),
          role: "tool",
          content: `⚠️ ${tool} 退出码: ${exitCode}`,
          timestamp: Date.now(),
        });
      }
      break;
    }
  }
}
