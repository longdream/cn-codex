import { useState } from "react";
import { useMobileStore } from "../stores/mobileStore";
import { sendMessage, genId } from "../api/http";

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
        placeholder={isStreaming ? "等待回复中..." : "输入消息..."}
        disabled={sending || isStreaming}
        rows={1}
      />
      <button
        className="chat-input-send"
        onClick={() => void handleSend()}
        disabled={!text.trim() || sending || isStreaming || !currentThreadId}
      >
        {sending ? "…" : "发送"}
      </button>
    </div>
  );
}
