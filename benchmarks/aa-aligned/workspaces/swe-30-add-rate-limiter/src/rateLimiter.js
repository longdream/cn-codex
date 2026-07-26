export class RateLimiter {
  constructor(options) {
    this.maxRequests = options.maxRequests;
    this.windowMs = options.windowMs;
    this.timestamps = new Map();
  }

  allow(key) {
    const now = Date.now();
    const cutoff = now - this.windowMs;
    let timestamps = this.timestamps.get(key) || [];
    timestamps = timestamps.filter(t => t > cutoff);
    if (timestamps.length >= this.maxRequests) {
      this.timestamps.set(key, timestamps);
      return false;
    }
    timestamps.push(now);
    this.timestamps.set(key, timestamps);
    return true;
  }

  reset(key) {
    this.timestamps.delete(key);
  }

  remaining(key) {
    const now = Date.now();
    const cutoff = now - this.windowMs;
    const timestamps = (this.timestamps.get(key) || []).filter(t => t > cutoff);
    return Math.max(0, this.maxRequests - timestamps.length);
  }
}