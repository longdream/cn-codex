export function deepClone(obj) {
  return JSON.parse(JSON.stringify(obj));
}

export function merge(target, source) {
  return { ...target, ...source };
}