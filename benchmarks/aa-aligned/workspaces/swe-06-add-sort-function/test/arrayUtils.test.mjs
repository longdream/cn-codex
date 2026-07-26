import test from "node:test";
import assert from "node:assert/strict";
import { sortBy, unique } from "../src/arrayUtils.js";

test("sortBy", () => {
  const items = [{n: 3}, {n: 1}, {n: 2}];
  const r = sortBy(items, "n");
  assert.equal(r[0].n, 1);
  assert.equal(r[1].n, 2);
  assert.equal(r[2].n, 3);
});

test("unique", () => {
  assert.deepEqual(unique([1, 2, 2, 3, 1, 4]), [1, 2, 3, 4]);
  assert.deepEqual(unique([]), []);
});
