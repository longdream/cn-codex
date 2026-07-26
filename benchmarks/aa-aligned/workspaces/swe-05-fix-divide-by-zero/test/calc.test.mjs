import test from "node:test";
import assert from "node:assert/strict";
import { add, subtract, multiply, divide } from "../src/calculator.js";

test("basic operations", () => {
  assert.equal(add(2, 3), 5);
  assert.equal(subtract(5, 3), 2);
  assert.equal(multiply(4, 3), 12);
});

test("divide by zero should throw", () => {
  assert.throws(() => divide(10, 0), /cannot divide by zero/i);
});
