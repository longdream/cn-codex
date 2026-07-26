export class EventEmitter {
  constructor() {
    this._events = new Map();
  }

  on(event, listener) {
    if (!this._events.has(event)) {
      this._events.set(event, []);
    }
    this._events.get(event).push(listener);
  }

  once(event, listener) {
    const wrapper = (...args) => {
      this.off(event, wrapper);
      listener(...args);
    };
    this.on(event, wrapper);
  }

  off(event, listener) {
    const listeners = this._events.get(event);
    if (!listeners) return;
    const idx = listeners.indexOf(listener);
    if (idx !== -1) listeners.splice(idx, 1);
  }

  emit(event, ...args) {
    const listeners = this._events.get(event);
    if (!listeners) return;
    for (const listener of [...listeners]) {
      listener(...args);
    }
  }
}