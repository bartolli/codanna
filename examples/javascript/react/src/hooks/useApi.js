import { useState, useCallback, useRef, useEffect } from 'react';

export function useApi(apiFunction) {
  const [data, setData] = useState(null);
  const [error, setError] = useState(null);
  const [loading, setLoading] = useState(false);
  const mountedRef = useRef(true);

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
    };
  }, []);

  const execute = useCallback(async (...args) => {
    setLoading(true);
    setError(null);

    try {
      const result = await apiFunction(...args);
      if (mountedRef.current) {
        setData(result);
        return result;
      }
    } catch (err) {
      if (mountedRef.current) {
        setError(err);
        throw err;
      }
    } finally {
      if (mountedRef.current) {
        setLoading(false);
      }
    }
  }, [apiFunction]);

  const reset = useCallback(() => {
    setData(null);
    setError(null);
    setLoading(false);
  }, []);

  return {
    data,
    error,
    loading,
    execute,
    reset,
  };
}

export function useLazyQuery(apiFunction) {
  const { data, error, loading, execute, reset } = useApi(apiFunction);

  return {
    data,
    error,
    loading,
    fetch: execute,
    refetch: execute,
    reset,
  };
}

export function useMutation(apiFunction, options = {}) {
  const { onSuccess, onError } = options;
  const { data, error, loading, execute, reset } = useApi(apiFunction);

  const mutate = useCallback(async (...args) => {
    try {
      const result = await execute(...args);
      if (onSuccess) {
        onSuccess(result);
      }
      return result;
    } catch (err) {
      if (onError) {
        onError(err);
      }
      throw err;
    }
  }, [execute, onSuccess, onError]);

  return {
    data,
    error,
    loading,
    mutate,
    reset,
  };
}
