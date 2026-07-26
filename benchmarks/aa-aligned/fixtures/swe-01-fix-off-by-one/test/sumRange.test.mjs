import test from "node:test";
import assert from "node:assert/strict";
import { sumRange } from "../src/sumRange.js";

test("sumRange inclusive", () => {
  assert.equal(sumRange(1, 3), 6);
  assert.equal(sumRange(0, 0), 0);
  assert.equal(sumRange(5, 5), 5);
  assert.equal(sumRange(2, 5), 14);
});

test("sumRange empty when end < start", () => {
  assert.equal(sumRange(3, 1), 0);
});
