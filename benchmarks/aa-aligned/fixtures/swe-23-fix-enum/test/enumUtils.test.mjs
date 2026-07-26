import test from "node:test";
import assert from "node:assert/strict";
import { parseEnum } from "../src/enumUtils.js";

test("parseEnum", () => {
  assert.ok(parseEnum("HIGH", ["high", "medium", "low"]));
  assert.ok(parseEnum("High", ["high", "medium", "low"]));
  assert.ok(parseEnum("HIGH", ["HIGH", "MEDIUM", "LOW"]));
  assert.ok(!parseEnum("unknown", ["high", "medium", "low"]));
});
