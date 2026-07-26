export function safeGet(obj, path) {
  return path.split(".").reduce((o, k) => {
    if (o == null) return undefined;
    return o[k];
  }, obj);
}