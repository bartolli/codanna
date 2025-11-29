import { EventEmitter } from './EventEmitter.js';

/**
 * Simple state management store
 * Redux-inspired but minimal
 */
export class Store extends EventEmitter {
  constructor(reducer, initialState = {}) {
    super();
    this.reducer = reducer;
    this.state = initialState;
    this.middlewares = [];
  }

  getState() {
    return this.state;
  }

  dispatch(action) {
    // Run middlewares
    const middlewareChain = this.middlewares.reduceRight(
      (next, middleware) => middleware(this)(next),
      (action) => {
        this.state = this.reducer(this.state, action);
        this.emit('change', this.state, action);
      }
    );

    middlewareChain(action);
    return action;
  }

  subscribe(listener) {
    return this.on('change', listener);
  }

  applyMiddleware(...middlewares) {
    this.middlewares = middlewares;
    return this;
  }

  replaceReducer(nextReducer) {
    this.reducer = nextReducer;
    this.dispatch({ type: '@@REPLACE' });
  }
}

/**
 * Combine multiple reducers into one
 */
export function combineReducers(reducers) {
  const keys = Object.keys(reducers);

  return function combination(state = {}, action) {
    const nextState = {};
    let hasChanged = false;

    for (const key of keys) {
      const reducer = reducers[key];
      const previousStateForKey = state[key];
      const nextStateForKey = reducer(previousStateForKey, action);

      nextState[key] = nextStateForKey;
      hasChanged = hasChanged || nextStateForKey !== previousStateForKey;
    }

    return hasChanged ? nextState : state;
  };
}

/**
 * Logger middleware
 */
export function loggerMiddleware(store) {
  return (next) => (action) => {
    console.group(action.type);
    console.log('prev state:', store.getState());
    console.log('action:', action);
    next(action);
    console.log('next state:', store.getState());
    console.groupEnd();
  };
}

/**
 * Thunk middleware for async actions
 */
export function thunkMiddleware(store) {
  return (next) => (action) => {
    if (typeof action === 'function') {
      return action(store.dispatch, store.getState);
    }
    return next(action);
  };
}

export default Store;
