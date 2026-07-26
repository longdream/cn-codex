import test from "node:test";
import assert from "node:assert/strict";
import { parseCSV, toCSV } from "../src/csvParser.js";

test("parseCSV", () => {
  const data = parseCSV("a,b,c\n1,2,3\n4,5,6");
  assert.equal(data.length, 2);
  assert.deepEqual(data[0], {a: "1", b: "2", c: "3"});
});

test("parseCSV with quotes", () => {
  const data = parseCSV('a,b\n1,"hello, world"');
  assert.equal(data[0].b, "hello, world");
});

test("toCSV", () => {
  const csv = toCSV([{a: 1, b: 2}, {a: 3, b: 4}]);
  assert.ok(csv.includes("a,b"));
  assert.ok(csv.includes("1,2"));
});
