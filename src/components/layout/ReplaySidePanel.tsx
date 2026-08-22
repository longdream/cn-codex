import {
  IconAlertTriangle,
  IconFolderOpen,
  IconLoader2,
  IconPlayerPlay,
  IconRefresh,
} from "@tabler/icons-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { useIntl } from "react-intl";
import {
  replayGenerateScript,
  replayGetDir,
  replayListScripts,
  replayRunScript,
  type ReplayRunResult,
  type ReplayScriptMeta,
} from "../../api/replay";
import { standaloneChat, standaloneThreadCreate } from "../../api/standalone";
import { revealInExplorer } from "../../api/window";
import { useAppStore } from "../../stores/appStore";

/** 自动修复重试上限：主链路最多修复 5 次后总结失败原因。 */
const MAX_FIX_ATTEMPTS = 5;

type RunState = { running: true } | ReplayRunResult;

function isRunning(state: RunState | undefined): state is { running: true } {
  return Boolean(state && "running" in state);
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
    `1. 优先用 read_file 读取脚本，用 apply_patch 修改脚本（保留完善的选择器回退、等待与断言容错）。`,
    `2. 如需了解页面当前状态，可用 browser_run 打开相关 URL 查看，或检查输出中的 url/screenshot。`,
    `3. 修复完成后我会立即重新运行该脚本验证，无需你执行。`,
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
  const runningRef = useRef(false);

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const list = await replayListScripts();
      setScripts(list);
    } catch (err) {
      setError(typeof err === "string" ? err : (err as Error).message);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void load();
    const handleUpdated = () => void load();
    window.addEventListener("cn-codex:replay-updated", handleUpdated);
    return () => {
      window.removeEventListener("cn-codex:replay-updated", handleUpdated);
    };
  }, [load]);

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

  const runWithAutoFix = useCallback(
    async (script: ReplayScriptMeta) => {
      if (runningRef.current) {
        return;
      }
      runningRef.current = true;
      setBusyId(script.id);
      setRunStates((prev) => ({ ...prev, [script.id]: { running: true } }));

      try {
        let result = await replayRunScript(script.id);
        setRunStates((prev) => ({ ...prev, [script.id]: result }));

        if (result.ok) {
          await load();
          return;
        }

        // 失败后接入主链路：主链路检查错误 + 页面，修复脚本，前端重新运行。
        const threadId = await ensureReplayThread(script);
        for (let attempt = 1; attempt <= MAX_FIX_ATTEMPTS; attempt += 1) {
          await sendToPipeline(threadId, buildFixMessage(script, result, attempt, MAX_FIX_ATTEMPTS));
          result = await replayRunScript(script.id);
          setRunStates((prev) => ({ ...prev, [script.id]: result }));
          if (result.ok) {
            await sendToPipeline(threadId, buildSuccessMessage(script, attempt));
            await load();
            return;
          }
        }

        // 达到上限：让主链路总结失败原因。
        await sendToPipeline(threadId, buildExhaustedMessage(script, result, MAX_FIX_ATTEMPTS));
        await load();
      } catch (err) {
        setError(typeof err === "string" ? err : (err as Error).message);
      } finally {
        runningRef.current = false;
        setBusyId(null);
      }
    },
    [ensureReplayThread, load, sendToPipeline],
  );

  const openScriptsDir = useCallback(async () => {
    const dir = await replayGetDir();
    await revealInExplorer(dir);
  }, []);

  const regenerate = useCallback(
    async (script: ReplayScriptMeta) => {
      setBusyId(script.id);
      setError(null);
      try {
        await replayGenerateScript(script.traceSessionId);
        await load();
      } catch (err) {
        setError(typeof err === "string" ? err : (err as Error).message);
      } finally {
        setBusyId(null);
      }
    },
    [load],
  );

  return (
    <div className="flex min-h-0 flex-1 flex-col overflow-hidden">
      <div className="flex items-center gap-2 border-b border-[var(--border-subtle)] px-3 py-2">
        <span className="min-w-0 flex-1 truncate text-xs font-semibold text-[var(--text-strong)]">
          {intl.formatMessage({ id: "rightPanel.replay" })}
        </span>
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
      </div>

      {error && (
        <div className="mx-3 mt-2 rounded-md border border-red-500/25 bg-red-500/10 px-2.5 py-2 text-[11px] text-red-300">
          {error}
        </div>
      )}

      <div className="min-h-0 flex-1 space-y-2 overflow-y-auto p-3">
        {loading && scripts.length === 0 ? (
          <div className="text-xs text-[var(--text-faint)]">
            {intl.formatMessage({ id: "replay.loading" })}
          </div>
        ) : scripts.length === 0 ? (
          <div className="space-y-2 rounded-md border border-[var(--border-subtle)] bg-[var(--bg-subtle)] px-3 py-4 text-xs text-[var(--text-faint)]">
            <div>{intl.formatMessage({ id: "replay.empty" })}</div>
            <div>{intl.formatMessage({ id: "replay.emptyHint" })}</div>
          </div>
        ) : (
          scripts.map((script) => {
            const busy = busyId === script.id;
            const runState = runStates[script.id];
            const running = isRunning(runState);
            const lastResult = runState && !isRunning(runState) ? runState : null;
            const status = lastResult
              ? lastResult.ok
                ? "success"
                : "failed"
              : script.lastStatus ?? null;
            const statusError = lastResult?.error ?? script.lastError ?? null;
            return (
              <div
                key={script.id}
                className="space-y-2 rounded-md border border-[var(--border-subtle)] bg-[var(--surface-main)] px-3 py-2.5"
              >
                <div className="flex items-start justify-between gap-2">
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
                </div>
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
                <div className="flex flex-wrap items-center gap-1.5">
                  <button
                    type="button"
                    disabled={busy}
                    onClick={() => void runWithAutoFix(script)}
                    className="app-button-secondary flex items-center gap-1 text-[11px] disabled:opacity-50"
                  >
                    {running ? (
                      <IconLoader2 size={12} stroke={1.8} className="animate-spin" />
                    ) : (
                      <IconPlayerPlay size={12} stroke={1.8} />
                    )}
                    {intl.formatMessage({ id: "replay.run" })}
                  </button>
                  <button
                    type="button"
                    disabled={busy}
                    onClick={() => void regenerate(script)}
                    className="app-button-secondary flex items-center gap-1 text-[11px] disabled:opacity-50"
                    title={intl.formatMessage({ id: "replay.regenerateHint" })}
                  >
                    <IconRefresh size={12} stroke={1.8} />
                    {intl.formatMessage({ id: "replay.regenerate" })}
                  </button>
                </div>
              </div>
            );
          })
        )}
      </div>
    </div>
  );
}
