import { EventEmitter } from './EventEmitter.js';

/**
 * Base component class for vanilla JS components
 * Provides lifecycle methods and template rendering
 */
export class Component extends EventEmitter {
  constructor(element, options = {}) {
    super();
    this.element = typeof element === 'string'
      ? document.querySelector(element)
      : element;

    if (!this.element) {
      throw new Error('Component element not found');
    }

    this.state = options.state || {};
    this.props = options.props || {};
    this.children = new Map();

    this.init();
  }

  init() {
    // Override in subclass
  }

  setState(partial) {
    const prevState = { ...this.state };
    this.state = { ...this.state, ...partial };

    if (this.shouldUpdate(prevState, this.state)) {
      this.update();
      this.emit('statechange', this.state, prevState);
    }
  }

  shouldUpdate(prevState, nextState) {
    return JSON.stringify(prevState) !== JSON.stringify(nextState);
  }

  render() {
    // Override in subclass - return HTML string
    return '';
  }

  update() {
    const html = this.render();
    this.element.innerHTML = html;
    this.afterUpdate();
  }

  afterUpdate() {
    // Override in subclass - bind events, etc.
  }

  mount(container) {
    const parent = typeof container === 'string'
      ? document.querySelector(container)
      : container;

    if (parent) {
      parent.appendChild(this.element);
      this.update();
      this.emit('mount');
    }
    return this;
  }

  unmount() {
    this.emit('beforeunmount');
    this.children.forEach((child) => child.unmount());
    this.children.clear();
    this.element.remove();
    this.removeAllListeners();
    this.emit('unmount');
  }

  addChild(key, child) {
    this.children.set(key, child);
    return child;
  }

  getChild(key) {
    return this.children.get(key);
  }

  removeChild(key) {
    const child = this.children.get(key);
    if (child) {
      child.unmount();
      this.children.delete(key);
    }
  }

  $(selector) {
    return this.element.querySelector(selector);
  }

  $$(selector) {
    return Array.from(this.element.querySelectorAll(selector));
  }

  delegate(eventType, selector, handler) {
    this.element.addEventListener(eventType, (event) => {
      const target = event.target.closest(selector);
      if (target && this.element.contains(target)) {
        handler.call(target, event, target);
      }
    });
  }
}

export default Component;
