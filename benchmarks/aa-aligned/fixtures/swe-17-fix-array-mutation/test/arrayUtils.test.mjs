import test from "node:test";
import assert from "node:assert/strict";
import { removeFalsy } from "../src/arrayUtils.js";

test("removeFalsy", () => {
  const input = [0, 1, false, 2, "", 3, null];
  const result = removeFalsy(input);
  assert.deepEqual(result, [1, 2, 3]);
  assert.deepEqual(input, [0, 1, false, 2, "", 3, null]);
});
