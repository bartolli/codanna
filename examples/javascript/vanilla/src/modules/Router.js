import { EventEmitter } from './EventEmitter.js';

/**
 * Simple client-side router
 * Supports hash and history mode
 */
export class Router extends EventEmitter {
  constructor(options = {}) {
    super();
    this.routes = new Map();
    this.mode = options.mode || 'hash'; // 'hash' or 'history'
    this.base = options.base || '';
    this.currentRoute = null;
    this.params = {};

    this.init();
  }

  init() {
    if (this.mode === 'hash') {
      window.addEventListener('hashchange', () => this.resolve());
    } else {
      window.addEventListener('popstate', () => this.resolve());
      document.addEventListener('click', (e) => this.handleClick(e));
    }
  }

  handleClick(event) {
    const link = event.target.closest('a[href]');
    if (!link) return;

    const href = link.getAttribute('href');
    if (!href || href.startsWith('http') || href.startsWith('#')) return;

    event.preventDefault();
    this.navigate(href);
  }

  addRoute(path, handler) {
    const pattern = this.pathToRegex(path);
    this.routes.set(path, { pattern, handler, path });
    return this;
  }

  pathToRegex(path) {
    const pattern = path
      .replace(/\//g, '\\/')
      .replace(/:(\w+)/g, '(?<$1>[^\\/]+)')
      .replace(/\*/g, '.*');

    return new RegExp(`^${pattern}$`);
  }

  getPath() {
    if (this.mode === 'hash') {
      return window.location.hash.slice(1) || '/';
    }
    return window.location.pathname.replace(this.base, '') || '/';
  }

  resolve() {
    const path = this.getPath();

    for (const [routePath, route] of this.routes) {
      const match = path.match(route.pattern);
      if (match) {
        this.params = match.groups || {};
        this.currentRoute = routePath;

        try {
          route.handler(this.params);
          this.emit('navigate', { path, params: this.params, route: routePath });
        } catch (error) {
          console.error('Route handler error:', error);
          this.emit('error', error);
        }
        return true;
      }
    }

    // No route matched - trigger 404
    this.emit('notfound', { path });
    return false;
  }

  navigate(path, options = {}) {
    const fullPath = this.mode === 'hash' ? `#${path}` : `${this.base}${path}`;

    if (options.replace) {
      if (this.mode === 'hash') {
        window.location.replace(fullPath);
      } else {
        history.replaceState(null, '', fullPath);
      }
    } else {
      if (this.mode === 'hash') {
        window.location.hash = path;
      } else {
        history.pushState(null, '', fullPath);
      }
    }

    if (this.mode === 'history') {
      this.resolve();
    }
  }

  back() {
    history.back();
  }

  forward() {
    history.forward();
  }

  start() {
    this.resolve();
    return this;
  }
}

export default Router;
