import { invoke } from "@tauri-apps/api/core";
import type { RequestId } from "../types";

export async function resolveApproval(
  requestId: RequestId,
  result: Record<string, unknown>,
): Promise<void> {
  return invoke("resolve_approval", { requestId, result });
}

export async function rejectApproval(
  requestId: RequestId,
  code: number,
  message: string,
): Promise<void> {
  return invoke("reject_approval", { requestId, code, message });
}
