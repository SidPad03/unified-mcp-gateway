import { useState, useEffect, useMemo } from 'react';
import { api, Tool, Backend } from '@/lib/api';
import { Search, Wrench, X, ChevronUp, ChevronDown, ChevronsUpDown } from 'lucide-react';
import clsx from 'clsx';
import { useAuth } from '@/hooks/useAuth';

const RISK_COLORS: Record<string, string> = {
  read: 'bg-emerald-500/10 text-emerald-400 border-emerald-500/20',
  write: 'bg-blue-500/10 text-blue-400 border-blue-500/20',
  admin: 'bg-orange-500/10 text-orange-400 border-orange-500/20',
  destructive: 'bg-red-500/10 text-red-400 border-red-500/20',
  execute: 'bg-purple-500/10 text-purple-400 border-purple-500/20',
  unclassified: 'bg-gray-500/10 text-gray-400 border-gray-500/20',
};

const ALL_RISK_CATEGORIES = ['read', 'write', 'admin', 'destructive', 'execute', 'unclassified'];

type SortKey = 'tool_name' | 'backend_name' | 'risk_category' | 'call_count_24h' | 'is_enabled';
type SortDir = 'asc' | 'desc';
type CallsRange = '24h' | '7d' | '30d';

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
      const data = await api.getBackends();
      setBackends(data);
    } catch {}
  };

  const backendHealth: Record<string, string> = {};
  backends.forEach(b => { backendHealth[b.name] = b.health_status; });

  const loadTools = async () => {
    try {
      const params: Record<string, string> = {};
      if (callsRange !== '24h') params.calls_range = callsRange;
      const data = await api.getTools(params);
      setTools(data);
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
      setTools(prev => prev.map(t => t.tool_id === toolId ? { ...t, risk_category: newRisk } : t));
      if (selectedTool?.tool_id === toolId) {
        setSelectedTool({ ...selectedTool, risk_category: newRisk });
      }
      setEditingRisk(null);
    } catch (e: any) {
      setPageError(e.message || 'Failed to update risk category');
    }
  };

  const handleSort = (key: SortKey) => {
    if (sortKey === key) {
      setSortDir(d => d === 'asc' ? 'desc' : 'asc');
    } else {
      setSortKey(key);
      setSortDir('asc');
    }
  };

  const SortIcon = ({ column }: { column: SortKey }) => {
    if (sortKey !== column) return <ChevronsUpDown className="w-3 h-3 opacity-30" />;
    return sortDir === 'asc' ? <ChevronUp className="w-3 h-3 text-accent" /> : <ChevronDown className="w-3 h-3 text-accent" />;
  };

  const filteredAndSorted = useMemo(() => {
    let result = tools.filter(t => {
      if (search) {
        const q = search.toLowerCase();
        const match = t.tool_name.toLowerCase().includes(q) ||
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
        case 'tool_name': cmp = a.tool_name.localeCompare(b.tool_name); break;
        case 'backend_name': cmp = a.backend_name.localeCompare(b.backend_name); break;
        case 'risk_category': cmp = (a.risk_category || 'unclassified').localeCompare(b.risk_category || 'unclassified'); break;
        case 'call_count_24h': cmp = a.call_count_24h - b.call_count_24h; break;
        case 'is_enabled': cmp = (a.is_enabled ? 1 : 0) - (b.is_enabled ? 1 : 0); break;
      }
      return sortDir === 'desc' ? -cmp : cmp;
    });

    return result;
  }, [tools, search, riskFilter, backendFilter, enabledFilter, sortKey, sortDir, backends, callsRange]);

  const backendNames = [...new Set(tools.map(t => t.backend_name))];

  const getToolStatus = (tool: Tool): 'enabled' | 'disabled' | 'disconnected' => {
    if (!tool.is_enabled) return 'disabled';
    const health = backendHealth[tool.backend_name];
    if (health && health !== 'healthy' && health !== 'idle') return 'disconnected';
    return 'enabled';
  };
  const callsLabel = callsRange === '24h' ? '24h' : callsRange === '7d' ? '7d' : '30d';

  const activeFilterCount = [riskFilter, backendFilter, enabledFilter, search].filter(Boolean).length;

  return (
    <div>
      <div className="mb-6">
        <h2 className="text-lg font-semibold text-white">Tool Inventory</h2>
        <p className="text-sm text-gray-500 mt-1">All aggregated tools across connected MCP backends</p>
      </div>

      {pageError && (
        <div className="mb-4 px-4 py-3 bg-danger/10 border border-danger/20 rounded-lg flex items-center justify-between">
          <p className="text-sm text-danger">{pageError}</p>
          <button onClick={() => setPageError('')} className="text-danger/60 hover:text-danger">
            <X className="w-4 h-4" />
          </button>
        </div>
      )}

      {/* Stats bar with calls range selector */}
      <div className="grid grid-cols-4 gap-4 mb-6">
        {[
          { label: 'Total Tools', value: tools.length, color: 'text-white' },
          { label: 'Enabled', value: tools.filter(t => t.is_enabled).length, color: 'text-success' },
          { label: 'Backends', value: backendNames.length, color: 'text-info' },
        ].map(stat => (
          <div key={stat.label} className="bg-surface border border-border rounded-xl p-4">
            <p className="text-xs text-gray-500 uppercase tracking-wider">{stat.label}</p>
            <p className={clsx('text-2xl font-semibold mt-1', stat.color)}>{stat.value}</p>
          </div>
        ))}
        <div className="bg-surface border border-border rounded-xl p-4">
          <div className="flex items-center justify-between">
            <p className="text-xs text-gray-500 uppercase tracking-wider">Calls ({callsLabel})</p>
            <div className="flex bg-surface-hover rounded-md p-0.5">
              {(['24h', '7d', '30d'] as CallsRange[]).map(range => (
                <button
                  key={range}
                  onClick={() => setCallsRange(range)}
                  className={clsx(
                    'px-2 py-0.5 text-[10px] font-medium rounded transition-colors',
                    callsRange === range ? 'bg-accent text-white' : 'text-gray-500 hover:text-gray-300'
                  )}
                >
                  {range}
                </button>
              ))}
            </div>
          </div>
          <p className="text-2xl font-semibold mt-1 text-accent">{tools.reduce((sum, t) => sum + t.call_count_24h, 0)}</p>
        </div>
      </div>

      {/* Risk breakdown chips */}
      {tools.length > 0 && (
        <div className="flex items-center gap-3 mb-4 flex-wrap">
          {ALL_RISK_CATEGORIES.map(cat => {
            const count = tools.filter(t => (t.risk_category || 'unclassified') === cat).length;
            if (count === 0) return null;
            return (
              <button
                key={cat}
                onClick={() => setRiskFilter(riskFilter === cat ? '' : cat)}
                className={clsx(
                  'inline-flex items-center gap-1.5 px-2.5 py-1 text-xs font-medium rounded-full border transition-colors',
                  RISK_COLORS[cat],
                  riskFilter === cat && 'ring-1 ring-white/20'
                )}
              >
                {cat} <span className="opacity-60">{count}</span>
              </button>
            );
          })}
        </div>
      )}

      {/* Filters row */}
      <div className="flex items-center gap-3 mb-4">
        <div className="relative flex-1 max-w-md">
          <Search className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-gray-500" />
          <input
            type="text"
            value={search}
            onChange={e => setSearch(e.target.value)}
            placeholder="Search by name, description, backend..."
            className="w-full pl-9 pr-3 py-2 bg-surface border border-border rounded-lg text-sm text-white placeholder-gray-600 focus:outline-none focus:border-accent/50 transition-colors"
          />
          {search && (
            <button onClick={() => setSearch('')} className="absolute right-2.5 top-1/2 -translate-y-1/2 text-gray-600 hover:text-gray-400">
              <X className="w-3.5 h-3.5" />
            </button>
          )}
        </div>
        <select
          value={riskFilter}
          onChange={e => setRiskFilter(e.target.value)}
          className="px-3 py-2 bg-surface border border-border rounded-lg text-sm text-gray-300 focus:outline-none focus:border-accent/50 transition-colors"
        >
          <option value="">All Risks</option>
          {ALL_RISK_CATEGORIES.map(r => <option key={r} value={r}>{r}</option>)}
        </select>
        <select
          value={backendFilter}
          onChange={e => setBackendFilter(e.target.value)}
          className="px-3 py-2 bg-surface border border-border rounded-lg text-sm text-gray-300 focus:outline-none focus:border-accent/50 transition-colors"
        >
          <option value="">All Backends</option>
          {backendNames.map(b => <option key={b} value={b}>{b}</option>)}
        </select>
        <select
          value={enabledFilter}
          onChange={e => setEnabledFilter(e.target.value)}
          className="px-3 py-2 bg-surface border border-border rounded-lg text-sm text-gray-300 focus:outline-none focus:border-accent/50 transition-colors"
        >
          <option value="">All Status</option>
          <option value="enabled">Enabled</option>
          <option value="disabled">Disabled</option>
          <option value="disconnected">Disconnected</option>
        </select>
        {activeFilterCount > 0 && (
          <button
            onClick={() => { setSearch(''); setRiskFilter(''); setBackendFilter(''); setEnabledFilter(''); }}
            className="flex items-center gap-1 px-2.5 py-2 text-xs text-gray-400 hover:text-white transition-colors"
          >
            <X className="w-3 h-3" />
            Clear ({activeFilterCount})
          </button>
        )}
      </div>

      <p className="text-xs text-gray-500 mb-2">{filteredAndSorted.length} of {tools.length} tools</p>

      {/* Table */}
      <div className="bg-surface border border-border rounded-xl overflow-hidden">
        <table className="w-full">
          <thead>
            <tr className="border-b border-border">
              {[
                { key: 'tool_name' as SortKey, label: 'Tool Name' },
                { key: 'backend_name' as SortKey, label: 'Backend' },
                { key: 'risk_category' as SortKey, label: 'Risk' },
                { key: 'call_count_24h' as SortKey, label: `Calls (${callsLabel})` },
                { key: 'is_enabled' as SortKey, label: 'Status' },
              ].map(col => (
                <th
                  key={col.key}
                  onClick={() => handleSort(col.key)}
                  className="text-left px-4 py-3 text-xs font-medium text-gray-500 uppercase tracking-wider cursor-pointer hover:text-gray-300 select-none transition-colors"
                >
                  <div className="flex items-center gap-1.5">
                    {col.label}
                    <SortIcon column={col.key} />
                  </div>
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {loading ? (
              <tr><td colSpan={5} className="px-4 py-12 text-center text-gray-500 text-sm">Loading tools...</td></tr>
            ) : filteredAndSorted.length === 0 ? (
              <tr><td colSpan={5} className="px-4 py-12 text-center text-gray-500 text-sm">No tools found</td></tr>
            ) : (
              filteredAndSorted.map(tool => (
                <tr
                  key={tool.tool_id}
                  className="border-b border-border/50 hover:bg-surface-hover transition-colors cursor-pointer"
                  onClick={() => setSelectedTool(selectedTool?.tool_id === tool.tool_id ? null : tool)}
                >
                  <td className="px-4 py-3">
                    <div className="flex items-center gap-2.5">
                      <div className="w-7 h-7 bg-accent/10 rounded-md flex items-center justify-center shrink-0">
                        <Wrench className="w-3.5 h-3.5 text-accent" />
                      </div>
                      <div className="min-w-0">
                        <p className="text-sm font-medium text-white truncate">{tool.tool_name}</p>
                        <p className="text-xs text-gray-500 truncate max-w-xs">{tool.description || 'No description'}</p>
                      </div>
                    </div>
                  </td>
                  <td className="px-4 py-3">
                    <span className="text-sm text-gray-300">{tool.backend_name}</span>
                  </td>
                  <td className="px-4 py-3" onClick={e => e.stopPropagation()}>
                    {auth.isAdmin && editingRisk === tool.tool_id ? (
                      <select
                        autoFocus
                        value={tool.risk_category || 'unclassified'}
                        onChange={e => changeRiskCategory(tool.tool_id, e.target.value)}
                        onBlur={() => setEditingRisk(null)}
                        className="text-xs px-2 py-1 bg-[#0a0a0f] border border-accent/40 rounded text-gray-300 focus:outline-none"
                      >
                        {ALL_RISK_CATEGORIES.map(r => <option key={r} value={r}>{r}</option>)}
                      </select>
                    ) : (
                      <span
                        className={clsx(
                          'inline-flex px-2 py-0.5 text-xs font-medium rounded-full border',
                          RISK_COLORS[tool.risk_category || 'unclassified'] || RISK_COLORS.unclassified,
                          auth.isAdmin && 'cursor-pointer hover:ring-1 hover:ring-accent/30'
                        )}
                        onClick={() => auth.isAdmin && setEditingRisk(tool.tool_id)}
                        title={auth.isAdmin ? 'Click to reclassify' : undefined}
                      >
                        {tool.risk_category || 'unclassified'}
                      </span>
                    )}
                  </td>
                  <td className="px-4 py-3">
                    <span className="text-sm text-gray-300">{tool.call_count_24h}</span>
                  </td>
                  <td className="px-4 py-3">
                    {(() => {
                      const status = getToolStatus(tool);
                      const statusConfig = {
                        enabled: { color: 'text-success', dot: 'bg-success', label: 'Enabled' },
                        disabled: { color: 'text-gray-500', dot: 'bg-gray-600', label: 'Disabled' },
                        disconnected: { color: 'text-warning', dot: 'bg-warning', label: 'Disconnected' },
                      }[status];
                      return (
                        <span className={clsx('inline-flex items-center gap-1 text-xs font-medium', statusConfig.color)}>
                          <span className={clsx('w-1.5 h-1.5 rounded-full', statusConfig.dot)} />
                          {statusConfig.label}
                        </span>
                      );
                    })()}
                  </td>
                </tr>
              ))
            )}
          </tbody>
        </table>
      </div>

      {/* Detail panel */}
      {selectedTool && (
        <div className="mt-4 bg-surface border border-border rounded-xl p-5">
          <div className="flex items-center justify-between mb-3">
            <h3 className="text-sm font-semibold text-white">Tool Details</h3>
            <button onClick={() => setSelectedTool(null)} className="text-gray-500 hover:text-gray-300">
              <X className="w-4 h-4" />
            </button>
          </div>
          <div className="grid grid-cols-2 gap-4 text-sm">
            <div><span className="text-gray-500">Full Name:</span> <span className="text-gray-200 ml-2">{selectedTool.tool_name}</span></div>
            <div><span className="text-gray-500">Original Name:</span> <span className="text-gray-200 ml-2">{selectedTool.original_name}</span></div>
            <div><span className="text-gray-500">Backend:</span> <span className="text-gray-200 ml-2">{selectedTool.backend_name}</span></div>
            <div className="flex items-center gap-2">
              <span className="text-gray-500">Risk Category:</span>
              {auth.isAdmin ? (
                <select
                  value={selectedTool.risk_category || 'unclassified'}
                  onChange={e => changeRiskCategory(selectedTool.tool_id, e.target.value)}
                  className="text-xs px-2 py-1 bg-[#0a0a0f] border border-border rounded text-gray-300 focus:outline-none focus:border-accent/50"
                >
                  {ALL_RISK_CATEGORIES.map(r => <option key={r} value={r}>{r}</option>)}
                </select>
              ) : (
                <span className={clsx('inline-flex px-2 py-0.5 text-xs font-medium rounded-full border ml-2', RISK_COLORS[selectedTool.risk_category || 'unclassified'] || RISK_COLORS.unclassified)}>
                  {selectedTool.risk_category || 'unclassified'}
                </span>
              )}
            </div>
            <div><span className="text-gray-500">Last Seen:</span> <span className="text-gray-200 ml-2">{new Date(selectedTool.last_seen).toLocaleString()}</span></div>
            <div><span className="text-gray-500">Calls ({callsLabel}):</span> <span className="text-gray-200 ml-2">{selectedTool.call_count_24h}</span></div>
            <div className="col-span-2"><span className="text-gray-500">Description:</span> <span className="text-gray-200 ml-2">{selectedTool.description || 'No description available'}</span></div>
          </div>
          {selectedTool.input_schema && (
            <div className="mt-4">
              <p className="text-xs text-gray-500 mb-2">Input Schema:</p>
              <pre className="bg-[#0a0a0f] p-3 rounded-lg text-xs text-gray-400 overflow-auto max-h-40">
                {JSON.stringify(selectedTool.input_schema, null, 2)}
              </pre>
            </div>
          )}
        </div>
      )}
    </div>
  );
}
