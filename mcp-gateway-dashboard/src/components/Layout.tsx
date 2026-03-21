import { ReactNode } from 'react';
import { NavLink, useLocation } from 'react-router-dom';
import {
  Wrench,
  ScrollText,
  BarChart3,
  Users,
  Shield,
  Server,
  LogOut,
  ChevronRight,
  Zap,
  Settings,
  Network,
} from 'lucide-react';
import clsx from 'clsx';

interface Props {
  children: ReactNode;
  auth: {
    user: { username: string; roles: string[] } | null;
    logout: () => void;
    isAdmin: boolean;
  };
}

const navItems = [
  { to: '/tools', icon: Wrench, label: 'Tools', description: 'Tool inventory', admin: true },
  { to: '/audit', icon: ScrollText, label: 'Audit', description: 'Audit timeline', admin: true },
  { to: '/metrics', icon: BarChart3, label: 'Metrics', description: 'Analytics', admin: true },
  { to: '/usage', icon: Network, label: 'Usage', description: 'Usage graph' },
  { to: '/backends', icon: Server, label: 'Backends', description: 'MCP servers' },
  { to: '/policies', icon: Shield, label: 'Policies', description: 'Security rules', admin: true },
  { to: '/users', icon: Users, label: 'Users', description: 'User management', admin: true },
  { to: '/settings', icon: Settings, label: 'Settings', description: 'App settings', admin: true },
];

export default function Layout({ children, auth }: Props) {
  const location = useLocation();

  return (
    <div className="flex h-screen overflow-hidden">
      {/* Sidebar */}
      <aside className="w-64 bg-surface border-r border-border flex flex-col shrink-0">
        {/* Logo */}
        <div className="p-5 border-b border-border">
          <div className="flex items-center gap-2.5">
            <div className="w-8 h-8 bg-accent/20 rounded-lg flex items-center justify-center">
              <Zap className="w-4.5 h-4.5 text-accent" />
            </div>
            <div>
              <h1 className="text-sm font-semibold tracking-tight text-white">MCP Gateway</h1>
              <p className="text-[10px] text-gray-500 uppercase tracking-widest mt-0.5">v{__APP_VERSION__}</p>
            </div>
          </div>
        </div>

        {/* Navigation */}
        <nav className="flex-1 p-3 space-y-0.5 overflow-y-auto">
          {navItems
            .filter(item => !item.admin || auth.isAdmin)
            .map(item => (
              <NavLink
                key={item.to}
                to={item.to}
                className={({ isActive }) =>
                  clsx(
                    'flex items-center gap-3 px-3 py-2.5 rounded-lg text-sm transition-all duration-150 group',
                    isActive
                      ? 'bg-accent/10 text-accent'
                      : 'text-gray-400 hover:text-gray-200 hover:bg-surface-hover'
                  )
                }
              >
                <item.icon className="w-4 h-4 shrink-0" />
                <span className="font-medium">{item.label}</span>
                <ChevronRight
                  className={clsx(
                    'w-3 h-3 ml-auto opacity-0 group-hover:opacity-50 transition-opacity',
                    location.pathname === item.to && 'opacity-50'
                  )}
                />
              </NavLink>
            ))}
        </nav>

        {/* User section */}
        <div className="p-3 border-t border-border">
          <div className="flex items-center justify-between px-3 py-2">
            <div className="min-w-0">
              <p className="text-sm font-medium text-gray-200 truncate">{auth.user?.username}</p>
              <p className="text-xs text-gray-500 truncate">{auth.user?.roles?.join(', ') ?? ''}</p>
            </div>
            <button
              onClick={auth.logout}
              className="p-1.5 text-gray-500 hover:text-gray-300 hover:bg-surface-hover rounded-md transition-colors"
              title="Sign out"
            >
              <LogOut className="w-4 h-4" />
            </button>
          </div>
        </div>
      </aside>

      {/* Main content */}
      <main className="flex-1 overflow-auto bg-[#0a0a0f]">
        <div className="p-8">
          {children}
        </div>
      </main>
    </div>
  );
}
