export function formatDate(date, fmt) {
  const pad = (n) => String(n).padStart(2, "0");
  const map = {
    "YYYY": date.getFullYear(),
    "MM": pad(date.getMonth() + 1),
    "DD": pad(date.getDate()),
  };
  let result = fmt;
  for (const [key, val] of Object.entries(map)) {
    result = result.replace(key, val);
  }
  return result;
}

export function daysBetween(d1, d2) {
  const msPerDay = 24 * 60 * 60 * 1000;
  return Math.round((d2.getTime() - d1.getTime()) / msPerDay);
}