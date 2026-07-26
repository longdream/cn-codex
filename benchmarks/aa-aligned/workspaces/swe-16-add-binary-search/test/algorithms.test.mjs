import test from "node:test";
import assert from "node:assert/strict";
import { binarySearch, quickSort } from "../src/algorithms.js";

test("binarySearch", () => {
  assert.equal(binarySearch([1, 3, 5, 7, 9], 5), 2);
  assert.equal(binarySearch([1, 3, 5, 7, 9], 2), -1);
  assert.equal(binarySearch([], 1), -1);
});

test("quickSort", () => {
  assert.deepEqual(quickSort([3, 1, 4, 1, 5]), [1, 1, 3, 4, 5]);
  assert.deepEqual(quickSort([]), []);
});
