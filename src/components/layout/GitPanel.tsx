import {
  IconAlertTriangle,
  IconArrowDown,
  IconArrowUp,
  IconCheck,
  IconChevronDown,
  IconFiles,
  IconGitBranch,
  IconGitCherryPick,
  IconGitCommit,
  IconGitMerge,
  IconHistory,
  IconMinus,
  IconPlus,
  IconRefresh,
  IconRotateClockwise2,
  IconX,
} from "@tabler/icons-react";
import { memo, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useIntl } from "react-intl";
import {
  gitBranchList,
  gitCherryPick,
  gitCheckout,
  gitCommit,
  gitCommitFiles,
  gitDiff,
  gitDiscard,
  gitFileDiffContents,
  gitLog,
  gitMerge,
  gitMergeAbort,
  gitMergeContinue,
  gitPull,
  gitPush,
  gitReset,
  gitRevert,
  gitStage,
  gitStatus,
  gitUnstage,
  type GitActionResponse,
  type GitCommitFileEntry,
  type GitLogEntry,
  type GitMergeMode,
  type GitStatusEntry,
  type GitStatusResponse,
} from "../../api/git";
import { windowOpenRunSummaryDiff } from "../../api/window";

type GitSection = "changes" | "branches" | "history" | "danger";
type DiffMode = "working" | "staged";
type CommitMode = "commit" | "commitAndPush";

interface GitPanelProps {
  workspaceCwd: string | null;
}

interface DiffSelection {
  path: string;
  mode: DiffMode;
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

function uniqPaths(entries: GitStatusEntry[]): string[] {
  return [...new Set(entries.map((entry) => entry.path))];
}

function formatActionNotice(result: GitActionResponse): string {
  if (result.stderr?.trim()) {
    return `${result.message}\n${result.stderr.trim()}`;
  }
  return result.message;
}

function GitPanelComponent({ workspaceCwd }: GitPanelProps) {
  const intl = useIntl();
  const commitMenuRef = useRef<HTMLDivElement | null>(null);
  const [section, setSection] = useState<GitSection>("changes");
  const [status, setStatus] = useState<GitStatusResponse | null>(null);
  const [history, setHistory] = useState<GitLogEntry[]>([]);
  const [branches, setBranches] = useState<Array<{ name: string; current: boolean; upstream?: string | null }>>([]);
  const [selectedBranch, setSelectedBranch] = useState("");
  const [newBranchName, setNewBranchName] = useState("");
  const [mergeMode, setMergeMode] = useState<GitMergeMode>("default");
  const [mergeNoCommit, setMergeNoCommit] = useState(false);
  const [selectedDiff, setSelectedDiff] = useState<DiffSelection | null>(null);
  const [diffText, setDiffText] = useState("");
  const [commitMessage, setCommitMessage] = useState("");
  const [commitMenuOpen, setCommitMenuOpen] = useState(false);
  const [expandedHistoryHash, setExpandedHistoryHash] = useState<string | null>(null);
  const [historyFilesByCommit, setHistoryFilesByCommit] = useState<Record<string, GitCommitFileEntry[]>>({});
  const [historyFilesLoading, setHistoryFilesLoading] = useState<string | null>(null);
  const [openingDiffPath, setOpeningDiffPath] = useState<string | null>(null);
  const [resetMode, setResetMode] = useState<"soft" | "mixed" | "hard">("mixed");
  const [resetTarget, setResetTarget] = useState("HEAD");
  const [revertCommit, setRevertCommit] = useState("");
  const [cherryCommit, setCherryCommit] = useState("");
  const [cherryNoCommit, setCherryNoCommit] = useState(false);
  const [loading, setLoading] = useState(false);
  const [busyAction, setBusyAction] = useState<string | null>(null);
  const [actionProgress, setActionProgress] = useState(0);
  const [actionPhase, setActionPhase] = useState<string | null>(null);
  const [errorText, setErrorText] = useState<string | null>(null);
  const [noticeText, setNoticeText] = useState<string | null>(null);

  const currentBranch = useMemo(() => branches.find((item) => item.current)?.name ?? "", [branches]);
  const changes = status?.changes ?? [];
  const stagedEntries = useMemo(() => changes.filter((entry) => entry.staged), [changes]);
  const unstagedEntries = useMemo(() => changes.filter((entry) => entry.unstaged), [changes]);
  const untrackedEntries = useMemo(() => changes.filter((entry) => entry.untracked), [changes]);
  const conflictedEntries = useMemo(
    () => changes.filter((entry) => entry.status === "conflicted"),
    [changes],
  );
  const conflictedCount = status?.conflictedCount ?? conflictedEntries.length;
  const nonConflictStagedEntries = useMemo(
    () => stagedEntries.filter((entry) => entry.status !== "conflicted"),
    [stagedEntries],
  );
  const nonConflictUnstagedEntries = useMemo(
    () => unstagedEntries.filter((entry) => entry.status !== "conflicted" && !entry.untracked),
    [unstagedEntries],
  );
  const allChangePaths = useMemo(() => uniqPaths(changes), [changes]);
  const stagedPaths = useMemo(() => uniqPaths(stagedEntries), [stagedEntries]);

  const refreshAll = useCallback(async () => {
    if (!workspaceCwd) {
      setStatus(null);
      setHistory([]);
      setBranches([]);
      setSelectedBranch("");
      setSelectedDiff(null);
      setDiffText("");
      setExpandedHistoryHash(null);
      setHistoryFilesByCommit({});
      setHistoryFilesLoading(null);
      setOpeningDiffPath(null);
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
      setSelectedBranch((prev) => {
        const names = new Set(branchResp.branches.map((branch) => branch.name));
        if (prev && names.has(prev)) {
          return prev;
        }
        return branchResp.current || branchResp.branches[0]?.name || "";
      });
    } catch (error) {
      setErrorText(normalizeError(error));
    } finally {
      setLoading(false);
    }
  }, [workspaceCwd]);

  const refreshBranches = useCallback(async () => {
    if (!workspaceCwd) {
      setBranches([]);
      setSelectedBranch("");
      return;
    }

    setLoading(true);
    setErrorText(null);
    try {
      const branchResp = await gitBranchList(workspaceCwd);
      setBranches(branchResp.branches);
      setSelectedBranch((prev) => {
        const names = new Set(branchResp.branches.map((branch) => branch.name));
        if (prev && names.has(prev)) {
          return prev;
        }
        return branchResp.current || branchResp.branches[0]?.name || "";
      });
      setNoticeText(intl.formatMessage({ id: "git.branchesRefreshed" }));
    } catch (error) {
      setErrorText(normalizeError(error));
    } finally {
      setLoading(false);
    }
  }, [intl, workspaceCwd]);

  const loadDiff = useCallback(
    async (selection: DiffSelection) => {
      if (!workspaceCwd) {
        return;
      }
      try {
        const resp = await gitDiff(workspaceCwd, selection.path, selection.mode === "staged");
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

  // 合并进行中时预填 MERGE_MSG，方便用户继续合并提交。
  useEffect(() => {
    if (!status?.mergeInProgress) {
      return;
    }
    const prepared = status.mergeMessage?.trim();
    if (!prepared) {
      return;
    }
    setCommitMessage((prev) => (prev.trim() ? prev : prepared));
  }, [status?.mergeInProgress, status?.mergeMessage]);

  useEffect(() => {
    if (!selectedDiff) {
      setDiffText("");
      return;
    }
    void loadDiff(selectedDiff);
  }, [loadDiff, selectedDiff]);

  useEffect(() => {
    if (!selectedDiff || !status) {
      return;
    }
    const stillExists = status.changes.some((entry) => entry.path === selectedDiff.path);
    if (!stillExists) {
      setSelectedDiff(null);
      setDiffText("");
    }
  }, [selectedDiff, status]);

  useEffect(() => {
    if (!commitMenuOpen) {
      return;
    }
    const handlePointerDown = (event: MouseEvent) => {
      if (commitMenuRef.current && !commitMenuRef.current.contains(event.target as Node)) {
        setCommitMenuOpen(false);
      }
    };
    window.addEventListener("mousedown", handlePointerDown);
    return () => {
      window.removeEventListener("mousedown", handlePointerDown);
    };
  }, [commitMenuOpen]);

  const runAction = useCallback(
    async (actionLabel: string, action: () => Promise<GitActionResponse>, options?: { showProgress?: boolean }) => {
      const showProgress = options?.showProgress === true;
      setBusyAction(actionLabel);
      setActionPhase(showProgress ? actionLabel : null);
      setActionProgress(showProgress ? 12 : 0);
      setNoticeText(null);
      setErrorText(null);
      let progressTimer: number | null = null;
      if (showProgress) {
        progressTimer = window.setInterval(() => {
          setActionProgress((prev) => {
            if (prev <= 0 || prev >= 90) return prev;
            return Math.min(90, prev + 4);
          });
        }, 350);
      }
      try {
        if (showProgress) {
          setActionProgress(28);
        }
        const result = await action();
        if (showProgress) {
          setActionProgress(82);
        }
        setNoticeText(formatActionNotice(result));
        await refreshAll();
        if (showProgress) {
          setActionProgress(100);
          await new Promise((resolve) => window.setTimeout(resolve, 220));
        }
      } catch (error) {
        setErrorText(normalizeError(error));
        // 合并冲突等失败场景也要刷新，才能显示 mergeInProgress / conflicted 文件。
        try {
          await refreshAll();
        } catch {
          // ignore secondary refresh failures
        }
      } finally {
        if (progressTimer !== null) {
          window.clearInterval(progressTimer);
        }
        setBusyAction(null);
        setActionPhase(null);
        setActionProgress(0);
      }
    },
    [refreshAll],
  );

  const handleSelectDiff = useCallback((path: string, mode: DiffMode) => {
    setSelectedDiff({ path, mode });
  }, []);

  const openDiffDetail = useCallback(
    async (params: {
      path: string;
      mode: "working" | "staged" | "commit";
      commit?: string;
      oldPath?: string | null;
      statusHint?: string;
    }) => {
      if (!workspaceCwd) {
        return;
      }
      const openKey = `${params.mode}:${params.commit ?? ""}:${params.path}`;
      setOpeningDiffPath(openKey);
      setErrorText(null);
      try {
        const contents = await gitFileDiffContents({
          path: params.path,
          mode: params.mode,
          commit: params.commit,
          oldPath: params.oldPath,
          cwd: workspaceCwd,
        });
        const emptyBecauseBinary = contents.isBinary;
        const emptyBecauseMissing =
          !contents.isBinary &&
          contents.beforeContent.trim().length === 0 &&
          contents.afterContent.trim().length === 0;
        await windowOpenRunSummaryDiff({
          path: contents.path || params.path,
          beforeContent: contents.beforeContent,
          afterContent: contents.afterContent,
          fileAction: contents.fileAction || params.statusHint || "modified",
          diffSource: emptyBecauseBinary || emptyBecauseMissing ? "empty" : "snapshot",
          canPersist: false,
          emptyHint: emptyBecauseBinary
            ? intl.formatMessage({ id: "git.diffBinaryUnavailable" })
            : emptyBecauseMissing
              ? intl.formatMessage({ id: "git.noDiffContent" })
              : undefined,
        });
      } catch (error) {
        setErrorText(
          intl.formatMessage({ id: "git.diffLoadFailed" }, { error: normalizeError(error) }),
        );
      } finally {
        setOpeningDiffPath(null);
      }
    },
    [intl, workspaceCwd],
  );

  const handleOpenWorkingDiff = useCallback(
    (entry: GitStatusEntry, mode: DiffMode) => {
      setSelectedDiff({ path: entry.path, mode });
      void openDiffDetail({
        path: entry.path,
        mode,
        oldPath: entry.oldPath,
        statusHint: entry.status,
      });
    },
    [openDiffDetail],
  );

  const handleToggleHistoryCommit = useCallback(
    async (commitHash: string) => {
      if (expandedHistoryHash === commitHash) {
        setExpandedHistoryHash(null);
        return;
      }
      setExpandedHistoryHash(commitHash);
      if (historyFilesByCommit[commitHash] || !workspaceCwd) {
        return;
      }
      setHistoryFilesLoading(commitHash);
      setErrorText(null);
      try {
        const resp = await gitCommitFiles(commitHash, workspaceCwd);
        setHistoryFilesByCommit((prev) => ({
          ...prev,
          [commitHash]: resp.files,
        }));
      } catch (error) {
        setErrorText(
          intl.formatMessage(
            { id: "git.historyFilesLoadFailed" },
            { error: normalizeError(error) },
          ),
        );
      } finally {
        setHistoryFilesLoading(null);
      }
    },
    [expandedHistoryHash, historyFilesByCommit, intl, workspaceCwd],
  );

  const handleStagePath = useCallback(
    async (path: string) => {
      if (!workspaceCwd) {
        return;
      }
      await runAction(intl.formatMessage({ id: "git.stage" }), () => gitStage([path], workspaceCwd));
    },
    [intl, runAction, workspaceCwd],
  );

  const handleUnstagePath = useCallback(
    async (path: string) => {
      if (!workspaceCwd) {
        return;
      }
      await runAction(intl.formatMessage({ id: "git.unstage" }), () => gitUnstage([path], workspaceCwd));
    },
    [intl, runAction, workspaceCwd],
  );

  const handleDiscardPath = useCallback(
    async (entry: GitStatusEntry) => {
      if (!workspaceCwd) {
        return;
      }
      const confirmText = entry.untracked
        ? intl.formatMessage({ id: "git.confirmDiscardUntracked" }, { path: entry.path })
        : intl.formatMessage({ id: "git.confirmDiscard" }, { path: entry.path });
      if (!window.confirm(confirmText)) {
        return;
      }
      await runAction(intl.formatMessage({ id: "git.discard" }), () =>
        gitDiscard([entry.path], {
          cwd: workspaceCwd,
          untracked: entry.untracked,
          confirmDangerous: true,
        }),
      );
    },
    [intl, runAction, workspaceCwd],
  );

  const handleStageAll = useCallback(async () => {
    if (!workspaceCwd || allChangePaths.length === 0) {
      return;
    }
    await runAction(intl.formatMessage({ id: "git.stageAll" }), () => gitStage(allChangePaths, workspaceCwd));
  }, [allChangePaths, intl, runAction, workspaceCwd]);

  const handleUnstageAll = useCallback(async () => {
    if (!workspaceCwd || stagedPaths.length === 0) {
      return;
    }
    await runAction(intl.formatMessage({ id: "git.unstageAll" }), () => gitUnstage(stagedPaths, workspaceCwd));
  }, [intl, runAction, stagedPaths, workspaceCwd]);

  const handleMergeContinue = useCallback(async () => {
    if (!workspaceCwd) {
      return;
    }
    if (conflictedCount > 0) {
      setErrorText(intl.formatMessage({ id: "git.mergeContinueHasConflicts" }, { count: conflictedCount }));
      setSection("changes");
      return;
    }
    const message = commitMessage.trim() || undefined;
    await runAction(intl.formatMessage({ id: "git.mergeContinue" }), async () => {
      const result = await gitMergeContinue({
        cwd: workspaceCwd,
        message,
      });
      setCommitMessage("");
      return result;
    });
  }, [commitMessage, conflictedCount, intl, runAction, workspaceCwd]);

  const handleMergeAbort = useCallback(async () => {
    if (!workspaceCwd) {
      return;
    }
    if (!window.confirm(intl.formatMessage({ id: "git.confirmMergeAbort" }))) {
      return;
    }
    await runAction(intl.formatMessage({ id: "git.mergeAbort" }), () => gitMergeAbort(workspaceCwd));
  }, [intl, runAction, workspaceCwd]);

  const handleFocusConflicts = useCallback(() => {
    setSection("changes");
    const firstConflict = conflictedEntries[0];
    if (firstConflict) {
      setSelectedDiff({
        path: firstConflict.path,
        mode: firstConflict.staged ? "staged" : "working",
      });
    }
  }, [conflictedEntries]);

  const handleCommit = useCallback(
    async (mode: CommitMode) => {
      if (!workspaceCwd) {
        return;
      }

      // 合并进行中时，提交按钮走“继续合并”语义。
      if (status?.mergeInProgress) {
        if (mode === "commitAndPush") {
          setErrorText(intl.formatMessage({ id: "git.mergeContinueNoPush" }));
          return;
        }
        await handleMergeContinue();
        return;
      }

      const trimmed = commitMessage.trim();
      if (!trimmed) {
        setErrorText(intl.formatMessage({ id: "git.commitEmptyError" }));
        return;
      }

      const actionLabel = intl.formatMessage({ id: mode === "commit" ? "git.tabCommit" : "git.commitAndPush" });
      setCommitMenuOpen(false);
      setBusyAction(actionLabel);
      setActionPhase(intl.formatMessage({ id: "git.progressCommitting" }));
      setActionProgress(mode === "commitAndPush" ? 18 : 28);
      setNoticeText(null);
      setErrorText(null);
      let progressTimer: number | null = window.setInterval(() => {
        setActionProgress((prev) => {
          if (prev <= 0 || prev >= 90) return prev;
          return Math.min(90, prev + 3);
        });
      }, 350);

      try {
        const commitResult = await gitCommit(trimmed, workspaceCwd);
        setCommitMessage("");
        setActionProgress(mode === "commitAndPush" ? 48 : 78);

        if (mode === "commitAndPush") {
          setActionPhase(intl.formatMessage({ id: "git.progressPushing" }));
          setActionProgress(62);
          try {
            const pushResult = await gitPush(workspaceCwd, "origin", currentBranch || undefined, false);
            setActionProgress(88);
            setNoticeText(
              formatActionNotice({
                ...pushResult,
                message: `${commitResult.message} ${pushResult.message}`.trim(),
                stderr: [commitResult.stderr, pushResult.stderr].filter(Boolean).join("\n"),
              }),
            );
          } catch (error) {
            setErrorText(
              `${intl.formatMessage({ id: "git.pushFailedAfterCommit" }, { error: normalizeError(error) })}\n${formatActionNotice(commitResult)}`,
            );
          }
        } else {
          setNoticeText(formatActionNotice(commitResult));
        }

        setSection("changes");
        await refreshAll();
        setActionProgress(100);
      } catch (error) {
        setErrorText(normalizeError(error));
      } finally {
        if (progressTimer !== null) {
          window.clearInterval(progressTimer);
        }
        // Keep the completed bar visible briefly so push feedback is noticeable.
        await new Promise((resolve) => window.setTimeout(resolve, 220));
        setBusyAction(null);
        setActionPhase(null);
        setActionProgress(0);
      }
    },
    [commitMessage, currentBranch, handleMergeContinue, intl, refreshAll, status?.mergeInProgress, workspaceCwd],
  );

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
      return result;
    });
  }, [intl, newBranchName, runAction, workspaceCwd]);

  const handleMerge = useCallback(async () => {
    if (!workspaceCwd || !selectedBranch) {
      return;
    }
    if (selectedBranch === currentBranch) {
      setErrorText(intl.formatMessage({ id: "git.mergeSameBranchError" }));
      return;
    }
    const modeLabel = intl.formatMessage({ id: `git.mergeMode.${mergeMode}` });
    const noCommitHint = mergeNoCommit ? intl.formatMessage({ id: "git.mergeNoCommitHint" }) : "";
    if (
      !window.confirm(
        intl.formatMessage(
          { id: "git.confirmMerge" },
          {
            branch: selectedBranch,
            current: currentBranch || "HEAD",
            mode: modeLabel,
            hint: noCommitHint,
          },
        ),
      )
    ) {
      return;
    }
    await runAction(intl.formatMessage({ id: "git.merge" }), () =>
      gitMerge(selectedBranch, {
        cwd: workspaceCwd,
        mode: mergeMode,
        noCommit: mergeNoCommit,
      }),
    );
    // 合并后回到变更页，便于查看冲突或继续提交。
    setSection("changes");
  }, [currentBranch, intl, mergeMode, mergeNoCommit, runAction, selectedBranch, workspaceCwd]);

  const handlePull = useCallback(async () => {
    if (!workspaceCwd) {
      return;
    }
    await runAction(
      intl.formatMessage({ id: "git.pull" }),
      () => gitPull(workspaceCwd, "origin", currentBranch || undefined, false),
      { showProgress: true },
    );
  }, [currentBranch, intl, runAction, workspaceCwd]);

  const handlePush = useCallback(async () => {
    if (!workspaceCwd) {
      return;
    }
    await runAction(
      intl.formatMessage({ id: "git.push" }),
      () => gitPush(workspaceCwd, "origin", currentBranch || undefined, false),
      { showProgress: true },
    );
  }, [currentBranch, intl, runAction, workspaceCwd]);

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

  const stagedCount = status?.stagedCount ?? 0;
  const iconButtonClass =
    "flex h-7 w-7 items-center justify-center rounded-[var(--radius-sm)] text-[var(--text-faint)] transition-colors hover:bg-[var(--surface-elevated)] hover:text-[var(--text-strong)] disabled:cursor-not-allowed disabled:opacity-35";

  const renderChangeRow = (
    entry: GitStatusEntry,
    diffMode: DiffMode,
    actionType: "stage" | "unstage",
  ) => {
    const isSelected = selectedDiff?.path === entry.path && selectedDiff.mode === diffMode;
    return (
      <div
        key={`${diffMode}-${entry.path}-${entry.status}`}
        className={`border-b border-[var(--border-subtle)] px-2 py-2 last:border-b-0 ${
          isSelected ? "bg-[var(--accent-soft)]" : "hover:bg-[var(--surface-elevated)]"
        }`}
      >
        <div className="flex items-start gap-2">
          <button
            type="button"
            onClick={() => handleOpenWorkingDiff(entry, diffMode)}
            className="min-w-0 flex-1 text-left"
            title={intl.formatMessage({ id: "git.openDiffDetail" })}
          >
            <div className="flex items-center gap-2">
              <span className={`text-[10px] font-semibold uppercase tracking-[0.08em] ${statusColor(entry.status)}`}>
                {entry.status}
              </span>
              {entry.staged && entry.unstaged && (
                <span className="rounded-full bg-[var(--surface-elevated)] px-1.5 py-0.5 text-[9px] text-[var(--text-faint)]">
                  {intl.formatMessage({ id: "git.partialChanges" })}
                </span>
              )}
            </div>
            {entry.oldPath ? (
              <div className="mt-0.5 break-all text-[11px] text-[var(--text-faint)]">
                {entry.oldPath} -&gt; {entry.path}
              </div>
            ) : (
              <div className="mt-0.5 break-all text-[12px] text-[var(--text-base)]">{entry.path}</div>
            )}
          </button>
          <button
            type="button"
            onClick={() => void (actionType === "stage" ? handleStagePath(entry.path) : handleUnstagePath(entry.path))}
            disabled={busyAction !== null}
            className="flex h-6 w-6 items-center justify-center rounded-[var(--radius-sm)] border border-[var(--border-subtle)] text-[var(--text-muted)] transition-colors hover:bg-[var(--surface-soft)] hover:text-[var(--text-strong)] disabled:opacity-35"
            title={intl.formatMessage({ id: actionType === "stage" ? "git.stage" : "git.unstage" })}
            aria-label={intl.formatMessage({ id: actionType === "stage" ? "git.stage" : "git.unstage" })}
          >
            {actionType === "stage" ? <IconPlus size={12} stroke={2} /> : <IconMinus size={12} stroke={2} />}
          </button>
          <button
            type="button"
            onClick={() => void handleDiscardPath(entry)}
            disabled={busyAction !== null}
            className="flex h-6 w-6 items-center justify-center rounded-[var(--radius-sm)] border border-[rgba(239,68,68,0.35)] text-[var(--danger)] transition-colors hover:bg-[var(--danger-soft)] disabled:opacity-35"
            title={intl.formatMessage({ id: "git.discard" })}
            aria-label={intl.formatMessage({ id: "git.discard" })}
          >
            <IconX size={12} stroke={2} />
          </button>
        </div>
      </div>
    );
  };

  const renderChangeSection = (
    titleId: string,
    entries: GitStatusEntry[],
    diffMode: DiffMode,
    actionType: "stage" | "unstage",
  ) => {
    if (entries.length === 0) {
      return null;
    }

    return (
      <div className="border-b border-[var(--border-subtle)] last:border-b-0">
        <div className="flex items-center justify-between px-2 py-1.5 text-[10px] font-semibold uppercase tracking-[0.08em] text-[var(--text-faint)]">
          <span>{intl.formatMessage({ id: titleId })}</span>
          <span>{entries.length}</span>
        </div>
        {entries.map((entry) => renderChangeRow(entry, diffMode, actionType))}
      </div>
    );
  };

  const sectionButton = (
    nextSection: GitSection,
    icon: JSX.Element,
    titleId: string,
  ) => (
    <button
      type="button"
      onClick={() => setSection(nextSection)}
      className={`flex h-7 w-7 items-center justify-center rounded-[var(--radius-sm)] transition-colors ${
        section === nextSection
          ? "bg-[var(--accent-soft)] text-[var(--accent-strong)]"
          : "text-[var(--text-muted)] hover:bg-[var(--surface-elevated)] hover:text-[var(--text-strong)]"
      }`}
      title={intl.formatMessage({ id: titleId })}
      aria-label={intl.formatMessage({ id: titleId })}
    >
      {icon}
    </button>
  );

  return (
    <div className="flex h-full select-text flex-col overflow-hidden">
      <div className="border-b border-[var(--border-subtle)] px-3 py-3">
        <div className="flex items-start gap-2">
          <div className="min-w-0 flex-1">
            <div className="flex items-center gap-1.5 text-xs font-semibold text-[var(--text-strong)]">
              <IconGitBranch size={14} stroke={1.8} />
              <span className="truncate">{currentBranch || status?.branch || "unknown"}</span>
            </div>
            <div className="mt-1 flex flex-wrap items-center gap-1 text-[10px] text-[var(--text-faint)]">
              {status?.upstream && (
                <span className="rounded-full border border-[var(--border-subtle)] bg-[var(--surface-soft)] px-1.5 py-0.5">
                  {status.upstream}
                </span>
              )}
              {status?.ahead ? (
                <span className="rounded-full border border-[var(--border-subtle)] bg-[var(--surface-soft)] px-1.5 py-0.5">
                  ↑{status.ahead}
                </span>
              ) : null}
              {status?.behind ? (
                <span className="rounded-full border border-[var(--border-subtle)] bg-[var(--surface-soft)] px-1.5 py-0.5">
                  ↓{status.behind}
                </span>
              ) : null}
              {status?.mergeInProgress ? (
                <span className="rounded-full border border-[rgba(245,158,11,0.45)] bg-[rgba(245,158,11,0.14)] px-1.5 py-0.5 text-amber-200">
                  {intl.formatMessage({ id: "git.mergeInProgressTitle" })}
                </span>
              ) : null}
            </div>
          </div>
          <div className="flex shrink-0 items-center gap-1">
            <button
              type="button"
              onClick={() => void refreshAll()}
              className={iconButtonClass}
              title={intl.formatMessage({ id: "git.refreshTitle" })}
              aria-label={intl.formatMessage({ id: "git.refreshTitle" })}
            >
              <IconRefresh size={14} stroke={1.8} className={loading ? "animate-spin" : ""} />
            </button>
            <button
              type="button"
              onClick={() => void handlePull()}
              disabled={busyAction !== null}
              className={iconButtonClass}
              title={intl.formatMessage({ id: "git.pull" })}
              aria-label={intl.formatMessage({ id: "git.pull" })}
            >
              <IconArrowDown size={14} stroke={1.8} />
            </button>
            <button
              type="button"
              onClick={() => void handlePush()}
              disabled={busyAction !== null}
              className={iconButtonClass}
              title={intl.formatMessage({ id: "git.push" })}
              aria-label={intl.formatMessage({ id: "git.push" })}
            >
              <IconArrowUp size={14} stroke={1.8} />
            </button>
          </div>
        </div>

        <div className="mt-3 rounded-[var(--radius-md)] border border-[var(--border-subtle)] bg-[var(--surface-soft)] p-2.5">
          <textarea
            value={commitMessage}
            onChange={(event) => setCommitMessage(event.target.value)}
            onKeyDown={(event) => {
              if ((event.metaKey || event.ctrlKey) && event.key === "Enter") {
                event.preventDefault();
                void handleCommit(event.shiftKey ? "commitAndPush" : "commit");
              }
            }}
            placeholder={intl.formatMessage({ id: "git.commitPlaceholder" })}
            className="min-h-[74px] w-full rounded-[var(--radius-sm)] border border-[var(--border-subtle)] bg-[var(--surface-main)] px-2.5 py-2 text-xs text-[var(--text-base)] outline-none transition-colors focus:border-[var(--accent-border)]"
          />
          <div className="mt-2 flex flex-wrap items-center gap-2">
            <div className="flex min-w-0 flex-1 flex-wrap items-center gap-1">
              <button
                type="button"
                onClick={() => void handleStageAll()}
                disabled={busyAction !== null || allChangePaths.length === 0}
                className={`${iconButtonClass} shrink-0`}
                title={intl.formatMessage({ id: "git.stageAll" })}
                aria-label={intl.formatMessage({ id: "git.stageAll" })}
              >
                <IconPlus size={13} stroke={2} />
              </button>
              <button
                type="button"
                onClick={() => void handleUnstageAll()}
                disabled={busyAction !== null || stagedPaths.length === 0}
                className={`${iconButtonClass} shrink-0`}
                title={intl.formatMessage({ id: "git.unstageAll" })}
                aria-label={intl.formatMessage({ id: "git.unstageAll" })}
              >
                <IconMinus size={13} stroke={2} />
              </button>
              <span className="min-w-0 rounded-full border border-[var(--border-subtle)] bg-[var(--surface-main)] px-2 py-1 text-[10px] text-[var(--text-faint)]">
                {intl.formatMessage({ id: "git.stagedFileCount" }, { count: stagedCount })}
              </span>
            </div>

            <div ref={commitMenuRef} className="relative ml-auto flex shrink-0 items-stretch">
              <button
                type="button"
                onClick={() => void handleCommit("commit")}
                disabled={
                  busyAction !== null ||
                  (status?.mergeInProgress
                    ? conflictedCount > 0
                    : stagedCount <= 0)
                }
                className="inline-flex h-7 w-7 shrink-0 items-center justify-center rounded-l-[var(--radius-sm)] border border-[var(--accent-border)] bg-[var(--accent-soft)] text-[var(--accent-strong)] transition-colors hover:bg-[rgba(34,197,94,0.18)] disabled:cursor-not-allowed disabled:opacity-40"
                title={
                  status?.mergeInProgress
                    ? intl.formatMessage({ id: "git.mergeContinue" })
                    : intl.formatMessage({ id: "git.tabCommit" })
                }
                aria-label={
                  status?.mergeInProgress
                    ? intl.formatMessage({ id: "git.mergeContinue" })
                    : intl.formatMessage({ id: "git.tabCommit" })
                }
              >
                <IconCheck size={13} stroke={2} />
              </button>
              <button
                type="button"
                onClick={() => setCommitMenuOpen((open) => !open)}
                disabled={busyAction !== null || status?.mergeInProgress || stagedCount <= 0}
                className="inline-flex h-7 w-7 shrink-0 items-center justify-center rounded-r-[var(--radius-sm)] border border-l-0 border-[var(--accent-border)] bg-[var(--accent-soft)] text-[var(--accent-strong)] transition-colors hover:bg-[rgba(34,197,94,0.18)] disabled:cursor-not-allowed disabled:opacity-40"
                title={intl.formatMessage({ id: "git.commitMenu" })}
                aria-label={intl.formatMessage({ id: "git.commitMenu" })}
              >
                <IconChevronDown size={13} stroke={2} />
              </button>
              {commitMenuOpen && (
                <div className="absolute right-0 top-[calc(100%+6px)] z-10 min-w-[170px] rounded-[var(--radius-sm)] border border-[var(--border-subtle)] bg-[var(--surface-panel)] p-1 shadow-[var(--shadow-strong)]">
                  <button
                    type="button"
                    onClick={() => void handleCommit("commit")}
                    className="flex w-full items-center gap-2 rounded-[var(--radius-sm)] px-2 py-1.5 text-left text-xs text-[var(--text-base)] transition-colors hover:bg-[var(--surface-elevated)] hover:text-[var(--text-strong)]"
                  >
                    <IconGitCommit size={13} stroke={1.9} />
                    {intl.formatMessage({ id: "git.tabCommit" })}
                  </button>
                  <button
                    type="button"
                    onClick={() => void handleCommit("commitAndPush")}
                    className="flex w-full items-center gap-2 rounded-[var(--radius-sm)] px-2 py-1.5 text-left text-xs text-[var(--text-base)] transition-colors hover:bg-[var(--surface-elevated)] hover:text-[var(--text-strong)]"
                  >
                    <IconArrowUp size={13} stroke={1.9} />
                    {intl.formatMessage({ id: "git.commitAndPush" })}
                  </button>
                </div>
              )}
            </div>
          </div>
          <div className="mt-2 text-[10px] text-[var(--text-faint)]">
            {status?.mergeInProgress
              ? intl.formatMessage({ id: "git.mergeContinueHint" })
              : intl.formatMessage({ id: "git.commitShortcutHint" })}
          </div>
        </div>
      </div>

      <div className="flex items-center gap-1 border-b border-[var(--border-subtle)] px-2 py-2">
        {sectionButton("changes", <IconFiles size={14} stroke={1.8} />, "git.openChanges")}
        {sectionButton("branches", <IconGitBranch size={14} stroke={1.8} />, "git.openBranches")}
        {sectionButton("history", <IconHistory size={14} stroke={1.8} />, "git.openHistory")}
        {sectionButton("danger", <IconAlertTriangle size={14} stroke={1.8} />, "git.openDanger")}
      </div>

      {(errorText || noticeText) && (
        <div
          className={`mx-2 mt-2 rounded-[var(--radius-sm)] border px-2 py-1.5 text-xs ${
            errorText
              ? "border-[rgba(239,68,68,0.35)] bg-[var(--danger-soft)] text-[var(--danger)]"
              : "border-[var(--accent-border)] bg-[var(--accent-soft)] text-[var(--accent-strong)]"
          }`}
        >
          <pre className="thin-scrollbar max-h-24 select-text overflow-auto whitespace-pre-wrap">{errorText ?? noticeText}</pre>
        </div>
      )}

      {status?.mergeInProgress && section !== "branches" ? (
        <div className="mx-2 mt-2 flex items-center justify-between gap-2 rounded-[var(--radius-sm)] border border-[rgba(245,158,11,0.4)] bg-[rgba(245,158,11,0.12)] px-2.5 py-1.5 text-[11px] text-amber-200">
          <span className="min-w-0 flex-1 truncate">
            {conflictedCount > 0
              ? intl.formatMessage({ id: "git.mergeConflictHint" }, { count: conflictedCount })
              : intl.formatMessage({ id: "git.mergeReadyHint" })}
          </span>
          <div className="flex shrink-0 items-center gap-1">
            {conflictedCount > 0 ? (
              <button
                type="button"
                onClick={handleFocusConflicts}
                className={iconButtonClass}
                title={intl.formatMessage({ id: "git.viewConflicts" })}
                aria-label={intl.formatMessage({ id: "git.viewConflicts" })}
              >
                <IconFiles size={14} stroke={1.8} />
              </button>
            ) : (
              <button
                type="button"
                onClick={() => void handleMergeContinue()}
                disabled={busyAction !== null}
                className={iconButtonClass}
                title={intl.formatMessage({ id: "git.mergeContinue" })}
                aria-label={intl.formatMessage({ id: "git.mergeContinue" })}
              >
                <IconCheck size={14} stroke={1.8} />
              </button>
            )}
            <button
              type="button"
              onClick={() => void handleMergeAbort()}
              disabled={busyAction !== null}
              className={iconButtonClass}
              title={intl.formatMessage({ id: "git.mergeAbort" })}
              aria-label={intl.formatMessage({ id: "git.mergeAbort" })}
            >
              <IconX size={14} stroke={1.8} />
            </button>
          </div>
        </div>
      ) : null}

      <div className="min-h-0 flex-1 overflow-hidden px-2 py-2">
        {section === "changes" ? (
          <div className="flex h-full min-h-0 flex-col gap-2">
            <div className="flex flex-wrap gap-1 text-[10px] text-[var(--text-faint)]">
              <span className="rounded-full border border-[var(--border-subtle)] bg-[var(--surface-soft)] px-2 py-1">
                {intl.formatMessage({ id: "git.sectionStaged" })} {status?.stagedCount ?? 0}
              </span>
              <span className="rounded-full border border-[var(--border-subtle)] bg-[var(--surface-soft)] px-2 py-1">
                {intl.formatMessage({ id: "git.sectionChanges" })} {status?.unstagedCount ?? 0}
              </span>
              <span className="rounded-full border border-[var(--border-subtle)] bg-[var(--surface-soft)] px-2 py-1">
                {intl.formatMessage({ id: "git.sectionUntracked" })} {status?.untrackedCount ?? 0}
              </span>
              {conflictedCount > 0 ? (
                <span className="rounded-full border border-[rgba(245,158,11,0.45)] bg-[rgba(245,158,11,0.14)] px-2 py-1 text-amber-200">
                  {intl.formatMessage({ id: "git.sectionConflicts" })} {conflictedCount}
                </span>
              ) : null}
            </div>

            <div className="min-h-0 overflow-auto rounded-[var(--radius-sm)] border border-[var(--border-subtle)]">
              {changes.length === 0 ? (
                <div className="p-3 text-xs text-[var(--text-faint)]">{intl.formatMessage({ id: "git.workingClean" })}</div>
              ) : (
                <>
                  {renderChangeSection("git.sectionConflicts", conflictedEntries, "working", "stage")}
                  {renderChangeSection("git.sectionStaged", nonConflictStagedEntries, "staged", "unstage")}
                  {renderChangeSection("git.sectionChanges", nonConflictUnstagedEntries, "working", "stage")}
                  {renderChangeSection("git.sectionUntracked", untrackedEntries, "working", "stage")}
                </>
              )}
            </div>

            <div className="flex min-h-0 flex-1 flex-col overflow-hidden rounded-[var(--radius-sm)] border border-[var(--border-subtle)]">
              <div className="flex items-center justify-between border-b border-[var(--border-subtle)] px-2 py-1">
                <span className="truncate text-[11px] text-[var(--text-faint)]">
                  {selectedDiff?.path ?? intl.formatMessage({ id: "git.selectFileForDiff" })}
                </span>
                <div className="flex items-center gap-1">
                  <button
                    type="button"
                    onClick={() => selectedDiff && handleSelectDiff(selectedDiff.path, "working")}
                    className={`rounded-[var(--radius-sm)] px-2 py-0.5 text-[11px] ${
                      selectedDiff?.mode === "working"
                        ? "bg-[var(--accent-soft)] text-[var(--accent-strong)]"
                        : "text-[var(--text-muted)] hover:bg-[var(--surface-elevated)]"
                    }`}
                  >
                    {intl.formatMessage({ id: "git.diffModeWorking" })}
                  </button>
                  <button
                    type="button"
                    onClick={() => selectedDiff && handleSelectDiff(selectedDiff.path, "staged")}
                    className={`rounded-[var(--radius-sm)] px-2 py-0.5 text-[11px] ${
                      selectedDiff?.mode === "staged"
                        ? "bg-[var(--accent-soft)] text-[var(--accent-strong)]"
                        : "text-[var(--text-muted)] hover:bg-[var(--surface-elevated)]"
                    }`}
                  >
                    {intl.formatMessage({ id: "git.diffModeStaged" })}
                  </button>
                </div>
              </div>
              <pre className="thin-scrollbar min-h-0 flex-1 select-text overflow-auto bg-[var(--surface-main)] px-3 py-2 text-[11px] leading-relaxed text-[var(--text-base)]">
                <code>{diffText || intl.formatMessage({ id: "git.noDiffContent" })}</code>
              </pre>
            </div>
          </div>
        ) : section === "branches" ? (
          <div className="flex h-full flex-col gap-2">
            <div className="flex items-center justify-between gap-2 rounded-[var(--radius-sm)] border border-[var(--border-subtle)] bg-[var(--surface-soft)] px-2.5 py-2 text-[11px] text-[var(--text-muted)]">
              <span className="min-w-0 flex-1">{intl.formatMessage({ id: "git.branchSectionHint" })}</span>
              <button
                type="button"
                onClick={() => void refreshBranches()}
                disabled={loading || busyAction !== null}
                className={iconButtonClass}
                title={intl.formatMessage({ id: "git.refreshBranches" })}
                aria-label={intl.formatMessage({ id: "git.refreshBranches" })}
              >
                <IconRefresh size={14} stroke={1.8} className={loading ? "animate-spin" : ""} />
              </button>
            </div>
            {status?.mergeInProgress ? (
              <div className="rounded-[var(--radius-sm)] border border-[rgba(245,158,11,0.4)] bg-[rgba(245,158,11,0.12)] px-2.5 py-2 text-[11px] text-amber-200">
                <div className="mb-2 flex items-start gap-2">
                  <IconAlertTriangle size={14} stroke={1.8} className="mt-0.5 shrink-0" />
                  <div className="min-w-0 flex-1">
                    <div className="font-semibold">{intl.formatMessage({ id: "git.mergeInProgressTitle" })}</div>
                    <div className="mt-0.5 text-[var(--text-muted)]">
                      {conflictedCount > 0
                        ? intl.formatMessage({ id: "git.mergeConflictHint" }, { count: conflictedCount })
                        : intl.formatMessage({ id: "git.mergeReadyHint" })}
                    </div>
                  </div>
                </div>
                <div className="flex flex-wrap items-center gap-1">
                  {conflictedCount > 0 ? (
                    <button
                      type="button"
                      onClick={handleFocusConflicts}
                      className={iconButtonClass}
                      title={intl.formatMessage({ id: "git.viewConflicts" })}
                      aria-label={intl.formatMessage({ id: "git.viewConflicts" })}
                    >
                      <IconFiles size={14} stroke={1.8} />
                    </button>
                  ) : (
                    <button
                      type="button"
                      onClick={() => void handleMergeContinue()}
                      disabled={busyAction !== null}
                      className={iconButtonClass}
                      title={intl.formatMessage({ id: "git.mergeContinue" })}
                      aria-label={intl.formatMessage({ id: "git.mergeContinue" })}
                    >
                      <IconCheck size={14} stroke={1.8} />
                    </button>
                  )}
                  <button
                    type="button"
                    onClick={() => void handleMergeAbort()}
                    disabled={busyAction !== null}
                    className={iconButtonClass}
                    title={intl.formatMessage({ id: "git.mergeAbort" })}
                    aria-label={intl.formatMessage({ id: "git.mergeAbort" })}
                  >
                    <IconX size={14} stroke={1.8} />
                  </button>
                </div>
              </div>
            ) : null}
            <div className="rounded-[var(--radius-sm)] border border-[var(--border-subtle)] p-2">
              <div className="mb-2 text-[11px] font-semibold text-[var(--text-faint)]">{intl.formatMessage({ id: "git.checkout" })}</div>
              <div className="flex items-center gap-1">
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
                  className={iconButtonClass}
                  title={intl.formatMessage({ id: "git.checkout" })}
                  aria-label={intl.formatMessage({ id: "git.checkout" })}
                >
                  <IconGitBranch size={14} stroke={1.8} />
                </button>
              </div>
            </div>
            <div className="rounded-[var(--radius-sm)] border border-[var(--border-subtle)] p-2">
              <div className="mb-2 text-[11px] font-semibold text-[var(--text-faint)]">{intl.formatMessage({ id: "git.createAndCheckout" })}</div>
              <div className="flex items-center gap-1">
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
                  className={iconButtonClass}
                  title={intl.formatMessage({ id: "git.createAndCheckout" })}
                  aria-label={intl.formatMessage({ id: "git.createAndCheckout" })}
                >
                  <IconPlus size={14} stroke={1.8} />
                </button>
              </div>
            </div>
            <div className="rounded-[var(--radius-sm)] border border-[var(--border-subtle)] p-2">
              <div className="mb-2 text-[11px] font-semibold text-[var(--text-faint)]">{intl.formatMessage({ id: "git.merge" })}</div>
              <div className="mb-2 text-[11px] text-[var(--text-muted)]">
                {intl.formatMessage(
                  { id: "git.mergeSectionHint" },
                  { current: currentBranch || status?.branch || "HEAD" },
                )}
              </div>
              <div className="flex flex-col gap-2">
                <div className="flex items-center gap-1">
                  <select
                    value={selectedBranch}
                    onChange={(event) => setSelectedBranch(event.target.value)}
                    className="min-w-0 flex-1 rounded-[var(--radius-sm)] border border-[var(--border-subtle)] bg-[var(--surface-soft)] px-2 py-1 text-xs text-[var(--text-base)]"
                  >
                    {branches.map((branch) => (
                      <option key={`merge-${branch.name}`} value={branch.name}>
                        {branch.name}
                        {branch.current ? " *" : ""}
                      </option>
                    ))}
                  </select>
                  <select
                    value={mergeMode}
                    onChange={(event) => setMergeMode(event.target.value as GitMergeMode)}
                    className="w-[108px] shrink-0 rounded-[var(--radius-sm)] border border-[var(--border-subtle)] bg-[var(--surface-soft)] px-2 py-1 text-xs text-[var(--text-base)]"
                    title={intl.formatMessage({ id: "git.mergeModeLabel" })}
                  >
                    <option value="default">{intl.formatMessage({ id: "git.mergeMode.default" })}</option>
                    <option value="no-ff">{intl.formatMessage({ id: "git.mergeMode.no-ff" })}</option>
                    <option value="ff-only">{intl.formatMessage({ id: "git.mergeMode.ff-only" })}</option>
                    <option value="squash">{intl.formatMessage({ id: "git.mergeMode.squash" })}</option>
                  </select>
                  <button
                    type="button"
                    onClick={() => void handleMerge()}
                    disabled={
                      !selectedBranch ||
                      selectedBranch === currentBranch ||
                      busyAction !== null ||
                      Boolean(status?.mergeInProgress)
                    }
                    className={iconButtonClass}
                    title={intl.formatMessage({ id: "git.mergeIntoCurrent" })}
                    aria-label={intl.formatMessage({ id: "git.mergeIntoCurrent" })}
                  >
                    <IconGitMerge size={14} stroke={1.8} />
                  </button>
                </div>
                <label className="flex items-center gap-1 text-[11px] text-[var(--text-faint)]">
                  <input
                    type="checkbox"
                    checked={mergeNoCommit}
                    onChange={(event) => setMergeNoCommit(event.target.checked)}
                    disabled={mergeMode === "squash"}
                  />
                  {intl.formatMessage({ id: "git.mergeNoCommit" })}
                </label>
              </div>
            </div>
          </div>
        ) : section === "history" ? (
          <div className="thin-scrollbar h-full overflow-auto rounded-[var(--radius-sm)] border border-[var(--border-subtle)]">
            {history.length === 0 ? (
              <div className="p-3 text-xs text-[var(--text-faint)]">{intl.formatMessage({ id: "git.noHistory" })}</div>
            ) : (
              history.map((entry) => (
                <div key={entry.hash} className="border-b border-[var(--border-subtle)] last:border-b-0">
                  <button
                    type="button"
                    onClick={() => void handleToggleHistoryCommit(entry.hash)}
                    className="w-full px-3 py-2 text-left transition-colors hover:bg-[var(--surface-elevated)]"
                    title={intl.formatMessage({ id: "git.historyExpandHint" })}
                  >
                    <div className="flex items-center gap-2 text-[11px] text-[var(--text-faint)]">
                      <span className="inline-flex items-center gap-1">
                        <IconGitCommit size={12} stroke={1.8} />
                        {entry.shortHash}
                      </span>
                      <span className="truncate">{entry.author}</span>
                      <IconChevronDown
                        size={12}
                        stroke={1.8}
                        className={`ml-auto shrink-0 transition-transform ${
                          expandedHistoryHash === entry.hash ? "rotate-180" : ""
                        }`}
                      />
                    </div>
                    <div className="mt-0.5 text-xs text-[var(--text-base)]">{entry.message}</div>
                    <div className="mt-0.5 text-[11px] text-[var(--text-faint)]">{entry.date}</div>
                  </button>
                  {expandedHistoryHash === entry.hash && (
                    <div className="border-t border-[var(--border-subtle)] bg-[var(--surface-soft)] px-2 py-1.5">
                      {historyFilesLoading === entry.hash ? (
                        <div className="px-1 py-1 text-[11px] text-[var(--text-faint)]">
                          {intl.formatMessage({ id: "git.historyFilesLoading" })}
                        </div>
                      ) : (historyFilesByCommit[entry.hash] ?? []).length === 0 ? (
                        <div className="px-1 py-1 text-[11px] text-[var(--text-faint)]">
                          {intl.formatMessage({ id: "git.historyNoFiles" })}
                        </div>
                      ) : (
                        (historyFilesByCommit[entry.hash] ?? []).map((file) => {
                          const openKey = `commit:${entry.hash}:${file.path}`;
                          const isOpening = openingDiffPath === openKey;
                          return (
                            <button
                              key={`${entry.hash}:${file.path}:${file.oldPath ?? ""}`}
                              type="button"
                              disabled={isOpening}
                              onClick={() =>
                                void openDiffDetail({
                                  path: file.path,
                                  mode: "commit",
                                  commit: entry.hash,
                                  oldPath: file.oldPath,
                                  statusHint: file.status,
                                })
                              }
                              className="flex w-full items-start gap-2 rounded-[var(--radius-sm)] px-1.5 py-1.5 text-left transition-colors hover:bg-[var(--surface-elevated)] disabled:opacity-60"
                              title={intl.formatMessage({ id: "git.openDiffDetail" })}
                            >
                              <span
                                className={`mt-0.5 shrink-0 text-[10px] font-semibold uppercase tracking-[0.08em] ${statusColor(file.status)}`}
                              >
                                {file.status}
                              </span>
                              <span className="min-w-0 flex-1 break-all text-[11px] text-[var(--text-base)]">
                                {file.oldPath ? `${file.oldPath} -> ${file.path}` : file.path}
                              </span>
                            </button>
                          );
                        })
                      )}
                    </div>
                  )}
                </div>
              ))
            )}
          </div>
        ) : (
          <div className="flex h-full flex-col gap-2">
            <div className="rounded-[var(--radius-sm)] border border-[var(--border-subtle)] bg-[var(--danger-soft)] px-2.5 py-2 text-[11px] text-[var(--danger)]">
              {intl.formatMessage({ id: "git.dangerSectionHint" })}
            </div>
            <div className="rounded-[var(--radius-sm)] border border-[var(--border-subtle)] p-2">
              <div className="mb-2 text-[11px] font-semibold text-[var(--text-faint)]">{intl.formatMessage({ id: "git.executeReset" })}</div>
              <div className="flex flex-col gap-2">
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
                </div>
                <div className="flex justify-end">
                  <button
                    type="button"
                    onClick={() => void handleReset()}
                    disabled={busyAction !== null}
                    className={iconButtonClass}
                    title={intl.formatMessage({ id: "git.executeReset" })}
                    aria-label={intl.formatMessage({ id: "git.executeReset" })}
                  >
                    <IconRotateClockwise2 size={14} stroke={1.8} />
                  </button>
                </div>
              </div>
            </div>

            <div className="rounded-[var(--radius-sm)] border border-[var(--border-subtle)] p-2">
              <div className="mb-2 text-[11px] font-semibold text-[var(--text-faint)]">{intl.formatMessage({ id: "git.executeRevert" })}</div>
              <div className="flex items-center gap-1">
                <input
                  value={revertCommit}
                  onChange={(event) => setRevertCommit(event.target.value)}
                  placeholder="revert commit"
                  className="min-w-0 flex-1 rounded-[var(--radius-sm)] border border-[var(--border-subtle)] bg-[var(--surface-soft)] px-2 py-1 text-xs text-[var(--text-base)]"
                />
                <button
                  type="button"
                  onClick={() => void handleRevert()}
                  disabled={busyAction !== null}
                  className={iconButtonClass}
                  title={intl.formatMessage({ id: "git.executeRevert" })}
                  aria-label={intl.formatMessage({ id: "git.executeRevert" })}
                >
                  <IconHistory size={14} stroke={1.8} />
                </button>
              </div>
            </div>

            <div className="rounded-[var(--radius-sm)] border border-[var(--border-subtle)] p-2">
              <div className="mb-2 text-[11px] font-semibold text-[var(--text-faint)]">{intl.formatMessage({ id: "git.cherryPickLabel" })}</div>
              <div className="flex items-center gap-1">
                <input
                  value={cherryCommit}
                  onChange={(event) => setCherryCommit(event.target.value)}
                  placeholder={intl.formatMessage({ id: "git.cherryPickPlaceholder" })}
                  className="min-w-0 flex-1 rounded-[var(--radius-sm)] border border-[var(--border-subtle)] bg-[var(--surface-soft)] px-2 py-1 text-xs text-[var(--text-base)]"
                />
                <button
                  type="button"
                  onClick={() => void handleCherryPick()}
                  disabled={busyAction !== null}
                  className={iconButtonClass}
                  title={intl.formatMessage({ id: "git.cherryPickLabel" })}
                  aria-label={intl.formatMessage({ id: "git.cherryPickLabel" })}
                >
                  <IconGitCherryPick size={14} stroke={1.8} />
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
        )}
      </div>

      {busyAction && (
        <div className="border-t border-[var(--border-subtle)] px-3 py-2">
          <div className="mb-1.5 flex items-center justify-between gap-2 text-[11px] text-[var(--text-faint)]">
            <span className="min-w-0 truncate">
              {actionPhase
                ? intl.formatMessage({ id: "git.busyRunning" }, { action: actionPhase })
                : intl.formatMessage({ id: "git.busyRunning" }, { action: busyAction })}
            </span>
            {actionProgress > 0 && (
              <span className="shrink-0 font-mono text-[10px] text-[var(--text-muted)]">{Math.max(0, Math.min(100, actionProgress))}%</span>
            )}
          </div>
          <div className="h-2 overflow-hidden rounded-full bg-[var(--surface-soft)]">
            <div
              className={`h-full rounded-full bg-[var(--accent)] transition-[width] duration-300 ease-out ${
                actionProgress > 0 ? "" : "animate-pulse"
              }`}
              style={{ width: `${actionProgress > 0 ? Math.max(8, Math.min(100, actionProgress)) : 35}%` }}
            />
          </div>
        </div>
      )}
    </div>
  );
}

export const GitPanel = memo(GitPanelComponent);
