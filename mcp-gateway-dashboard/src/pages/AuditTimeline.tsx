import React, { useState, useEffect } from 'react';
import { api, AuditEvent, User } from '@/lib/api';
import { Search, Download, ChevronLeft, ChevronRight, Clock, CheckCircle, XCircle, ShieldOff, X, Filter, Trash2 } from 'lucide-react';
import clsx from 'clsx';
import { SUPPORTED_APPS } from '@/lib/connectors';

const STATUS_CONFIG: Record<string, { icon: typeof CheckCircle; color: string; bg: string }> = {
  success: { icon: CheckCircle, color: 'text-success', bg: 'bg-success/10' },
  error: { icon: XCircle, color: 'text-danger', bg: 'bg-danger/10' },
  denied: { icon: ShieldOff, color: 'text-warning', bg: 'bg-warning/10' },
  timeout: { icon: Clock, color: 'text-gray-400', bg: 'bg-gray-500/10' },
};

const RISK_CATEGORIES = ['read', 'write', 'admin', 'destructive', 'execute', 'unclassified'];
const POLICY_DECISIONS = ['allow', 'deny', 'conditional'];

const APP_COLORS: Record<string, string> = {
  claude: 'bg-orange-500/10 text-orange-400',
  claudedesktop: 'bg-orange-500/10 text-orange-400',
  cursor: 'bg-gray-500/10 text-gray-400',
  vscode: 'bg-blue-500/10 text-blue-400',
  openwebui: 'bg-gray-400/10 text-gray-300',
  clawbot: 'bg-purple-500/10 text-purple-400',
  codex: 'bg-emerald-500/10 text-emerald-400',
  lmstudio: 'bg-violet-500/10 text-violet-400',
};

export default function AuditTimeline() {
  const [events, setEvents] = useState<AuditEvent[]>([]);
  const [total, setTotal] = useState(0);
  const [loading, setLoading] = useState(true);
  const [page, setPage] = useState(0);
  const [pageError, setPageError] = useState('');

  // Filters
  const [search, setSearch] = useState('');
  const [statusFilter, setStatusFilter] = useState('');
  const [userFilter, setUserFilter] = useState('');
  const [clientFilter, setClientFilter] = useState('');
  const [backendFilter, setBackendFilter] = useState('');
  const [riskFilter, setRiskFilter] = useState('');
  const [policyFilter, setPolicyFilter] = useState('');
  const [applicationFilter, setApplicationFilter] = useState('');
  const [dateFrom, setDateFrom] = useState('');
  const [dateTo, setDateTo] = useState('');
  const [showFilters, setShowFilters] = useState(false);

  const [selectedEvent, setSelectedEvent] = useState<AuditEvent | null>(null);
  const [showClearConfirm, setShowClearConfirm] = useState(false);
  const [users, setUsers] = useState<User[]>([]);
  const limit = 20;

  // Derived filter values from events for autocomplete
  const [knownUsers, setKnownUsers] = useState<string[]>([]);
  const [knownClients, setKnownClients] = useState<string[]>([]);
  const [knownBackends, setKnownBackends] = useState<string[]>([]);

  useEffect(() => {
    loadEvents();
  }, [page, statusFilter, userFilter, clientFilter, backendFilter, riskFilter, policyFilter, applicationFilter, dateFrom, dateTo]);

  useEffect(() => {
    // Load initial set to extract filter values
    loadFilterOptions();
  }, []);

  useEffect(() => {
    api.getUsers().then(setUsers).catch(() => {});
  }, []);

  const userMap = new Map(users.map(u => [u.user_id, u.username]));

  const loadFilterOptions = async () => {
    try {
      const data = await api.getAuditEvents({ limit: '500', offset: '0' });
      const users = [...new Set(data.events.map(e => e.user_id).filter(Boolean) as string[])];
      const clients = [...new Set(data.events.map(e => e.client_id).filter(Boolean) as string[])];
      const backends = [...new Set(data.events.map(e => e.backend_name).filter(Boolean))];
      setKnownUsers(users);
      setKnownClients(clients);
      setKnownBackends(backends);
    } catch {}
  };

  const loadEvents = async () => {
    setLoading(true);
    try {
      const params: Record<string, string> = {
        limit: String(limit),
        offset: String(page * limit),
      };
      if (statusFilter) params.status = statusFilter;
      if (search) params.tool_name = search;
      if (userFilter) params.user_id = userFilter;
      if (clientFilter) params.client_id = clientFilter;
      if (backendFilter) params.backend = backendFilter;
      if (riskFilter) params.risk_category = riskFilter;
      if (policyFilter) params.policy_decision = policyFilter;
      if (applicationFilter) params.application = applicationFilter;
      if (dateFrom) params.from = `${dateFrom}T00:00:00Z`;
      if (dateTo) params.to = `${dateTo}T23:59:59Z`;

      const data = await api.getAuditEvents(params);
      setEvents(data.events);
      setTotal(data.total);
      setPageError('');
    } catch (e: any) {
      setPageError(e.message || 'Failed to load audit events');
    } finally {
      setLoading(false);
    }
  };

  const totalPages = Math.ceil(total / limit);

  const handleExport = async () => {
    try {
      const data = await api.getAuditEvents({ limit: '10000', offset: '0' });
      const blob = new Blob([JSON.stringify(data.events, null, 2)], { type: 'application/json' });
      const url = URL.createObjectURL(blob);
      const a = document.createElement('a');
      a.href = url;
      a.download = `audit-export-${new Date().toISOString().slice(0, 10)}.json`;
      a.click();
      URL.revokeObjectURL(url);
    } catch (e: any) {
      setPageError(e.message || 'Export failed');
    }
  };

  const handleClearAudit = async () => {
    try {
      await api.clearAudit();
      setShowClearConfirm(false);
      setEvents([]);
      setTotal(0);
      setPage(0);
      loadFilterOptions();
    } catch (e: any) {
      setPageError(e.message || 'Failed to clear audit');
      setShowClearConfirm(false);
    }
  };

  const activeFilterCount = [statusFilter, userFilter, clientFilter, backendFilter, riskFilter, policyFilter, applicationFilter, dateFrom, dateTo].filter(Boolean).length;

  const clearAllFilters = () => {
    setSearch('');
    setStatusFilter('');
    setUserFilter('');
    setClientFilter('');
    setBackendFilter('');
    setRiskFilter('');
    setPolicyFilter('');
    setApplicationFilter('');
    setDateFrom('');
    setDateTo('');
    setPage(0);
  };

  return (
    <div>
      <div className="flex items-center justify-between mb-6">
        <div>
          <h2 className="text-lg font-semibold text-white">Audit Timeline</h2>
          <p className="text-sm text-gray-500 mt-1">{total} total events recorded</p>
        </div>
        <div className="flex items-center gap-2">
          <button
            onClick={handleExport}
            className="flex items-center gap-2 px-3.5 py-2 bg-surface border border-border rounded-lg text-sm text-gray-300 hover:border-border-hover hover:text-white transition-colors"
          >
            <Download className="w-4 h-4" />
            Export
          </button>
          <button
            onClick={() => setShowClearConfirm(true)}
            className="flex items-center gap-2 px-3.5 py-2 bg-surface border border-danger/30 rounded-lg text-sm text-danger/70 hover:border-danger hover:text-danger transition-colors"
          >
            <Trash2 className="w-4 h-4" />
            Clear All
          </button>
        </div>
      </div>

      {pageError && (
        <div className="mb-4 px-4 py-3 bg-danger/10 border border-danger/20 rounded-lg flex items-center justify-between">
          <p className="text-sm text-danger">{pageError}</p>
          <button onClick={() => setPageError('')} className="text-danger/60 hover:text-danger">
            <X className="w-4 h-4" />
          </button>
        </div>
      )}

      {/* Clear confirmation dialog */}
      {showClearConfirm && (
        <div className="mb-4 px-4 py-4 bg-danger/5 border border-danger/20 rounded-lg">
          <p className="text-sm text-white mb-1 font-medium">Clear all audit events?</p>
          <p className="text-xs text-gray-400 mb-3">This will permanently delete all audit events and reset metrics counters. This action cannot be undone.</p>
          <div className="flex items-center gap-2">
            <button
              onClick={handleClearAudit}
              className="px-3 py-1.5 bg-danger text-white text-xs font-medium rounded-lg hover:bg-danger/80 transition-colors"
            >
              Yes, clear everything
            </button>
            <button
              onClick={() => setShowClearConfirm(false)}
              className="px-3 py-1.5 bg-surface border border-border text-gray-300 text-xs rounded-lg hover:border-border-hover transition-colors"
            >
              Cancel
            </button>
          </div>
        </div>
      )}

      {/* Primary filters */}
      <div className="flex items-center gap-3 mb-3">
        <div className="relative flex-1 max-w-sm">
          <Search className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-gray-500" />
          <input
            type="text"
            value={search}
            onChange={e => setSearch(e.target.value)}
            onKeyDown={e => e.key === 'Enter' && loadEvents()}
            placeholder="Search by tool name..."
            className="w-full pl-9 pr-3 py-2 bg-surface border border-border rounded-lg text-sm text-white placeholder-gray-600 focus:outline-none focus:border-accent/50 transition-colors"
          />
        </div>
        <button
          onClick={() => setShowFilters(!showFilters)}
          className={clsx(
            'flex items-center gap-2 px-3 py-2 border rounded-lg text-sm transition-colors',
            showFilters || activeFilterCount > 0
              ? 'bg-accent/10 border-accent/30 text-accent'
              : 'bg-surface border-border text-gray-300 hover:border-border-hover'
          )}
        >
          <Filter className="w-4 h-4" />
          Filters
          {activeFilterCount > 0 && (
            <span className="flex items-center justify-center w-4.5 h-4.5 text-[10px] font-semibold bg-accent text-white rounded-full">{activeFilterCount}</span>
          )}
        </button>
        {activeFilterCount > 0 && (
          <button onClick={clearAllFilters} className="flex items-center gap-1 text-xs text-gray-400 hover:text-white transition-colors">
            <X className="w-3 h-3" />
            Clear all
          </button>
        )}
      </div>

      {/* Advanced filters panel */}
      {showFilters && (
        <div className="mb-4 bg-surface border border-border rounded-xl p-4">
          <div className="grid grid-cols-4 gap-3">
            <div>
              <label className="block text-[10px] font-medium text-gray-500 uppercase tracking-wider mb-1.5">Status</label>
              <select
                value={statusFilter}
                onChange={e => { setStatusFilter(e.target.value); setPage(0); }}
                className="w-full px-2.5 py-1.5 bg-[#0a0a0f] border border-border rounded-lg text-xs text-gray-300 focus:outline-none focus:border-accent/50"
              >
                <option value="">All Statuses</option>
                <option value="success">Success</option>
                <option value="error">Error</option>
                <option value="denied">Denied</option>
                <option value="timeout">Timeout</option>
              </select>
            </div>
            <div>
              <label className="block text-[10px] font-medium text-gray-500 uppercase tracking-wider mb-1.5">User</label>
              <select
                value={userFilter}
                onChange={e => { setUserFilter(e.target.value); setPage(0); }}
                className="w-full px-2.5 py-1.5 bg-[#0a0a0f] border border-border rounded-lg text-xs text-gray-300 focus:outline-none focus:border-accent/50"
              >
                <option value="">All Users</option>
                {knownUsers.map(u => <option key={u} value={u}>{userMap.get(u) || u.slice(0, 8)}</option>)}
              </select>
            </div>
            <div>
              <label className="block text-[10px] font-medium text-gray-500 uppercase tracking-wider mb-1.5">AI Agent / Client</label>
              <select
                value={clientFilter}
                onChange={e => { setClientFilter(e.target.value); setPage(0); }}
                className="w-full px-2.5 py-1.5 bg-[#0a0a0f] border border-border rounded-lg text-xs text-gray-300 focus:outline-none focus:border-accent/50"
              >
                <option value="">All Clients</option>
                {knownClients.map(c => <option key={c} value={c}>{c}</option>)}
              </select>
            </div>
            <div>
              <label className="block text-[10px] font-medium text-gray-500 uppercase tracking-wider mb-1.5">Backend</label>
              <select
                value={backendFilter}
                onChange={e => { setBackendFilter(e.target.value); setPage(0); }}
                className="w-full px-2.5 py-1.5 bg-[#0a0a0f] border border-border rounded-lg text-xs text-gray-300 focus:outline-none focus:border-accent/50"
              >
                <option value="">All Backends</option>
                {knownBackends.map(b => <option key={b} value={b}>{b}</option>)}
              </select>
            </div>
            <div>
              <label className="block text-[10px] font-medium text-gray-500 uppercase tracking-wider mb-1.5">Application</label>
              <select
                value={applicationFilter}
                onChange={e => { setApplicationFilter(e.target.value); setPage(0); }}
                className="w-full px-2.5 py-1.5 bg-[#0a0a0f] border border-border rounded-lg text-xs text-gray-300 focus:outline-none focus:border-accent/50"
              >
                <option value="">All Apps</option>
                {SUPPORTED_APPS.map(a => <option key={a} value={a}>{a}</option>)}
              </select>
            </div>
            <div>
              <label className="block text-[10px] font-medium text-gray-500 uppercase tracking-wider mb-1.5">Risk Category</label>
              <select
                value={riskFilter}
                onChange={e => { setRiskFilter(e.target.value); setPage(0); }}
                className="w-full px-2.5 py-1.5 bg-[#0a0a0f] border border-border rounded-lg text-xs text-gray-300 focus:outline-none focus:border-accent/50"
              >
                <option value="">All Risks</option>
                {RISK_CATEGORIES.map(r => <option key={r} value={r}>{r}</option>)}
              </select>
            </div>
            <div>
              <label className="block text-[10px] font-medium text-gray-500 uppercase tracking-wider mb-1.5">Policy Decision</label>
              <select
                value={policyFilter}
                onChange={e => { setPolicyFilter(e.target.value); setPage(0); }}
                className="w-full px-2.5 py-1.5 bg-[#0a0a0f] border border-border rounded-lg text-xs text-gray-300 focus:outline-none focus:border-accent/50"
              >
                <option value="">All Decisions</option>
                {POLICY_DECISIONS.map(d => <option key={d} value={d}>{d}</option>)}
              </select>
            </div>
            <div>
              <label className="block text-[10px] font-medium text-gray-500 uppercase tracking-wider mb-1.5">From Date</label>
              <input
                type="date"
                value={dateFrom}
                onChange={e => { setDateFrom(e.target.value); setPage(0); }}
                className="w-full px-2.5 py-1.5 bg-[#0a0a0f] border border-border rounded-lg text-xs text-gray-300 focus:outline-none focus:border-accent/50"
              />
            </div>
            <div>
              <label className="block text-[10px] font-medium text-gray-500 uppercase tracking-wider mb-1.5">To Date</label>
              <input
                type="date"
                value={dateTo}
                onChange={e => { setDateTo(e.target.value); setPage(0); }}
                className="w-full px-2.5 py-1.5 bg-[#0a0a0f] border border-border rounded-lg text-xs text-gray-300 focus:outline-none focus:border-accent/50"
              />
            </div>
          </div>
        </div>
      )}

      {/* Table */}
      <div className="bg-surface border border-border rounded-xl overflow-hidden">
        <table className="w-full">
          <thead>
            <tr className="border-b border-border">
              <th className="text-left px-4 py-3 text-xs font-medium text-gray-500 uppercase tracking-wider">Timestamp</th>
              <th className="text-left px-3 py-3 text-xs font-medium text-gray-500 uppercase tracking-wider">Status</th>
              <th className="text-left px-3 py-3 text-xs font-medium text-gray-500 uppercase tracking-wider">Tool</th>
              <th className="text-left px-3 py-3 text-xs font-medium text-gray-500 uppercase tracking-wider">Backend</th>
              <th className="text-left px-3 py-3 text-xs font-medium text-gray-500 uppercase tracking-wider">User</th>
              <th className="text-left px-3 py-3 text-xs font-medium text-gray-500 uppercase tracking-wider">App</th>
              <th className="text-left px-3 py-3 text-xs font-medium text-gray-500 uppercase tracking-wider">Duration</th>
              <th className="text-left px-3 py-3 text-xs font-medium text-gray-500 uppercase tracking-wider">Risk</th>
            </tr>
          </thead>
          <tbody>
            {loading ? (
              <tr><td colSpan={8} className="px-4 py-12 text-center text-gray-500 text-sm">Loading events...</td></tr>
            ) : events.length === 0 ? (
              <tr><td colSpan={8} className="px-4 py-12 text-center text-gray-500 text-sm">No audit events found</td></tr>
            ) : (
              events.map(event => {
                const config = STATUS_CONFIG[event.status] || STATUS_CONFIG.success;
                const Icon = config.icon;
                const isSelected = selectedEvent?.event_id === event.event_id;
                const username = event.user_id ? (userMap.get(event.user_id) || event.user_id.slice(0, 8)) : 'anonymous';
                return (
                  <React.Fragment key={event.event_id}>
                    <tr
                      className={clsx(
                        'border-b border-border/50 hover:bg-surface-hover transition-colors cursor-pointer',
                        isSelected && 'bg-accent/5'
                      )}
                      onClick={() => setSelectedEvent(isSelected ? null : event)}
                    >
                      <td className="px-4 py-2.5 text-xs text-gray-400 whitespace-nowrap">{new Date(event.timestamp).toLocaleString()}</td>
                      <td className="px-3 py-2.5">
                        <span className={clsx('inline-flex items-center gap-1.5 text-xs font-medium', config.color)}>
                          <Icon className="w-3.5 h-3.5" />
                          {event.status}
                        </span>
                      </td>
                      <td className="px-3 py-2.5 text-sm font-medium text-white">{event.tool_name}</td>
                      <td className="px-3 py-2.5 text-xs text-gray-400">{event.backend_name}</td>
                      <td className="px-3 py-2.5 text-xs text-gray-300">{username}</td>
                      <td className="px-3 py-2.5">
                        {event.application ? (
                          <span className={clsx('text-xs px-1.5 py-0.5 rounded', APP_COLORS[event.application] || 'bg-gray-500/10 text-gray-400')}>
                            {event.application}
                          </span>
                        ) : (
                          <span className="text-xs text-gray-600">-</span>
                        )}
                      </td>
                      <td className="px-3 py-2.5 text-xs text-gray-400 tabular-nums">{event.duration_ms ? `${event.duration_ms.toFixed(1)}ms` : '-'}</td>
                      <td className="px-3 py-2.5">
                        {event.risk_category ? (
                          <span className="text-xs text-gray-500 bg-surface-active px-2 py-0.5 rounded">{event.risk_category}</span>
                        ) : (
                          <span className="text-xs text-gray-600">-</span>
                        )}
                      </td>
                    </tr>
                    {isSelected && (
                      <tr>
                        <td colSpan={8} className="px-4 py-4 bg-surface-hover border-b border-border/50">
                          <div className="grid grid-cols-4 gap-4 text-sm">
                            <div><span className="text-gray-500">Event ID:</span><br /><span className="text-gray-300 text-xs font-mono">{event.event_id}</span></div>
                            <div><span className="text-gray-500">Trace ID:</span><br /><span className="text-gray-300 text-xs font-mono">{event.trace_id}</span></div>
                            <div><span className="text-gray-500">Session:</span><br /><span className="text-gray-300 text-xs font-mono">{event.session_id || 'N/A'}</span></div>
                            <div><span className="text-gray-500">User ID:</span><br /><span className="text-gray-300 text-xs">{event.user_id || 'anonymous'}</span></div>
                            <div><span className="text-gray-500">Application:</span><br /><span className={clsx('text-xs', event.application ? (APP_COLORS[event.application] || 'text-gray-300') : 'text-gray-500')}>{event.application || 'unknown'}</span></div>
                            <div><span className="text-gray-500">Client / AI Agent:</span><br /><span className="text-gray-300 text-xs">{event.client_id || 'N/A'}</span></div>
                            <div><span className="text-gray-500">Policy:</span><br /><span className="text-gray-300 text-xs">{event.policy_decision || 'default'} {event.policy_id ? `(${event.policy_id.slice(0, 8)})` : ''}</span></div>
                            <div><span className="text-gray-500">Backend:</span><br /><span className="text-gray-300 text-xs">{event.backend_name}</span></div>
                            {event.request_hash && (
                              <div className="col-span-4"><span className="text-gray-500">Request Hash:</span><br /><span className="text-gray-300 text-xs font-mono">{event.request_hash}</span></div>
                            )}
                            {event.error_message && (
                              <div className="col-span-4"><span className="text-danger">Error:</span><br /><span className="text-gray-300 text-xs">{event.error_message}</span></div>
                            )}
                          </div>
                        </td>
                      </tr>
                    )}
                  </React.Fragment>
                );
              })
            )}
          </tbody>
        </table>
      </div>

      {/* Pagination */}
      {totalPages > 1 && (
        <div className="flex items-center justify-between mt-4 px-2">
          <span className="text-sm text-gray-500">
            Showing {page * limit + 1}-{Math.min((page + 1) * limit, total)} of {total}
          </span>
          <div className="flex items-center gap-2">
            <button
              onClick={() => setPage(Math.max(0, page - 1))}
              disabled={page === 0}
              className="p-1.5 bg-surface border border-border rounded-md text-gray-400 hover:text-white disabled:opacity-30 transition-colors"
            >
              <ChevronLeft className="w-4 h-4" />
            </button>
            <span className="text-sm text-gray-400 px-2">
              {page + 1} / {totalPages}
            </span>
            <button
              onClick={() => setPage(Math.min(totalPages - 1, page + 1))}
              disabled={page >= totalPages - 1}
              className="p-1.5 bg-surface border border-border rounded-md text-gray-400 hover:text-white disabled:opacity-30 transition-colors"
            >
              <ChevronRight className="w-4 h-4" />
            </button>
          </div>
        </div>
      )}
    </div>
  );
}
