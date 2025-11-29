const PREFIX = 'app_';

function getKey(key) {
  return `${PREFIX}${key}`;
}

export const storage = {
  get(key, defaultValue = null) {
    try {
      const item = localStorage.getItem(getKey(key));
      if (item === null) {
        return defaultValue;
      }
      return JSON.parse(item);
    } catch (error) {
      console.error(`Error reading from storage: ${key}`, error);
      return defaultValue;
    }
  },

  set(key, value) {
    try {
      localStorage.setItem(getKey(key), JSON.stringify(value));
      return true;
    } catch (error) {
      console.error(`Error writing to storage: ${key}`, error);
      return false;
    }
  },

  remove(key) {
    try {
      localStorage.removeItem(getKey(key));
      return true;
    } catch (error) {
      console.error(`Error removing from storage: ${key}`, error);
      return false;
    }
  },

  clear() {
    try {
      const keys = Object.keys(localStorage).filter(k => k.startsWith(PREFIX));
      keys.forEach(k => localStorage.removeItem(k));
      return true;
    } catch (error) {
      console.error('Error clearing storage', error);
      return false;
    }
  },
};

export const sessionStorage = {
  get(key, defaultValue = null) {
    try {
      const item = window.sessionStorage.getItem(getKey(key));
      if (item === null) {
        return defaultValue;
      }
      return JSON.parse(item);
    } catch (error) {
      console.error(`Error reading from session storage: ${key}`, error);
      return defaultValue;
    }
  },

  set(key, value) {
    try {
      window.sessionStorage.setItem(getKey(key), JSON.stringify(value));
      return true;
    } catch (error) {
      console.error(`Error writing to session storage: ${key}`, error);
      return false;
    }
  },
};
