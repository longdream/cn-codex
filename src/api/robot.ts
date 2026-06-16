import { invoke } from "@tauri-apps/api/core";
import type { RobotSummary, RobotDetail } from "../types/robot";

export async function robotList(): Promise<RobotSummary[]> {
  return invoke("robot_list");
}

export async function robotRead(robotId: string): Promise<RobotDetail> {
  return invoke("robot_read", { robotId });
}

export async function robotDelete(robotId: string): Promise<void> {
  return invoke("robot_delete", { robotId });
}
