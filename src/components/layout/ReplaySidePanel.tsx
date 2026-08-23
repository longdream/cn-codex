import {
  IconAlertTriangle,
  IconArrowLeft,
  IconEye,
  IconFolderOpen,
  IconLoader2,
  IconMessage2,
  IconPlayerPlay,
  IconPlayerRecord,
  IconPlayerStop,
  IconRefresh,
  IconTrash,
} from "@tabler/icons-react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { useCallback, useEffect, useRef, useState } from "react";
import { useIntl } from "react-intl";
import {
  launchBrowser,
  recordingStart,
  recordingStatus,
  recordingStop,
  type TraceFile,
} from "../../api/recording";
import {
  replayDeleteScript,
  replayGetDir,
  replayListScripts,
  replayReadScript,
  replayRunScript,
  replayStopScript,
  type ReplayRunResult,
  type ReplayScriptMeta,
} from "../../api/replay";
import {
  standaloneChat,
  standaloneThreadCreate,
  standaloneTurnInterrupt,
} from "../../api/standalone";
import { revealInExplorer } from "../../api/window";
import { useAppStore } from "../../stores/appStore";

/** 自动修复重试上限：主链路最多修复 5 次后总结失败原因。 */
const MAX_FIX_ATTEMPTS = 5;

type RunState = { running: true } | ReplayRunResult;

type ActiveAutomation = {
  token: number;
  scriptId: string;
  threadId: string | null;
};

function isRunning(state: RunState | undefined): state is { running: true } {
  return Boolean(state && "running" in state);
}

function formatRecordTime(seconds: number): string {
  const m = Math.floor(seconds / 60);
  const s = seconds % 60;
  return `${m.toString().padStart(2, "0")}:${s.toString().padStart(2, "0")}`;
}

function buildGenerateMessage(sessionId: string, sessionName: string): string {
  return [
    `【生成回放脚本】`,
    `请根据录制记录生成一个 Playwright Python 回放脚本。`,
    `- 录制文件：../${sessionId}.trace.json（用 read_file 读取）`,
    `- 输出脚本：${sessionId}.py（当前目录，用 write_file 创建；若已存在请传 overwrite=true 覆盖）`,
    `- 录制名称：${sessionName}`,
    ``,
    `录制事件类型说明（据此生成对应动作）：`,
    `- navigate：页面跳转 → page.goto(url)`,
    `- click：鼠标点击 → element.click()`,
    `- type：文本输入（含用户名/密码，value 即输入内容）→ 先 click/focus 同一输入框，再 page.fill(selector, value)`,
    `- hover：鼠标悬停 → page.hover(selector)，点击下拉菜单项前先 hover 触发菜单`,
    `- key：键盘按键（如 Enter 搜索/提交）→ page.press(selector, "Enter")`,
    `- select：下拉选择 → page.select_option(selector, value)`,
    `- submit：表单提交 → 通常可忽略，或点击提交按钮`,
    ``,
    `脚本要求（务必遵守）：`,
    `1. 使用 Playwright sync API，脚本自包含、可独立运行；`,
    `2. 每个步骤都要有中文注释，说明「步骤 N：做什么」；`,
    `3. 运行时打印每个步骤的提示，例如 print("步骤 N：...")；`,
    `4. 容错：等待选择器、多候选选择器回退、每步重试、导航后等待页面稳定；不要把空 selector 传给 Playwright，也不要用 page.click("")；`,
    `5. 对每个 type 事件必须先点击/聚焦输入框再 fill；即使 trace 没有对应 click，也必须根据 selectorCandidates 或后续 type 事件补一个 click；连续 type 事件合并为一次最终值；`,
    `6. click 事件 selector 为空时，优先使用 selectorCandidates、tagName 和相邻事件推断可点击祖先；若仍无法定位则跳过该事件并记录提示，不得生成无目标点击；`,
    `7. 仅对 trace 中真实的 HTTP(S) URL 使用 page.goto；登录/SSO 跳转不要断言脆弱的完整 URL，使用宽松的 host/path 片段和最终页面元素校验；`,
    `8. 断言：关键操作后校验元素可见或 URL 变化；`,
    `9. 失败时打印一行 REPLAY_RESULT JSON（含 step/url/title/error）并以非 0 退出码结束。`,
    `10. 写入完成后必须重新读取脚本并做 Python 语法自检（至少确认 try/for/函数缩进完整；环境可用时运行 py_compile）；发现 SyntaxError 或 IndentationError 必须先修复并覆盖写入。`,
  ].join("\n");
}

function buildFixMessage(
  script: ReplayScriptMeta,
  result: ReplayRunResult,
  attempt: number,
  max: number,
): string {
  const output = [result.error, result.stderr, result.stdout]
    .filter((s) => s && s.trim().length > 0)
    .join("\n")
    .slice(0, 6000);
  return [
    `【回放自动修复】第 ${attempt}/${max} 次`,
    `回放脚本执行失败，请根据错误信息和当前打开的页面检查问题并修复脚本。`,
    `- 脚本：${script.name}`,
    `- 路径：${script.path}`,
    `- 起始页：${script.startUrl}`,
    ``,
    `执行输出/错误：`,
    `\`\`\``,
    output || "(无输出)",
    `\`\`\``,
    ``,
    `要求：`,
    `1. 优先用 read_file 读取脚本，用 apply_patch 修改脚本（保留完善的选择器回退、等待与断言容错）。检查所有 type 前是否有 click/focus，不能只修复报错行。`,
    `2. 如需了解页面当前状态，可用 browser_run 打开相关 URL 查看，或检查输出中的 url/screenshot。`,
    `3. 修复完成后我会立即重新运行该脚本验证，无需你执行。`,
    `4. 如果错误是 SyntaxError 或 IndentationError，先检查并修复整个脚本的缩进和代码块，再用 py_compile（或等价方式）确认语法有效。`,
  ].join("\n");
}

function buildSuccessMessage(script: ReplayScriptMeta, attempts: number): string {
  return [
    `【回放自动修复】成功`,
    `脚本「${script.name}」经过 ${attempts} 次修复后已执行成功，无报错。`,
  ].join("\n");
}

function buildExhaustedMessage(
  script: ReplayScriptMeta,
  result: ReplayRunResult,
  max: number,
): string {
  const output = [result.error, result.stderr, result.stdout]
    .filter((s) => s && s.trim().length > 0)
    .join("\n")
    .slice(0, 6000);
  return [
    `【回放自动修复】已达重试上限 ${max} 次，脚本仍执行失败。`,
    `请总结失败根因，并说明已尝试的修复步骤和仍然失败的原因。`,
    `- 脚本：${script.name}`,
    `- 路径：${script.path}`,
    ``,
    `最后一次执行输出/错误：`,
    `\`\`\``,
    output || "(无输出)",
    `\`\`\``,
  ].join("\n");
}

export function ReplaySidePanel() {
  const intl = useIntl();
  const [scripts, setScripts] = useState<ReplayScriptMeta[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [busyId, setBusyId] = useState<string | null>(null);
  const [runStates, setRunStates] = useState<Record<string, RunState>>({});
  const nextAutomationTokenRef = useRef(0);
  const activeAutomationRef = useRef<ActiveAutomation | null>(null);
  const cancelledAutomationTokenRef = useRef<number | null>(null);

  // 录制状态（右侧面板内置录制按钮，无需通过对话启动）
  const [recording, setRecording] = useState(false);
  const [recordElapsed, setRecordElapsed] = useState(0);
  const [recordError, setRecordError] = useState<string | null>(null);
  const [generating, setGenerating] = useState(false);
  const timerRef = useRef<ReturnType<typeof setInterval> | null>(null);

  // 脚本详情查看
  const [selectedScript, setSelectedScript] = useState<ReplayScriptMeta | null>(null);
  const [scriptContent, setScriptContent] = useState<string | null>(null);
  const [scriptContentLoading, setScriptContentLoading] = useState(false);

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const list = await replayListScripts();
      setScripts(list);
      setSelectedScript((prev) =>
        prev ? list.find((s) => s.id === prev.id) ?? prev : prev,
      );
    } catch (err) {
      setError(typeof err === "string" ? err : (err as Error).message);
    } finally {
      setLoading(false);
    }
  }, []);

  const stopTimer = useCallback(() => {
    if (timerRef.current) {
      clearInterval(timerRef.current);
      timerRef.current = null;
    }
  }, []);

  const startTimer = useCallback(() => {
    stopTimer();
    setRecordElapsed(0);
    timerRef.current = setInterval(() => {
      setRecordElapsed((prev) => prev + 1);
    }, 1000);
  }, [stopTimer]);

  const ensureReplayThread = useCallback(
    async (script: ReplayScriptMeta): Promise<string> => {
      const store = useAppStore.getState();
      const existingThreadId = Object.entries(store.threadPreferences).find(
        ([, pref]) => pref?.replayScriptId === script.id,
      )?.[0];
      if (existingThreadId && store.threads.some((t) => t.id === existingThreadId)) {
        await store.loadThread(existingThreadId);
        return existingThreadId;
      }

      const rootPath = await replayGetDir();
      const createResp = await standaloneThreadCreate();
      const threadId = createResp?.thread?.id;
      if (!threadId) {
        throw new Error(intl.formatMessage({ id: "replay.err.threadFailed" }));
      }

      store.bindThreadReplay(threadId, {
        scriptId: script.id,
        scriptName: script.name,
        rootPath,
      });
      store.setCurrentThread(threadId);
      store.setThreadSmartbrainEnabled(true);
      store.addThread({
        id: threadId,
        preview: intl.formatMessage({ id: "replay.joinPreview" }, { name: script.name }),
        updatedAt: Date.now(),
      });
      store.setShowSettings(false);
      return threadId;
    },
    [intl],
  );

  const sendToPipeline = useCallback(async (threadId: string, text: string) => {
    const store = useAppStore.getState();
    if (store.currentThreadId !== threadId) {
      await store.loadThread(threadId);
    }
    const cwd = store.resolveThreadCwd(threadId) || undefined;
    const clientMessageId = crypto.randomUUID();
    store.addMessage({
      id: clientMessageId,
      role: "user",
      content: text,
      timestamp: Date.now(),
    });
    await standaloneChat(
      threadId,
      text,
      cwd,
      "chat",
      [],
      undefined,
      undefined,
      {
        provider: store.buildEffectiveChatProviderOverride(null, null),
        smartbrainEnabled: store.threadPreferences[threadId]?.smartbrainEnabled ?? false,
        subagentEnabled: store.threadPreferences[threadId]?.subagentEnabled ?? false,
      },
      clientMessageId,
    );
  }, []);

  const generateViaAgent = useCallback(
    async (sessionId: string, sessionName: string) => {
      if (activeAutomationRef.current) {
        return;
      }
      const token = nextAutomationTokenRef.current + 1;
      nextAutomationTokenRef.current = token;
      activeAutomationRef.current = {
        token,
        scriptId: sessionId,
        threadId: null,
      };
      cancelledAutomationTokenRef.current = null;
      const script: ReplayScriptMeta = {
        id: sessionId,
        name: sessionName || sessionId,
        path: "",
        traceSessionId: sessionId,
        createdAt: Date.now(),
        updatedAt: Date.now(),
        stepCount: 0,
        startUrl: "",
      };
      setGenerating(true);
      setError(null);
      try {
        const threadId = await ensureReplayThread(script);
        if (
          cancelledAutomationTokenRef.current === token ||
          activeAutomationRef.current?.token !== token
        ) {
          return;
        }
        const activeGeneration = activeAutomationRef.current;
        if (!activeGeneration) {
          return;
        }
        activeGeneration.threadId = threadId;
        await sendToPipeline(threadId, buildGenerateMessage(sessionId, sessionName));
        if (cancelledAutomationTokenRef.current === token) {
          return;
        }
        await load();
      } catch (err) {
        if (cancelledAutomationTokenRef.current !== token) {
          setError(typeof err === "string" ? err : (err as Error).message);
        }
      } finally {
        if (activeAutomationRef.current?.token === token) {
          activeAutomationRef.current = null;
          cancelledAutomationTokenRef.current = null;
          setGenerating(false);
        }
      }
    },
    [ensureReplayThread, load, sendToPipeline],
  );

  useEffect(() => {
    void load();
    const handleUpdated = () => void load();
    window.addEventListener("cn-codex:replay-updated", handleUpdated);

    const unlisteners: UnlistenFn[] = [];
    listen<TraceFile>("recording-completed", (e) => {
      setRecording(false);
      stopTimer();
      // 录制完成后由主链路 AI 生成回放脚本（中文注释 + 步骤提示）。
      void generateViaAgent(e.payload.sessionId, e.payload.sessionName);
      void load();
    }).then((fn) => unlisteners.push(fn));
    listen<string>("recording-started", () => {
      setRecording(true);
      startTimer();
    }).then((fn) => unlisteners.push(fn));

    return () => {
      window.removeEventListener("cn-codex:replay-updated", handleUpdated);
      for (const unlisten of unlisteners) {
        unlisten();
      }
    };
  }, [load, startTimer, stopTimer, generateViaAgent]);

  const startRecording = useCallback(async () => {
    setRecordError(null);
    try {
      const status = await recordingStatus();
      if (!status.browserRunning) {
        await launchBrowser();
      }
      await recordingStart();
      setRecording(true);
      startTimer();
    } catch (err) {
      setRecordError(String(err));
    }
  }, [startTimer]);

  const stopRecording = useCallback(async () => {
    setRecordError(null);
    stopTimer();
    try {
      await recordingStop();
      setRecording(false);
      void load();
    } catch (err) {
      setRecordError(String(err));
      setRecording(false);
    }
  }, [load, stopTimer]);

  const stopAutomation = useCallback(async () => {
    const active = activeAutomationRef.current;
    if (!active) {
      return;
    }

    cancelledAutomationTokenRef.current = active.token;
    setError(null);

    const stopTasks: Promise<unknown>[] = [replayStopScript(active.scriptId)];
    if (active.threadId) {
      stopTasks.push(standaloneTurnInterrupt(active.threadId));
    }
    await Promise.allSettled(stopTasks);
  }, []);

  const runWithAutoFix = useCallback(
    async (script: ReplayScriptMeta) => {
      if (activeAutomationRef.current) {
        return;
      }
      const token = nextAutomationTokenRef.current + 1;
      nextAutomationTokenRef.current = token;
      activeAutomationRef.current = {
        token,
        scriptId: script.id,
        threadId: null,
      };
      cancelledAutomationTokenRef.current = null;
      setBusyId(script.id);
      setRunStates((prev) => ({ ...prev, [script.id]: { running: true } }));

      try {
        let result = await replayRunScript(script.id);
        if (
          cancelledAutomationTokenRef.current === token ||
          activeAutomationRef.current?.token !== token
        ) {
          return;
        }
        setRunStates((prev) => ({ ...prev, [script.id]: result }));

        if (result.ok) {
          await load();
          return;
        }

        const threadId = await ensureReplayThread(script);
        if (
          cancelledAutomationTokenRef.current === token ||
          activeAutomationRef.current?.token !== token
        ) {
          return;
        }
        const activeRun = activeAutomationRef.current;
        if (!activeRun) {
          return;
        }
        activeRun.threadId = threadId;
        for (let attempt = 1; attempt <= MAX_FIX_ATTEMPTS; attempt += 1) {
          if (cancelledAutomationTokenRef.current === token) {
            return;
          }
          await sendToPipeline(threadId, buildFixMessage(script, result, attempt, MAX_FIX_ATTEMPTS));
          if (cancelledAutomationTokenRef.current === token) {
            return;
          }
          result = await replayRunScript(script.id);
          if (cancelledAutomationTokenRef.current === token) {
            return;
          }
          setRunStates((prev) => ({ ...prev, [script.id]: result }));
          if (result.ok) {
            if (cancelledAutomationTokenRef.current === token) {
              return;
            }
            await sendToPipeline(threadId, buildSuccessMessage(script, attempt));
            if (cancelledAutomationTokenRef.current === token) {
              return;
            }
            await load();
            return;
          }
        }

        if (cancelledAutomationTokenRef.current === token) {
          return;
        }
        await sendToPipeline(threadId, buildExhaustedMessage(script, result, MAX_FIX_ATTEMPTS));
        await load();
      } catch (err) {
        if (cancelledAutomationTokenRef.current !== token) {
          setError(typeof err === "string" ? err : (err as Error).message);
        }
      } finally {
        if (activeAutomationRef.current?.token === token) {
          if (cancelledAutomationTokenRef.current === token) {
            setRunStates((prev) => ({
              ...prev,
              [script.id]: {
                ok: false,
                exitCode: null,
                stdout: "",
                stderr: "",
                durationMs: 0,
                error: intl.formatMessage({ id: "replay.stoppedByUser" }),
              },
            }));
          }
          activeAutomationRef.current = null;
          cancelledAutomationTokenRef.current = null;
          setBusyId(null);
        }
      }
    },
    [ensureReplayThread, intl, load, sendToPipeline],
  );

  const openScriptsDir = useCallback(async () => {
    const dir = await replayGetDir();
    await revealInExplorer(dir);
  }, []);

  const loadScriptContent = useCallback(
    async (script: ReplayScriptMeta) => {
      setScriptContentLoading(true);
      try {
        const res = await replayReadScript(script.id);
        setScriptContent(res.content);
      } catch (err) {
        setScriptContent(
          `${intl.formatMessage({ id: "replay.readFailed" })}: ${
            typeof err === "string" ? err : (err as Error).message
          }`,
        );
      } finally {
        setScriptContentLoading(false);
      }
    },
    [intl],
  );

  const openScriptDetail = useCallback(
    async (script: ReplayScriptMeta) => {
      setSelectedScript(script);
      setScriptContent(null);
      await loadScriptContent(script);
    },
    [loadScriptContent],
  );

  const backToList = useCallback(() => {
    setSelectedScript(null);
    setScriptContent(null);
  }, []);

  const openConversation = useCallback(
    async (script: ReplayScriptMeta) => {
      setBusyId(script.id);
      setError(null);
      try {
        await ensureReplayThread(script);
        useAppStore.getState().setShowSettings(false);
      } catch (err) {
        setError(typeof err === "string" ? err : (err as Error).message);
      } finally {
        setBusyId(null);
      }
    },
    [ensureReplayThread],
  );

  const deleteScript = useCallback(
    async (script: ReplayScriptMeta) => {
      const ok = window.confirm(
        intl.formatMessage({ id: "replay.deleteConfirm" }, { name: script.name }),
      );
      if (!ok) return;
      setBusyId(script.id);
      setError(null);
      try {
        await replayDeleteScript(script.id);
        if (selectedScript?.id === script.id) {
          setSelectedScript(null);
          setScriptContent(null);
        }
        await load();
      } catch (err) {
        setError(typeof err === "string" ? err : (err as Error).message);
      } finally {
        setBusyId(null);
      }
    },
    [intl, load, selectedScript?.id],
  );

  const renderStatus = (script: ReplayScriptMeta) => {
    const runState = runStates[script.id];
    const running = isRunning(runState);
    const lastResult = runState && !isRunning(runState) ? runState : null;
    const status = lastResult
      ? lastResult.ok
        ? "success"
        : "failed"
      : script.lastStatus ?? null;
    const statusError = lastResult?.error ?? script.lastError ?? null;
    return { running, status, statusError };
  };

  const renderScriptActions = (script: ReplayScriptMeta, compact = false) => {
    const busy = busyId === script.id;
    const { running } = renderStatus(script);
    return (
      <div className="flex flex-wrap items-center gap-1.5">
        <button
          type="button"
          disabled={!running && busy}
          onClick={() => void (running ? stopAutomation() : runWithAutoFix(script))}
          className={`app-button-secondary flex items-center gap-1 text-[11px] disabled:opacity-50 ${
            running ? "text-red-300" : ""
          }`}
          title={
            running
              ? intl.formatMessage({ id: "replay.stop" })
              : intl.formatMessage({ id: "replay.run" })
          }
        >
          {running ? (
            <IconPlayerStop size={12} stroke={1.8} />
          ) : (
            <IconPlayerPlay size={12} stroke={1.8} />
          )}
          {intl.formatMessage({ id: running ? "replay.stop" : "replay.run" })}
        </button>
        <button
          type="button"
          disabled={busy}
          onClick={() => void openConversation(script)}
          className="app-button-secondary flex items-center gap-1 text-[11px] disabled:opacity-50"
          title={intl.formatMessage({ id: "replay.conversationHint" })}
        >
          <IconMessage2 size={12} stroke={1.8} />
          {intl.formatMessage({ id: "replay.conversation" })}
        </button>
        {!compact && (
          <button
            type="button"
            disabled={busy}
            onClick={() => void openScriptDetail(script)}
            className="app-button-secondary flex items-center gap-1 text-[11px] disabled:opacity-50"
          >
            <IconEye size={12} stroke={1.8} />
            {intl.formatMessage({ id: "replay.viewScript" })}
          </button>
        )}
        <button
          type="button"
          disabled={busy}
          onClick={() => void deleteScript(script)}
          className="app-button-secondary flex items-center gap-1 text-[11px] text-red-300 disabled:opacity-50"
          title={intl.formatMessage({ id: "replay.delete" })}
        >
          <IconTrash size={12} stroke={1.8} />
          {intl.formatMessage({ id: "replay.delete" })}
        </button>
      </div>
    );
  };

  const automationActive =
    generating || Object.values(runStates).some((state) => isRunning(state));

  return (
    <div className="flex min-h-0 flex-1 flex-col overflow-hidden">
      <div className="flex items-center gap-1.5 border-b border-[var(--border-subtle)] px-3 py-2">
        {selectedScript ? (
          <>
            <button
              type="button"
              onClick={backToList}
              className="flex h-6 w-6 flex-shrink-0 items-center justify-center rounded-[var(--radius-sm)] text-[var(--text-faint)] transition-colors hover:bg-[var(--surface-elevated)] hover:text-[var(--text-strong)]"
              title={intl.formatMessage({ id: "replay.back" })}
            >
              <IconArrowLeft size={14} stroke={1.8} />
            </button>
            <span className="min-w-0 flex-1 truncate text-xs font-semibold text-[var(--text-strong)]">
              {selectedScript.name}
            </span>
            {automationActive && (
              <button
                type="button"
                onClick={() => void stopAutomation()}
                className="flex h-7 items-center gap-1.5 rounded-[var(--radius-sm)] bg-red-500/15 px-2 text-[11px] text-red-300 transition-colors hover:bg-red-500/25"
                title={intl.formatMessage({ id: "replay.stop" })}
              >
                <IconPlayerStop size={13} stroke={1.8} />
                {intl.formatMessage({ id: "replay.stop" })}
              </button>
            )}
          </>
        ) : (
          <>
            <span className="min-w-0 flex-1 truncate text-xs font-semibold text-[var(--text-strong)]">
              {intl.formatMessage({ id: "rightPanel.replay" })}
            </span>
            {automationActive ? (
              <button
                type="button"
                onClick={() => void stopAutomation()}
                className="flex h-7 items-center gap-1.5 rounded-[var(--radius-sm)] bg-red-500/15 px-2 text-[11px] text-red-300 transition-colors hover:bg-red-500/25"
                title={intl.formatMessage({ id: "replay.stop" })}
              >
                <IconPlayerStop size={13} stroke={1.8} />
                {intl.formatMessage({ id: "replay.stop" })}
              </button>
            ) : (
              <button
                type="button"
                onClick={() => void (recording ? stopRecording() : startRecording())}
                className={`flex h-7 items-center gap-1.5 rounded-[var(--radius-sm)] px-2 text-[11px] transition-colors ${
                  recording
                    ? "bg-red-500/15 text-red-300 hover:bg-red-500/25"
                    : "bg-[var(--accent-soft)] text-[var(--accent-strong)] hover:bg-[var(--surface-elevated)]"
                }`}
                title={
                  recording
                    ? intl.formatMessage({ id: "replay.stopRecord" })
                    : intl.formatMessage({ id: "replay.record" })
                }
              >
                {recording ? (
                  <IconPlayerStop size={13} stroke={1.8} />
                ) : (
                  <IconPlayerRecord size={13} stroke={1.8} />
                )}
                {recording
                  ? `${intl.formatMessage({ id: "replay.stopRecord" })} ${formatRecordTime(recordElapsed)}`
                  : intl.formatMessage({ id: "replay.record" })}
              </button>
            )}
            <button
              type="button"
              onClick={openScriptsDir}
              className="flex h-6 w-6 items-center justify-center rounded-[var(--radius-sm)] text-[var(--text-faint)] transition-colors hover:bg-[var(--surface-elevated)] hover:text-[var(--text-strong)]"
              title={intl.formatMessage({ id: "replay.openFolder" })}
            >
              <IconFolderOpen size={14} stroke={1.8} />
            </button>
            <button
              type="button"
              onClick={() => void load()}
              className="flex h-6 w-6 items-center justify-center rounded-[var(--radius-sm)] text-[var(--text-faint)] transition-colors hover:bg-[var(--surface-elevated)] hover:text-[var(--text-strong)]"
              title={intl.formatMessage({ id: "replay.refresh" })}
            >
              <IconRefresh size={14} stroke={1.8} className={loading ? "animate-spin" : undefined} />
            </button>
          </>
        )}
      </div>

      {(error || recordError) && (
        <div className="mx-3 mt-2 rounded-md border border-red-500/25 bg-red-500/10 px-2.5 py-2 text-[11px] text-red-300">
          {error || recordError}
        </div>
      )}

      {selectedScript ? (
        <div className="flex min-h-0 flex-1 flex-col overflow-hidden">
          <div className="space-y-2 border-b border-[var(--border-subtle)] px-3 py-2.5">
            <div className="flex items-start justify-between gap-2">
              <div className="min-w-0">
                <div className="truncate font-mono text-[10px] text-[var(--text-faint)]">
                  {selectedScript.id}
                </div>
                <div className="text-[11px] text-[var(--text-muted)]">
                  {intl.formatMessage(
                    { id: "replay.stepCount" },
                    { count: selectedScript.stepCount },
                  )}
                </div>
              </div>
              {(() => {
                const { running, status, statusError } = renderStatus(selectedScript);
                return (
                  <div
                    className={`shrink-0 text-[11px] ${
                      running
                        ? "text-[var(--accent)]"
                        : status === "success"
                          ? "text-green-400"
                          : status === "failed"
                            ? "text-red-400"
                            : "text-[var(--text-faint)]"
                    }`}
                  >
                    {running
                      ? intl.formatMessage({ id: "replay.status.running" })
                      : status === "success"
                        ? intl.formatMessage({ id: "replay.status.success" })
                        : status === "failed"
                          ? intl.formatMessage({ id: "replay.status.failed" })
                          : intl.formatMessage({ id: "replay.status.idle" })}
                    {statusError ? ` · ${statusError}` : ""}
                  </div>
                );
              })()}
            </div>
            {selectedScript.startUrl ? (
              <div className="truncate text-[11px] text-[var(--text-faint)]">
                {selectedScript.startUrl}
              </div>
            ) : null}
            {renderScriptActions(selectedScript, true)}
          </div>
          <div className="min-h-0 flex-1 overflow-auto p-3">
            <div className="mb-1.5 text-[11px] font-medium text-[var(--text-faint)]">
              {intl.formatMessage({ id: "replay.scriptContent" })}
            </div>
            {scriptContentLoading ? (
              <div className="text-xs text-[var(--text-faint)]">
                {intl.formatMessage({ id: "replay.loading" })}
              </div>
            ) : (
              <pre className="thin-scrollbar whitespace-pre-wrap break-words rounded-md border border-[var(--border-subtle)] bg-[var(--surface-main)] p-2.5 font-mono text-[11px] leading-relaxed text-[var(--text-base)]">
                {scriptContent ?? ""}
              </pre>
            )}
          </div>
        </div>
      ) : (
        <div className="min-h-0 flex-1 space-y-2 overflow-y-auto p-3">
          {generating && (
            <div className="flex items-center gap-2 rounded-md border border-[var(--accent-border)] bg-[var(--accent-soft)] px-2.5 py-2 text-[11px] text-[var(--accent-strong)]">
              <IconLoader2 size={12} stroke={1.8} className="animate-spin" />
              {intl.formatMessage({ id: "replay.generating" })}
            </div>
          )}
          {loading && scripts.length === 0 ? (
            <div className="text-xs text-[var(--text-faint)]">
              {intl.formatMessage({ id: "replay.loading" })}
            </div>
          ) : scripts.length === 0 && !generating ? (
            <div className="space-y-2 rounded-md border border-[var(--border-subtle)] bg-[var(--bg-subtle)] px-3 py-4 text-xs text-[var(--text-faint)]">
              <div>{intl.formatMessage({ id: "replay.empty" })}</div>
              <div>{intl.formatMessage({ id: "replay.emptyHint" })}</div>
            </div>
          ) : (
            scripts.map((script) => {
              const { running, status, statusError } = renderStatus(script);
              return (
                <div
                  key={script.id}
                  className="space-y-2 rounded-md border border-[var(--border-subtle)] bg-[var(--surface-main)] px-3 py-2.5"
                >
                  <button
                    type="button"
                    onClick={() => void openScriptDetail(script)}
                    className="flex w-full items-start justify-between gap-2 text-left"
                    title={intl.formatMessage({ id: "replay.viewScript" })}
                  >
                    <div className="min-w-0">
                      <div className="truncate text-xs font-semibold text-[var(--text-strong)]">
                        {script.name}
                      </div>
                      <div className="truncate font-mono text-[10px] text-[var(--text-faint)]">
                        {script.id}
                      </div>
                    </div>
                    <div
                      className={`shrink-0 text-[11px] ${
                        running
                          ? "text-[var(--accent)]"
                          : status === "success"
                            ? "text-green-400"
                            : status === "failed"
                              ? "text-red-400"
                              : "text-[var(--text-faint)]"
                      }`}
                    >
                      {running
                        ? intl.formatMessage({ id: "replay.status.running" })
                        : status === "success"
                          ? intl.formatMessage({ id: "replay.status.success" })
                          : status === "failed"
                            ? intl.formatMessage({ id: "replay.status.failed" })
                            : intl.formatMessage({ id: "replay.status.idle" })}
                    </div>
                  </button>
                  <div className="text-[11px] text-[var(--text-muted)]">
                    {intl.formatMessage(
                      { id: "replay.stepCount" },
                      { count: script.stepCount },
                    )}
                  </div>
                  {script.startUrl ? (
                    <div className="truncate text-[11px] text-[var(--text-faint)]">
                      {script.startUrl}
                    </div>
                  ) : null}
                  {statusError ? (
                    <div className="flex items-start gap-1 text-[11px] text-red-300">
                      <IconAlertTriangle size={12} stroke={1.8} className="mt-0.5 shrink-0" />
                      <span className="min-w-0 break-words">{statusError}</span>
                    </div>
                  ) : null}
                  {renderScriptActions(script)}
                </div>
              );
            })
          )}
        </div>
      )}
    </div>
  );
}
