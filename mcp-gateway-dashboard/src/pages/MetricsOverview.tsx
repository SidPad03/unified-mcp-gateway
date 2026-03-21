import { useState, useEffect, useCallback } from 'react';
import { api, MetricsSummary } from '@/lib/api';
import {
  BarChart, Bar,
  XAxis, YAxis, CartesianGrid, Tooltip, ResponsiveContainer, PieChart, Pie, Cell,
  AreaChart, Area,
} from 'recharts';
import { Activity, TrendingUp, Clock, AlertTriangle, Server, Zap, Settings2, Eye, EyeOff, RefreshCw, X } from 'lucide-react';
import clsx from 'clsx';

// ── Chart constants ─────────────────────────────────────────────────
const RISK_COLORS: Record<string, string> = {
  read: '#22c55e',
  write: '#3b82f6',
  admin: '#f59e0b',
  destructive: '#ef4444',
  execute: '#8b5cf6',
  unclassified: '#52525b',
};

type WidgetId = 'stats' | 'topTools' | 'latency' | 'riskBreakdown' | 'backendHealth' | 'hourlyVolume';

interface WidgetConfig {
  id: WidgetId;
  label: string;
  visible: boolean;
}

const DEFAULT_WIDGETS: WidgetConfig[] = [
  { id: 'stats', label: 'Stat Cards', visible: true },
  { id: 'hourlyVolume', label: 'Call Volume Trend', visible: true },
  { id: 'topTools', label: 'Top Tools by Volume', visible: true },
  { id: 'latency', label: 'Latency Percentiles', visible: true },
  { id: 'riskBreakdown', label: 'Calls by Risk Category', visible: true },
  { id: 'backendHealth', label: 'Backend Health', visible: true },
];

const FULL_WIDTH_WIDGETS: WidgetId[] = ['stats', 'hourlyVolume'];

function loadWidgetConfig(): WidgetConfig[] {
  try {
    const stored = localStorage.getItem('mcpgw_metrics_widgets');
    if (stored) return JSON.parse(stored);
  } catch {}
  return DEFAULT_WIDGETS;
}

function saveWidgetConfig(widgets: WidgetConfig[]) {
  localStorage.setItem('mcpgw_metrics_widgets', JSON.stringify(widgets));
}

// ── Shared tooltip ──────────────────────────────────────────────────
const tooltipStyle = {
  contentStyle: {
    background: 'rgba(15,15,23,0.95)',
    border: '1px solid rgba(30,30,46,0.8)',
    borderRadius: '12px',
    fontSize: '11px',
    padding: '8px 12px',
    boxShadow: '0 8px 32px rgba(0,0,0,0.5)',
    backdropFilter: 'blur(8px)',
  },
  labelStyle: { color: '#9ca3af', fontSize: '10px', marginBottom: '2px' },
  cursor: { stroke: '#7c5cfc', strokeWidth: 1, strokeDasharray: '4 4' },
};

// ── Stat card ───────────────────────────────────────────────────────
function StatCard({ label, value, icon: Icon, color, trend }: {
  label: string;
  value: string;
  icon: typeof Activity;
  color: string;
  trend?: string;
}) {
  return (
    <div className="group relative overflow-hidden rounded-2xl border border-border bg-surface transition-all duration-200 hover:border-border-hover">
      <div
        className="absolute top-0 right-0 w-24 h-24 opacity-[0.04] transition-opacity group-hover:opacity-[0.08]"
        style={{
          background: `radial-gradient(circle at top right, ${color}, transparent 70%)`,
        }}
      />
      <div className="relative p-5">
        <div className="flex items-start justify-between mb-3">
          <div
            className="w-9 h-9 rounded-xl flex items-center justify-center"
            style={{ background: `${color}12`, border: `1px solid ${color}20` }}
          >
            <Icon className="w-4 h-4" style={{ color }} />
          </div>
          {trend && (
            <span className="text-[10px] text-gray-500 tabular-nums">{trend}</span>
          )}
        </div>
        <p className="text-2xl font-bold text-white tabular-nums tracking-tight">{value}</p>
        <p className="text-[11px] text-gray-500 mt-1 tracking-wide">{label}</p>
      </div>
    </div>
  );
}

// ── Widget card wrapper ─────────────────────────────────────────────
function WidgetCard({ title, children, className, noPadding }: {
  title: string;
  children: React.ReactNode;
  className?: string;
  noPadding?: boolean;
}) {
  return (
    <div className={clsx('rounded-2xl border border-border bg-surface overflow-hidden transition-colors hover:border-border-hover', className)}>
      <div className="px-5 pt-5 pb-0">
        <h3 className="text-[11px] uppercase tracking-[0.15em] font-semibold text-gray-500 mb-4">{title}</h3>
      </div>
      <div className={noPadding ? '' : 'px-5 pb-5'}>
        {children}
      </div>
    </div>
  );
}

// ── Main component ──────────────────────────────────────────────────
export default function MetricsOverview() {
  const [metrics, setMetrics] = useState<MetricsSummary | null>(null);
  const [loading, setLoading] = useState(true);
  const [pageError, setPageError] = useState('');
  const [widgets, setWidgets] = useState<WidgetConfig[]>(loadWidgetConfig);
  const [showCustomize, setShowCustomize] = useState(false);
  const [refreshInterval, setRefreshInterval] = useState(30);

  useEffect(() => {
    loadMetrics();
    const interval = setInterval(loadMetrics, refreshInterval * 1000);
    return () => clearInterval(interval);
  }, [refreshInterval]);

  const loadMetrics = async () => {
    try {
      const data = await api.getMetricsSummary();
      setMetrics(data);
      setPageError('');
    } catch (e: any) {
      setPageError(e.message || 'Failed to load metrics');
    } finally {
      setLoading(false);
    }
  };

  const toggleWidget = (id: WidgetId) => {
    const next = widgets.map(w => w.id === id ? { ...w, visible: !w.visible } : w);
    setWidgets(next);
    saveWidgetConfig(next);
  };

  const moveWidget = (idx: number, dir: -1 | 1) => {
    const target = idx + dir;
    if (target < 0 || target >= widgets.length) return;
    const next = [...widgets];
    [next[idx], next[target]] = [next[target], next[idx]];
    setWidgets(next);
    saveWidgetConfig(next);
  };

  const resetWidgets = () => {
    setWidgets(DEFAULT_WIDGETS);
    saveWidgetConfig(DEFAULT_WIDGETS);
  };

  const isVisible = useCallback((id: WidgetId) => widgets.find(w => w.id === id)?.visible ?? true, [widgets]);

  if (loading && !metrics) {
    return (
      <div className="flex items-center justify-center h-64">
        <div className="flex flex-col items-center gap-3">
          <RefreshCw className="w-5 h-5 text-accent animate-spin" />
          <p className="text-xs text-gray-600">Loading metrics...</p>
        </div>
      </div>
    );
  }

  if (pageError && !metrics) {
    return (
      <div className="p-8">
        <div className="px-4 py-3 bg-danger/10 border border-danger/20 rounded-xl">
          <p className="text-sm text-danger">{pageError}</p>
          <button onClick={loadMetrics} className="mt-2 text-xs text-danger/80 hover:text-danger underline">Retry</button>
        </div>
      </div>
    );
  }

  if (!metrics) return null;

  // ── Stat cards config ───────────────────────────────────────────
  const statCards = [
    { label: 'Total Calls', value: metrics.total_tool_calls.toLocaleString(), icon: Activity, color: '#7c5cfc' },
    { label: 'Calls (24h)', value: metrics.calls_last_24h.toLocaleString(), icon: TrendingUp, color: '#3b82f6' },
    { label: 'Avg Latency', value: `${metrics.avg_latency_ms.toFixed(1)}ms`, icon: Clock, color: '#22c55e' },
    { label: 'Error Rate', value: `${metrics.error_rate.toFixed(1)}%`, icon: AlertTriangle, color: metrics.error_rate > 5 ? '#ef4444' : '#f59e0b' },
    { label: 'Backends', value: `${metrics.active_backends}/${metrics.total_backends}`, icon: Server, color: '#06b6d4' },
    { label: 'Policies', value: metrics.active_policies.toString(), icon: Zap, color: '#a855f7' },
  ];

  // ── Widget renderer ─────────────────────────────────────────────
  const renderWidget = (id: WidgetId) => {
    switch (id) {
      case 'stats':
        return (
          <div className="grid grid-cols-6 gap-3">
            {statCards.map(s => (
              <StatCard key={s.label} {...s} />
            ))}
          </div>
        );

      case 'topTools':
        return (
          <WidgetCard title="Top Tools — 24h" className="h-full">
            {metrics.top_tools_24h.length === 0 ? (
              <p className="text-xs text-gray-600 text-center py-12">No tool call data available</p>
            ) : (
              <div className="space-y-2">
                {metrics.top_tools_24h.slice(0, 8).map((tool, i) => {
                  const maxCount = metrics.top_tools_24h[0]?.call_count || 1;
                  const pct = (tool.call_count / maxCount) * 100;
                  return (
                    <div key={tool.tool_name} className="group">
                      <div className="flex items-center justify-between mb-1">
                        <div className="flex items-center gap-2 min-w-0">
                          <span className="text-[10px] text-gray-600 w-4 text-right tabular-nums">{i + 1}</span>
                          <span className="text-xs text-gray-300 truncate" title={tool.tool_name}>{tool.tool_name}</span>
                        </div>
                        <div className="flex items-center gap-3 shrink-0 ml-3">
                          <span className="text-[10px] text-gray-600 tabular-nums">{tool.avg_duration_ms?.toFixed(0) || 0}ms</span>
                          {(tool.error_count || 0) > 0 && (
                            <span className="text-[10px] text-danger tabular-nums">{tool.error_count} err</span>
                          )}
                          <span className="text-xs text-gray-400 font-medium tabular-nums w-10 text-right">{tool.call_count}</span>
                        </div>
                      </div>
                      <div className="h-1 rounded-full bg-surface-hover overflow-hidden ml-6">
                        <div
                          className="h-full rounded-full transition-all duration-500"
                          style={{
                            width: `${pct}%`,
                            background: 'linear-gradient(90deg, #7c5cfc, #3b82f6)',
                            opacity: 0.6 + (0.4 * (pct / 100)),
                          }}
                        />
                      </div>
                    </div>
                  );
                })}
              </div>
            )}
          </WidgetCard>
        );

      case 'latency':
        return (
          <WidgetCard title="Latency Percentiles" className="h-full">
            <div className="flex items-end justify-center gap-10 pt-4 pb-2">
              {[
                { label: 'p50', value: metrics.latency_percentiles.p50, color: '#22c55e' },
                { label: 'p95', value: metrics.latency_percentiles.p95, color: '#f59e0b' },
                { label: 'p99', value: metrics.latency_percentiles.p99, color: '#ef4444' },
              ].map(p => {
                const maxVal = Math.max(metrics.latency_percentiles.p99, 1);
                const height = Math.max((p.value / maxVal) * 160, 28);
                return (
                  <div key={p.label} className="flex flex-col items-center gap-3">
                    <span className="text-lg font-bold text-white tabular-nums">{p.value.toFixed(0)}<span className="text-[10px] text-gray-500 ml-0.5">ms</span></span>
                    <div className="relative">
                      <div
                        className="w-16 rounded-xl transition-all duration-700 ease-out"
                        style={{
                          height: `${height}px`,
                          background: `linear-gradient(to top, ${p.color}08, ${p.color}25)`,
                          border: `1px solid ${p.color}25`,
                        }}
                      />
                      <div
                        className="absolute top-0 left-0 right-0 h-0.5 rounded-t-xl"
                        style={{ backgroundColor: p.color, boxShadow: `0 0 12px ${p.color}60` }}
                      />
                    </div>
                    <span className="text-[10px] uppercase tracking-wider font-semibold" style={{ color: p.color }}>{p.label}</span>
                  </div>
                );
              })}
            </div>
          </WidgetCard>
        );

      case 'riskBreakdown':
        return (
          <WidgetCard title="Calls by Risk" className="h-full flex flex-col">
            {metrics.calls_by_risk.length === 0 ? (
              <p className="text-xs text-gray-600 text-center py-12 flex-1 flex items-center justify-center">No risk data</p>
            ) : (
              <div className="flex-1 flex flex-col">
                <ResponsiveContainer width="100%" height={190}>
                  <PieChart>
                    <Pie
                      data={metrics.calls_by_risk}
                      cx="50%"
                      cy="50%"
                      innerRadius={50}
                      outerRadius={80}
                      paddingAngle={4}
                      dataKey="count"
                      nameKey="risk_category"
                      stroke="none"
                      cornerRadius={4}
                    >
                      {metrics.calls_by_risk.map((entry) => (
                        <Cell key={entry.risk_category} fill={RISK_COLORS[entry.risk_category] || '#52525b'} />
                      ))}
                    </Pie>
                    <Tooltip
                      {...tooltipStyle}
                      formatter={(value: any, name: any) => [`${Number(value).toLocaleString()} calls`, String(name)]}
                    />
                  </PieChart>
                </ResponsiveContainer>
                <div className="flex flex-wrap gap-x-4 gap-y-1.5 justify-center mt-auto pt-2">
                  {metrics.calls_by_risk.map((item) => (
                    <div key={item.risk_category} className="flex items-center gap-1.5">
                      <div className="w-2 h-2 rounded-sm" style={{ backgroundColor: RISK_COLORS[item.risk_category] || '#52525b' }} />
                      <span className="text-[10px] text-gray-500">{item.risk_category}</span>
                      <span className="text-[10px] text-gray-600 tabular-nums">{item.count}</span>
                    </div>
                  ))}
                </div>
              </div>
            )}
          </WidgetCard>
        );

      case 'backendHealth':
        return (
          <WidgetCard title="Backend Health" className="h-full flex flex-col">
            <div className="space-y-2 overflow-y-auto max-h-64 pr-1">
              {metrics.backend_health.map(backend => {
                const isHealthy = backend.status === 'healthy';
                const isUnhealthy = backend.status === 'unhealthy';
                return (
                  <div
                    key={backend.name}
                    className="flex items-center justify-between px-3.5 py-3 rounded-xl bg-surface-hover/60 border border-transparent hover:border-border transition-colors"
                  >
                    <div className="flex items-center gap-3">
                      <div className="relative">
                        <div className={clsx('w-2.5 h-2.5 rounded-full',
                          isHealthy ? 'bg-success' : isUnhealthy ? 'bg-danger' : 'bg-gray-600'
                        )} />
                        {isHealthy && (
                          <div className="absolute inset-0 rounded-full bg-success animate-ping opacity-30" />
                        )}
                      </div>
                      <div>
                        <p className="text-[13px] font-medium text-white">{backend.name}</p>
                        <p className="text-[10px] text-gray-600">{backend.tool_count} tools</p>
                      </div>
                    </div>
                    <span className={clsx('text-[10px] font-semibold uppercase tracking-wider px-2.5 py-1 rounded-lg',
                      isHealthy ? 'text-success bg-success/8' : isUnhealthy ? 'text-danger bg-danger/8' : 'text-gray-500 bg-gray-500/8'
                    )}>
                      {backend.status}
                    </span>
                  </div>
                );
              })}
              {metrics.backend_health.length === 0 && (
                <p className="text-xs text-gray-600 text-center py-12">No backends configured</p>
              )}
            </div>
          </WidgetCard>
        );

      case 'hourlyVolume':
        return (
          <WidgetCard title="Call Volume — 24h" className="col-span-2" noPadding>
            {metrics.hourly_volume.length === 0 ? (
              <p className="text-xs text-gray-600 text-center py-16 px-5">No volume data available</p>
            ) : (
              <div className="px-2 pb-3">
                <ResponsiveContainer width="100%" height={200}>
                  <AreaChart data={metrics.hourly_volume} margin={{ top: 4, right: 12, bottom: 0, left: 12 }}>
                    <defs>
                      <linearGradient id="volumeGradient" x1="0" y1="0" x2="0" y2="1">
                        <stop offset="0%" stopColor="#7c5cfc" stopOpacity={0.25} />
                        <stop offset="50%" stopColor="#7c5cfc" stopOpacity={0.08} />
                        <stop offset="100%" stopColor="#7c5cfc" stopOpacity={0} />
                      </linearGradient>
                    </defs>
                    <CartesianGrid strokeDasharray="3 3" stroke="#1e1e2e" vertical={false} />
                    <XAxis
                      dataKey="hour"
                      tick={{ fill: '#52525b', fontSize: 9 }}
                      axisLine={false}
                      tickLine={false}
                      tickFormatter={(v: string) => v}
                    />
                    <YAxis
                      tick={{ fill: '#52525b', fontSize: 9 }}
                      axisLine={false}
                      tickLine={false}
                      width={30}
                    />
                    <Tooltip
                      {...tooltipStyle}
                      labelFormatter={(v: any) => String(v)}
                      formatter={(value: any) => [Number(value).toLocaleString(), 'calls']}
                    />
                    <Area
                      type="monotone"
                      dataKey="count"
                      stroke="#7c5cfc"
                      strokeWidth={2}
                      fill="url(#volumeGradient)"
                      dot={false}
                      activeDot={{ r: 4, fill: '#7c5cfc', stroke: '#0a0a0f', strokeWidth: 2 }}
                    />
                  </AreaChart>
                </ResponsiveContainer>
              </div>
            )}
          </WidgetCard>
        );

      default:
        return null;
    }
  };

  const orderedWidgets = widgets.filter(w => w.visible);

  return (
    <div>
      {/* ── Header ─────────────────────────────────────────────────── */}
      <div className="flex items-center justify-between mb-6">
        <div>
          <h2 className="text-lg font-semibold text-white tracking-tight">Metrics</h2>
          <p className="text-[11px] text-gray-600 mt-0.5">Real-time analytics and performance monitoring</p>
        </div>
        <div className="flex items-center gap-2">
          <select
            value={refreshInterval}
            onChange={e => setRefreshInterval(Number(e.target.value))}
            className="px-2.5 py-1.5 bg-transparent border border-border rounded-lg text-[11px] text-gray-400 focus:outline-none focus:border-accent/40 appearance-none pr-6 cursor-pointer"
          >
            <option value={10}>10s</option>
            <option value={30}>30s</option>
            <option value={60}>60s</option>
            <option value={300}>5m</option>
          </select>
          <button
            onClick={() => setShowCustomize(!showCustomize)}
            className={clsx(
              'flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-[11px] transition-all border',
              showCustomize
                ? 'bg-accent/10 border-accent/30 text-accent'
                : 'border-border text-gray-500 hover:text-gray-300 hover:border-border-hover'
            )}
          >
            <Settings2 className="w-3.5 h-3.5" />
            Customize
          </button>
        </div>
      </div>

      {pageError && (
        <div className="mb-4 px-4 py-3 bg-danger/10 border border-danger/20 rounded-xl flex items-center justify-between">
          <p className="text-xs text-danger">{pageError}</p>
          <button onClick={() => setPageError('')} className="text-danger/60 hover:text-danger">
            <X className="w-3.5 h-3.5" />
          </button>
        </div>
      )}

      {/* ── Customize panel ────────────────────────────────────────── */}
      {showCustomize && (
        <div className="mb-5 bg-surface border border-border rounded-2xl p-4">
          <div className="flex items-center justify-between mb-3">
            <h4 className="text-[10px] font-semibold text-gray-500 uppercase tracking-[0.15em]">Widget Visibility & Order</h4>
            <button onClick={resetWidgets} className="text-[10px] text-gray-600 hover:text-gray-300 transition-colors">Reset</button>
          </div>
          <div className="space-y-1">
            {widgets.map((w, i) => (
              <div key={w.id} className="flex items-center gap-3 py-1.5 px-2 rounded-lg hover:bg-surface-hover transition-colors">
                <div className="flex items-center gap-1">
                  <button onClick={() => moveWidget(i, -1)} disabled={i === 0} className="text-gray-600 hover:text-gray-300 disabled:opacity-20 text-[10px]">&#9650;</button>
                  <button onClick={() => moveWidget(i, 1)} disabled={i === widgets.length - 1} className="text-gray-600 hover:text-gray-300 disabled:opacity-20 text-[10px]">&#9660;</button>
                </div>
                <button onClick={() => toggleWidget(w.id)} className="flex items-center gap-2 flex-1">
                  {w.visible ? <Eye className="w-3.5 h-3.5 text-accent" /> : <EyeOff className="w-3.5 h-3.5 text-gray-600" />}
                  <span className={clsx('text-xs', w.visible ? 'text-gray-200' : 'text-gray-600')}>{w.label}</span>
                </button>
              </div>
            ))}
          </div>
        </div>
      )}

      {/* ── Widgets ────────────────────────────────────────────────── */}
      {(() => {
        const elements: React.ReactNode[] = [];
        let gridBatch: WidgetConfig[] = [];

        const flushGrid = () => {
          if (gridBatch.length === 0) return;
          elements.push(
            <div key={`grid-${gridBatch.map(w => w.id).join('-')}`} className="grid grid-cols-2 gap-4 mb-4 items-stretch">
              {gridBatch.map(w => <div key={w.id}>{renderWidget(w.id)}</div>)}
            </div>
          );
          gridBatch = [];
        };

        const rendered = new Set<string>();
        for (const w of orderedWidgets) {
          if (rendered.has(w.id)) continue;
          rendered.add(w.id);

          if (FULL_WIDTH_WIDGETS.includes(w.id)) {
            flushGrid();
            elements.push(<div key={w.id} className="mb-4">{renderWidget(w.id)}</div>);
          } else {
            gridBatch.push(w);
            if (gridBatch.length === 2) flushGrid();
          }
        }
        flushGrid();
        return elements;
      })()}
    </div>
  );
}
