export function safeGet(obj, path) { return path.split(".").reduce((o, k) => o[k], obj); }
