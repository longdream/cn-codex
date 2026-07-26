import test from "node:test";
import assert from "node:assert/strict";
import { RateLimiter } from "../src/rateLimiter.js";

test("RateLimiter allow", () => {
  const rl = new RateLimiter({ maxRequests: 2, windowMs: 1000 });
  assert.ok(rl.allow("user1"));
  assert.ok(rl.allow("user1"));
  assert.ok(!rl.allow("user1"));
});

test("RateLimiter reset", () => {
  const rl = new RateLimiter({ maxRequests: 1, windowMs: 1000 });
  rl.allow("user1");
  rl.reset("user1");
  assert.ok(rl.allow("user1"));
});

test("RateLimiter remaining", () => {
  const rl = new RateLimiter({ maxRequests: 3, windowMs: 1000 });
  rl.allow("user1");
  assert.equal(rl.remaining("user1"), 2);
});
