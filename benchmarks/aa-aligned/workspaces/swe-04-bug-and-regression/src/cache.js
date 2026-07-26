export class Cache {
  constructor(options = {}) {
    this.ttlMs = options.ttlMs ?? 1000;
    this.now = options.now ?? (() => Date.now());
    this.store = new Map();
  }

  set(key, value) {
    this.store.set(key, { value, expiresAt: this.now() + this.ttlMs });
  }

  get(key) {
    const hit = this.store.get(key);
    if (!hit) return undefined;
    if (this.now() >= hit.expiresAt) {
      this.store.delete(key);
      return undefined;
    }
    return hit.value;
  }
}
