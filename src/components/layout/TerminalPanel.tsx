import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import "@xterm/xterm/css/xterm.css";
import { IconPlus, IconX } from "@tabler/icons-react";
import { useAppStore } from "../../stores/appStore";

interface TerminalTab {
  id: string;
  sessionId: string;
  label: string;
  terminal: Terminal;
  fitAddon: FitAddon;
}

export function TerminalPanel() {
  const workspaceCwd = useAppStore((s) => s.workspaceCwd);
  const [tabs, setTabs] = useState<TerminalTab[]>([]);
  const [activeTabId, setActiveTabId] = useState<string | null>(null);
  const containerRef = useRef<HTMLDivElement>(null);
  const cleanupRef = useRef<Map<string, (() => void)>>(new Map());

  const activeTab = tabs.find((t) => t.id === activeTabId) ?? null;

  const createTerminalTab = useCallback(async () => {
    try {
      const sessionId = await invoke<string>("terminal_create", {
        cwd: workspaceCwd ?? undefined,
      });

      const terminal = new Terminal({
        cursorBlink: true,
        fontSize: 13,
        fontFamily: "'Cascadia Code', 'Fira Code', Consolas, monospace",
        theme: {
          background: "#1a1a2e",
          foreground: "#e0e0e0",
          cursor: "#4ade80",
          selectionBackground: "rgba(74, 222, 128, 0.25)",
        },
        scrollback: 5000,
      });

      const fitAddon = new FitAddon();
      terminal.loadAddon(fitAddon);

      terminal.onData((data) => {
        invoke("terminal_write", { sessionId, data }).catch(console.error);
      });

      const tabId = crypto.randomUUID();

      const unlisten = await listen<{
        sessionId: string;
        data: string;
        closed?: boolean;
      }>("terminal-output", (e) => {
        if (e.payload.sessionId !== sessionId) return;
        if (e.payload.closed) {
          terminal.write("\r\n[Process exited]\r\n");
          return;
        }
        terminal.write(e.payload.data);
      });

      cleanupRef.current.set(tabId, () => {
        unlisten();
        terminal.dispose();
        invoke("terminal_close", { sessionId }).catch(console.error);
      });

      const newTab: TerminalTab = {
        id: tabId,
        sessionId,
        label: `Terminal ${tabs.length + 1}`,
        terminal,
        fitAddon,
      };

      setTabs((prev) => [...prev, newTab]);
      setActiveTabId(tabId);
    } catch (err) {
      console.error("Failed to create terminal:", err);
    }
  }, [workspaceCwd, tabs.length]);

  const closeTab = useCallback(
    (tabId: string) => {
      const cleanup = cleanupRef.current.get(tabId);
      if (cleanup) {
        cleanup();
        cleanupRef.current.delete(tabId);
      }
      setTabs((prev) => {
        const next = prev.filter((t) => t.id !== tabId);
        if (activeTabId === tabId) {
          setActiveTabId(next[next.length - 1]?.id ?? null);
        }
        return next;
      });
    },
    [activeTabId],
  );

  useEffect(() => {
    if (tabs.length === 0) {
      createTerminalTab();
    }
  }, []);

  useEffect(() => {
    if (!activeTab || !containerRef.current) return;
    const el = containerRef.current;
    el.innerHTML = "";
    activeTab.terminal.open(el);
    activeTab.fitAddon.fit();

    const cols = activeTab.terminal.cols;
    const rows = activeTab.terminal.rows;
    invoke("terminal_resize", {
      sessionId: activeTab.sessionId,
      cols,
      rows,
    }).catch(console.error);

    const observer = new ResizeObserver(() => {
      try {
        activeTab.fitAddon.fit();
        const newCols = activeTab.terminal.cols;
        const newRows = activeTab.terminal.rows;
        invoke("terminal_resize", {
          sessionId: activeTab.sessionId,
          cols: newCols,
          rows: newRows,
        }).catch(console.error);
      } catch {
        // ignore fit errors during resize
      }
    });
    observer.observe(el);

    return () => {
      observer.disconnect();
    };
  }, [activeTab?.id]);

  useEffect(() => {
    return () => {
      for (const cleanup of cleanupRef.current.values()) {
        cleanup();
      }
      cleanupRef.current.clear();
    };
  }, []);

  return (
    <div className="flex flex-1 flex-col overflow-hidden">
      {/* Tab bar */}
      <div className="flex items-center gap-1 border-b border-[var(--border-subtle)] px-2 py-1">
        {tabs.map((tab) => (
          <div
            key={tab.id}
            className={`group flex items-center gap-1 rounded-[var(--radius-sm)] px-2 py-1 text-[11px] transition-colors cursor-pointer ${
              activeTabId === tab.id
                ? "bg-[var(--accent-soft)] text-[var(--accent-strong)]"
                : "text-[var(--text-muted)] hover:bg-[var(--surface-elevated)]"
            }`}
            onClick={() => setActiveTabId(tab.id)}
          >
            <span className="truncate">{tab.label}</span>
            <button
              type="button"
              onClick={(e) => {
                e.stopPropagation();
                closeTab(tab.id);
              }}
              className="flex-shrink-0 opacity-0 transition-opacity group-hover:opacity-100 hover:text-[var(--danger)]"
            >
              <IconX size={10} stroke={2} />
            </button>
          </div>
        ))}
        <button
          type="button"
          onClick={() => void createTerminalTab()}
          className="flex h-5 w-5 items-center justify-center rounded-[var(--radius-sm)] text-[var(--text-faint)] transition-colors hover:bg-[var(--surface-elevated)] hover:text-[var(--text-strong)]"
          title="New terminal"
        >
          <IconPlus size={12} stroke={2} />
        </button>
      </div>

      {/* Terminal content */}
      <div ref={containerRef} className="flex-1 overflow-hidden bg-[#1a1a2e] p-1" />
    </div>
  );
}
