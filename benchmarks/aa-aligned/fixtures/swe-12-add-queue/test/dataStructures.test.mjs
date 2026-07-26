import test from "node:test";
import assert from "node:assert/strict";
import { Queue, Stack } from "../src/dataStructures.js";

test("Queue", () => {
  const q = new Queue();
  assert.equal(q.size(), 0);
  q.enqueue(1); q.enqueue(2);
  assert.equal(q.size(), 2);
  assert.equal(q.peek(), 1);
  assert.equal(q.dequeue(), 1);
  assert.equal(q.size(), 1);
});

test("Stack", () => {
  const s = new Stack();
  s.push(1); s.push(2);
  assert.equal(s.peek(), 2);
  assert.equal(s.pop(), 2);
  assert.equal(s.size(), 1);
});