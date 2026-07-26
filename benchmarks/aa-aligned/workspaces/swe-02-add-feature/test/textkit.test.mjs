import test from "node:test";
import assert from "node:assert/strict";
import { slugify, truncate, countWords } from "../src/textkit.js";

test("slugify still works", () => {
  assert.equal(slugify("Hello World"), "hello-world");
});

test("truncate", () => {
  assert.equal(truncate("abcdef", 10), "abcdef");
  assert.equal(truncate("abcdef", 4), "abc\u2026");
  assert.equal(truncate("a", 1), "a");
  assert.equal(truncate("ab", 1), "\u2026");
  assert.equal(truncate("xyz", 0), "");
});

test("countWords", () => {
  assert.equal(countWords(""), 0);
  assert.equal(countWords("  "), 0);
  assert.equal(countWords("one"), 1);
  assert.equal(countWords("one two  three"), 3);
});
