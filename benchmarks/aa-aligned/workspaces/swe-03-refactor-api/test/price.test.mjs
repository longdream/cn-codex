import test from "node:test";
import assert from "node:assert/strict";
import { calcTotal, lineTotal, calcTotalV2 } from "../src/price.js";

const items = [
  { price: 10, qty: 2 },
  { price: 5 },
];

test("legacy calcTotal", () => {
  assert.equal(calcTotal(items), 25);
});

test("lineTotal", () => {
  assert.equal(lineTotal({ price: 3, qty: 4 }), 12);
  assert.equal(lineTotal({ price: 7 }), 7);
});

test("calcTotalV2", () => {
  const r = calcTotalV2(items, { taxRate: 0.1 });
  assert.equal(r.subtotal, 25);
  assert.equal(r.tax, 2.5);
  assert.equal(r.total, 27.5);
});

test("calcTotalV2 rounding", () => {
  const r = calcTotalV2([{ price: 1.005, qty: 1 }], { taxRate: 0 });
  assert.equal(r.subtotal, 1.01);
  assert.equal(r.tax, 0);
  assert.equal(r.total, 1.01);
});
