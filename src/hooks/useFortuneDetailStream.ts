import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { useCallback, useEffect, useState } from "react";
import type { BaziProfile } from "../stores/settingsStore";
import type {
  FortuneDetailCompletedNotification,
  FortuneDetailDeltaNotification,
  FortuneDetailErrorNotification,
  FortuneDetailStartedNotification,
} from "../types/notifications";
import {
  getCachedFortuneDetail,
  parseFortuneDetailResponse,
  setCachedFortuneDetail,
  startFortuneDetailStream,
  type FortuneDetail,
  type FortuneSummary,
} from "../utils/fortune";

export type FortuneDetailStreamStatus = "idle" | "loading" | "streaming" | "completed" | "error";

export interface FortuneDetailStreamState {
  requestId: string | null;
  status: FortuneDetailStreamStatus;
  streamedText: string;
  detail: FortuneDetail | null;
  error: string | null;
}

export const INITIAL_FORTUNE_DETAIL_STREAM_STATE: FortuneDetailStreamState = {
  requestId: null,
  status: "idle",
  streamedText: "",
  detail: null,
  error: null,
};

type FortuneDetailStreamAction =
  | { type: "request:start"; requestId: string }
  | { type: "request:failed"; message: string }
  | { type: "cache:hit"; detail: FortuneDetail }
  | { type: "event:started"; payload: FortuneDetailStartedNotification }
  | { type: "event:delta"; payload: FortuneDetailDeltaNotification }
  | { type: "event:completed"; payload: FortuneDetailCompletedNotification }
  | { type: "event:error"; payload: FortuneDetailErrorNotification }
  | { type: "reset" };

function isMatchedRequest(
  state: FortuneDetailStreamState,
  requestId: string | undefined,
): boolean {
  return Boolean(state.requestId) && state.requestId === requestId;
}

export function reduceFortuneDetailStreamState(
  state: FortuneDetailStreamState,
  action: FortuneDetailStreamAction,
): FortuneDetailStreamState {
  switch (action.type) {
    case "request:start":
      return {
        requestId: action.requestId,
        status: "loading",
        streamedText: "",
        detail: null,
        error: null,
      };
    case "request:failed":
      return {
        ...state,
        status: "error",
        error: action.message,
      };
    case "cache:hit":
      return {
        requestId: null,
        status: "completed",
        streamedText: "",
        detail: action.detail,
        error: null,
      };
    case "event:started":
      if (!isMatchedRequest(state, action.payload.requestId)) {
        return state;
      }
      return {
        ...state,
        status: "streaming",
        streamedText: "",
        detail: null,
        error: null,
      };
    case "event:delta":
      if (!isMatchedRequest(state, action.payload.requestId)) {
        return state;
      }
      return {
        ...state,
        status: "streaming",
        streamedText: `${state.streamedText}${action.payload.delta ?? ""}`,
      };
    case "event:completed":
      if (!isMatchedRequest(state, action.payload.requestId)) {
        return state;
      }
      {
        const finalText = typeof action.payload.text === "string" && action.payload.text.trim()
          ? action.payload.text
          : state.streamedText;
        const parsed = parseFortuneDetailResponse(finalText);
        if (!parsed) {
          return {
            ...state,
            status: "error",
            streamedText: finalText,
            error: "Failed to parse fortune detail response from LLM",
          };
        }
        return {
          ...state,
          status: "completed",
          streamedText: finalText,
          detail: parsed,
          error: null,
        };
      }
    case "event:error":
      if (!isMatchedRequest(state, action.payload.requestId)) {
        return state;
      }
      return {
        ...state,
        status: "error",
        error: action.payload.message || "Fortune detail stream failed",
      };
    case "reset":
      return INITIAL_FORTUNE_DETAIL_STREAM_STATE;
    default:
      return state;
  }
}

interface FortuneDetailStreamListeners {
  onStarted: (payload: FortuneDetailStartedNotification) => void;
  onDelta: (payload: FortuneDetailDeltaNotification) => void;
  onCompleted: (payload: FortuneDetailCompletedNotification) => void;
  onError: (payload: FortuneDetailErrorNotification) => void;
}

export async function registerFortuneDetailStreamListeners(
  listeners: FortuneDetailStreamListeners,
): Promise<() => void> {
  const unlistenFns: UnlistenFn[] = await Promise.all([
    listen<FortuneDetailStartedNotification>("fortune-detail-started", (event) => {
      listeners.onStarted(event.payload);
    }),
    listen<FortuneDetailDeltaNotification>("fortune-detail-delta", (event) => {
      listeners.onDelta(event.payload);
    }),
    listen<FortuneDetailCompletedNotification>("fortune-detail-completed", (event) => {
      listeners.onCompleted(event.payload);
    }),
    listen<FortuneDetailErrorNotification>("fortune-detail-error", (event) => {
      listeners.onError(event.payload);
    }),
  ]);
  return () => {
    for (const unlisten of unlistenFns) {
      unlisten();
    }
  };
}

function generateFortuneRequestId(): string {
  if (typeof crypto !== "undefined" && typeof crypto.randomUUID === "function") {
    return crypto.randomUUID();
  }
  return `fortune-${Date.now()}-${Math.random().toString(16).slice(2)}`;
}

interface UseFortuneDetailStreamOptions {
  summary: FortuneSummary | null;
  baziProfile?: BaziProfile | null;
  autoStart?: boolean;
}

interface UseFortuneDetailStreamResult extends FortuneDetailStreamState {
  start: () => Promise<void>;
  reset: () => void;
}

export function useFortuneDetailStream({
  summary,
  baziProfile,
  autoStart = true,
}: UseFortuneDetailStreamOptions): UseFortuneDetailStreamResult {
  const [state, setState] = useState<FortuneDetailStreamState>(
    INITIAL_FORTUNE_DETAIL_STREAM_STATE,
  );

  const dispatch = useCallback((action: FortuneDetailStreamAction) => {
    setState((prev) => reduceFortuneDetailStreamState(prev, action));
  }, []);

  useEffect(() => {
    let cancelled = false;
    let dispose: (() => void) | null = null;

    registerFortuneDetailStreamListeners({
      onStarted: (payload) => {
        if (!cancelled) dispatch({ type: "event:started", payload });
      },
      onDelta: (payload) => {
        if (!cancelled) dispatch({ type: "event:delta", payload });
      },
      onCompleted: (payload) => {
        if (!cancelled) dispatch({ type: "event:completed", payload });
      },
      onError: (payload) => {
        if (!cancelled) dispatch({ type: "event:error", payload });
      },
    })
      .then((cleanup) => {
        if (cancelled) {
          cleanup();
          return;
        }
        dispose = cleanup;
      })
      .catch((err) => {
        if (!cancelled) {
          dispatch({
            type: "request:failed",
            message: err instanceof Error ? err.message : String(err),
          });
        }
      });

    return () => {
      cancelled = true;
      if (dispose) {
        dispose();
      }
    };
  }, [dispatch]);

  const start = useCallback(async () => {
    if (!summary) {
      return;
    }
    const cachedDetail = await getCachedFortuneDetail(summary, baziProfile);
    if (cachedDetail) {
      dispatch({ type: "cache:hit", detail: cachedDetail });
      return;
    }

    const requestId = generateFortuneRequestId();
    dispatch({ type: "request:start", requestId });
    try {
      await startFortuneDetailStream(summary, baziProfile, { requestId });
    } catch (err) {
      dispatch({
        type: "request:failed",
        message: err instanceof Error ? err.message : String(err),
      });
    }
  }, [summary, baziProfile, dispatch]);

  useEffect(() => {
    if (
      state.status !== "completed"
      || !state.detail
      || !state.requestId
      || !summary
    ) {
      return;
    }
    void setCachedFortuneDetail(summary, state.detail, baziProfile).catch(() => {});
  }, [state.status, state.detail, state.requestId, summary, baziProfile]);

  useEffect(() => {
    if (!autoStart || !summary) {
      return;
    }
    void start();
  }, [autoStart, summary, start]);

  const reset = useCallback(() => {
    dispatch({ type: "reset" });
  }, [dispatch]);

  return {
    ...state,
    start,
    reset,
  };
}
