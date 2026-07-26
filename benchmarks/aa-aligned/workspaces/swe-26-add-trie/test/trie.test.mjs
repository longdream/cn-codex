import test from "node:test";
import assert from "node:assert/strict";
import { Trie } from "../src/trie.js";

test("Trie", () => {
  const t = new Trie();
  t.insert("hello");
  t.insert("world");
  assert.ok(t.search("hello"));
  assert.ok(!t.search("hell"));
  assert.ok(t.startsWith("hel"));
  assert.ok(!t.startsWith("xyz"));
});
