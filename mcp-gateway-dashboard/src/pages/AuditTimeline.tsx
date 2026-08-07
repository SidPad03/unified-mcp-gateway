import React, { useState, useEffect } from 'react';
import { api, AuditEvent, User } from '@/lib/api';
import { ChevronLeft, ChevronRight, Download, ScrollText, Search, SlidersHorizontal, Trash2, X } from 'lucide-react';
import clsx from 'clsx';
import { SUPPORTED_APPS } from '@/lib/connectors';
import { fmt } from '@/lib/format';
import {
  Badge,
  Banner,
  Button,
  Card,
  ConfirmModal,
  EmptyState,
  Field,
  IconButton,
  Input,
  Label,
  Loading,
  Mono,
  PageHeader,
  RISK_LEVELS,
  RiskBadge,
  Select,
  StatusLabel,
  Table,
  TableMessage,
  Td,
  Th,
  Tone,
  railStyle,
} from '@/components/ui';

/**
 * How an outcome is toned.
 *
 * Red means *broken* and amber means *stopped*. A policy denial is the gateway
 * working, not failing, so it must not read the same as an upstream 502 — but
 * it is still the line you came here to find, so it does not fade into neutral
 * either.
 *
 * An unrecognised status must never fall through to "success": painting an
 * unknown outcome green is how `tool_error` once rendered as a green check and
 * hid failed calls from the audit trail. Unknown reads as unknown.
 */
const STATUS_TONE: Record<string, { tone: Tone; label: string }> = {
  success: { tone: 'ok', label: 'success' },
  error: { tone: 'deny', label: 'error' },
  tool_error: { tone: 'deny', label: 'tool error' },
  denied: { tone: 'warn', label: 'denied' },
  timeout: { tone: 'warn', label: 'timeout' },
};
const UNKNOWN_STATUS = { tone: 'neutral' as Tone, label: 'unknown' };

const POLICY_DECISIONS = ['allow', 'deny', 'conditional'];

export default function AuditTimeline() {
  const [events, setEvents] = useState<AuditEvent[]>([]);
  const [total, setTotal] = useState(0);
  const [loading, setLoading] = useState(true);
  const [page, setPage] = useState(0);
  const [pageError, setPageError] = useState('');

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
  const [clearing, setClearing] = useState(false);
  const [users, setUsers] = useState<User[]>([]);
  const limit = 20;

  const [knownUsers, setKnownUsers] = useState<string[]>([]);
  const [knownClients, setKnownClients] = useState<string[]>([]);
  const [knownBackends, setKnownBackends] = useState<string[]>([]);

  useEffect(() => {
    loadEvents();
  }, [
    page,
    statusFilter,
    userFilter,
    clientFilter,
    backendFilter,
    riskFilter,
    policyFilter,
    applicationFilter,
    dateFrom,
    dateTo,
  ]);

  useEffect(() => {
    loadFilterOptions();
    api.getUsers().then(setUsers).catch(() => {});
  }, []);

  const userMap = new Map(users.map(u => [u.user_id, u.username]));

  const loadFilterOptions = async () => {
    try {
      const data = await api.getAuditEvents({ limit: '500', offset: '0' });
      setKnownUsers([...new Set(data.events.map(e => e.user_id).filter(Boolean) as string[])]);
      setKnownClients([...new Set(data.events.map(e => e.client_id).filter(Boolean) as string[])]);
      setKnownBackends([...new Set(data.events.map(e => e.backend_name).filter(Boolean))]);
    } catch {
      /* filters degrade to free text; the timeline still loads */
    }
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
      setPageError(e.message || 'Failed to export events');
    }
  };

  const handleClearAudit = async () => {
    setClearing(true);
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
    } finally {
      setClearing(false);
    }
  };

  const activeFilters = [
    statusFilter,
    userFilter,
    clientFilter,
    backendFilter,
    riskFilter,
    policyFilter,
    applicationFilter,
    dateFrom,
    dateTo,
  ].filter(Boolean).length;

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
      <PageHeader
        title="Audit"
        description="Every call the gateway routed, in order, with the policy decision that let it through or stopped it. Append-only: the record is the point."
        actions={
          <>
            <Button icon={Download} onClick={handleExport}>
              Export
            </Button>
            <Button icon={Trash2} variant="danger" onClick={() => setShowClearConfirm(true)}>
              Clear
            </Button>
          </>
        }
      />

      {pageError && (
        <Banner tone="deny" onDismiss={() => setPageError('')} className="mb-4">
          {pageError}
        </Banner>
      )}

      <div className="flex items-center gap-2 mb-3 flex-wrap">
        <div className="relative flex-1 min-w-[220px] max-w-sm">
          <Search className="absolute left-2.5 top-1/2 -translate-y-1/2 w-3.5 h-3.5 text-ink-4 pointer-events-none" />
          <Input
            type="search"
            value={search}
            onChange={e => setSearch(e.target.value)}
            onKeyDown={e => e.key === 'Enter' && loadEvents()}
            placeholder="Search by tool name..."
            className="pl-8"
          />
        </div>
        <Button
          icon={SlidersHorizontal}
          onClick={() => setShowFilters(f => !f)}
          className={clsx(activeFilters > 0 && 'border-beam-edge text-beam')}
        >
          Filters
          {activeFilters > 0 && (
            <span className="ml-0.5 tabular-nums text-beam">{activeFilters}</span>
          )}
        </Button>
        {activeFilters > 0 && (
          <Button variant="ghost" icon={X} onClick={clearAllFilters}>
            Clear filters
          </Button>
        )}
        <div className="ml-auto text-2xs text-ink-4 tabular-nums">
          {fmt.count(total)} events recorded
        </div>
      </div>

      {showFilters && (
        <Card className="mb-4 animate-rise">
          <div className="grid grid-cols-2 md:grid-cols-3 xl:grid-cols-5 gap-3">
            <Field label="Status">
              <Select
                value={statusFilter}
                onChange={e => {
                  setStatusFilter(e.target.value);
                  setPage(0);
                }}
                className="w-full"
              >
                <option value="">All statuses</option>
                {['success', 'error', 'tool_error', 'denied', 'timeout'].map(s => (
                  <option key={s} value={s}>
                    {s}
                  </option>
                ))}
              </Select>
            </Field>
            <Field label="User">
              <Select
                value={userFilter}
                onChange={e => {
                  setUserFilter(e.target.value);
                  setPage(0);
                }}
                className="w-full"
              >
                <option value="">All users</option>
                {knownUsers.map(u => (
                  <option key={u} value={u}>
                    {userMap.get(u) || u.slice(0, 8)}
                  </option>
                ))}
              </Select>
            </Field>
            <Field label="Client">
              <Select
                value={clientFilter}
                onChange={e => {
                  setClientFilter(e.target.value);
                  setPage(0);
                }}
                className="w-full"
              >
                <option value="">All clients</option>
                {knownClients.map(c => (
                  <option key={c} value={c}>
                    {c}
                  </option>
                ))}
              </Select>
            </Field>
            <Field label="Backend">
              <Select
                value={backendFilter}
                onChange={e => {
                  setBackendFilter(e.target.value);
                  setPage(0);
                }}
                className="w-full"
              >
                <option value="">All backends</option>
                {knownBackends.map(b => (
                  <option key={b} value={b}>
                    {b}
                  </option>
                ))}
              </Select>
            </Field>
            <Field label="Application">
              <Select
                value={applicationFilter}
                onChange={e => {
                  setApplicationFilter(e.target.value);
                  setPage(0);
                }}
                className="w-full"
              >
                <option value="">All apps</option>
                {SUPPORTED_APPS.map(a => (
                  <option key={a} value={a}>
                    {a}
                  </option>
                ))}
              </Select>
            </Field>
            <Field label="Risk">
              <Select
                value={riskFilter}
                onChange={e => {
                  setRiskFilter(e.target.value);
                  setPage(0);
                }}
                className="w-full"
              >
                <option value="">All risks</option>
                {RISK_LEVELS.map(r => (
                  <option key={r} value={r}>
                    {r}
                  </option>
                ))}
              </Select>
            </Field>
            <Field label="Policy decision">
              <Select
                value={policyFilter}
                onChange={e => {
                  setPolicyFilter(e.target.value);
                  setPage(0);
                }}
                className="w-full"
              >
                <option value="">All decisions</option>
                {POLICY_DECISIONS.map(d => (
                  <option key={d} value={d}>
                    {d}
                  </option>
                ))}
              </Select>
            </Field>
            <Field label="From">
              <Input
                type="date"
                value={dateFrom}
                onChange={e => {
                  setDateFrom(e.target.value);
                  setPage(0);
                }}
              />
            </Field>
            <Field label="To">
              <Input
                type="date"
                value={dateTo}
                onChange={e => {
                  setDateTo(e.target.value);
                  setPage(0);
                }}
              />
            </Field>
          </div>
        </Card>
      )}

      <Table>
        <thead>
          <tr>
            <Th>Time</Th>
            <Th>Outcome</Th>
            <Th>Tool</Th>
            <Th hide="xl">Backend</Th>
            <Th hide="xl">User</Th>
            <Th hide="xl">App</Th>
            <Th align="right" hide="sm">Duration</Th>
            <Th hide="md">Risk</Th>
          </tr>
        </thead>
        <tbody>
          {loading ? (
            <TableMessage colSpan={8}>
              <Loading label="Loading events..." />
            </TableMessage>
          ) : events.length === 0 ? (
            <TableMessage colSpan={8}>
              <EmptyState
                icon={ScrollText}
                title={activeFilters > 0 || search ? 'Nothing matches those filters' : 'No calls recorded yet'}
                message={
                  activeFilters > 0 || search
                    ? 'Widen the window or clear the filters.'
                    : 'Once an AI client routes a tool call through the gateway, it lands here.'
                }
                action={
                  activeFilters > 0 ? <Button onClick={clearAllFilters}>Clear filters</Button> : undefined
                }
              />
            </TableMessage>
          ) : (
            events.map(event => {
              const status = STATUS_TONE[event.status] || UNKNOWN_STATUS;
              const expanded = selectedEvent?.event_id === event.event_id;
              const username = event.user_id
                ? userMap.get(event.user_id) || event.user_id.slice(0, 8)
                : 'anonymous';
              return (
                <React.Fragment key={event.event_id}>
                  <tr
                    onClick={() => setSelectedEvent(expanded ? null : event)}
                    className={clsx(
                      'cursor-pointer transition-colors duration-150 hover:bg-raised',
                      expanded && 'bg-raised'
                    )}
                  >
                    {/* The rail carries the verdict. Down a page of events it
                        reads as a column of green with amber and red notches —
                        the health of the gateway, without reading a word. */}
                    {/* The rail lives on the first cell, so this column is
                        never the one that gets hidden — it just gets shorter. */}
                    <Td style={railStyle(status.tone)} className="whitespace-nowrap">
                      <Mono className="text-ink-3 sm:hidden">{fmt.time(event.timestamp)}</Mono>
                      <Mono className="text-ink-3 hidden sm:inline">
                        {fmt.dateTime(event.timestamp)}
                      </Mono>
                    </Td>
                    <Td>
                      <StatusLabel tone={status.tone}>{status.label}</StatusLabel>
                    </Td>
                    <Td>
                      <Mono className="text-ink truncate block w-[min(150px,34vw)] sm:w-[190px] md:w-[230px] xl:w-[240px]">
                        {event.tool_name}
                      </Mono>
                    </Td>
                    <Td hide="xl">
                      <Mono className="text-ink-3">{event.backend_name}</Mono>
                    </Td>
                    <Td hide="xl">{username}</Td>
                    <Td hide="xl">
                      {event.application ? (
                        <Badge>{event.application}</Badge>
                      ) : (
                        <span className="text-ink-4">—</span>
                      )}
                    </Td>
                    <Td align="right" hide="sm">
                      <Mono className={event.duration_ms == null ? 'text-ink-4' : 'text-ink-2'}>
                        {fmt.duration(event.duration_ms)}
                      </Mono>
                    </Td>
                    <Td hide="md">
                      {event.risk_category ? (
                        <RiskBadge risk={event.risk_category} />
                      ) : (
                        <span className="text-ink-4">—</span>
                      )}
                    </Td>
                  </tr>

                  {expanded && (
                    <tr>
                      <td colSpan={8} className="bg-raised border-b border-line px-3.5 py-4">
                        <dl className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-x-8 gap-y-3">
                          <Detail label="Time" mono>
                            {fmt.dateTime(event.timestamp)}
                          </Detail>
                          <Detail label="Duration" mono>
                            {fmt.duration(event.duration_ms)}
                          </Detail>
                          <Detail label="Risk">{event.risk_category || 'unclassified'}</Detail>
                          <Detail label="Event ID" mono>
                            {event.event_id}
                          </Detail>
                          <Detail label="Trace ID" mono>
                            {event.trace_id}
                          </Detail>
                          <Detail label="Session" mono>
                            {event.session_id || '—'}
                          </Detail>
                          <Detail label="User ID" mono>
                            {event.user_id || 'anonymous'}
                          </Detail>
                          <Detail label="Application">{event.application || 'unknown'}</Detail>
                          <Detail label="Client" mono>
                            {event.client_id || '—'}
                          </Detail>
                          <Detail label="Policy">
                            {event.policy_decision || 'default'}
                            {event.policy_id && (
                              <Mono className="text-ink-4 ml-1.5">
                                ({event.policy_id.slice(0, 8)})
                              </Mono>
                            )}
                          </Detail>
                          <Detail label="Backend" mono>
                            {event.backend_name}
                          </Detail>
                          {event.risk_flags?.length > 0 && (
                            <div className="col-span-2 lg:col-span-4">
                              <Label className="block mb-1.5">Risk flags</Label>
                              <div className="flex flex-wrap gap-1.5">
                                {event.risk_flags.map(f => (
                                  <Badge key={f} tone="warn">
                                    {f}
                                  </Badge>
                                ))}
                              </div>
                            </div>
                          )}
                          {event.request_hash && (
                            <Detail label="Request hash" mono className="col-span-2">
                              {event.request_hash}
                            </Detail>
                          )}
                          {event.response_hash && (
                            <Detail label="Response hash" mono className="col-span-2">
                              {event.response_hash}
                            </Detail>
                          )}
                        </dl>
                        {event.error_message && (
                          <Banner tone="deny" className="mt-4">
                            {event.error_message}
                          </Banner>
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

      {totalPages > 1 && (
        <div className="flex items-center justify-between mt-3.5">
          <span className="text-2xs text-ink-4 tabular-nums">
            {fmt.count(page * limit + 1)}-{fmt.count(Math.min((page + 1) * limit, total))} of{' '}
            {fmt.count(total)}
          </span>
          <div className="flex items-center gap-1.5">
            <IconButton
              icon={ChevronLeft}
              label="Previous page"
              onClick={() => setPage(p => Math.max(0, p - 1))}
              disabled={page === 0}
            />
            <span className="text-2xs text-ink-3 tabular-nums px-1">
              {page + 1} / {totalPages}
            </span>
            <IconButton
              icon={ChevronRight}
              label="Next page"
              onClick={() => setPage(p => Math.min(totalPages - 1, p + 1))}
              disabled={page >= totalPages - 1}
            />
          </div>
        </div>
      )}

      <ConfirmModal
        open={showClearConfirm}
        onClose={() => setShowClearConfirm(false)}
        onConfirm={handleClearAudit}
        loading={clearing}
        title="Clear the entire audit trail?"
        description="This permanently deletes every recorded event and resets the metrics counters. It cannot be undone."
        confirmLabel="Clear everything"
      >
        <p className="text-xs text-ink-2">
          <Mono className="text-ink">{fmt.count(total)}</Mono> events will be deleted. Export
          first if you need to keep them.
        </p>
      </ConfirmModal>
    </div>
  );
}

function Detail({
  label,
  children,
  mono,
  className,
}: {
  label: string;
  children: React.ReactNode;
  mono?: boolean;
  className?: string;
}) {
  return (
    <div className={clsx('min-w-0', className)}>
      <dt>
        <Label>{label}</Label>
      </dt>
      <dd className={clsx('text-2xs text-ink-2 mt-1 break-all', mono && 'font-mono')}>{children}</dd>
    </div>
  );
}
