import { useMobileStore } from "../stores/mobileStore";

export function genId(): string {
  if (typeof crypto !== "undefined" && crypto.randomUUID) {
    return crypto.randomUUID();
  }
  return "xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx".replace(/[xy]/g, (c) => {
    const r = (Math.random() * 16) | 0;
    return (c === "x" ? r : (r & 0x3) | 0x8).toString(16);
  });
}

function getBaseUrl(): string {
  return window.location.origin;
}

export async function fetchThreads() {
  try {
    const res = await fetch(`${getBaseUrl()}/api/threads`);
    const data = await res.json();
    useMobileStore.getState().setThreads(data);
  } catch (e) {
    console.error("Failed to fetch threads:", e);
  }
}

export async function fetchActiveThread() {
  try {
    const res = await fetch(`${getBaseUrl()}/api/active-thread`);
    const data = await res.json();
    const threadId = data.threadId as string | null;
    useMobileStore.getState().setActiveThreadId(threadId);
    if (threadId) {
      useMobileStore.getState().setCurrentThread(threadId);
      await fetchThreadMessages(threadId);
    }
  } catch (e) {
    console.error("Failed to fetch active thread:", e);
  }
}

export async function fetchThreadMessages(threadId: string) {
  try {
    const res = await fetch(`${getBaseUrl()}/api/threads/${threadId}/messages`);
    if (!res.ok) {
      console.error("Messages API error:", res.status);
      return;
    }
    const data = await res.json();
    if (!Array.isArray(data)) {
      console.error("Messages API returned non-array:", typeof data);
      return;
    }
    const messages = data.map((m: { role: string; content: string; timestamp: number }) => ({
      id: genId(),
      role: m.role as "user" | "assistant" | "system" | "tool",
      content: m.content || "",
      timestamp: m.timestamp,
    }));
    useMobileStore.getState().setMessages(messages);
  } catch (e) {
    console.error("Failed to fetch messages:", e);
  }
}

export async function sendMessage(threadId: string, message: string) {
  const res = await fetch(`${getBaseUrl()}/api/threads/${threadId}/chat`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ message }),
  });
  if (!res.ok) {
    throw new Error(`Send failed: ${res.status}`);
  }
  return res.json();
}
