import { useEffect, useState, useCallback } from "react";
import { IconPlayerPlay } from "@tabler/icons-react";
import { useAppStore } from "../../stores/appStore";
import { rejectApproval } from "../../api/approval";

/**
 * 机器人提问倒计时横幅。
 * 当后端检测到 AI 在机器人模式下向用户提问时，
 * 会发送 robot-waiting-for-input 事件并阻塞等待。
 * 此组件展示剩余秒数，用户可手动"跳过等待"立刻让 AI 继续。
 */
export function RobotWaitBanner() {
  const countdown = useAppStore((s) => s.robotWaitCountdown);
  const [remainMs, setRemainMs] = useState(0);

  useEffect(() => {
    if (!countdown) {
      setRemainMs(0);
      return;
    }
    // 计算初始剩余时间
    const elapsed = Date.now() - countdown.startedAt;
    const initial = Math.max(0, countdown.countdownMs - elapsed);
    setRemainMs(initial);

    const timer = setInterval(() => {
      const nowElapsed = Date.now() - countdown.startedAt;
      const left = Math.max(0, countdown.countdownMs - nowElapsed);
      setRemainMs(left);
      if (left <= 0) clearInterval(timer);
    }, 200);

    return () => clearInterval(timer);
  }, [countdown]);

  // 用户手动点击"跳过等待"，通过 reject 让后端走自动继续分支
  const handleSkip = useCallback(async () => {
    if (!countdown) return;
    try {
      await rejectApproval(countdown.callId, -1, "user_skipped");
    } catch {
      // 忽略——后端可能已经超时
    }
    useAppStore.getState().setRobotWaitCountdown(null);
  }, [countdown]);

  if (!countdown || remainMs <= 0) return null;

  const seconds = Math.ceil(remainMs / 1000);

  return (
    <div className="flex items-center gap-2 px-4 py-2 border-t border-[var(--chat-line)] bg-[var(--chat-bg-secondary)]">
      <span className="text-xs text-[var(--chat-muted)]">
        AI 正在等待你补充信息，{seconds} 秒后将自动继续…
      </span>
      <button
        onClick={handleSkip}
        className="flex items-center gap-1 px-2 py-0.5 text-xs rounded
                   bg-[var(--accent)] text-white hover:opacity-80 transition-opacity"
      >
        <IconPlayerPlay size={12} stroke={2} />
        跳过等待
      </button>
    </div>
  );
}
