import test from "node:test";
import assert from "node:assert/strict";
import { retry } from "../src/promiseUtils.js";

test("retry succeeds", async () => {
  let attempts = 0;
  const r = await retry(async () => { attempts++; return "ok"; }, 3);
  assert.equal(r, "ok");
  assert.equal(attempts, 1);
});

test("retry fails finally", async () => {
  let attempts = 0;
  await assert.rejects(() => retry(async () => { attempts++; throw new Error("fail"); }, 3));
  assert.equal(attempts, 3);
});
