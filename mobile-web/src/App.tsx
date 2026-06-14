import { useEffect } from "react";
import { useMobileStore } from "./stores/mobileStore";
import { connectWebSocket } from "./api/ws";
import { fetchThreads, fetchActiveThread } from "./api/http";
import { Header } from "./components/Header";
import { ThreadList } from "./components/ThreadList";
import { ChatView } from "./components/ChatView";
import { ChatInput } from "./components/ChatInput";

export default function App() {
  const view = useMobileStore((s) => s.view);

  useEffect(() => {
    connectWebSocket();
    void fetchThreads().then(() => fetchActiveThread());
  }, []);

  return (
    <div className="app-shell">
      <Header />
      <main className="app-main">
        {view === "threads" ? <ThreadList /> : <ChatView />}
      </main>
      {view === "chat" && <ChatInput />}
    </div>
  );
}
