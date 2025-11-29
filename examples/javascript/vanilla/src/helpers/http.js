/**
 * HTTP request helpers
 * Fetch wrapper with common patterns
 */

const DEFAULT_HEADERS = {
  'Content-Type': 'application/json',
};

export class HttpError extends Error {
  constructor(message, status, data) {
    super(message);
    this.name = 'HttpError';
    this.status = status;
    this.data = data;
  }
}

async function handleResponse(response) {
  const contentType = response.headers.get('content-type') || '';

  let data;
  if (contentType.includes('application/json')) {
    data = await response.json();
  } else if (contentType.includes('text/')) {
    data = await response.text();
  } else {
    data = await response.blob();
  }

  if (!response.ok) {
    throw new HttpError(
      data.message || response.statusText,
      response.status,
      data
    );
  }

  return data;
}

function buildUrl(url, params) {
  if (!params || Object.keys(params).length === 0) {
    return url;
  }

  const searchParams = new URLSearchParams();
  for (const [key, value] of Object.entries(params)) {
    if (value !== undefined && value !== null) {
      if (Array.isArray(value)) {
        value.forEach((v) => searchParams.append(key, v));
      } else {
        searchParams.append(key, value);
      }
    }
  }

  const separator = url.includes('?') ? '&' : '?';
  return `${url}${separator}${searchParams.toString()}`;
}

export async function request(url, options = {}) {
  const {
    method = 'GET',
    headers = {},
    body,
    params,
    timeout = 30000,
    ...rest
  } = options;

  const finalUrl = buildUrl(url, params);

  const controller = new AbortController();
  const timeoutId = setTimeout(() => controller.abort(), timeout);

  try {
    const response = await fetch(finalUrl, {
      method,
      headers: { ...DEFAULT_HEADERS, ...headers },
      body: body ? JSON.stringify(body) : undefined,
      signal: controller.signal,
      ...rest,
    });

    return await handleResponse(response);
  } finally {
    clearTimeout(timeoutId);
  }
}

export function get(url, params, options = {}) {
  return request(url, { ...options, method: 'GET', params });
}

export function post(url, body, options = {}) {
  return request(url, { ...options, method: 'POST', body });
}

export function put(url, body, options = {}) {
  return request(url, { ...options, method: 'PUT', body });
}

export function patch(url, body, options = {}) {
  return request(url, { ...options, method: 'PATCH', body });
}

export function del(url, options = {}) {
  return request(url, { ...options, method: 'DELETE' });
}

/**
 * Create an API client with base URL
 */
export function createClient(baseUrl, defaultOptions = {}) {
  const makeRequest = (method) => (path, bodyOrParams, options = {}) => {
    const url = `${baseUrl}${path}`;
    const merged = { ...defaultOptions, ...options };

    if (method === 'GET') {
      return request(url, { ...merged, method, params: bodyOrParams });
    }
    return request(url, { ...merged, method, body: bodyOrParams });
  };

  return {
    get: makeRequest('GET'),
    post: makeRequest('POST'),
    put: makeRequest('PUT'),
    patch: makeRequest('PATCH'),
    delete: (path, options) => request(`${baseUrl}${path}`, { ...defaultOptions, ...options, method: 'DELETE' }),
  };
}
