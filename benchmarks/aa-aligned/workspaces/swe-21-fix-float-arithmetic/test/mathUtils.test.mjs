import test from "node:test";
import assert from "node:assert/strict";
import { add, multiply } from "../src/mathUtils.js";

test("add precision", () => {
  assert.equal(add(0.1, 0.2), 0.3);
  assert.equal(add(0.01, 0.02), 0.03);
});

test("multiply precision", () => {
  assert.equal(multiply(0.1, 0.2), 0.02);
});
