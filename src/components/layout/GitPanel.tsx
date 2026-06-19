import {
  IconArrowDown,
  IconArrowUp,
  IconGitBranch,
  IconGitCherryPick,
  IconGitCommit,
  IconHistory,
  IconRefresh,
  IconRotateClockwise2,
} from "@tabler/icons-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useIntl } from "react-intl";
import {
  gitBranchList,
  gitCherryPick,
  gitCheckout,
  gitCommit,
  gitDiff,
  gitLog,
  gitPull,
  gitPush,
  gitReset,
  gitRevert,
  gitStage,
  gitStatus,
  gitUnstage,
  type GitLogEntry,
  type GitStatusEntry,
  type GitStatusResponse,
} from "../../api/git";

type GitView = "changes" | "commit" | "history";

interface GitPanelProps {
  workspaceCwd: string | null;
}

function normalizeError(error: unknown): string {
  if (typeof error === "string") {
    return error;
  }
  if (error instanceof Error) {
    return error.message;
  }
  return String(error);
}

function statusColor(status: string): string {
  switch (status) {
    case "added":
      return "text-emerald-400";
    case "deleted":
      return "text-rose-400";
    case "renamed":
      return "text-indigo-400";
    case "conflicted":
      return "text-amber-300";
    case "untracked":
      return "text-cyan-300";
    default:
      return "text-[var(--text-muted)]";
  }
}

export function GitPanel({ workspaceCwd }: GitPanelProps) {
  const intl = useIntl();
  const [view, setView] = useState<GitView>("changes");
  const [status, setStatus] = useState<GitStatusResponse | null>(null);
  const [history, setHistory] = useState<GitLogEntry[]>([]);
  const [branches, setBranches] = useState<Array<{ name: string; current: boolean; upstream?: string | null }>>([]);
  const [selectedBranch, setSelectedBranch] = useState("");
  const [syncBranch, setSyncBranch] = useState("");
  const [newBranchName, setNewBranchName] = useState("");
  const [selectedPath, setSelectedPath] = useState<string | null>(null);
  const [diffMode, setDiffMode] = useState<"working" | "staged">("working");
  const [diffText, setDiffText] = useState("");
  const [commitMessage, setCommitMessage] = useState("");
  const [resetMode, setResetMode] = useState<"soft" | "mixed" | "hard">("mixed");
  const [resetTarget, setResetTarget] = useState("HEAD");
  const [revertCommit, setRevertCommit] = useState("");
  const [cherryCommit, setCherryCommit] = useState("");
  const [cherryNoCommit, setCherryNoCommit] = useState(false);
  const [loading, setLoading] = useState(false);
  const [busyAction, setBusyAction] = useState<string | null>(null);
  const [errorText, setErrorText] = useState<string | null>(null);
  const [noticeText, setNoticeText] = useState<string | null>(null);

  const currentBranch = useMemo(() => branches.find((item) => item.current)?.name ?? "", [branches]);

  const refreshAll = useCallback(async () => {
    if (!workspaceCwd) {
      setStatus(null);
      setHistory([]);
      setBranches([]);
      setSelectedBranch("");
      setSyncBranch("");
      setSelectedPath(null);
      setDiffText("");
      return;
    }

    setLoading(true);
    setErrorText(null);
    try {
      const [statusResp, logResp, branchResp] = await Promise.all([
        gitStatus(workspaceCwd),
        gitLog(workspaceCwd, 40),
        gitBranchList(workspaceCwd),
      ]);
      setStatus(statusResp);
      setHistory(logResp.entries);
      setBranches(branchResp.branches);
      setSelectedBranch((prev) => prev || branchResp.current || branchResp.branches[0]?.name || "");
      setSyncBranch((prev) => prev || branchResp.current || "");
    } catch (error) {
      setErrorText(normalizeError(error));
    } finally {
      setLoading(false);
    }
  }, [workspaceCwd]);

  const loadDiff = useCallback(
    async (path: string, mode: "working" | "staged") => {
      if (!workspaceCwd) {
        return;
      }
      try {
        const resp = await gitDiff(workspaceCwd, path, mode === "staged");
        setDiffText(resp.text);
      } catch (error) {
        setDiffText(intl.formatMessage({ id: "git.diffLoadFailed" }, { error: normalizeError(error) }));
      }
    },
    [intl, workspaceCwd],
  );

  useEffect(() => {
    void refreshAll();
  }, [refreshAll]);

  useEffect(() => {
    if (!selectedPath) {
      setDiffText("");
      return;
    }
    void loadDiff(selectedPath, diffMode);
  }, [diffMode, loadDiff, selectedPath]);

  useEffect(() => {
    if (!selectedPath || !status) {
      return;
    }
    // 刷新后如果该文件已经不在变更列表，自动清理右侧 diff，避免显示过时内容。
    const stillExists = status.changes.some((entry) => entry.path === selectedPath);
    if (!stillExists) {
      setSelectedPath(null);
      setDiffText("");
    }
  }, [selectedPath, status]);

  const runAction = useCallback(
    async (actionLabel: string, action: () => Promise<{ message: string; stdout: string; stderr: string }>) => {
      setBusyAction(actionLabel);
      setNoticeText(null);
      setErrorText(null);
      try {
        const result = await action();
        setNoticeText(result.message || intl.formatMessage({ id: "git.actionDone" }, { action: actionLabel }));
        if (result.stderr?.trim()) {
          setNoticeText(`${result.message}\n${result.stderr.trim()}`);
        }
        await refreshAll();
      } catch (error) {
        setErrorText(normalizeError(error));
      } finally {
        setBusyAction(null);
      }
    },
    [intl, refreshAll],
  );

  const handleStageToggle = useCallback(
    async (entry: GitStatusEntry) => {
      if (!workspaceCwd) {
        return;
      }
      if (entry.staged) {
        await runAction(intl.formatMessage({ id: "git.unstage" }), () => gitUnstage([entry.path], workspaceCwd));
      } else {
        await runAction(intl.formatMessage({ id: "git.stage" }), () => gitStage([entry.path], workspaceCwd));
      }
    },
    [intl, runAction, workspaceCwd],
  );

  const handleCommit = useCallback(async () => {
    if (!workspaceCwd) {
      return;
    }
    const trimmed = commitMessage.trim();
    if (!trimmed) {
      setErrorText(intl.formatMessage({ id: "git.commitEmptyError" }));
      return;
    }
    await runAction(intl.formatMessage({ id: "git.tabCommit" }), async () => {
      const result = await gitCommit(trimmed, workspaceCwd);
      setCommitMessage("");
      return result;
    });
  }, [commitMessage, intl, runAction, workspaceCwd]);

  const handleCheckout = useCallback(async () => {
    if (!workspaceCwd || !selectedBranch) {
      return;
    }
    await runAction(intl.formatMessage({ id: "git.checkout" }), () => gitCheckout(selectedBranch, false, workspaceCwd));
  }, [intl, runAction, selectedBranch, workspaceCwd]);

  const handleCreateBranch = useCallback(async () => {
    if (!workspaceCwd) {
      return;
    }
    const trimmed = newBranchName.trim();
    if (!trimmed) {
      setErrorText(intl.formatMessage({ id: "git.branchNameEmpty" }));
      return;
    }
    await runAction(intl.formatMessage({ id: "git.createAndCheckout" }), async () => {
      const result = await gitCheckout(trimmed, true, workspaceCwd);
      setNewBranchName("");
      setSelectedBranch(trimmed);
      setSyncBranch(trimmed);
      return result;
    });
  }, [intl, newBranchName, runAction, workspaceCwd]);

  const handlePull = useCallback(async () => {
    if (!workspaceCwd) {
      return;
    }
    await runAction(intl.formatMessage({ id: "git.pull" }), () => gitPull(workspaceCwd, "origin", syncBranch || currentBranch || undefined, false));
  }, [currentBranch, intl, runAction, syncBranch, workspaceCwd]);

  const handlePush = useCallback(async () => {
    if (!workspaceCwd) {
      return;
    }
    await runAction(intl.formatMessage({ id: "git.push" }), () => gitPush(workspaceCwd, "origin", syncBranch || currentBranch || undefined, false));
  }, [currentBranch, intl, runAction, syncBranch, workspaceCwd]);

  const handleReset = useCallback(async () => {
    if (!workspaceCwd) {
      return;
    }
    const target = resetTarget.trim() || "HEAD";
    const warn =
      resetMode === "hard"
        ? intl.formatMessage({ id: "git.confirmHardReset" }, { target })
        : intl.formatMessage({ id: "git.confirmReset" }, { mode: resetMode.toUpperCase(), target });
    if (!window.confirm(warn)) {
      return;
    }
    await runAction("Reset", () => gitReset(resetMode, target, true, workspaceCwd));
  }, [intl, resetMode, resetTarget, runAction, workspaceCwd]);

  const handleRevert = useCallback(async () => {
    if (!workspaceCwd) {
      return;
    }
    const commit = revertCommit.trim();
    if (!commit) {
      setErrorText(intl.formatMessage({ id: "git.revertCommitEmpty" }));
      return;
    }
    if (!window.confirm(intl.formatMessage({ id: "git.confirmRevert" }, { commit }))) {
      return;
    }
    await runAction("Revert", () => gitRevert(commit, true, workspaceCwd, true));
  }, [intl, revertCommit, runAction, workspaceCwd]);

  const handleCherryPick = useCallback(async () => {
    if (!workspaceCwd) {
      return;
    }
    const commit = cherryCommit.trim();
    if (!commit) {
      setErrorText(intl.formatMessage({ id: "git.cherryPickCommitEmpty" }));
      return;
    }
    const modeHint = cherryNoCommit ? intl.formatMessage({ id: "git.cherryPickNoCommitHint" }) : "";
    if (!window.confirm(intl.formatMessage({ id: "git.confirmCherryPick" }, { commit, hint: modeHint }))) {
      return;
    }
    await runAction("Cherry-pick", () =>
      gitCherryPick(commit, true, workspaceCwd, cherryNoCommit),
    );
  }, [cherryCommit, cherryNoCommit, intl, runAction, workspaceCwd]);

  if (!workspaceCwd) {
    return (
      <div className="flex h-full items-center justify-center px-4 text-center text-xs text-[var(--text-faint)]">
        {intl.formatMessage({ id: "git.noProject" })}
      </div>
    );
  }

  const changes = status?.changes ?? [];
  const stagedCount = status?.stagedCount ?? 0;

  return (
    <div className="flex h-full flex-col overflow-hidden">
      <div className="flex items-center gap-2 border-b border-[var(--border-subtle)] px-3 py-2">
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-1.5 text-xs font-semibold text-[var(--text-strong)]">
            <IconGitBranch size={14} stroke={1.8} />
            <span className="truncate">{currentBranch || status?.branch || "unknown"}</span>
          </div>
          <div className="truncate text-[11px] text-[var(--text-faint)]">
            {workspaceCwd}
          </div>
        </div>
        <button
          type="button"
          onClick={() => void refreshAll()}
          className="flex h-7 w-7 items-center justify-center rounded-[var(--radius-sm)] text-[var(--text-faint)] transition-colors hover:bg-[var(--surface-elevated)] hover:text-[var(--text-strong)]"
          title={intl.formatMessage({ id: "git.refreshTitle" })}
        >
          <IconRefresh size={14} stroke={1.8} className={loading ? "animate-spin" : ""} />
        </button>
      </div>

      <div className="flex flex-wrap items-center gap-1 border-b border-[var(--border-subtle)] px-2 py-2">
        <button
          type="button"
          onClick={() => setView("changes")}
          className={`rounded-[var(--radius-sm)] px-2 py-1 text-xs ${
            view === "changes"
              ? "bg-[var(--accent-soft)] text-[var(--accent-strong)]"
              : "text-[var(--text-muted)] hover:bg-[var(--surface-elevated)] hover:text-[var(--text-strong)]"
          }`}
        >
          {intl.formatMessage({ id: "git.tabChanges" })}
        </button>
        <button
          type="button"
          onClick={() => setView("commit")}
          className={`rounded-[var(--radius-sm)] px-2 py-1 text-xs ${
            view === "commit"
              ? "bg-[var(--accent-soft)] text-[var(--accent-strong)]"
              : "text-[var(--text-muted)] hover:bg-[var(--surface-elevated)] hover:text-[var(--text-strong)]"
          }`}
        >
          {intl.formatMessage({ id: "git.tabCommit" })}
        </button>
        <button
          type="button"
          onClick={() => setView("history")}
          className={`rounded-[var(--radius-sm)] px-2 py-1 text-xs ${
            view === "history"
              ? "bg-[var(--accent-soft)] text-[var(--accent-strong)]"
              : "text-[var(--text-muted)] hover:bg-[var(--surface-elevated)] hover:text-[var(--text-strong)]"
          }`}
        >
          {intl.formatMessage({ id: "git.tabHistory" })}
        </button>
      </div>

      <div className="flex items-center gap-1 border-b border-[var(--border-subtle)] px-2 py-2">
        <select
          value={selectedBranch}
          onChange={(event) => setSelectedBranch(event.target.value)}
          className="min-w-0 flex-1 rounded-[var(--radius-sm)] border border-[var(--border-subtle)] bg-[var(--surface-soft)] px-2 py-1 text-xs text-[var(--text-base)]"
        >
          {branches.map((branch) => (
            <option key={branch.name} value={branch.name}>
              {branch.name}
            </option>
          ))}
        </select>
        <button
          type="button"
          onClick={() => void handleCheckout()}
          disabled={!selectedBranch || busyAction !== null}
          className="rounded-[var(--radius-sm)] border border-[var(--border-subtle)] px-2 py-1 text-xs text-[var(--text-base)] transition-colors hover:bg-[var(--surface-elevated)] disabled:opacity-40"
        >
          {intl.formatMessage({ id: "git.checkout" })}
        </button>
      </div>

      <div className="flex items-center gap-1 border-b border-[var(--border-subtle)] px-2 py-2">
        <input
          value={newBranchName}
          onChange={(event) => setNewBranchName(event.target.value)}
          placeholder={intl.formatMessage({ id: "git.newBranchPlaceholder" })}
          className="min-w-0 flex-1 rounded-[var(--radius-sm)] border border-[var(--border-subtle)] bg-[var(--surface-soft)] px-2 py-1 text-xs text-[var(--text-base)]"
        />
        <button
          type="button"
          onClick={() => void handleCreateBranch()}
          disabled={busyAction !== null}
          className="rounded-[var(--radius-sm)] border border-[var(--border-subtle)] px-2 py-1 text-xs text-[var(--text-base)] transition-colors hover:bg-[var(--surface-elevated)] disabled:opacity-40"
        >
          {intl.formatMessage({ id: "git.createAndCheckout" })}
        </button>
      </div>

      <div className="flex items-center gap-1 border-b border-[var(--border-subtle)] px-2 py-2">
        <input
          value={syncBranch}
          onChange={(event) => setSyncBranch(event.target.value)}
          placeholder={intl.formatMessage({ id: "git.syncBranchPlaceholder" })}
          className="min-w-0 flex-1 rounded-[var(--radius-sm)] border border-[var(--border-subtle)] bg-[var(--surface-soft)] px-2 py-1 text-xs text-[var(--text-base)]"
        />
        <button
          type="button"
          onClick={() => void handlePull()}
          disabled={busyAction !== null}
          className="inline-flex items-center gap-1 rounded-[var(--radius-sm)] border border-[var(--border-subtle)] px-2 py-1 text-xs text-[var(--text-base)] transition-colors hover:bg-[var(--surface-elevated)] disabled:opacity-40"
        >
          <IconArrowDown size={12} />
          {intl.formatMessage({ id: "git.pull" })}
        </button>
        <button
          type="button"
          onClick={() => void handlePush()}
          disabled={busyAction !== null}
          className="inline-flex items-center gap-1 rounded-[var(--radius-sm)] border border-[var(--border-subtle)] px-2 py-1 text-xs text-[var(--text-base)] transition-colors hover:bg-[var(--surface-elevated)] disabled:opacity-40"
        >
          <IconArrowUp size={12} />
          {intl.formatMessage({ id: "git.push" })}
        </button>
      </div>

      {(errorText || noticeText) && (
        <div
          className={`mx-2 mt-2 rounded-[var(--radius-sm)] border px-2 py-1.5 text-xs ${
            errorText
              ? "border-[rgba(239,68,68,0.35)] bg-[var(--danger-soft)] text-[var(--danger)]"
              : "border-[var(--accent-border)] bg-[var(--accent-soft)] text-[var(--accent-strong)]"
          }`}
        >
          <pre className="thin-scrollbar max-h-24 overflow-auto whitespace-pre-wrap">{errorText ?? noticeText}</pre>
        </div>
      )}

      <div className="min-h-0 flex-1 overflow-hidden px-2 py-2">
        {view === "changes" ? (
          <div className="flex h-full min-h-0 flex-col gap-2">
            <div className="rounded-[var(--radius-sm)] border border-[var(--border-subtle)] bg-[var(--surface-soft)] px-2 py-1 text-[11px] text-[var(--text-muted)]">
              staged: {status?.stagedCount ?? 0} | unstaged: {status?.unstagedCount ?? 0} | untracked: {status?.untrackedCount ?? 0}
            </div>
            <div className="min-h-0 flex-1 overflow-auto rounded-[var(--radius-sm)] border border-[var(--border-subtle)]">
              {changes.length === 0 ? (
                <div className="p-3 text-xs text-[var(--text-faint)]">{intl.formatMessage({ id: "git.workingClean" })}</div>
              ) : (
                changes.map((entry) => (
                  <div
                    key={`${entry.path}-${entry.status}`}
                    className={`border-b border-[var(--border-subtle)] px-2 py-2 last:border-b-0 ${
                      selectedPath === entry.path ? "bg-[var(--accent-soft)]" : "hover:bg-[var(--surface-elevated)]"
                    }`}
                  >
                    <div className="flex items-start gap-2">
                      <button
                        type="button"
                        onClick={() => {
                          setSelectedPath(entry.path);
                          void loadDiff(entry.path, diffMode);
                        }}
                        className="min-w-0 flex-1 text-left"
                      >
                        <div className={`text-[11px] font-semibold uppercase ${statusColor(entry.status)}`}>
                          {entry.status}
                        </div>
                        {entry.oldPath ? (
                          <div className="break-all text-[11px] text-[var(--text-faint)]">
                            {entry.oldPath} -&gt; {entry.path}
                          </div>
                        ) : (
                          <div className="break-all text-[12px] text-[var(--text-base)]">{entry.path}</div>
                        )}
                      </button>
                      <button
                        type="button"
                        onClick={() => void handleStageToggle(entry)}
                        disabled={busyAction !== null}
                        className="rounded-[var(--radius-sm)] border border-[var(--border-subtle)] px-2 py-1 text-[11px] text-[var(--text-base)] hover:bg-[var(--surface-soft)] disabled:opacity-40"
                      >
                        {entry.staged ? intl.formatMessage({ id: "git.unstage" }) : intl.formatMessage({ id: "git.stage" })}
                      </button>
                    </div>
                  </div>
                ))
              )}
            </div>
            <div className="flex min-h-0 flex-1 flex-col overflow-hidden rounded-[var(--radius-sm)] border border-[var(--border-subtle)]">
              <div className="flex items-center justify-between border-b border-[var(--border-subtle)] px-2 py-1">
                <span className="truncate text-[11px] text-[var(--text-faint)]">{selectedPath ?? intl.formatMessage({ id: "git.selectFileForDiff" })}</span>
                <div className="flex items-center gap-1">
                  <button
                    type="button"
                    onClick={() => setDiffMode("working")}
                    className={`rounded-[var(--radius-sm)] px-2 py-0.5 text-[11px] ${
                      diffMode === "working"
                        ? "bg-[var(--accent-soft)] text-[var(--accent-strong)]"
                        : "text-[var(--text-muted)] hover:bg-[var(--surface-elevated)]"
                    }`}
                  >
                    {intl.formatMessage({ id: "git.diffModeWorking" })}
                  </button>
                  <button
                    type="button"
                    onClick={() => setDiffMode("staged")}
                    className={`rounded-[var(--radius-sm)] px-2 py-0.5 text-[11px] ${
                      diffMode === "staged"
                        ? "bg-[var(--accent-soft)] text-[var(--accent-strong)]"
                        : "text-[var(--text-muted)] hover:bg-[var(--surface-elevated)]"
                    }`}
                  >
                    {intl.formatMessage({ id: "git.diffModeStaged" })}
                  </button>
                </div>
              </div>
              <pre className="thin-scrollbar min-h-0 flex-1 overflow-auto bg-[var(--surface-main)] px-3 py-2 text-[11px] leading-relaxed text-[var(--text-base)]">
                <code>{diffText || intl.formatMessage({ id: "git.noDiffContent" })}</code>
              </pre>
            </div>
          </div>
        ) : view === "commit" ? (
          <div className="flex h-full flex-col gap-2">
            <div className="rounded-[var(--radius-sm)] border border-[var(--border-subtle)] bg-[var(--surface-soft)] px-2 py-1 text-[11px] text-[var(--text-muted)]">
              {intl.formatMessage({ id: "git.stagedFileCount" }, { count: stagedCount })}
            </div>
            <textarea
              value={commitMessage}
              onChange={(event) => setCommitMessage(event.target.value)}
              placeholder={intl.formatMessage({ id: "git.commitPlaceholder" })}
              className="min-h-[100px] w-full rounded-[var(--radius-sm)] border border-[var(--border-subtle)] bg-[var(--surface-main)] px-2 py-2 text-xs text-[var(--text-base)]"
            />
            <button
              type="button"
              onClick={() => void handleCommit()}
              disabled={busyAction !== null || stagedCount <= 0}
              className="inline-flex items-center justify-center gap-1 rounded-[var(--radius-sm)] border border-[var(--border-subtle)] px-3 py-2 text-xs text-[var(--text-base)] transition-colors hover:bg-[var(--surface-elevated)] disabled:opacity-40"
            >
              <IconGitCommit size={14} />
              {intl.formatMessage({ id: "git.tabCommit" })}
            </button>

            <div className="mt-2 rounded-[var(--radius-sm)] border border-[var(--border-subtle)] p-2">
              <div className="mb-2 text-[11px] font-semibold text-[var(--text-muted)]">{intl.formatMessage({ id: "git.advancedDanger" })}</div>
              <div className="grid grid-cols-2 gap-2">
                <select
                  value={resetMode}
                  onChange={(event) => setResetMode(event.target.value as "soft" | "mixed" | "hard")}
                  className="rounded-[var(--radius-sm)] border border-[var(--border-subtle)] bg-[var(--surface-soft)] px-2 py-1 text-xs text-[var(--text-base)]"
                >
                  <option value="soft">reset --soft</option>
                  <option value="mixed">reset --mixed</option>
                  <option value="hard">reset --hard</option>
                </select>
                <input
                  value={resetTarget}
                  onChange={(event) => setResetTarget(event.target.value)}
                  placeholder="target (HEAD)"
                  className="rounded-[var(--radius-sm)] border border-[var(--border-subtle)] bg-[var(--surface-soft)] px-2 py-1 text-xs text-[var(--text-base)]"
                />
                <button
                  type="button"
                  onClick={() => void handleReset()}
                  disabled={busyAction !== null}
                  className="inline-flex items-center justify-center gap-1 rounded-[var(--radius-sm)] border border-[var(--border-subtle)] px-2 py-1 text-xs text-[var(--text-base)] hover:bg-[var(--surface-elevated)] disabled:opacity-40"
                >
                  <IconRotateClockwise2 size={13} />
                  {intl.formatMessage({ id: "git.executeReset" })}
                </button>
              </div>

              <div className="mt-2 grid grid-cols-2 gap-2">
                <input
                  value={revertCommit}
                  onChange={(event) => setRevertCommit(event.target.value)}
                  placeholder="revert commit"
                  className="rounded-[var(--radius-sm)] border border-[var(--border-subtle)] bg-[var(--surface-soft)] px-2 py-1 text-xs text-[var(--text-base)]"
                />
                <button
                  type="button"
                  onClick={() => void handleRevert()}
                  disabled={busyAction !== null}
                  className="inline-flex items-center justify-center gap-1 rounded-[var(--radius-sm)] border border-[var(--border-subtle)] px-2 py-1 text-xs text-[var(--text-base)] hover:bg-[var(--surface-elevated)] disabled:opacity-40"
                >
                  <IconHistory size={13} />
                  {intl.formatMessage({ id: "git.executeRevert" })}
                </button>
              </div>

              <div className="mt-2 grid grid-cols-2 gap-2">
                <input
                  value={cherryCommit}
                  onChange={(event) => setCherryCommit(event.target.value)}
                  placeholder="cherry-pick commit"
                  className="rounded-[var(--radius-sm)] border border-[var(--border-subtle)] bg-[var(--surface-soft)] px-2 py-1 text-xs text-[var(--text-base)]"
                />
                <button
                  type="button"
                  onClick={() => void handleCherryPick()}
                  disabled={busyAction !== null}
                  className="inline-flex items-center justify-center gap-1 rounded-[var(--radius-sm)] border border-[var(--border-subtle)] px-2 py-1 text-xs text-[var(--text-base)] hover:bg-[var(--surface-elevated)] disabled:opacity-40"
                >
                  <IconGitCherryPick size={13} />
                  Cherry-pick
                </button>
              </div>
              <label className="mt-2 flex items-center gap-1 text-[11px] text-[var(--text-faint)]">
                <input
                  type="checkbox"
                  checked={cherryNoCommit}
                  onChange={(event) => setCherryNoCommit(event.target.checked)}
                />
                {intl.formatMessage({ id: "git.cherryPickNoCommit" })}
              </label>
            </div>
          </div>
        ) : (
          <div className="thin-scrollbar h-full overflow-auto rounded-[var(--radius-sm)] border border-[var(--border-subtle)]">
            {history.length === 0 ? (
              <div className="p-3 text-xs text-[var(--text-faint)]">{intl.formatMessage({ id: "git.noHistory" })}</div>
            ) : (
              history.map((entry) => (
                <div key={entry.hash} className="border-b border-[var(--border-subtle)] px-3 py-2 last:border-b-0">
                  <div className="flex items-center gap-2 text-[11px] text-[var(--text-faint)]">
                    <span className="inline-flex items-center gap-1">
                      <IconGitCommit size={12} />
                      {entry.shortHash}
                    </span>
                    <span>{entry.author}</span>
                  </div>
                  <div className="mt-0.5 text-xs text-[var(--text-base)]">{entry.message}</div>
                  <div className="mt-0.5 text-[11px] text-[var(--text-faint)]">{entry.date}</div>
                </div>
              ))
            )}
          </div>
        )}
      </div>

      {busyAction && (
        <div className="border-t border-[var(--border-subtle)] px-3 py-2 text-[11px] text-[var(--text-faint)]">
          {intl.formatMessage({ id: "git.busyRunning" }, { action: busyAction })}
        </div>
      )}
    </div>
  );
}
