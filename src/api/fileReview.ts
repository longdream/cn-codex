import { invoke } from "@tauri-apps/api/core";

export interface FileReviewFile {
  path: string;
  action: string;
  moveTo?: string;
  baseContent?: string;
  candidateContent?: string;
  editedContent?: string;
  keep: boolean;
}

export interface FileReviewSession {
  threadId: string;
  callId: string;
  rawPatch: string;
  createdAtMs: number;
  updatedAtMs: number;
  files: FileReviewFile[];
}

export interface FileReviewActionResponse {
  ok: boolean;
  message: string;
  changedFiles: Array<{ path: string; action: string }>;
}

/**
 * 获取后端缓存的待审阅会话。
 *
 * 注意：该数据来自 apply_patch 的“写盘前”阶段，文件尚未真正落盘。
 */
export async function fileReviewGet(
  threadId: string,
  callId: string,
): Promise<FileReviewSession> {
  return invoke("file_review_get", { threadId, callId });
}

/**
 * 更新单文件审阅数据（keep 勾选或编辑后的内容）。
 *
 * 约束：
 * - 对 deleted 文件传 editedContent 会返回后端校验错误；
 * - editedContent 太长会返回后端长度限制错误。
 */
export async function fileReviewUpdate(
  threadId: string,
  callId: string,
  path: string,
  keep?: boolean,
  editedContent?: string,
): Promise<FileReviewSession> {
  return invoke("file_review_update", { threadId, callId, path, keep, editedContent });
}

/**
 * 应用审阅结果并真正写盘。
 *
 * keepAll 为 true 时会忽略 keepPaths，直接应用全部候选文件。
 */
export async function fileReviewApply(
  threadId: string,
  callId: string,
  keepAll?: boolean,
  keepPaths?: string[],
): Promise<FileReviewActionResponse> {
  return invoke("file_review_apply", { threadId, callId, keepAll, keepPaths });
}

/**
 * 取消审阅并丢弃后端缓存，不写入任何文件。
 */
export async function fileReviewCancel(
  threadId: string,
  callId: string,
): Promise<FileReviewActionResponse> {
  return invoke("file_review_cancel", { threadId, callId });
}
