import { useEffect } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { useAppStore, type ToolCallItem } from "../stores/appStore";

function toolDisplayLabel(name: string, args: string): string {
  try {
    const parsed = JSON.parse(args);
    switch (name) {
      case "shell":
        return Array.isArray(parsed.command)
          ? parsed.command.join(" ")
          : String(parsed.command ?? "shell");
      case "read_file":
        return parsed.path ?? "read_file";
      case "write_file":
        return parsed.path ?? "write_file";
      case "list_directory":
        return parsed.path ?? ".";
      default:
        return name;
    }
  } catch {
    return name;
  }
}

export function useTauriEvents() {
  useEffect(() => {
    let cancelled = false;
    const unlisten: UnlistenFn[] = [];

    const setup = async () => {
      const listeners: Array<Promise<UnlistenFn>> = [
        listen<{ delta: string }>("agent-message-delta", (e) => {
          const delta = e.payload.delta;
          const prev = useAppStore.getState().streamingText;
          if (prev.length === 0) {
            console.log("[event] agent-message-delta: first chunk:", JSON.stringify(delta.slice(0, 80)));
          }
          useAppStore.getState().appendStreamingText(delta);
        }),

        listen<{ threadId: string; turn: { id: string } }>(
          "turn-started",
          (e) => {
            console.log("[event] turn-started:", e.payload);
            useAppStore.getState().setCurrentTurnId(e.payload.turn?.id ?? null);
            useAppStore.getState().setStreaming(true);
            useAppStore.getState().clearStreamingText();
          },
        ),

        listen<{ threadId: string; turn: { id: string } }>(
          "turn-completed",
          (e) => {
            const store = useAppStore.getState();
            const text = store.streamingText;
            console.log("[event] turn-completed:", e.payload, "pendingText length:", text.length, "messages:", store.messages.length);
            if (text) {
              store.addMessage({
                id: crypto.randomUUID(),
                role: "assistant",
                content: text,
                timestamp: Date.now(),
              });
            }
            store.clearStreamingText();
            store.setStreaming(false);
            store.setCurrentTurnId(null);
          },
        ),

        listen<{
          threadId: string;
          calls: Array<{ id: string; name: string; arguments: string }>;
        }>("tool-calls-start", (e) => {
          console.log("[event] tool-calls-start:", e.payload.calls.length, "calls:", e.payload.calls.map((c) => c.name));
          const store = useAppStore.getState();
          const pendingText = store.streamingText;
          if (pendingText) {
            console.log("[event] flushing pendingText:", pendingText.slice(0, 100));
            store.addMessage({
              id: crypto.randomUUID(),
              role: "assistant",
              content: pendingText,
              timestamp: Date.now(),
            });
            store.clearStreamingText();
          }

          const items: ToolCallItem[] = e.payload.calls.map((c) => ({
            id: c.id,
            name: c.name,
            arguments: c.arguments,
            status: "running" as const,
            displayLabel: toolDisplayLabel(c.name, c.arguments),
          }));
          store.addMessage({
            id: `tcg-${Date.now()}`,
            role: "system",
            content: "",
            timestamp: Date.now(),
            toolCalls: items,
          });
        }),

        listen<{ threadId: string; callId?: string; tool: string; exitCode?: number; output?: string }>(
          "tool-exec-end",
          (e) => {
            console.log("[event] tool-exec-end:", e.payload.tool, "callId:", e.payload.callId, "exit:", e.payload.exitCode, "output length:", e.payload.output?.length ?? 0);
            const status = e.payload.exitCode === 0 ? "success" : "failed";
            const store = useAppStore.getState();
            const output = e.payload.output;

            if (e.payload.callId) {
              store.updateToolCallStatus(e.payload.callId, status, output);
              return;
            }

            const msgs = store.messages;
            for (let i = msgs.length - 1; i >= 0; i--) {
              const tc = msgs[i].toolCalls;
              if (!tc) continue;
              const runningIdx = tc.findIndex(
                (c) => c.status === "running" && c.name === e.payload.tool,
              );
              if (runningIdx >= 0) {
                store.updateToolCallStatus(tc[runningIdx].id, status, output);
                return;
              }
            }
            console.warn("[event] tool-exec-end: no matching running tool found for", e.payload.tool);
          },
        ),

        listen<{ threadId: string; tool: string; exitCode?: number }>(
          "tool-exec-start",
          (e) => {
            console.log("[event] tool-exec-start:", e.payload.tool);
          },
        ),

        listen<{ threadId: string; results: Array<{ id: string; tool: string; success: boolean }> }>(
          "tool-calls-end",
          (e) => {
            console.log("[event] tool-calls-end:", e.payload.results.length, "results");
          },
        ),

        listen<{ error?: { message?: string }; message?: string; threadId?: string }>(
          "server-error",
          (e) => {
            console.error("[event] server-error:", e.payload);
            const msg =
              e.payload.message ??
              e.payload.error?.message ??
              JSON.stringify(e.payload);
            useAppStore.getState().addMessage({
              id: crypto.randomUUID(),
              role: "system",
              content: `Error: ${msg}`,
              timestamp: Date.now(),
            });
          },
        ),
      ];

      const fns = await Promise.all(listeners);

      if (cancelled) {
        fns.forEach((fn) => fn());
        return;
      }

      unlisten.push(...fns);
    };

    setup();

    return () => {
      cancelled = true;
      unlisten.forEach((fn) => fn());
    };
  }, []);
}
