import test from "node:test";
import assert from "node:assert/strict";
import { deepClone, merge } from "../src/objectUtils.js";

test("deepClone", () => {
  const obj = { a: 1, b: { c: 2 } };
  const clone = deepClone(obj);
  assert.deepEqual(clone, obj);
  clone.b.c = 3;
  assert.equal(obj.b.c, 2);
});

test("merge", () => {
  const r = merge({ a: 1, b: 2 }, { b: 3, c: 4 });
  assert.equal(r.a, 1);
  assert.equal(r.b, 3);
  assert.equal(r.c, 4);
});
