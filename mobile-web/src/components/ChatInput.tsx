import { useState } from "react";
import { useMobileStore } from "../stores/mobileStore";
import { sendMessage, interruptTurn, genId } from "../api/http";

export function ChatInput() {
  const [text, setText] = useState("");
  const [sending, setSending] = useState(false);
  const currentThreadId = useMobileStore((s) => s.currentThreadId);
  const isStreaming = useMobileStore((s) => s.isStreaming);

  const handleSend = async () => {
    const msg = text.trim();
    if (!msg || !currentThreadId || sending || isStreaming) return;

    useMobileStore.getState().addMessage({
      id: genId(),
      role: "user",
      content: msg,
      timestamp: Math.floor(Date.now() / 1000),
    });
    setText("");
    setSending(true);

    try {
      await sendMessage(currentThreadId, msg);
    } catch (e) {
      console.error("Send message error:", e);
    } finally {
      setSending(false);
    }
  };

  const handleStop = async () => {
    if (!currentThreadId) return;
    try {
      await interruptTurn(currentThreadId);
    } catch (e) {
      console.error("Interrupt error:", e);
    }
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      void handleSend();
    }
  };

  return (
    <div className="chat-input">
      <textarea
        className="chat-input-textarea"
        value={text}
        onChange={(e) => setText(e.target.value)}
        onKeyDown={handleKeyDown}
        placeholder={isStreaming ? "AI 正在回复..." : "输入消息..."}
        disabled={sending}
        rows={1}
      />
      {isStreaming ? (
        <button
          className="chat-input-stop"
          onClick={() => void handleStop()}
        >
          停止
        </button>
      ) : (
        <button
          className="chat-input-send"
          onClick={() => void handleSend()}
          disabled={!text.trim() || sending || !currentThreadId}
        >
          {sending ? "…" : "发送"}
        </button>
      )}
    </div>
  );
}
