import { useState, useCallback, useEffect } from 'react';
import { api, User } from '@/lib/api';

export function useAuth() {
  const [user, setUser] = useState<User | null>(() => {
    const stored = localStorage.getItem('mcpgw_user');
    return stored ? JSON.parse(stored) : null;
  });
  const [isLoading, setIsLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const login = useCallback(async (username: string, password: string) => {
    setIsLoading(true);
    setError(null);
    try {
      const res = await api.login(username, password);
      localStorage.setItem('mcpgw_token', res.token);
      localStorage.setItem('mcpgw_user', JSON.stringify(res.user));
      setUser(res.user);
      return true;
    } catch (e: any) {
      setError(e.message || 'Login failed');
      return false;
    } finally {
      setIsLoading(false);
    }
  }, []);

  const logout = useCallback(() => {
    localStorage.removeItem('mcpgw_token');
    localStorage.removeItem('mcpgw_user');
    setUser(null);
  }, []);

  const isAdmin = user?.roles?.includes('owner') ?? false;

  return { user, login, logout, isLoading, error, isAdmin };
}
