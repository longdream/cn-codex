import test from "node:test";
import assert from "node:assert/strict";
import { EventEmitter } from "../src/events.js";

test("EventEmitter", () => {
  const ee = new EventEmitter();
  let calls = [];
  ee.on("test", (a, b) => calls.push([a, b]));
  ee.emit("test", 1, 2);
  assert.equal(calls.length, 1);
  assert.deepEqual(calls[0], [1, 2]);
});

test("once", () => {
  const ee = new EventEmitter();
  let count = 0;
  ee.once("test", () => count++);
  ee.emit("test");
  ee.emit("test");
  assert.equal(count, 1);
});

test("off", () => {
  const ee = new EventEmitter();
  let count = 0;
  const fn = () => count++;
  ee.on("test", fn);
  ee.emit("test");
  ee.off("test", fn);
  ee.emit("test");
  assert.equal(count, 1);
});
