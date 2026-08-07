import { useCallback, useEffect, useSyncExternalStore } from 'react';
import { api, UpdateStatus } from '@/lib/api';

/**
 * One update check, shared by everything that shows the result.
 *
 * The sidebar footer has to know on every page load, and the Settings panel has
 * to be able to force a fresh check on demand. Two independent `useState`s would
 * let the footer keep claiming an update after Settings had just confirmed there
 * wasn't one, so the result lives in a module-level store instead.
 *
 * **Why an automatic check is affordable now.** The old rule here was "explicit
 * click only", to avoid every browser spending from GitHub's 60-per-hour
 * unauthenticated IP budget. That budget is not the browser's to spend: the
 * check goes through the gateway, which caches the upstream call for 30 minutes
 * and serves every operator from it (see `api/updates.rs`). What is left is one
 * cheap same-origin request, throttled to six hours here because a page load is
 * not new information.
 */
const CACHE_KEY = 'mcpgw_update_status';
const MAX_AGE_MS = 6 * 60 * 60 * 1000;

interface Entry {
  status: UpdateStatus;
  checkedAt: number;
}

interface Store {
  entry: Entry | null;
  checking: boolean;
}

function readCache(): Entry | null {
  try {
    const raw = localStorage.getItem(CACHE_KEY);
    if (!raw) return null;
    const parsed = JSON.parse(raw) as Entry;
    // A cache written by an older build may not have the shape we expect.
    return parsed && parsed.status && typeof parsed.checkedAt === 'number' ? parsed : null;
  } catch {
    return null;
  }
}

// Replaced wholesale rather than mutated: `useSyncExternalStore` compares by
// identity, so an in-place edit would not re-render anything.
let store: Store = { entry: readCache(), checking: false };
const listeners = new Set<() => void>();

function set(next: Store) {
  store = next;
  listeners.forEach(notify => notify());
}

function subscribe(notify: () => void) {
  listeners.add(notify);
  return () => {
    listeners.delete(notify);
  };
}

const getSnapshot = () => store;

async function check(force: boolean): Promise<void> {
  if (store.checking) return;
  set({ ...store, checking: true });

  let entry: Entry;
  try {
    entry = { status: await api.checkForUpdates(__APP_VERSION__, force), checkedAt: Date.now() };
  } catch (e: any) {
    // The endpoint returns 200 with `error` set when GitHub is unreachable, so
    // landing here means the gateway itself did not answer. Either way the
    // distinction that matters is "could not find out" vs "up to date".
    entry = {
      status: {
        current_version: __APP_VERSION__,
        update_available: false,
        checked_at: new Date().toISOString(),
        source_repo: '',
        error: e?.message || 'Could not check for updates',
      },
      checkedAt: Date.now(),
    };
  }

  try {
    localStorage.setItem(CACHE_KEY, JSON.stringify(entry));
  } catch {
    /* private mode, or storage full — the in-memory value still serves this session */
  }
  set({ entry, checking: false });
}

/**
 * @param auto Check on mount if the cached answer is older than six hours.
 *   Passive callers pass `true`; Settings drives its own cache-busting check
 *   from the button and leaves this off.
 */
export function useUpdateCheck(auto = false) {
  const { entry, checking } = useSyncExternalStore(subscribe, getSnapshot, getSnapshot);

  useEffect(() => {
    if (!auto || checking) return;
    if (entry && Date.now() - entry.checkedAt < MAX_AGE_MS) return;
    void check(false);
  }, [auto, checking, entry]);

  const refresh = useCallback(() => check(true), []);
  const status = entry?.status ?? null;

  return {
    status,
    /** Never true when the check itself failed — `error` and `update_available` are different answers. */
    available: !!status?.update_available && !status.error,
    latestVersion: status?.latest_version,
    checking,
    refresh,
  };
}
