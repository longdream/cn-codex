import {
  IconAlertTriangle,
  IconArrowLeft,
  IconEye,
  IconFolderOpen,
  IconLoader2,
  IconMessage2,
  IconPencil,
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
  replayRenameScript,
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

function shouldAutoFix(result: ReplayRunResult): boolean {
  if (result.ok) {
    return false;
  }
  // 用户主动停止（fixable=false）不自动修复；其余失败（含无输出）都反馈主链路分析修复。
  return result.fixable !== false;
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
    `- navigate：页面跳转。必须看 cause 字段，禁止把自动跳转写成顺序 page.goto：`,
    `  - cause=user：用户在地址栏输入/收藏夹打开 → 仅此时使用 page.goto(url)`,
    `  - cause=redirect：HTTP 302 / JS / meta / SSO 自动跳转 → 禁止 page.goto，应 wait_for_url 或等待目标页元素出现`,
    `  - cause=link / form：由前面的 click/submit 触发 → 禁止再 goto，等待导航完成即可`,
    `  - cause=reload → page.reload()`,
    `- click：鼠标点击 → element.click()`,
    `- type：输入框内容变化。value 是变化后的框内容，previousValue 是变化前，inputType 区分插入/删除：`,
    `  - insertText / insertFromPaste / insertCompositionText / compositionend → 输入（KEYIN）`,
    `  - deleteContentBackward / deleteContentForward / deleteByCut / deleteContent → 删除，不是新的 KEYIN`,
    `  - 同一输入框的连续 type/key 是一次编辑过程，合并为最终稳定 value 再 fill，不要对 ju、junlong 等中间值依次 fill`,
    `- hover：鼠标悬停 → page.hover(selector)，点击下拉菜单项前先 hover 触发菜单`,
    `- key：键盘按键细节（Enter/Tab/Escape/Backspace/Delete/方向键/Ctrl 快捷键）→ page.press(selector, key)`,
    `  - Backspace/Delete 是删除，不要当成输入；Ctrl+A / Ctrl+V 等按 modifiers 拼成 "Control+A"`,
    `- select：下拉选择 → page.select_option(selector, value)`,
    `- submit：表单提交 → 通常可忽略，或点击提交按钮`,
    ``,
    `脚本要求（务必遵守）：`,
    `1. 使用 Playwright sync API，脚本自包含、可独立运行；`,
    `2. 每个步骤都要有中文注释，说明「步骤 N：做什么」；`,
    `3. 运行时打印每个步骤的提示，例如 print("步骤 N：...")；`,
    `4. 容错：等待选择器、多候选选择器回退、每步重试、导航后等待页面稳定；不要把空 selector 传给 Playwright，也不要用 page.click("")；`,
    `5. 每个输入框只 fill 最终稳定值：先 click/focus 再 fill；即使 trace 没有对应 click，也必须根据 selectorCandidates 补一个 click；中间 KEYIN/删除/退格全部合并，禁止对每个 type 依次 fill；`,
    `6. click 事件 selector 为空时，优先使用 selectorCandidates、tagName 和相邻事件推断可点击祖先；若仍无法定位则跳过该事件并记录提示，不得生成无目标点击；`,
    `7. 仅对 cause=user 的 HTTP(S) URL 使用 page.goto；SSO/登录自动跳转（cause=redirect/form/link）只用宽松 host/path 片段等待；navigate 紧跟 click/submit 时即使 cause 缺失也不要再 goto；不要断言带 code/state 的完整回调 URL；`,
    `8. 断言：关键操作后校验元素可见或 URL 变化；`,
    `9. 成功时必须在关闭浏览器之前打印一行 REPLAY_RESULT JSON（ok: true, step, url, title）；失败时同样打印（ok: false, error）并以非 0 退出。browser.close()/context.close() 必须包在 try/except 中：用户手动关闭浏览器窗口不得当作回放失败。`,
    `10. 写入完成后必须重新读取脚本并做 Python 语法自检（至少确认 try/for/函数缩进完整；环境可用时运行 py_compile）；发现 SyntaxError 或 IndentationError 必须先修复并覆盖写入。`,
    ``,
    `写完脚本后请自行测试并修复（不要只写完就结束）：`,
    `1. 用 recording_control 工具的 run_replay 动作运行脚本（script_id 传 ${sessionId}），拿到返回的 ok/error/stderr/stdout；`,
    `2. 若 ok=false，根据 error/stderr/stdout 定位失败步骤，用 apply_patch 修改脚本后再次 run_replay；`,
    `3. 反复「运行 → 修复 → 再运行」，直到 ok=true，或已连续修复 5 次仍未通过；`,
    `4. 最终汇报：是否通过、修复了哪些问题；若仍未通过，说明根因和剩余问题。`,
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
  const [automatingId, setAutomatingId] = useState<string | null>(null);
  const [renamingId, setRenamingId] = useState<string | null>(null);
  const [renameDraft, setRenameDraft] = useState("");
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
  // 步骤列表展开：点击「步骤数」才显示，避免 hover 时展开遮挡操作按钮
  const [expandedStepsId, setExpandedStepsId] = useState<string | null>(null);

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
      setAutomatingId(sessionId);
      const script: ReplayScriptMeta = {
        id: sessionId,
        name: sessionName || sessionId,
        path: "",
        traceSessionId: sessionId,
        createdAt: Date.now(),
        updatedAt: Date.now(),
        stepCount: 0,
        steps: [],
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
          setAutomatingId(null);
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
      setAutomatingId(script.id);
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

        if (!shouldAutoFix(result)) {
          setError(
            result.error?.trim()
              || intl.formatMessage({ id: "replay.skipFixNoOutput" }),
          );
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
        setRunStates((prev) => ({ ...prev, [script.id]: { running: true } }));
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
          if (!shouldAutoFix(result)) {
            setError(
              result.error?.trim()
                || intl.formatMessage({ id: "replay.skipFixNoOutput" }),
            );
            await load();
            return;
          }
          setRunStates((prev) => ({ ...prev, [script.id]: { running: true } }));
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
                fixable: false,
              },
            }));
          }
          activeAutomationRef.current = null;
          cancelledAutomationTokenRef.current = null;
          setAutomatingId(null);
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

  const beginRename = useCallback((script: ReplayScriptMeta) => {
    setRenamingId(script.id);
    setRenameDraft(script.name);
  }, []);

  const cancelRename = useCallback(() => {
    setRenamingId(null);
    setRenameDraft("");
  }, []);

  const commitRename = useCallback(
    async (script: ReplayScriptMeta) => {
      const nextName = renameDraft.trim();
      if (!nextName || nextName === script.name) {
        cancelRename();
        return;
      }
      setBusyId(script.id);
      setError(null);
      try {
        const saved = await replayRenameScript(script.id, nextName);
        setScripts((prev) =>
          prev.map((item) => (item.id === script.id ? { ...item, name: saved } : item)),
        );
        setSelectedScript((prev) =>
          prev && prev.id === script.id ? { ...prev, name: saved } : prev,
        );
        cancelRename();
      } catch (err) {
        setError(typeof err === "string" ? err : (err as Error).message);
      } finally {
        setBusyId((prev) => (prev === script.id ? null : prev));
      }
    },
    [cancelRename, renameDraft],
  );

  const renderStatus = (script: ReplayScriptMeta) => {
    const runState = runStates[script.id];
    const running = automatingId === script.id || isRunning(runState);
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
    const otherBusy = Boolean(automatingId && automatingId !== script.id);
    return (
      <div className="flex flex-wrap items-center gap-1.5">
        <button
          type="button"
          disabled={running || busy || otherBusy}
          onClick={() => void runWithAutoFix(script)}
          className="app-button-secondary flex items-center gap-1 text-[11px] disabled:opacity-50"
          title={intl.formatMessage({ id: "replay.run" })}
        >
          <IconPlayerPlay size={12} stroke={1.8} />
          {intl.formatMessage({ id: "replay.run" })}
        </button>
        {running ? (
          <button
            type="button"
            onClick={() => void stopAutomation()}
            className="app-button-secondary flex items-center gap-1 text-[11px] text-red-300"
            title={intl.formatMessage({ id: "replay.stopHint" })}
          >
            <IconPlayerStop size={12} stroke={1.8} />
            {intl.formatMessage({ id: "replay.stop" })}
          </button>
        ) : null}
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
          disabled={busy || running}
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

  const automationActive = generating || automatingId !== null;

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
            {renamingId === selectedScript.id ? (
              <input
                autoFocus
                value={renameDraft}
                maxLength={80}
                onChange={(event) => setRenameDraft(event.target.value)}
                onBlur={() => void commitRename(selectedScript)}
                onKeyDown={(event) => {
                  if (event.key === "Enter") {
                    event.preventDefault();
                    void commitRename(selectedScript);
                  } else if (event.key === "Escape") {
                    event.preventDefault();
                    cancelRename();
                  }
                }}
                className="min-w-0 flex-1 rounded-[var(--radius-sm)] border border-[var(--accent-border)] bg-[var(--surface-elevated)] px-1.5 py-0.5 text-xs font-semibold text-[var(--text-strong)] outline-none"
                aria-label={intl.formatMessage({ id: "replay.rename" })}
              />
            ) : (
              <div className="flex min-w-0 flex-1 items-center gap-1">
                <span className="min-w-0 truncate text-xs font-semibold text-[var(--text-strong)]">
                  {selectedScript.name}
                </span>
                <button
                  type="button"
                  onClick={() => beginRename(selectedScript)}
                  className="flex h-5 w-5 shrink-0 items-center justify-center rounded-[var(--radius-sm)] text-[var(--text-faint)] hover:bg-[var(--surface-elevated)] hover:text-[var(--text-strong)]"
                  title={intl.formatMessage({ id: "replay.rename" })}
                >
                  <IconPencil size={11} stroke={1.8} />
                </button>
              </div>
            )}
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
                  <button
                    type="button"
                    onClick={() =>
                      setExpandedStepsId((prev) =>
                        prev === selectedScript.id ? null : selectedScript.id,
                      )
                    }
                    className="text-left text-[11px] text-[var(--text-muted)] hover:underline"
                  >
                    {intl.formatMessage(
                      { id: "replay.stepCount" },
                      { count: selectedScript.stepCount },
                    )}
                  </button>
                  {(selectedScript.steps ?? []).length > 0 &&
                  expandedStepsId === selectedScript.id ? (
                    <ol className="mt-1.5 max-h-56 space-y-1 overflow-auto rounded-md border border-[var(--border-subtle)] bg-[var(--bg-subtle)] px-2 py-1.5">
                      {(selectedScript.steps ?? []).map((step, index) => (
                        <li
                          key={`${selectedScript.id}-detail-step-${index}`}
                          className="break-words text-[11px] leading-snug text-[var(--text-base)]"
                        >
                          {step}
                        </li>
                      ))}
                    </ol>
                  ) : null}
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
              const steps = script.steps ?? [];
              const renaming = renamingId === script.id;
              return (
                <div
                  key={script.id}
                  className="group space-y-2 rounded-md border border-[var(--border-subtle)] bg-[var(--surface-main)] px-3 py-2.5"
                >
                  <div className="flex items-start justify-between gap-2">
                    <div className="min-w-0 flex-1">
                      {renaming ? (
                        <input
                          autoFocus
                          value={renameDraft}
                          maxLength={80}
                          onChange={(event) => setRenameDraft(event.target.value)}
                          onBlur={() => void commitRename(script)}
                          onKeyDown={(event) => {
                            if (event.key === "Enter") {
                              event.preventDefault();
                              void commitRename(script);
                            } else if (event.key === "Escape") {
                              event.preventDefault();
                              cancelRename();
                            }
                          }}
                          className="w-full rounded-[var(--radius-sm)] border border-[var(--accent-border)] bg-[var(--surface-elevated)] px-1.5 py-0.5 text-xs font-semibold text-[var(--text-strong)] outline-none"
                          aria-label={intl.formatMessage({ id: "replay.rename" })}
                        />
                      ) : (
                        <div className="flex min-w-0 items-center gap-1">
                          <button
                            type="button"
                            onClick={() => void openScriptDetail(script)}
                            className="min-w-0 truncate text-left text-xs font-semibold text-[var(--text-strong)] hover:underline"
                            title={intl.formatMessage({ id: "replay.viewScript" })}
                          >
                            {script.name}
                          </button>
                          <button
                            type="button"
                            disabled={busyId === script.id}
                            onClick={() => beginRename(script)}
                            className="flex h-5 w-5 shrink-0 items-center justify-center rounded-[var(--radius-sm)] text-[var(--text-faint)] hover:bg-[var(--surface-elevated)] hover:text-[var(--text-strong)] disabled:opacity-50"
                            title={intl.formatMessage({ id: "replay.rename" })}
                          >
                            <IconPencil size={11} stroke={1.8} />
                          </button>
                        </div>
                      )}
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
                  </div>
                  <div className="text-[11px] text-[var(--text-muted)]">
                    <button
                      type="button"
                      onClick={() =>
                        setExpandedStepsId((prev) =>
                          prev === script.id ? null : script.id,
                        )
                      }
                      className="text-left text-[11px] text-[var(--text-muted)] hover:underline"
                    >
                      {intl.formatMessage(
                        { id: "replay.stepCount" },
                        { count: script.stepCount },
                      )}
                    </button>
                    {steps.length > 0 && expandedStepsId === script.id ? (
                      <ol className="mt-1.5 max-h-56 space-y-1 overflow-auto rounded-md border border-[var(--border-subtle)] bg-[var(--bg-subtle)] px-2 py-1.5">
                        {steps.map((step, index) => (
                          <li
                            key={`${script.id}-step-${index}`}
                            className="break-words text-[11px] leading-snug text-[var(--text-base)]"
                          >
                            {step}
                          </li>
                        ))}
                      </ol>
                    ) : null}
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
