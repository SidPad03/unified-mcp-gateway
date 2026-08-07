import React, { useState, useEffect, useMemo } from 'react';
import { api, Tool, Backend } from '@/lib/api';
import { Search, Wrench, X } from 'lucide-react';
import clsx from 'clsx';
import { useAuth } from '@/hooks/useAuth';
import { fmt } from '@/lib/format';
import {
  Banner,
  Button,
  Card,
  EmptyState,
  Input,
  Label,
  Loading,
  MiniStat,
  Mono,
  PageHeader,
  RISK_LEVELS,
  RiskBadge,
  Segmented,
  Select,
  StatusLabel,
  Table,
  TableMessage,
  Td,
  Th,
  Tone,
  railStyle,
  riskTone,
} from '@/components/ui';

type SortKey = 'tool_name' | 'backend_name' | 'risk_category' | 'call_count_24h' | 'is_enabled';
type SortDir = 'asc' | 'desc';
type CallsRange = '24h' | '7d' | '30d';

const RANGES = [
  { value: '24h' as const, label: '24h' },
  { value: '7d' as const, label: '7d' },
  { value: '30d' as const, label: '30d' },
];

type ToolStatus = 'enabled' | 'disabled' | 'disconnected';

const STATUS: Record<ToolStatus, { tone: Tone; label: string }> = {
  enabled: { tone: 'ok', label: 'Enabled' },
  disabled: { tone: 'neutral', label: 'Disabled' },
  disconnected: { tone: 'warn', label: 'Disconnected' },
};

export default function ToolInventory() {
  const auth = useAuth();
  const [tools, setTools] = useState<Tool[]>([]);
  const [loading, setLoading] = useState(true);
  const [pageError, setPageError] = useState('');
  const [search, setSearch] = useState('');
  const [riskFilter, setRiskFilter] = useState<string>('');
  const [backendFilter, setBackendFilter] = useState<string>('');
  const [enabledFilter, setEnabledFilter] = useState<string>('');
  const [selectedTool, setSelectedTool] = useState<Tool | null>(null);
  const [editingRisk, setEditingRisk] = useState<string | null>(null);
  const [sortKey, setSortKey] = useState<SortKey>('tool_name');
  const [sortDir, setSortDir] = useState<SortDir>('asc');
  const [callsRange, setCallsRange] = useState<CallsRange>('24h');
  const [backends, setBackends] = useState<Backend[]>([]);

  useEffect(() => {
    loadTools();
    loadBackends();
  }, [callsRange]);

  // Keep selectedTool in sync with refreshed tools data
  useEffect(() => {
    if (selectedTool) {
      const updated = tools.find(t => t.tool_id === selectedTool.tool_id);
      if (updated) setSelectedTool(updated);
    }
  }, [tools]);

  const loadBackends = async () => {
    try {
      setBackends(await api.getBackends());
    } catch {
      /* the tools list is still useful without backend health */
    }
  };

  const backendHealth: Record<string, string> = {};
  backends.forEach(b => {
    backendHealth[b.name] = b.health_status;
  });

  const loadTools = async () => {
    try {
      const params: Record<string, string> = {};
      if (callsRange !== '24h') params.calls_range = callsRange;
      setTools(await api.getTools(params));
      setPageError('');
    } catch (e: any) {
      setPageError(e.message || 'Failed to load tools');
    } finally {
      setLoading(false);
    }
  };

  const changeRiskCategory = async (toolId: string, newRisk: string) => {
    try {
      await api.updateTool(toolId, { risk_category: newRisk });
      setTools(prev =>
        prev.map(t => (t.tool_id === toolId ? { ...t, risk_category: newRisk } : t))
      );
    } catch (e: any) {
      setPageError(e.message || 'Failed to update risk category');
    }
  };

  const handleSort = (key: SortKey) => {
    if (sortKey === key) setSortDir(d => (d === 'asc' ? 'desc' : 'asc'));
    else {
      setSortKey(key);
      setSortDir('asc');
    }
  };

  const getToolStatus = (tool: Tool): ToolStatus => {
    if (!tool.is_enabled) return 'disabled';
    const health = backendHealth[tool.backend_name];
    if (health && health !== 'healthy' && health !== 'idle') return 'disconnected';
    return 'enabled';
  };

  const filteredAndSorted = useMemo(() => {
    const result = tools.filter(t => {
      if (search) {
        const q = search.toLowerCase();
        const match =
          t.tool_name.toLowerCase().includes(q) ||
          t.description?.toLowerCase().includes(q) ||
          t.original_name?.toLowerCase().includes(q) ||
          t.backend_name?.toLowerCase().includes(q);
        if (!match) return false;
      }
      if (riskFilter && (t.risk_category || 'unclassified') !== riskFilter) return false;
      if (backendFilter && t.backend_name !== backendFilter) return false;
      if (enabledFilter && getToolStatus(t) !== enabledFilter) return false;
      return true;
    });

    result.sort((a, b) => {
      let cmp = 0;
      switch (sortKey) {
        case 'tool_name':
          cmp = a.tool_name.localeCompare(b.tool_name);
          break;
        case 'backend_name':
          cmp = a.backend_name.localeCompare(b.backend_name);
          break;
        case 'risk_category':
          cmp = RISK_LEVELS.indexOf((a.risk_category || 'unclassified') as any) -
            RISK_LEVELS.indexOf((b.risk_category || 'unclassified') as any);
          break;
        case 'call_count_24h':
          cmp = a.call_count_24h - b.call_count_24h;
          break;
        case 'is_enabled':
          cmp = (a.is_enabled ? 1 : 0) - (b.is_enabled ? 1 : 0);
          break;
      }
      return sortDir === 'desc' ? -cmp : cmp;
    });

    return result;
  }, [tools, search, riskFilter, backendFilter, enabledFilter, sortKey, sortDir, backends]);

  const backendNames = [...new Set(tools.map(t => t.backend_name))].sort();
  const activeFilters = [riskFilter, backendFilter, enabledFilter, search].filter(Boolean).length;
  const disconnected = tools.filter(t => getToolStatus(t) === 'disconnected').length;
  const totalCalls = tools.reduce((sum, t) => sum + t.call_count_24h, 0);
  const sort = { key: sortKey, dir: sortDir };

  return (
    <div>
      <PageHeader
        title="Tools"
        description="Every tool aggregated from the connected MCP backends. Risk classification is what the policy engine matches on: reclassify a tool and the rules that govern it change with it."
        actions={
          <Segmented value={callsRange} options={RANGES} onChange={setCallsRange} label="Call window" />
        }
      />

      {pageError && (
        <Banner tone="deny" onDismiss={() => setPageError('')} className="mb-4">
          {pageError}
        </Banner>
      )}

      {/* One figure leads; the rest are a supporting tier at half the size.
          Four equal boxes in four colours made you read all four to learn
          anything. */}
      <Card className="mb-5">
        <div className="flex items-end justify-between gap-8 flex-wrap">
          <div>
            <Label>Calls routed · {callsRange}</Label>
            <div className="text-2xl font-semibold tracking-[-0.02em] tabular-nums text-ink mt-1.5">
              {fmt.count(totalCalls)}
            </div>
          </div>
          <div className="flex items-end gap-7 flex-wrap">
            <MiniStat label="Tools" value={fmt.count(tools.length)} />
            <MiniStat label="Enabled" value={fmt.count(tools.filter(t => t.is_enabled).length)} />
            <MiniStat label="Backends" value={fmt.count(backendNames.length)} />
            <MiniStat
              label="Disconnected"
              value={fmt.count(disconnected)}
              tone={disconnected > 0 ? 'warn' : undefined}
            />
          </div>
        </div>
      </Card>

      {/* Risk as a filter ramp — quiet until it matters. */}
      {tools.length > 0 && (
        <div className="flex items-center gap-2 mb-3.5 flex-wrap">
          {RISK_LEVELS.map(level => {
            const count = tools.filter(t => (t.risk_category || 'unclassified') === level).length;
            if (count === 0) return null;
            return (
              <RiskBadge
                key={level}
                risk={level}
                count={count}
                active={riskFilter === level}
                onClick={() => setRiskFilter(riskFilter === level ? '' : level)}
              />
            );
          })}
        </div>
      )}

      <div className="flex items-center gap-2 mb-3 flex-wrap">
        <div className="relative flex-1 min-w-[220px] max-w-md">
          <Search className="absolute left-2.5 top-1/2 -translate-y-1/2 w-3.5 h-3.5 text-ink-4 pointer-events-none" />
          <Input
            type="search"
            value={search}
            onChange={e => setSearch(e.target.value)}
            placeholder="Search name, description, backend..."
            className="pl-8"
          />
        </div>
        <Select value={riskFilter} onChange={e => setRiskFilter(e.target.value)} aria-label="Filter by risk">
          <option value="">All risks</option>
          {RISK_LEVELS.map(r => (
            <option key={r} value={r}>
              {r}
            </option>
          ))}
        </Select>
        <Select
          value={backendFilter}
          onChange={e => setBackendFilter(e.target.value)}
          aria-label="Filter by backend"
        >
          <option value="">All backends</option>
          {backendNames.map(b => (
            <option key={b} value={b}>
              {b}
            </option>
          ))}
        </Select>
        <Select
          value={enabledFilter}
          onChange={e => setEnabledFilter(e.target.value)}
          aria-label="Filter by status"
        >
          <option value="">All statuses</option>
          <option value="enabled">Enabled</option>
          <option value="disabled">Disabled</option>
          <option value="disconnected">Disconnected</option>
        </Select>
        {activeFilters > 0 && (
          <Button
            variant="ghost"
            icon={X}
            onClick={() => {
              setSearch('');
              setRiskFilter('');
              setBackendFilter('');
              setEnabledFilter('');
            }}
          >
            Clear filters
          </Button>
        )}
      </div>

      <p className="text-2xs text-ink-4 mb-2 tabular-nums">
        {filteredAndSorted.length === tools.length
          ? `${fmt.count(tools.length)} tools`
          : `${fmt.count(filteredAndSorted.length)} of ${fmt.count(tools.length)} tools`}
      </p>

      <Table>
        <thead>
          <tr>
            <Th sortKey="tool_name" sort={sort} onSort={handleSort}>
              Tool
            </Th>
            <Th sortKey="backend_name" sort={sort} onSort={handleSort} hide="xl">
              Backend
            </Th>
            <Th sortKey="risk_category" sort={sort} onSort={handleSort}>
              Risk
            </Th>
            <Th sortKey="call_count_24h" sort={sort} onSort={handleSort} align="right" hide="sm">
              Calls · {callsRange}
            </Th>
            <Th sortKey="is_enabled" sort={sort} onSort={handleSort} hide="md">
              Status
            </Th>
          </tr>
        </thead>
        <tbody>
          {loading ? (
            <TableMessage colSpan={5}>
              <Loading label="Loading tools..." />
            </TableMessage>
          ) : filteredAndSorted.length === 0 ? (
            <TableMessage colSpan={5}>
              <EmptyState
                icon={Wrench}
                title={tools.length === 0 ? 'No tools registered yet' : 'Nothing matches those filters'}
                message={
                  tools.length === 0
                    ? 'Add an MCP backend and the gateway will register its tools automatically.'
                    : 'Widen the search or clear the filters to see the full inventory.'
                }
                action={
                  activeFilters > 0 ? (
                    <Button
                      onClick={() => {
                        setSearch('');
                        setRiskFilter('');
                        setBackendFilter('');
                        setEnabledFilter('');
                      }}
                    >
                      Clear filters
                    </Button>
                  ) : undefined
                }
              />
            </TableMessage>
          ) : (
            filteredAndSorted.map(tool => {
              const expanded = selectedTool?.tool_id === tool.tool_id;
              const status = STATUS[getToolStatus(tool)];
              return (
                <React.Fragment key={tool.tool_id}>
                  <tr
                    onClick={() => setSelectedTool(expanded ? null : tool)}
                    className={clsx(
                      'cursor-pointer transition-colors duration-150 hover:bg-raised',
                      expanded && 'bg-raised'
                    )}
                  >
                    {/* The gate rail: destructive tools are a red edge you can
                        find by scrolling, without reading a word. */}
                    <Td style={railStyle(riskTone(tool.risk_category))}>
                      <div className="w-[min(168px,42vw)] sm:w-[240px] md:w-[300px] xl:w-[400px]">
                        <div className="font-mono text-xs text-ink truncate">{tool.tool_name}</div>
                        <div className="text-2xs text-ink-3 truncate mt-0.5">
                          {tool.description || 'No description'}
                        </div>
                      </div>
                    </Td>
                    <Td hide="xl">
                      <Mono className="text-ink-2">{tool.backend_name}</Mono>
                    </Td>
                    {/* Click to reclassify. A select on every row would turn a
                        list you scan into a form you fill in — the badge is the
                        reading state, the select only appears when editing. */}
                    <Td onClick={e => e.stopPropagation()}>
                      {auth.isAdmin && editingRisk === tool.tool_id ? (
                        <Select
                          autoFocus
                          value={tool.risk_category || 'unclassified'}
                          onChange={e => {
                            changeRiskCategory(tool.tool_id, e.target.value);
                            setEditingRisk(null);
                          }}
                          onBlur={() => setEditingRisk(null)}
                          aria-label={`Risk for ${tool.tool_name}`}
                          className="h-6 text-micro"
                        >
                          {RISK_LEVELS.map(r => (
                            <option key={r} value={r}>
                              {r}
                            </option>
                          ))}
                        </Select>
                      ) : (
                        <RiskBadge
                          risk={tool.risk_category}
                          onClick={auth.isAdmin ? () => setEditingRisk(tool.tool_id) : undefined}
                          className={auth.isAdmin ? 'hover:border-line-strong' : undefined}
                        />
                      )}
                    </Td>
                    <Td align="right" hide="sm">
                      <Mono className="text-ink">{fmt.count(tool.call_count_24h)}</Mono>
                    </Td>
                    <Td hide="md">
                      <StatusLabel tone={status.tone}>{status.label}</StatusLabel>
                    </Td>
                  </tr>

                  {expanded && (
                    <tr>
                      <td colSpan={5} className="bg-raised border-b border-line px-3.5 py-4">
                        <dl className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-x-8 gap-y-2.5">
                          <Detail label="Original name">
                            <Mono>{tool.original_name}</Mono>
                          </Detail>
                          <Detail label="Backend">
                            <Mono>{tool.backend_name}</Mono>
                          </Detail>
                          <Detail label="Last seen">{fmt.relative(tool.last_seen)}</Detail>
                          <Detail label={`Calls · ${callsRange}`}>
                            <Mono>{fmt.count(tool.call_count_24h)}</Mono>
                          </Detail>
                          <Detail label="Status">
                            <StatusLabel tone={status.tone}>{status.label}</StatusLabel>
                          </Detail>
                          <Detail label="Risk">
                            <RiskBadge risk={tool.risk_category} />
                          </Detail>
                          <div className="sm:col-span-2 lg:col-span-3">
                            <Label className="block mb-1">Description</Label>
                            <p className="text-xs text-ink-2">
                              {tool.description || 'No description'}
                            </p>
                          </div>
                        </dl>
                        {tool.input_schema && (
                          <div className="mt-4">
                            <Label className="block mb-1.5">Input schema</Label>
                            <pre className="bg-inset border border-line rounded-row p-3 text-2xs font-mono text-ink-2 overflow-auto max-h-48">
                              {JSON.stringify(tool.input_schema, null, 2)}
                            </pre>
                          </div>
                        )}
                      </td>
                    </tr>
                  )}
                </React.Fragment>
              );
            })
          )}
        </tbody>
      </Table>
    </div>
  );
}

function Detail({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="min-w-0">
      <dt>
        <Label>{label}</Label>
      </dt>
      <dd className="text-xs text-ink-2 mt-1 truncate">{children}</dd>
    </div>
  );
}
