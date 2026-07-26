import test from "node:test";
import assert from "node:assert/strict";
import { memoize } from "../src/memoize.js";

test("memoize basic", () => {
  let callCount = 0;
  const fn = memoize((x) => { callCount++; return x * 2; });
  assert.equal(fn(5), 10);
  assert.equal(fn(5), 10);
  assert.equal(callCount, 1);
});

test("memoize object args", () => {
  let callCount = 0;
  const fn = memoize((obj) => { callCount++; return obj.x; });
  assert.equal(fn({x: 1}), 1);
  assert.equal(fn({x: 1}), 1);
  assert.equal(callCount, 1);
});
