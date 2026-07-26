import test from "node:test";
import assert from "node:assert/strict";
import { validateEmail } from "../src/validator.js";

test("valid emails", () => {
  assert.ok(validateEmail("user@example.com"));
  assert.ok(validateEmail("first.last@example.com"));
  assert.ok(validateEmail("user+tag@example.co.uk"));
});

test("invalid emails", () => {
  assert.ok(!validateEmail(""));
  assert.ok(!validateEmail("notanemail"));
  assert.ok(!validateEmail("@example.com"));
});
