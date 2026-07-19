import { describe, expect, it } from "vitest";
import { shouldAcceptEventSequence } from "../utils/turnEventSequence";

describe("turn event sequencing", () => {
  it("accepts new monotonic events and rejects duplicates or stale events", () => {
    expect(shouldAcceptEventSequence(0, 1)).toBe(true);
    expect(shouldAcceptEventSequence(4, 5)).toBe(true);
    expect(shouldAcceptEventSequence(5, 5)).toBe(false);
    expect(shouldAcceptEventSequence(5, 3)).toBe(false);
  });

  it("keeps compatibility with unsequenced legacy events", () => {
    expect(shouldAcceptEventSequence(10, undefined)).toBe(true);
    expect(shouldAcceptEventSequence(10, "invalid")).toBe(true);
  });
});
