import test from "node:test";
import assert from "node:assert/strict";
import { pipe, compose } from "../src/fpUtils.js";

test("pipe", () => {
  const add1 = x => x + 1;
  const double = x => x * 2;
  const fn = pipe(add1, double);
  assert.equal(fn(5), 12);
});

test("compose", () => {
  const add1 = x => x + 1;
  const double = x => x * 2;
  const fn = compose(add1, double);
  assert.equal(fn(5), 11);
});
