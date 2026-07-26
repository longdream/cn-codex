export function parseEnum(value, validValues) {
  return validValues.some(v => v.toLowerCase() === String(value).toLowerCase());
}