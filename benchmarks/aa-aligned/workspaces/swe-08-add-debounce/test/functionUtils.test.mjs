import test from "node:test";
import assert from "node:assert/strict";
import { debounce, throttle } from "../src/functionUtils.js";

test("debounce basic", async () => {
  let count = 0;
  const fn = debounce(() => { count++; }, 50);
  fn(); fn(); fn();
  await new Promise(r => setTimeout(r, 100));
  assert.equal(count, 1);
});

test("throttle basic", async () => {
  let count = 0;
  const fn = throttle(() => { count++; }, 50);
  fn(); fn(); fn();
  await new Promise(r => setTimeout(r, 100));
  assert.ok(count >= 1);
});
