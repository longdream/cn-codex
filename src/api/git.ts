import { invoke } from "@tauri-apps/api/core";

export interface GitStatusEntry {
  path: string;
  oldPath?: string | null;
  status: string;
  staged: boolean;
  unstaged: boolean;
  untracked: boolean;
}

export interface GitStatusResponse {
  branch: string;
  upstream?: string | null;
  ahead: number;
  behind: number;
  isClean: boolean;
  mergeInProgress?: boolean;
  conflictedCount?: number;
  mergeMessage?: string | null;
  stagedCount: number;
  unstagedCount: number;
  untrackedCount: number;
  changes: GitStatusEntry[];
}

export interface GitDiffResponse {
  text: string;
  isEmpty: boolean;
}

export interface GitCommitFileEntry {
  path: string;
  oldPath?: string | null;
  status: string;
}

export interface GitCommitFilesResponse {
  commit: string;
  files: GitCommitFileEntry[];
}

export interface GitFileDiffContentsResponse {
  path: string;
  oldPath?: string | null;
  beforeContent: string;
  afterContent: string;
  fileAction: string;
  isBinary: boolean;
  existsBefore: boolean;
  existsAfter: boolean;
}

export interface GitLogEntry {
  hash: string;
  shortHash: string;
  author: string;
  date: string;
  message: string;
}

export interface GitLogResponse {
  entries: GitLogEntry[];
}

export interface GitBranchEntry {
  name: string;
  current: boolean;
  upstream?: string | null;
  isRemote: boolean;
}

export interface GitBranchListResponse {
  current?: string | null;
  branches: GitBranchEntry[];
}

export interface GitActionResponse {
  ok: boolean;
  message: string;
  stdout: string;
  stderr: string;
}

export async function gitStatus(cwd?: string): Promise<GitStatusResponse> {
  return invoke("git_status", { cwd });
}

export async function gitDiff(
  cwd?: string,
  path?: string,
  staged?: boolean,
): Promise<GitDiffResponse> {
  return invoke("git_diff", { cwd, path, staged });
}

export async function gitCommitFiles(
  commit: string,
  cwd?: string,
): Promise<GitCommitFilesResponse> {
  return invoke("git_commit_files", { cwd, commit });
}

export async function gitFileDiffContents(params: {
  path: string;
  mode?: "working" | "staged" | "commit";
  commit?: string;
  oldPath?: string | null;
  cwd?: string;
}): Promise<GitFileDiffContentsResponse> {
  return invoke("git_file_diff_contents", {
    cwd: params.cwd,
    path: params.path,
    mode: params.mode,
    commit: params.commit,
    oldPath: params.oldPath ?? undefined,
  });
}

export async function gitLog(cwd?: string, limit?: number): Promise<GitLogResponse> {
  return invoke("git_log", { cwd, limit });
}

export async function gitBranchList(cwd?: string): Promise<GitBranchListResponse> {
  return invoke("git_branch_list", { cwd });
}

export async function gitFetch(cwd?: string): Promise<GitActionResponse> {
  return invoke("git_fetch", { cwd });
}

export async function gitStage(paths: string[], cwd?: string): Promise<GitActionResponse> {
  return invoke("git_stage", { cwd, paths });
}

export async function gitStageAll(cwd?: string): Promise<GitActionResponse> {
  return invoke("git_stage_all", { cwd });
}

export async function gitUnstage(paths: string[], cwd?: string): Promise<GitActionResponse> {
  return invoke("git_unstage", { cwd, paths });
}

export async function gitDiscard(
  paths: string[],
  options?: {
    cwd?: string;
    untracked?: boolean;
    confirmDangerous?: boolean;
  },
): Promise<GitActionResponse> {
  return invoke("git_discard", {
    cwd: options?.cwd,
    paths,
    untracked: options?.untracked ?? false,
    confirmDangerous: options?.confirmDangerous ?? true,
  });
}

export async function gitCommit(message: string, cwd?: string): Promise<GitActionResponse> {
  return invoke("git_commit", { cwd, message });
}

export async function gitCheckout(
  branch: string,
  create?: boolean,
  cwd?: string,
  track?: boolean,
): Promise<GitActionResponse> {
  return invoke("git_checkout", { cwd, branch, create, track });
}

export async function gitPull(
  cwd?: string,
  remote?: string,
  branch?: string,
  rebase?: boolean,
): Promise<GitActionResponse> {
  return invoke("git_pull", { cwd, remote, branch, rebase });
}

export async function gitPush(
  cwd?: string,
  remote?: string,
  branch?: string,
  setUpstream?: boolean,
): Promise<GitActionResponse> {
  return invoke("git_push", { cwd, remote, branch, setUpstream });
}

export async function gitReset(
  mode: "soft" | "mixed" | "hard",
  target: string,
  confirmDangerous: boolean,
  cwd?: string,
): Promise<GitActionResponse> {
  return invoke("git_reset", { cwd, mode, target, confirmDangerous });
}

export async function gitRevert(
  commit: string,
  confirmDangerous: boolean,
  cwd?: string,
  noEdit = true,
): Promise<GitActionResponse> {
  return invoke("git_revert", { cwd, commit, noEdit, confirmDangerous });
}

export async function gitCherryPick(
  commit: string,
  confirmDangerous: boolean,
  cwd?: string,
  noCommit = false,
): Promise<GitActionResponse> {
  return invoke("git_cherry_pick", { cwd, commit, noCommit, confirmDangerous });
}

export type GitMergeMode = "default" | "no-ff" | "ff-only" | "squash";

export async function gitMerge(
  branch: string,
  options?: {
    cwd?: string;
    mode?: GitMergeMode;
    noCommit?: boolean;
    message?: string;
  },
): Promise<GitActionResponse> {
  return invoke("git_merge", {
    cwd: options?.cwd,
    branch,
    mode: options?.mode,
    noCommit: options?.noCommit ?? false,
    message: options?.message,
  });
}

export async function gitMergeAbort(cwd?: string): Promise<GitActionResponse> {
  return invoke("git_merge_abort", { cwd });
}

export async function gitMergeContinue(
  options?: {
    cwd?: string;
    message?: string;
  },
): Promise<GitActionResponse> {
  return invoke("git_merge_continue", {
    cwd: options?.cwd,
    message: options?.message,
  });
}
