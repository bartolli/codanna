import { apiClient } from '@services/api';
import { validateEmail } from '@utils/validation';

const TOKEN_KEY = 'auth_token';

export const authService = {
  async login({ email, password }) {
    if (!validateEmail(email)) {
      throw new Error('Invalid email format');
    }

    const response = await apiClient.post('/auth/login', { email, password });

    if (response.token) {
      localStorage.setItem(TOKEN_KEY, response.token);
    }

    return response.user;
  },

  async logout() {
    try {
      await apiClient.post('/auth/logout');
    } finally {
      localStorage.removeItem(TOKEN_KEY);
    }
  },

  async refreshToken() {
    const token = localStorage.getItem(TOKEN_KEY);
    if (!token) {
      throw new Error('No token to refresh');
    }

    const response = await apiClient.post('/auth/refresh', { token });
    localStorage.setItem(TOKEN_KEY, response.token);
    return response.token;
  },

  getToken() {
    return localStorage.getItem(TOKEN_KEY);
  },

  isAuthenticated() {
    return !!this.getToken();
  },
};
