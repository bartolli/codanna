/**
 * Simple EventEmitter implementation
 * No dependencies, pure JavaScript
 */
export class EventEmitter {
  constructor() {
    this.events = new Map();
  }

  on(event, listener) {
    if (!this.events.has(event)) {
      this.events.set(event, new Set());
    }
    this.events.get(event).add(listener);

    // Return unsubscribe function
    return () => this.off(event, listener);
  }

  once(event, listener) {
    const wrapper = (...args) => {
      this.off(event, wrapper);
      listener.apply(this, args);
    };
    wrapper.originalListener = listener;
    return this.on(event, wrapper);
  }

  off(event, listener) {
    const listeners = this.events.get(event);
    if (!listeners) return false;

    // Handle wrapped listeners from once()
    for (const l of listeners) {
      if (l === listener || l.originalListener === listener) {
        listeners.delete(l);
        return true;
      }
    }
    return false;
  }

  emit(event, ...args) {
    const listeners = this.events.get(event);
    if (!listeners || listeners.size === 0) {
      return false;
    }

    for (const listener of listeners) {
      try {
        listener.apply(this, args);
      } catch (error) {
        console.error(`Error in event listener for "${event}":`, error);
      }
    }
    return true;
  }

  removeAllListeners(event) {
    if (event) {
      this.events.delete(event);
    } else {
      this.events.clear();
    }
  }

  listenerCount(event) {
    const listeners = this.events.get(event);
    return listeners ? listeners.size : 0;
  }

  eventNames() {
    return Array.from(this.events.keys());
  }
}

export default EventEmitter;
