/**
 * Main application entry point
 * Vanilla JavaScript - no frameworks, no jsconfig
 */

import { Router } from './modules/Router.js';
import { Store, combineReducers, thunkMiddleware } from './modules/Store.js';
import { Component } from './modules/Component.js';
import { $, createElement, html } from './helpers/dom.js';
import { get, createClient } from './helpers/http.js';
import { formatRelative, pluralize } from './helpers/format.js';

// Reducers
function todosReducer(state = [], action) {
  switch (action.type) {
    case 'ADD_TODO':
      return [...state, { id: Date.now(), text: action.text, completed: false }];
    case 'TOGGLE_TODO':
      return state.map((todo) =>
        todo.id === action.id ? { ...todo, completed: !todo.completed } : todo
      );
    case 'REMOVE_TODO':
      return state.filter((todo) => todo.id !== action.id);
    case 'SET_TODOS':
      return action.todos;
    default:
      return state;
  }
}

function filterReducer(state = 'all', action) {
  switch (action.type) {
    case 'SET_FILTER':
      return action.filter;
    default:
      return state;
  }
}

// Store
const store = new Store(
  combineReducers({
    todos: todosReducer,
    filter: filterReducer,
  }),
  { todos: [], filter: 'all' }
).applyMiddleware(thunkMiddleware);

// API client
const api = createClient('/api');

// Async actions
function fetchTodos() {
  return async (dispatch) => {
    try {
      const todos = await api.get('/todos');
      dispatch({ type: 'SET_TODOS', todos });
    } catch (error) {
      console.error('Failed to fetch todos:', error);
    }
  };
}

// Components
class TodoItem extends Component {
  init() {
    this.delegate('click', '.toggle', () => {
      store.dispatch({ type: 'TOGGLE_TODO', id: this.props.todo.id });
    });

    this.delegate('click', '.remove', () => {
      store.dispatch({ type: 'REMOVE_TODO', id: this.props.todo.id });
    });
  }

  render() {
    const { todo } = this.props;
    const completedClass = todo.completed ? 'completed' : '';

    return `
      <li class="todo-item ${completedClass}" data-id="${todo.id}">
        <button class="toggle" aria-label="Toggle todo">
          ${todo.completed ? '[x]' : '[ ]'}
        </button>
        <span class="text">${todo.text}</span>
        <button class="remove" aria-label="Remove todo">x</button>
      </li>
    `;
  }
}

class TodoList extends Component {
  init() {
    store.subscribe(() => this.update());

    this.delegate('submit', '.add-form', (e) => {
      e.preventDefault();
      const input = this.$('.add-input');
      const text = input.value.trim();

      if (text) {
        store.dispatch({ type: 'ADD_TODO', text });
        input.value = '';
      }
    });

    this.delegate('click', '.filter-btn', (e) => {
      const filter = e.target.dataset.filter;
      store.dispatch({ type: 'SET_FILTER', filter });
    });
  }

  getFilteredTodos() {
    const { todos, filter } = store.getState();

    switch (filter) {
      case 'active':
        return todos.filter((t) => !t.completed);
      case 'completed':
        return todos.filter((t) => t.completed);
      default:
        return todos;
    }
  }

  render() {
    const { filter } = store.getState();
    const todos = this.getFilteredTodos();
    const remaining = store.getState().todos.filter((t) => !t.completed).length;

    return `
      <div class="todo-app">
        <h1>Todos</h1>

        <form class="add-form">
          <input
            type="text"
            class="add-input"
            placeholder="What needs to be done?"
            autofocus
          />
          <button type="submit">Add</button>
        </form>

        <ul class="todo-list">
          ${todos.map((todo) => `
            <li class="todo-item ${todo.completed ? 'completed' : ''}" data-id="${todo.id}">
              <button class="toggle">${todo.completed ? '[x]' : '[ ]'}</button>
              <span class="text">${todo.text}</span>
              <button class="remove">x</button>
            </li>
          `).join('')}
        </ul>

        <footer class="todo-footer">
          <span>${remaining} ${pluralize(remaining, 'item')} left</span>

          <div class="filters">
            <button class="filter-btn ${filter === 'all' ? 'active' : ''}" data-filter="all">All</button>
            <button class="filter-btn ${filter === 'active' ? 'active' : ''}" data-filter="active">Active</button>
            <button class="filter-btn ${filter === 'completed' ? 'active' : ''}" data-filter="completed">Completed</button>
          </div>
        </footer>
      </div>
    `;
  }
}

// Router setup
const router = new Router({ mode: 'hash' });

router
  .addRoute('/', () => {
    const app = new TodoList($('#app'));
    app.update();
  })
  .addRoute('/about', () => {
    $('#app').innerHTML = `
      <div class="about">
        <h1>About</h1>
        <p>A vanilla JavaScript todo app demonstrating:</p>
        <ul>
          <li>Custom EventEmitter</li>
          <li>Redux-like Store</li>
          <li>Simple Component system</li>
          <li>Hash-based Router</li>
        </ul>
        <a href="#/">Back to todos</a>
      </div>
    `;
  });

// Initialize
document.addEventListener('DOMContentLoaded', () => {
  router.start();
});

export { store, router };
