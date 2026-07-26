import test from "node:test";
import assert from "node:assert/strict";
import { toNumber } from "../src/typeUtils.js";

test("toNumber", () => {
  assert.equal(toNumber("42"), 42);
  assert.equal(toNumber("3.14"), 3.14);
  assert.ok(Number.isNaN(toNumber("1a")));
  assert.ok(Number.isNaN(toNumber("abc")));
  assert.equal(toNumber(""), 0);
});
