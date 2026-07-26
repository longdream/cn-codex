import test from "node:test";
import assert from "node:assert/strict";
import { escapeHtml } from "../src/stringUtils.js";

test("escapeHtml", () => {
  assert.equal(escapeHtml('<script>alert("xss")</script>'), '&lt;script&gt;alert("xss")&lt;/script&gt;');
  assert.equal(escapeHtml("it's a test"), "it&#39;s a test");
  assert.equal(escapeHtml("a & b"), "a &amp; b");
});
