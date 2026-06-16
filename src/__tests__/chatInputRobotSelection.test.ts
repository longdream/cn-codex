import { describe, expect, it } from "vitest";

import { resolveSelectedRobotId } from "../components/chat/ChatInput";

describe("resolveSelectedRobotId", () => {
  it("returns null when no robot is selected", () => {
    expect(resolveSelectedRobotId([], null)).toBeNull();
  });

  it("keeps selection when robot still exists", () => {
    const robots = [{ id: "fullstack-bot" }, { id: "review-bot" }];
    expect(resolveSelectedRobotId(robots, "review-bot")).toBe("review-bot");
  });

  it("clears selection when robot was deleted", () => {
    const robots = [{ id: "fullstack-bot" }];
    expect(resolveSelectedRobotId(robots, "review-bot")).toBeNull();
  });
});
