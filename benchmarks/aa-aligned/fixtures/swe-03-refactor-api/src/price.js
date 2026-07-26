export function calcTotal(items) {
  return items.reduce((sum, item) => sum + item.price * (item.qty ?? 1), 0);
}

// TODO: add lineTotal(item)
// TODO: add calcTotalV2(items, options)
