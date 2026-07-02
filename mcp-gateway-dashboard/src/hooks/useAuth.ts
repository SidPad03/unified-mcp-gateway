import { useState, useCallback, useEffect } from 'react';
import { api, User } from '@/lib/api';

export function useAuth() {
  const [user, setUser] = useState<User | null>(() => {
    const stored = localStorage.getItem('mcpgw_user');
    if (!stored) return null;
    try {
      return JSON.parse(stored);
    } catch {
      // Corrupt/partial value in storage — drop it instead of crashing the app
      // (an unhandled throw here white-screens the whole dashboard on load).
      localStorage.removeItem('mcpgw_user');
      localStorage.removeItem('mcpgw_token');
      return null;
    }
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

  // Clear the first-login "must change password" flag once the user has set a
  // new password, persisting the change so a page reload doesn't re-prompt.
  const completePasswordChange = useCallback(() => {
    setUser(prev => {
      if (!prev) return prev;
      const updated = { ...prev, must_change_password: false };
      localStorage.setItem('mcpgw_user', JSON.stringify(updated));
      return updated;
    });
  }, []);

  const isAdmin = user?.roles?.includes('owner') ?? false;
  const mustChangePassword = user?.must_change_password ?? false;

  return { user, login, logout, isLoading, error, isAdmin, mustChangePassword, completePasswordChange };
}
