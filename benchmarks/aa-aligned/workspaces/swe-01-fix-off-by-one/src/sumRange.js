/**
 * Sum integers in inclusive range [start, end].
 * BUG: currently excludes `end`.
 */
export function sumRange(start, end) {
  if (end < start) return 0;
  let total = 0;
  for (let i = start; i <= end; i += 1) {
    total += i;
  }
  return total;
}