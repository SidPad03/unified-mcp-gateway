import { useCallback, useEffect, useState } from 'react';

export type ThemeMode = 'light' | 'dark' | 'system';
export type Resolved = 'light' | 'dark';

const KEY = 'mcpgw_theme';
const query = () => window.matchMedia('(prefers-color-scheme: dark)');

function readMode(): ThemeMode {
  try {
    const v = localStorage.getItem(KEY);
    return v === 'light' || v === 'dark' ? v : 'system';
  } catch {
    return 'system';
  }
}

function resolve(mode: ThemeMode): Resolved {
  if (mode !== 'system') return mode;
  return query().matches ? 'dark' : 'light';
}

/**
 * The theme, as an explicit `data-theme` on <html>.
 *
 * index.html sets the attribute before first paint; this hook only keeps it in
 * step afterwards. Resolving "system" here rather than in CSS means the
 * stylesheet never has to carry a duplicate `prefers-color-scheme` block, and
 * it means "system" keeps tracking the OS live — a stored 'dark' does not.
 */
export function useTheme() {
  const [mode, setModeState] = useState<ThemeMode>(readMode);
  const [resolved, setResolved] = useState<Resolved>(() => resolve(readMode()));

  const apply = useCallback((next: ThemeMode) => {
    const r = resolve(next);
    document.documentElement.dataset.theme = r;
    setResolved(r);
  }, []);

  const setMode = useCallback(
    (next: ThemeMode) => {
      setModeState(next);
      try {
        if (next === 'system') localStorage.removeItem(KEY);
        else localStorage.setItem(KEY, next);
      } catch {
        /* storage disabled — the theme still applies for this session */
      }
      apply(next);
    },
    [apply]
  );

  // Only while following the system: a stored preference must win over the OS.
  useEffect(() => {
    if (mode !== 'system') return;
    const mq = query();
    const onChange = () => apply('system');
    mq.addEventListener('change', onChange);
    return () => mq.removeEventListener('change', onChange);
  }, [mode, apply]);

  const cycle = useCallback(() => {
    setMode(mode === 'system' ? 'light' : mode === 'light' ? 'dark' : 'system');
  }, [mode, setMode]);

  return { mode, resolved, setMode, cycle };
}
