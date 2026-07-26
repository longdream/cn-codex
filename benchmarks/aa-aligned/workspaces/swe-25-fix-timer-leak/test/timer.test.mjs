import test from "node:test";
import assert from "node:assert/strict";
import { start, stop } from "../src/timer.js";

test("timer restart", async () => {
  let count = 0;
  start(() => count++, 10);
  await new Promise(r => setTimeout(r, 30));
  stop();
  const c1 = count;
  start(() => count++, 10);
  await new Promise(r => setTimeout(r, 30));
  stop();
  assert.ok(count > c1);
});
