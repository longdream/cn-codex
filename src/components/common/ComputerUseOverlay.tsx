import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { windowSetComputerUseOverlay } from "../../api/window";
import { useAppStore } from "../../stores/appStore";
import { isComputerUseToolCall, resolveComputerUseActive } from "../../utils/computerUse";
import { ComputerUseOverlayFrame, COMPUTER_USE_CURSOR_CSS } from "./ComputerUseOverlayFrame";

/**
 * 主窗口侧 Computer Use 控制器：
 * - 通过既有消息状态检测激活
 * - 优先打开整屏透明置顶 overlay 窗
 * - 若后端窗口能力不可用，则回退到主窗内覆盖层
 */
export function ComputerUseOverlay() {
  const messages = useAppStore((s) => s.messages);
  const isStreaming = useAppStore((s) => s.isStreaming);
  const threadRuntimeStates = useAppStore((s) => s.threadRuntimeStates);
  const lastActiveRef = useRef<boolean | null>(null);
  const syncSeqRef = useRef(0);
  const desiredActiveRef = useRef(false);
  const prevHadRunningRef = useRef(false);
  /** 用户手动关闭后，在本轮「无 running 工具」期间抑制遮罩，避免粘滞。 */
  const [userDismissed, setUserDismissed] = useState(false);
  const [useScreenOverlay, setUseScreenOverlay] = useState(true);

  const systemActive = useMemo(
    () =>
      resolveComputerUseActive({
        messages,
        isStreaming,
        threadRuntimeStates,
      }),
    [isStreaming, messages, threadRuntimeStates],
  );

  // 仅当真正有 running 的 CU 工具时，取消用户 dismiss，允许再次显示。
  const hasRunningComputerUse = useMemo(() => {
    const allMessages = [
      ...messages,
      ...Object.values(threadRuntimeStates).flatMap((runtime) => runtime.messages ?? []),
    ];
    return allMessages.some((message) =>
      message.toolCalls?.some(
        (toolCall) =>
          toolCall.status === "running" && isComputerUseToolCall(toolCall.name, toolCall.arguments),
      ),
    );
  }, [messages, threadRuntimeStates]);

  useEffect(() => {
    // 仅在「新出现」running CU 工具时取消 dismiss；
    // 避免用户 Esc 关闭后立刻被当前仍 running 的工具重新点亮。
    if (hasRunningComputerUse && !prevHadRunningRef.current) {
      setUserDismissed(false);
    }
    prevHadRunningRef.current = hasRunningComputerUse;
  }, [hasRunningComputerUse]);

  // 系统判定已空闲时，清掉 dismiss 标记，避免影响后续会话。
  useEffect(() => {
    if (!systemActive && userDismissed) {
      setUserDismissed(false);
    }
  }, [systemActive, userDismissed]);

  const active = systemActive && !userDismissed;

  useEffect(() => {
    desiredActiveRef.current = active;
  }, [active]);

  const forceCloseOverlay = useCallback(async () => {
    setUserDismissed(true);
    desiredActiveRef.current = false;
    lastActiveRef.current = false;
    try {
      await windowSetComputerUseOverlay(false);
    } catch {
      // ignore — 主窗内回退层会随 active=false 消失
    }
  }, []);

  useEffect(() => {
    // 主窗始终同步受控光标：整屏覆盖窗是 click-through，不会接管系统光标。
    if (active) {
      document.documentElement.style.setProperty("--computer-use-cursor", COMPUTER_USE_CURSOR_CSS);
      document.body.classList.add("computer-use-controlling");
    } else {
      document.documentElement.style.removeProperty("--computer-use-cursor");
      document.body.classList.remove("computer-use-controlling");
    }

    return () => {
      document.documentElement.style.removeProperty("--computer-use-cursor");
      document.body.classList.remove("computer-use-controlling");
    };
  }, [active]);

  // Esc 强制关闭「控制中」标记（整屏层 click-through 时也能从主窗关掉）。
  useEffect(() => {
    if (!active) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        void forceCloseOverlay();
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [active, forceCloseOverlay]);

  useEffect(() => {
    let cancelled = false;

    const syncOverlay = async () => {
      if (!useScreenOverlay) {
        return;
      }
      const seq = ++syncSeqRef.current;
      const nextActive = active;

      // 已是激活态时跳过重复 show；关闭路径始终下发，避免残留外框 / 竞态重开。
      if (lastActiveRef.current === nextActive && nextActive) {
        return;
      }

      try {
        await windowSetComputerUseOverlay(nextActive);
        // 异步竞态：晚到的 show 可能在 hide 之后完成，必须按最新意图纠正。
        if (cancelled) {
          return;
        }
        if (desiredActiveRef.current !== nextActive) {
          if (nextActive && !desiredActiveRef.current) {
            await windowSetComputerUseOverlay(false);
            if (!cancelled && seq === syncSeqRef.current) {
              lastActiveRef.current = false;
            }
          }
          return;
        }
        if (seq === syncSeqRef.current) {
          lastActiveRef.current = nextActive;
        }
      } catch (error) {
        if (cancelled || seq !== syncSeqRef.current) {
          return;
        }
        console.warn(
          "[computer-use] fullscreen overlay unavailable, fallback to app frame",
          error,
        );
        // 关闭整屏覆盖失败时仍允许主窗内回退层接管，避免残留外框。
        if (!nextActive) {
          lastActiveRef.current = false;
        }
        setUseScreenOverlay(false);
      }
    };

    void syncOverlay();

    return () => {
      cancelled = true;
    };
  }, [active, useScreenOverlay]);

  useEffect(() => {
    return () => {
      // 组件卸载时尽量关掉整屏覆盖，避免残留。
      if (useScreenOverlay) {
        void windowSetComputerUseOverlay(false).catch(() => {
          // ignore cleanup failure
        });
        lastActiveRef.current = false;
      }
    };
  }, [useScreenOverlay]);

  // 后端整屏覆盖可用时，主窗不再渲染应用内边框。
  if (useScreenOverlay) {
    return null;
  }

  return (
    <ComputerUseOverlayFrame
      active={active}
      variant="app"
      applyCursor={false}
      onDismiss={() => {
        void forceCloseOverlay();
      }}
    />
  );
}
