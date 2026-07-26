import test from "node:test";
import assert from "node:assert/strict";
import { safeGet } from "../src/safeAccess.js";

test("safeGet", () => {
  const obj = { a: { b: { c: 42 } } };
  assert.equal(safeGet(obj, "a.b.c"), 42);
  assert.deepEqual(safeGet(obj, "a.b"), { c: 42 });
  assert.equal(safeGet({ a: null }, "a.b"), undefined);
  assert.equal(safeGet({}, "x.y.z"), undefined);
});