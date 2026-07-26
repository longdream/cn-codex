import test from "node:test";
import assert from "node:assert/strict";
import { Cache } from "../src/cache.js";

test("basic set/get", () => {
  let t = 1000;
  const c = new Cache({ ttlMs: 100, now: () => t });
  c.set("a", 1);
  assert.equal(c.get("a"), 1);
  t = 1200;
  assert.equal(c.get("a"), undefined);
});

test("expired", () => {
  let t = 1000;
  const c = new Cache({ ttlMs: 100, now: () => t });
  c.set("a", 1);
  t = 1100;
  assert.equal(c.get("a"), undefined);
});
