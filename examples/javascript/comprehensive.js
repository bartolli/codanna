// JavaScript comprehensive example for parser testing and audit reports
// Covers: classes, functions, imports, exports, JSX, JSDoc, modern syntax

import React, { useState, useEffect, useCallback } from 'react';
import axios from 'axios';
import * as utils from './utils';
import './styles.css';

// ============================================================================
// Constants and Variables
// ============================================================================

/**
 * API base URL for all requests
 * @type {string}
 */
export const API_URL = 'https://api.example.com';

const TIMEOUT_MS = 5000;

let requestCount = 0;

var legacyFlag = true;

// ============================================================================
// Classes
// ============================================================================

/**
 * Base service class for API interactions
 */
class BaseService {
    constructor(baseUrl) {
        this.baseUrl = baseUrl;
        this.headers = {
            'Content-Type': 'application/json'
        };
    }

    /**
     * Make an HTTP request
     * @param {string} endpoint - API endpoint
     * @param {Object} options - Request options
     * @returns {Promise<Object>} Response data
     */
    async request(endpoint, options = {}) {
        const url = `${this.baseUrl}${endpoint}`;
        const response = await axios({
            url,
            headers: this.headers,
            timeout: TIMEOUT_MS,
            ...options
        });
        requestCount++;
        return response.data;
    }
}

/**
 * User service extending BaseService
 * Handles user-related API operations
 */
export class UserService extends BaseService {
    constructor() {
        super(API_URL);
    }

    /**
     * Get user by ID
     * @param {number} id - User ID
     * @returns {Promise<Object>} User data
     */
    async getUser(id) {
        return this.request(`/users/${id}`);
    }

    /**
     * Update user profile
     * @param {number} id - User ID
     * @param {Object} data - Profile data
     */
    async updateUser(id, data) {
        return this.request(`/users/${id}`, {
            method: 'PUT',
            data
        });
    }

    /**
     * Delete user
     * @param {number} id - User ID
     */
    deleteUser(id) {
        return this.request(`/users/${id}`, { method: 'DELETE' });
    }
}

// ============================================================================
// Functions
// ============================================================================

/**
 * Format a date for display
 * @param {Date} date - Date to format
 * @returns {string} Formatted date string
 */
export function formatDate(date) {
    return date.toLocaleDateString('en-US', {
        year: 'numeric',
        month: 'long',
        day: 'numeric'
    });
}

/**
 * Debounce function execution
 * @param {Function} fn - Function to debounce
 * @param {number} delay - Delay in milliseconds
 * @returns {Function} Debounced function
 */
function debounce(fn, delay) {
    let timeoutId;
    return function (...args) {
        clearTimeout(timeoutId);
        timeoutId = setTimeout(() => fn.apply(this, args), delay);
    };
}

// Arrow function with implicit return
const double = (x) => x * 2;

// Arrow function with block body
const fetchData = async (url) => {
    const response = await axios.get(url);
    return response.data;
};

// Generator function
function* idGenerator() {
    let id = 0;
    while (true) {
        yield id++;
    }
}

// Async generator
async function* asyncDataStream(urls) {
    for (const url of urls) {
        yield await fetchData(url);
    }
}

// ============================================================================
// React Components (JSX)
// ============================================================================

/**
 * Button component with click handler
 * @param {Object} props - Component props
 * @param {string} props.label - Button label
 * @param {Function} props.onClick - Click handler
 * @param {boolean} [props.disabled] - Disabled state
 */
export function Button({ label, onClick, disabled = false }) {
    return (
        <button
            className="btn"
            onClick={onClick}
            disabled={disabled}
        >
            {label}
        </button>
    );
}

/**
 * User profile component
 * Displays user information and handles updates
 */
export function UserProfile({ userId }) {
    const [user, setUser] = useState(null);
    const [loading, setLoading] = useState(true);
    const [error, setError] = useState(null);

    const userService = new UserService();

    useEffect(() => {
        let mounted = true;

        async function loadUser() {
            try {
                const data = await userService.getUser(userId);
                if (mounted) {
                    setUser(data);
                    setLoading(false);
                }
            } catch (err) {
                if (mounted) {
                    setError(err.message);
                    setLoading(false);
                }
            }
        }

        loadUser();

        return () => {
            mounted = false;
        };
    }, [userId]);

    const handleRefresh = useCallback(() => {
        setLoading(true);
        userService.getUser(userId)
            .then(setUser)
            .catch(err => setError(err.message))
            .finally(() => setLoading(false));
    }, [userId]);

    if (loading) return <div>Loading...</div>;
    if (error) return <div>Error: {error}</div>;
    if (!user) return <div>No user found</div>;

    return (
        <div className="user-profile">
            <h1>{user.name}</h1>
            <p>{user.email}</p>
            <Button label="Refresh" onClick={handleRefresh} />
        </div>
    );
}

/**
 * Main App component
 */
export default function App() {
    const [selectedUserId, setSelectedUserId] = useState(1);

    return (
        <div className="app">
            <header>
                <h1>User Management</h1>
            </header>
            <main>
                <UserProfile userId={selectedUserId} />
            </main>
        </div>
    );
}

// ============================================================================
// Hoisting examples
// ============================================================================

// Function declaration (hoisted)
function hoistedFunction() {
    return 'I am hoisted';
}

// Variable hoisting with var
console.log(hoistedVar); // undefined (hoisted but not initialized)
var hoistedVar = 'hoisted';

// ============================================================================
// Module exports
// ============================================================================

export { debounce, double, fetchData };
export { idGenerator as createIdGenerator };
