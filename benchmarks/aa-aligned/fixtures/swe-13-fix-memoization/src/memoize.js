export function memoize(fn) { const cache = {}; return function(...args) { return fn(...args); }; }
