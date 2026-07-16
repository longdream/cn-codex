import { useEffect, useMemo, useRef, useState } from "react";
import { windowSetComputerUseOverlay } from "../../api/window";
import { useAppStore } from "../../stores/appStore";
import { resolveComputerUseActive } from "../../utils/computerUse";
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
  const [useScreenOverlay, setUseScreenOverlay] = useState(true);

  const active = useMemo(
    () =>
      resolveComputerUseActive({
        messages,
        isStreaming,
        threadRuntimeStates,
      }),
    [isStreaming, messages, threadRuntimeStates],
  );

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

  useEffect(() => {
    let cancelled = false;

    const syncOverlay = async () => {
      if (!useScreenOverlay) {
        return;
      }
      const seq = ++syncSeqRef.current;
      const nextActive = active;

      // 已是激活态时跳过重复 show；关闭路径始终下发，避免残留外框。
      if (lastActiveRef.current === nextActive && nextActive) {
        return;
      }

      try {
        await windowSetComputerUseOverlay(nextActive);
        // 忽略过期的异步结果，避免后到的 show 把已结束的 hide 覆盖掉。
        if (!cancelled && seq === syncSeqRef.current) {
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

  return <ComputerUseOverlayFrame active={active} variant="app" applyCursor={false} />;
}
