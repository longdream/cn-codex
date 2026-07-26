import test from "node:test";
import assert from "node:assert/strict";
import { fetchWithTimeout } from "../src/asyncUtils.js";

test("timeout rejects", async () => {
  const slow = new Promise(r => setTimeout(r, 1000));
  await assert.rejects(() => fetchWithTimeout(slow, 50));
});
