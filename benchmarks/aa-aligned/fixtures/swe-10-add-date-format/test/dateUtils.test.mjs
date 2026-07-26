import test from "node:test";
import assert from "node:assert/strict";
import { formatDate, daysBetween } from "../src/dateUtils.js";

test("formatDate", () => {
  const d = new Date(2026, 0, 15);
  assert.equal(formatDate(d, "YYYY-MM-DD"), "2026-01-15");
});

test("daysBetween", () => {
  assert.equal(daysBetween(new Date("2026-01-01"), new Date("2026-01-10")), 9);
});
