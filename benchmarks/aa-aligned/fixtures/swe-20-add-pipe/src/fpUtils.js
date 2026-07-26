export function pipe(...fns) {
  return function(x) {
    return fns.reduce((v, fn) => fn(v), x);
  };
}

export function compose(...fns) {
  return function(x) {
    return fns.reduceRight((v, fn) => fn(v), x);
  };
}