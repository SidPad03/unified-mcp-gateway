import { ReactNode, useEffect, useState } from 'react';
import { NavLink, useLocation } from 'react-router-dom';
import {
  BarChart3,
  Download,
  LogOut,
  Menu,
  Monitor,
  Moon,
  Network,
  ScrollText,
  Server,
  Settings,
  Shield,
  Sun,
  Users,
  Wrench,
  X,
} from 'lucide-react';
import clsx from 'clsx';
import { BrandLockup } from '@/components/BrandMark';
import { IconButton } from '@/components/ui';
import { useTheme } from '@/hooks/useTheme';
import { useUpdateCheck } from '@/hooks/useUpdateCheck';

interface Props {
  children: ReactNode;
  auth: {
    user: { username: string; roles: string[] } | null;
    logout: () => void;
    isAdmin: boolean;
  };
}

/**
 * Navigation is the product, not scaffolding — it is the claim about what this
 * thing is for. Three groups, in the order the questions actually get asked:
 * what is connected and what may it do; what went through; who runs it.
 */
const GROUPS = [
  {
    label: 'Gateway',
    items: [
      { to: '/backends', icon: Server, label: 'Backends', admin: false },
      { to: '/tools', icon: Wrench, label: 'Tools', admin: true },
      { to: '/policies', icon: Shield, label: 'Policies', admin: true },
    ],
  },
  {
    label: 'Traffic',
    items: [
      { to: '/usage', icon: Network, label: 'Usage', admin: false },
      { to: '/audit', icon: ScrollText, label: 'Audit', admin: true },
      { to: '/metrics', icon: BarChart3, label: 'Metrics', admin: true },
    ],
  },
  {
    label: 'Administration',
    items: [
      { to: '/users', icon: Users, label: 'Users', admin: true },
      { to: '/settings', icon: Settings, label: 'Settings', admin: true },
    ],
  },
] as const;

/**
 * Pages that own their whole area and supply their own padding.
 *
 * The alternative — a page clawing the shell's padding back with a negative
 * margin — is what the usage graph used to do (`-m-8` against the old `p-8`),
 * and it silently broke the moment the shell's padding changed: 4px of bleed on
 * desktop, 12px on mobile. The shell decides, and it says so here.
 */
const FULL_BLEED = new Set(['/usage']);

export default function Layout({ children, auth }: Props) {
  const location = useLocation();
  const [navOpen, setNavOpen] = useState(false);
  const bleed = FULL_BLEED.has(location.pathname);

  // The off-canvas nav is a navigation surface, so a navigation closes it.
  useEffect(() => setNavOpen(false), [location.pathname]);

  useEffect(() => {
    if (!navOpen) return;
    const onKey = (e: KeyboardEvent) => e.key === 'Escape' && setNavOpen(false);
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [navOpen]);

  return (
    // dvh, not vh: on mobile browsers `100vh` includes the collapsing URL bar,
    // so a vh-sized shell puts its own bottom edge off-screen. A flex column
    // with `min-h-0` on the scroller also avoids doing viewport arithmetic by
    // hand — the top bar simply takes its height and the content gets the rest.
    <div className="flex flex-col lg:flex-row h-dvh overflow-hidden">
      {/* Narrow screens: a slim bar with the mark and a way into the nav. */}
      <div className="lg:hidden flex items-center gap-3 h-12 shrink-0 px-3 border-b border-line bg-void">
        <IconButton icon={Menu} label="Open navigation" onClick={() => setNavOpen(true)} />
        <BrandLockup size={20} />
      </div>

      {navOpen && (
        <div
          className="lg:hidden fixed inset-0 z-40 bg-scrim"
          onClick={() => setNavOpen(false)}
          aria-hidden="true"
        />
      )}

      <Sidebar auth={auth} open={navOpen} onClose={() => setNavOpen(false)} />

      {/* The sidebar shares the canvas colour — one plane, divided by a hairline.
          Giving it its own fill would split the app into two worlds. */}
      <main className="flex-1 min-h-0 min-w-0 overflow-y-auto">
        {bleed ? (
          children
        ) : (
          <div className="px-4 py-5 sm:px-5 sm:py-6 lg:px-7 lg:py-7">{children}</div>
        )}
      </main>
    </div>
  );
}

function Sidebar({
  auth,
  open,
  onClose,
}: {
  auth: Props['auth'];
  open: boolean;
  onClose: () => void;
}) {
  const groups = GROUPS.map(g => ({
    ...g,
    items: g.items.filter(i => !i.admin || auth.isAdmin),
  })).filter(g => g.items.length > 0);

  return (
    <aside
      className={clsx(
        'w-[232px] shrink-0 bg-void border-r border-line flex flex-col',
        // 232 next to an unbounded content column says navigation serves
        // content. A 320 would say they are peers, and they are not.
        'max-lg:fixed max-lg:inset-y-0 max-lg:left-0 max-lg:z-50 max-lg:transition-transform',
        'max-lg:duration-200 max-lg:ease-[var(--ease-out-quint)]',
        open ? 'max-lg:translate-x-0' : 'max-lg:-translate-x-full'
      )}
    >
      <div className="flex items-center justify-between gap-2 h-14 px-4 shrink-0">
        <BrandLockup size={22} />
        <IconButton icon={X} label="Close navigation" onClick={onClose} className="lg:hidden" />
      </div>

      <nav className="flex-1 overflow-y-auto px-2 pb-2">
        {groups.map((group, i) => (
          <div key={group.label} className={i > 0 ? 'mt-5' : ''}>
            <div className="px-3 mb-1.5 text-micro font-semibold uppercase tracking-[0.16em] text-ink-4">
              {group.label}
            </div>
            <div className="space-y-px">
              {group.items.map(item => (
                <NavLink
                  key={item.to}
                  to={item.to}
                  className={({ isActive }) =>
                    clsx(
                      'relative flex items-center gap-2.5 h-8 pl-3 pr-2.5 rounded-row',
                      'text-xs font-medium select-none',
                      'transition-[color,background-color] duration-150 ease-[var(--ease-out-quint)]',
                      isActive
                        ? 'text-ink bg-raised'
                        : 'text-ink-3 hover:text-ink-2 hover:bg-raised'
                    )
                  }
                >
                  {({ isActive }) => (
                    <>
                      {/* The gate rail, at its smallest: the active marker is a
                          2px lit edge, not a filled pill. Same idea as the rail
                          on an audit row, one scale down. */}
                      {isActive && (
                        <span className="absolute left-0 top-1/2 -translate-y-1/2 h-4 w-[2px] rounded-full bg-beam" />
                      )}
                      <item.icon className="w-4 h-4 shrink-0" />
                      <span className="truncate">{item.label}</span>
                    </>
                  )}
                </NavLink>
              ))}
            </div>
          </div>
        ))}
      </nav>

      <div className="shrink-0 border-t border-line px-3 py-2.5">
        <UpdateNotice isAdmin={auth.isAdmin} />
        <div className="flex items-center gap-2">
          <div className="min-w-0 flex-1">
            <p className="text-xs font-medium text-ink truncate">{auth.user?.username}</p>
            <p className="text-micro text-ink-4 truncate">
              {auth.user?.roles?.join(', ') || 'no role'}
            </p>
          </div>
          <ThemeToggle />
          <IconButton icon={LogOut} label="Sign out" onClick={auth.logout} />
        </div>
      </div>
    </aside>
  );
}

/**
 * The one place the dashboard says a release exists.
 *
 * Deliberately the quietest thing in the sidebar: 10px, one line, and absent
 * entirely when there is nothing to say. Settings owns the detail — the notes,
 * the release link, and the button that re-checks — so this only has to be
 * noticed and clicked.
 *
 * Owners only, because `/settings` is an owner route: anyone else would get a
 * notice they cannot act on behind a link that redirects them away.
 */
function UpdateNotice({ isAdmin }: { isAdmin: boolean }) {
  const { available, latestVersion } = useUpdateCheck(isAdmin);
  if (!isAdmin || !available) return null;

  return (
    <NavLink
      to="/settings"
      title={latestVersion ? `Version ${latestVersion} is available` : undefined}
      className="flex items-center gap-1.5 mb-2 text-micro font-medium text-beam transition-opacity hover:opacity-75"
    >
      <Download className="w-3 h-3 shrink-0" />
      <span className="truncate">Update available</span>
    </NavLink>
  );
}

/**
 * system → light → dark → system. The label says which, because three states on
 * one button are not guessable from an icon alone.
 */
export function ThemeToggle() {
  const { mode, cycle } = useTheme();
  const Icon = mode === 'system' ? Monitor : mode === 'light' ? Sun : Moon;
  const next = mode === 'system' ? 'light' : mode === 'light' ? 'dark' : 'system';

  return (
    <IconButton
      icon={Icon}
      label={`Theme: ${mode}. Switch to ${next}.`}
      onClick={cycle}
    />
  );
}
