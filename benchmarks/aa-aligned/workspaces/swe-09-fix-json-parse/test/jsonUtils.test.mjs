import test from "node:test";
import assert from "node:assert/strict";
import { safeParse } from "../src/jsonUtils.js";

test("safeParse valid", () => {
  const r = safeParse('{"a":1}');
  assert.equal(r.error, null);
  assert.deepEqual(r.data, {a: 1});
});

test("safeParse invalid", () => {
  const r = safeParse("not json");
  assert.equal(r.error, "invalid json");
  assert.equal(r.data, null);
});
