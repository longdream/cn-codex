import { useCallback, useEffect, useRef, useState } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import {
  launchBrowser,
  recordingStart,
  recordingStop,
  recordingStatus,
  type TraceFile,
} from "../api/recording";

export type RecordingState =
  | "hidden"
  | "ready"
  | "recording"
  | "processing"
  | "completed";

interface UseRecordingReturn {
  state: RecordingState;
  elapsedSeconds: number;
  lastTrace: TraceFile | null;
  error: string | null;
  handleStart: () => Promise<void>;
  handleStop: () => Promise<void>;
  handleLaunchBrowser: () => Promise<void>;
  handleDismiss: () => void;
}

export function useRecording(): UseRecordingReturn {
  const [state, setState] = useState<RecordingState>("hidden");
  const [elapsedSeconds, setElapsedSeconds] = useState(0);
  const [lastTrace, setLastTrace] = useState<TraceFile | null>(null);
  const [error, setError] = useState<string | null>(null);
  const timerRef = useRef<ReturnType<typeof setInterval> | null>(null);

  const startTimer = useCallback(() => {
    setElapsedSeconds(0);
    timerRef.current = setInterval(() => {
      setElapsedSeconds((prev) => prev + 1);
    }, 1000);
  }, []);

  const stopTimer = useCallback(() => {
    if (timerRef.current) {
      clearInterval(timerRef.current);
      timerRef.current = null;
    }
  }, []);

  useEffect(() => {
    return () => stopTimer();
  }, [stopTimer]);

  // Listen for Tauri events from agent or backend
  useEffect(() => {
    const unlisteners: UnlistenFn[] = [];

    const setup = async () => {
      unlisteners.push(
        await listen<boolean>("recording-toggle-visibility", (e) => {
          if (e.payload) {
            setState((prev) => (prev === "hidden" ? "ready" : prev));
          } else {
            setState("hidden");
            stopTimer();
          }
        }),
      );

      unlisteners.push(
        await listen<TraceFile>("recording-completed", (e) => {
          setLastTrace(e.payload);
          setState("completed");
          stopTimer();
          // 录制完成时后端已自动生成回放脚本，通知回放面板刷新。
          window.dispatchEvent(new CustomEvent("cn-codex:replay-updated"));
        }),
      );
    };

    setup();
    return () => {
      for (const unlisten of unlisteners) {
        unlisten();
      }
    };
  }, [stopTimer]);

  const handleLaunchBrowser = useCallback(async () => {
    setError(null);
    try {
      await launchBrowser();
      setState("ready");
    } catch (err) {
      setError(String(err));
    }
  }, []);

  const handleStart = useCallback(async () => {
    setError(null);
    try {
      // Check if browser is running; if not, launch it
      const status = await recordingStatus();
      if (!status.browserRunning) {
        await launchBrowser();
      }

      await recordingStart();
      setState("recording");
      startTimer();
    } catch (err) {
      setError(String(err));
    }
  }, [startTimer]);

  const handleStop = useCallback(async () => {
    setError(null);
    setState("processing");
    stopTimer();
    try {
      const trace = await recordingStop();
      setLastTrace(trace);
      setState("completed");
    } catch (err) {
      setError(String(err));
      setState("ready");
    }
  }, [stopTimer]);

  const handleDismiss = useCallback(() => {
    setState("hidden");
    setLastTrace(null);
    setError(null);
    stopTimer();
  }, [stopTimer]);

  return {
    state,
    elapsedSeconds,
    lastTrace,
    error,
    handleStart,
    handleStop,
    handleLaunchBrowser,
    handleDismiss,
  };
}
