import { useState, useEffect } from 'react';
import { api, MetricsSummary } from '@/lib/api';
import { XAxis, YAxis, CartesianGrid, Tooltip, ResponsiveContainer, AreaChart, Area } from 'recharts';
import { BarChart3, Eye, EyeOff, RotateCcw, Server, SlidersHorizontal } from 'lucide-react';
import clsx from 'clsx';
import SecurityPostureCard from '@/components/SecurityPostureCard';
import { fmt } from '@/lib/format';
import { BarRow, ChartCard, RISK_FILL, StackedBar, axisProps, tooltipProps } from '@/components/chart';
import {
  Banner,
  Button,
  Card,
  EmptyState,
  IconButton,
  Label,
  Loading,
  MiniStat,
  Mono,
  PageHeader,
  RailList,
  RailRow,
  Select,
  StatusLabel,
  Tone,
} from '@/components/ui';

type WidgetId = 'stats' | 'topTools' | 'latency' | 'riskBreakdown' | 'backendHealth' | 'hourlyVolume';

interface WidgetConfig {
  id: WidgetId;
  label: string;
  visible: boolean;
}

const DEFAULT_WIDGETS: WidgetConfig[] = [
  { id: 'stats', label: 'Summary', visible: true },
  { id: 'hourlyVolume', label: 'Call volume', visible: true },
  { id: 'topTools', label: 'Top tools', visible: true },
  { id: 'latency', label: 'Latency percentiles', visible: true },
  { id: 'riskBreakdown', label: 'Calls by risk', visible: true },
  { id: 'backendHealth', label: 'Backend health', visible: true },
];

const FULL_WIDTH_WIDGETS: WidgetId[] = ['stats', 'hourlyVolume'];

function loadWidgetConfig(): WidgetConfig[] {
  try {
    const stored = localStorage.getItem('mcpgw_metrics_widgets');
    if (stored) {
      // Reconcile against the current widget set so a stored layout from an
      // older build neither drops new widgets nor resurrects removed ones.
      const parsed: WidgetConfig[] = JSON.parse(stored);
      const known = new Map(DEFAULT_WIDGETS.map(w => [w.id, w]));
      const merged = parsed.filter(w => known.has(w.id)).map(w => ({ ...known.get(w.id)!, visible: w.visible }));
      const seen = new Set(merged.map(w => w.id));
      return [...merged, ...DEFAULT_WIDGETS.filter(w => !seen.has(w.id))];
    }
  } catch {
    /* corrupt layout — fall through to the default rather than white-screen */
  }
  return DEFAULT_WIDGETS;
}

function saveWidgetConfig(widgets: WidgetConfig[]) {
  localStorage.setItem('mcpgw_metrics_widgets', JSON.stringify(widgets));
}

function healthTone(status: string): Tone {
  if (status === 'healthy') return 'ok';
  if (status === 'unhealthy' || status === 'crashed' || status === 'failed') return 'deny';
  if (status === 'idle' || status === 'stopped' || status === 'disabled') return 'neutral';
  return 'warn';
}

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
      setMetrics(await api.getMetricsSummary());
      setPageError('');
    } catch (e: any) {
      setPageError(e.message || 'Failed to load metrics');
    } finally {
      setLoading(false);
    }
  };

  const toggleWidget = (id: WidgetId) => {
    const next = widgets.map(w => (w.id === id ? { ...w, visible: !w.visible } : w));
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


  if (loading && !metrics) return <Loading label="Loading metrics..." />;

  if (pageError && !metrics) {
    return (
      <Banner tone="deny" action={<Button onClick={loadMetrics}>Retry</Button>}>
        {pageError}
      </Banner>
    );
  }

  if (!metrics) return null;

  const errorTone: Tone = metrics.error_rate > 0.05 ? 'deny' : metrics.error_rate > 0.01 ? 'warn' : 'ok';
  const backendTone: Tone =
    metrics.active_backends === metrics.total_backends
      ? 'ok'
      : metrics.active_backends === 0
        ? 'deny'
        : 'warn';

  const renderWidget = (id: WidgetId) => {
    switch (id) {
      /* One figure leads. Six equal cards in six colours meant reading all six
         to learn anything, and none of them was the answer to "is it busy". */
      case 'stats':
        return (
          <Card>
            <div className="flex items-end justify-between gap-8 flex-wrap">
              <div>
                <Label>Calls routed · 24h</Label>
                <div className="text-2xl font-semibold tracking-[-0.02em] tabular-nums text-ink mt-1.5">
                  {fmt.count(metrics.calls_last_24h)}
                </div>
                <div className="text-2xs text-ink-4 mt-1.5 tabular-nums">
                  {fmt.count(metrics.total_tool_calls)} since the gateway started
                </div>
              </div>
              <div className="flex items-end gap-7 flex-wrap">
                <MiniStat label="Avg latency" value={fmt.duration(metrics.avg_latency_ms)} />
                <MiniStat
                  label="Error rate"
                  value={fmt.percent(metrics.error_rate)}
                  tone={errorTone}
                />
                <MiniStat
                  label="Backends"
                  value={
                    <>
                      {metrics.active_backends}
                      <span className="text-ink-4 font-normal">/{metrics.total_backends}</span>
                    </>
                  }
                  tone={backendTone}
                />
                <MiniStat
                  label="Tools"
                  value={
                    <>
                      {metrics.enabled_tools}
                      <span className="text-ink-4 font-normal">/{metrics.total_tools}</span>
                    </>
                  }
                />
                <MiniStat label="Policies" value={fmt.count(metrics.active_policies)} />
                <MiniStat label="Users" value={fmt.count(metrics.total_users)} />
              </div>
            </div>
          </Card>
        );

      case 'hourlyVolume': {
        // An explicit numeric ceiling, not `[0, 'auto']`: recharts' nice-tick
        // pass will happily widen an 'auto' bound past the one you gave it and
        // draw a -45 tick on a series that cannot go below zero.
        const peak = Math.max(...metrics.hourly_volume.map(h => h.count), 1);
        const ceiling = Math.max(4, Math.ceil((peak * 1.08) / 4) * 4);
        return (
          <ChartCard title="Call volume · 24h" bleed>
            {metrics.hourly_volume.length === 0 ? (
              <EmptyState icon={BarChart3} title="No volume recorded yet" />
            ) : (
              <ResponsiveContainer width="100%" height={190}>
                <AreaChart
                  data={metrics.hourly_volume}
                  margin={{ top: 4, right: 16, bottom: 0, left: 4 }}
                >
                  <defs>
                    <linearGradient id="volumeFill" x1="0" y1="0" x2="0" y2="1">
                      <stop offset="0%" stopColor="var(--beam)" stopOpacity={0.22} />
                      <stop offset="100%" stopColor="var(--beam)" stopOpacity={0} />
                    </linearGradient>
                  </defs>
                  <CartesianGrid stroke="var(--line-soft)" vertical={false} />
                  <XAxis
                    dataKey="hour"
                    {...axisProps}
                    tickFormatter={(v: string) =>
                      new Date(v).toLocaleTimeString(undefined, { hour: '2-digit', hour12: false }) + ':00'
                    }
                    minTickGap={28}
                  />
                  <YAxis
                    {...axisProps}
                    width={34}
                    domain={[0, ceiling]}
                    ticks={[0, ceiling / 4, ceiling / 2, (ceiling * 3) / 4, ceiling]}
                    allowDecimals={false}
                  />
                  <Tooltip
                    {...tooltipProps}
                    labelFormatter={(v: any) => fmt.dateTime(v)}
                    formatter={(value: any) => [fmt.count(Number(value)), 'calls']}
                  />
                  <Area
                    type="monotone"
                    dataKey="count"
                    stroke="var(--beam)"
                    strokeWidth={1.75}
                    fill="url(#volumeFill)"
                    dot={false}
                    activeDot={{ r: 3, fill: 'var(--beam)', stroke: 'var(--panel)', strokeWidth: 2 }}
                  />
                </AreaChart>
              </ResponsiveContainer>
            )}
          </ChartCard>
        );
      }

      case 'topTools': {
        const max = metrics.top_tools_24h[0]?.call_count || 1;
        return (
          <ChartCard title="Top tools · 24h" className="h-full">
            {metrics.top_tools_24h.length === 0 ? (
              <EmptyState icon={BarChart3} title="No tool calls yet" />
            ) : (
              <div className="space-y-2.5">
                {metrics.top_tools_24h.slice(0, 8).map((tool, i) => (
                  <BarRow
                    key={tool.tool_name}
                    rank={i + 1}
                    label={tool.tool_name}
                    value={fmt.count(tool.call_count)}
                    fraction={tool.call_count / max}
                    tone={tool.error_count > 0 ? 'var(--warn)' : 'var(--beam)'}
                    trailing={
                      <>
                        <span className="text-micro text-ink-4 tabular-nums">
                          {fmt.duration(tool.avg_duration_ms)}
                        </span>
                        {tool.error_count > 0 && (
                          <span className="text-micro text-deny tabular-nums">
                            {tool.error_count} err
                          </span>
                        )}
                      </>
                    }
                  />
                ))}
              </div>
            )}
          </ChartCard>
        );
      }

      /* Three figures against the slowest, not three columns. The number is the
         point; the bar is only there to show how far apart they are. */
      case 'latency': {
        const worst = Math.max(metrics.latency_percentiles.p99, 1);
        const rows = [
          { label: 'p50', value: metrics.latency_percentiles.p50, tone: 'var(--beam)' },
          { label: 'p95', value: metrics.latency_percentiles.p95, tone: 'var(--warn)' },
          { label: 'p99', value: metrics.latency_percentiles.p99, tone: 'var(--deny)' },
        ];
        return (
          <ChartCard title="Latency percentiles" className="h-full">
            <div className="space-y-4 pt-1">
              {rows.map(r => (
                <div key={r.label}>
                  <div className="flex items-baseline justify-between mb-1.5">
                    <span className="text-micro font-semibold uppercase tracking-[0.14em] text-ink-3">
                      {r.label}
                    </span>
                    <span
                      className="text-md font-semibold tabular-nums"
                      style={{ color: r.tone }}
                    >
                      {fmt.duration(r.value)}
                    </span>
                  </div>
                  <div className="h-[3px] rounded-full bg-neutral-wash overflow-hidden">
                    <div
                      className="h-full rounded-full transition-[width] duration-500 ease-[var(--ease-out-quint)]"
                      style={{ width: `${Math.max((r.value / worst) * 100, 2)}%`, background: r.tone }}
                    />
                  </div>
                </div>
              ))}
            </div>
          </ChartCard>
        );
      }

      /* Risk is ordinal, so it is one bar read left-to-right from safe to
         severe — not a donut of six equally loud hues. */
      case 'riskBreakdown': {
        const ordered = ['read', 'write', 'execute', 'admin', 'destructive', 'unclassified']
          .map(k => metrics.calls_by_risk.find(r => r.risk_category === k))
          .filter(Boolean) as MetricsSummary['calls_by_risk'];
        const total = ordered.reduce((s, r) => s + r.count, 0);
        return (
          <ChartCard title="Calls by risk" className="h-full">
            {total === 0 ? (
              <EmptyState icon={BarChart3} title="No classified calls yet" />
            ) : (
              <>
                <StackedBar
                  className="mb-4"
                  segments={ordered.map(r => ({
                    key: r.risk_category,
                    value: r.count,
                    fill: RISK_FILL[r.risk_category] || RISK_FILL.unclassified,
                  }))}
                />
                <div className="space-y-1.5">
                  {ordered.map(r => (
                    <div key={r.risk_category} className="flex items-center gap-2.5 text-2xs">
                      <span
                        className="w-2 h-2 rounded-[2px] shrink-0"
                        style={{ background: RISK_FILL[r.risk_category] || RISK_FILL.unclassified }}
                      />
                      <span className="text-ink-2 flex-1">{r.risk_category}</span>
                      <span className="text-ink-4 tabular-nums">
                        {fmt.percent(r.count / total, 0)}
                      </span>
                      <span className="text-ink font-medium tabular-nums w-12 text-right">
                        {fmt.count(r.count)}
                      </span>
                    </div>
                  ))}
                </div>
              </>
            )}
          </ChartCard>
        );
      }

      case 'backendHealth':
        return (
          <ChartCard title="Backend health" className="h-full" bleed>
            {metrics.backend_health.length === 0 ? (
              <EmptyState icon={Server} title="No backends configured" />
            ) : (
              <RailList className="border-0 rounded-none shadow-none bg-transparent max-h-72 overflow-y-auto">
                {metrics.backend_health.map(backend => {
                  const tone = healthTone(backend.status);
                  return (
                    <RailRow
                      key={backend.name}
                      tone={tone}
                      trailing={
                        <StatusLabel tone={tone} pulsing={tone === 'ok'}>
                          {backend.status}
                        </StatusLabel>
                      }
                    >
                      <div className="min-w-0">
                        <Mono className="text-xs text-ink block truncate">{backend.name}</Mono>
                        <span className="text-micro text-ink-4 tabular-nums">
                          {backend.tool_count} tools
                        </span>
                      </div>
                    </RailRow>
                  );
                })}
              </RailList>
            )}
          </ChartCard>
        );

      default:
        return null;
    }
  };

  const orderedWidgets = widgets.filter(w => w.visible);

  return (
    <div>
      <PageHeader
        title="Metrics"
        description="Throughput, latency, and health for the gateway as a whole."
        actions={
          <>
            <Select
              value={refreshInterval}
              onChange={e => setRefreshInterval(Number(e.target.value))}
              aria-label="Refresh interval"
            >
              <option value={10}>Every 10s</option>
              <option value={30}>Every 30s</option>
              <option value={60}>Every 60s</option>
              <option value={300}>Every 5m</option>
            </Select>
            <Button
              icon={SlidersHorizontal}
              onClick={() => setShowCustomize(c => !c)}
              className={clsx(showCustomize && 'border-beam-edge text-beam')}
            >
              Customise
            </Button>
          </>
        }
      />

      {/* Non-hideable posture checklist (owner-only; self-hides for others). */}
      <SecurityPostureCard />

      {pageError && (
        <Banner tone="deny" onDismiss={() => setPageError('')} className="mb-4">
          {pageError}
        </Banner>
      )}

      {showCustomize && (
        <Card className="mb-4 animate-rise">
          <div className="flex items-center justify-between mb-3">
            <Label>Panels shown, and in what order</Label>
            <Button variant="ghost" size="sm" icon={RotateCcw} onClick={resetWidgets}>
              Reset
            </Button>
          </div>
          <div className="space-y-0.5">
            {widgets.map((w, i) => (
              <div
                key={w.id}
                className="flex items-center gap-2 h-8 px-2 rounded-row hover:bg-raised transition-colors"
              >
                <div className="flex items-center">
                  <IconButton
                    icon={() => <span className="text-micro leading-none">▲</span>}
                    label={`Move ${w.label} up`}
                    onClick={() => moveWidget(i, -1)}
                    disabled={i === 0}
                    className="h-6 w-5"
                  />
                  <IconButton
                    icon={() => <span className="text-micro leading-none">▼</span>}
                    label={`Move ${w.label} down`}
                    onClick={() => moveWidget(i, 1)}
                    disabled={i === widgets.length - 1}
                    className="h-6 w-5"
                  />
                </div>
                <button
                  type="button"
                  onClick={() => toggleWidget(w.id)}
                  aria-pressed={w.visible}
                  className="flex items-center gap-2.5 flex-1 text-left"
                >
                  {w.visible ? (
                    <Eye className="w-3.5 h-3.5 text-beam shrink-0" />
                  ) : (
                    <EyeOff className="w-3.5 h-3.5 text-ink-4 shrink-0" />
                  )}
                  <span className={clsx('text-xs', w.visible ? 'text-ink-2' : 'text-ink-4')}>
                    {w.label}
                  </span>
                </button>
              </div>
            ))}
          </div>
        </Card>
      )}

      {(() => {
        const elements: React.ReactNode[] = [];
        let gridBatch: WidgetConfig[] = [];

        const flushGrid = () => {
          if (gridBatch.length === 0) return;
          elements.push(
            <div
              key={`grid-${gridBatch.map(w => w.id).join('-')}`}
              className="grid grid-cols-1 xl:grid-cols-2 gap-3.5 mb-3.5 items-stretch"
            >
              {gridBatch.map(w => (
                <div key={w.id} className="min-w-0">
                  {renderWidget(w.id)}
                </div>
              ))}
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
            elements.push(
              <div key={w.id} className="mb-3.5">
                {renderWidget(w.id)}
              </div>
            );
          } else {
            gridBatch.push(w);
            if (gridBatch.length === 2) flushGrid();
          }
        }
        flushGrid();
        return elements;
      })()}

      {orderedWidgets.length === 0 && (
        <Card>
          <EmptyState
            icon={BarChart3}
            title="Every panel is hidden"
            message="Turn one back on from Customise."
            action={<Button onClick={resetWidgets}>Reset panels</Button>}
          />
        </Card>
      )}
    </div>
  );
}
