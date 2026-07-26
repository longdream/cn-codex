import test from "node:test";
import assert from "node:assert/strict";
import { hashString, hashCode } from "../src/hashUtils.js";

test("hashString", () => {
  const h1 = hashString("hello");
  const h2 = hashString("hello");
  const h3 = hashString("world");
  assert.equal(h1, h2);
  assert.notEqual(h1, h3);
  assert.ok(typeof h1 === "string" || typeof h1 === "number");
});

test("hashCode", () => {
  const h = hashCode("test");
  assert.ok(Number.isInteger(h));
});
