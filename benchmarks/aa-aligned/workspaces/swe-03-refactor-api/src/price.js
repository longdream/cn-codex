export function calcTotal(items) {
  return items.reduce((sum, item) => sum + item.price * (item.qty ?? 1), 0);
}

function round2(n) {
  return Math.round((n + Number.EPSILON) * 100) / 100;
}

export function lineTotal(item) {
  return item.price * (item.qty ?? 1);
}

export function calcTotalV2(items, options) {
  const rawSubtotal = items.reduce((sum, item) => sum + item.price * (item.qty ?? 1), 0);
  const subtotal = round2(rawSubtotal);
  const tax = round2(rawSubtotal * (options.taxRate ?? 0));
  const total = round2(subtotal + tax);
  return { subtotal, tax, total };
}