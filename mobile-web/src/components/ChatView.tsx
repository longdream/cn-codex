import { useEffect, useRef } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { useMobileStore } from "../stores/mobileStore";

export function ChatView() {
  const messages = useMobileStore((s) => s.messages);
  const streamingText = useMobileStore((s) => s.streamingText);
  const isStreaming = useMobileStore((s) => s.isStreaming);
  const bottomRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [messages, streamingText]);

  return (
    <div className="chat-view">
      {messages.length === 0 && !isStreaming && (
        <div className="empty-state">
          <p>暂无消息记录</p>
          <p className="empty-hint">在下方输入消息开始对话，或等待 PC 端推送内容</p>
        </div>
      )}

      {messages.map((msg) => (
        <div key={msg.id} className={`message message-${msg.role}`}>
          {msg.role === "tool" ? (
            <div className="tool-card">
              <span className="tool-card-text">{msg.content}</span>
            </div>
          ) : msg.role === "user" ? (
            <div className="user-bubble">
              <p className="message-text">{msg.content}</p>
            </div>
          ) : (
            <div className="assistant-block">
              <ReactMarkdown
                remarkPlugins={[remarkGfm]}
                components={{
                  code: ({ className, children }) => {
                    const code = String(children).replace(/\n$/, "");
                    const language = /language-([\w-]+)/.exec(className ?? "")?.[1] ?? "";
                    const isBlock = Boolean(language) || code.includes("\n");
                    if (isBlock) {
                      return (
                        <div className="code-block">
                          {language && <span className="code-lang">{language}</span>}
                          <pre><code>{code}</code></pre>
                        </div>
                      );
                    }
                    return <code className="inline-code">{code}</code>;
                  },
                  a: ({ href, children }) => (
                    <a href={href} target="_blank" rel="noopener noreferrer" className="msg-link">
                      {children}
                    </a>
                  ),
                }}
              >
                {msg.content}
              </ReactMarkdown>
            </div>
          )}
        </div>
      ))}

      {isStreaming && streamingText && (
        <div className="message message-assistant">
          <div className="assistant-block streaming">
            <ReactMarkdown remarkPlugins={[remarkGfm]}>{streamingText}</ReactMarkdown>
            <span className="cursor-blink" />
          </div>
        </div>
      )}

      {isStreaming && !streamingText && (
        <div className="message message-assistant">
          <div className="thinking-dots">
            <span />
            <span />
            <span />
          </div>
        </div>
      )}

      <div ref={bottomRef} />
    </div>
  );
}
