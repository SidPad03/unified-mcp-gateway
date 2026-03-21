import { useState, useEffect, useCallback, useMemo, useRef } from 'react';
import {
  ReactFlow,
  Background,
  Controls,
  type Node,
  type Edge,
  Position,
  Handle,
  type NodeProps,
} from '@xyflow/react';
import '@xyflow/react/dist/style.css';
import {
  api,
  type UsageGraph as UsageGraphData,
  type ConnectionStatus,
  type User,
  type AuditEvent,
} from '@/lib/api';
import { APP_LABELS, type AppSlug } from '@/lib/connectors';
import {
  RefreshCw,
  Users as UsersIcon,
  X,
  Server,
  Wrench,
  Clock,
  CheckCircle,
  XCircle,
  ShieldOff,
  Activity,
  MessageSquare,
  MousePointer2,
  Code2,
  Globe,
  Cpu,
  Monitor,
  Terminal,
  Bot,
  type LucideIcon,
  Loader2,
  ArrowRight,
  Layers,
  ChevronRight,
} from 'lucide-react';
import clsx from 'clsx';

// ── App metadata with proper names, colors, and icons ──────────────
const APP_META: Record<string, { label: string; color: string; Icon: LucideIcon }> = {
  claude:        { label: 'Claude Code',    color: '#e87b5f', Icon: Terminal },
  claudedesktop: { label: 'Claude Desktop', color: '#d4856a', Icon: Monitor },
  cursor:        { label: 'Cursor',         color: '#4d9de0', Icon: MousePointer2 },
  vscode:        { label: 'VS Code',        color: '#2fb4ab', Icon: Code2 },
  openwebui:     { label: 'Open WebUI',     color: '#69c97a', Icon: Globe },
  clawbot:       { label: 'Clawbot',        color: '#b07ce8', Icon: Bot },
  codex:         { label: 'Codex',          color: '#e06ead', Icon: Terminal },
  lmstudio:      { label: 'LM Studio',      color: '#d4a843', Icon: Cpu },
};

const fallbackAppMeta = { label: '', color: '#6b7280', Icon: MessageSquare };

const getAppMeta = (key: string) => {
  const meta = APP_META[key];
  if (meta) return meta;
  // Try to get label from connectors
  const label = APP_LABELS[key as AppSlug];
  return { ...fallbackAppMeta, label: label || key };
};

// ── Risk category colors & labels ──────────────────────────────────
const RISK_COLORS: Record<string, { bg: string; text: string; dot: string }> = {
  read:         { bg: '#22c55e18', text: '#22c55e', dot: '#22c55e' },
  write:        { bg: '#3b82f618', text: '#3b82f6', dot: '#3b82f6' },
  admin:        { bg: '#f59e0b18', text: '#f59e0b', dot: '#f59e0b' },
  destructive:  { bg: '#ef444418', text: '#ef4444', dot: '#ef4444' },
  execute:      { bg: '#a855f718', text: '#a855f7', dot: '#a855f7' },
  'external-api': { bg: '#6b728018', text: '#6b7280', dot: '#6b7280' },
  unclassified: { bg: '#6b728018', text: '#6b7280', dot: '#6b7280' },
};

const getRiskColor = (risk: string | null | undefined) =>
  RISK_COLORS[risk ?? ''] ?? RISK_COLORS.unclassified;

// ── Shared node shell ──────────────────────────────────────────────
function NodeShell({
  children,
  glowColor,
  selected,
}: {
  children: React.ReactNode;
  glowColor?: string;
  selected?: boolean;
}) {
  return (
    <div className="relative group">
      {glowColor && (
        <div
          className="absolute -inset-1 rounded-2xl opacity-20 blur-md transition-opacity duration-300 group-hover:opacity-35 pointer-events-none"
          style={{ background: glowColor }}
        />
      )}
      <div
        className={clsx(
          'relative px-4 py-3 rounded-2xl min-w-[160px] border transition-all duration-200 group-hover:border-white/10 cursor-pointer',
          selected && 'ring-1 ring-accent/50',
        )}
        style={{
          background: 'linear-gradient(135deg, rgba(15,15,23,0.95) 0%, rgba(22,22,31,0.9) 100%)',
          borderColor: selected ? 'rgba(124,92,252,0.4)' : 'rgba(30,30,46,0.6)',
          backdropFilter: 'blur(12px)',
          boxShadow: '0 2px 16px rgba(0,0,0,0.4)',
        }}
      >
        {children}
      </div>
    </div>
  );
}

// ── App Node ───────────────────────────────────────────────────────
function AppNodeComponent({ data, selected }: NodeProps) {
  const appKey = String(data.application);
  const meta = getAppMeta(appKey);
  const connected = Boolean(data.is_connected);
  const calls = Number(data.call_count) || 0;
  const { Icon } = meta;

  return (
    <NodeShell glowColor={connected ? meta.color : undefined} selected={selected}>
      <Handle
        type="source"
        position={Position.Right}
        className="!w-2.5 !h-2.5 !border-2 !border-[#0a0a0f] !rounded-full"
        style={{ background: meta.color }}
      />
      <div className="flex items-center gap-3">
        <div
          className="w-8 h-8 rounded-lg flex items-center justify-center shrink-0"
          style={{
            background: `${meta.color}18`,
            border: `1px solid ${meta.color}25`,
          }}
        >
          <Icon className="w-4 h-4" style={{ color: meta.color }} />
        </div>
        <div className="min-w-0">
          <div className="flex items-center gap-2">
            <span className="text-[13px] font-semibold text-white truncate">
              {meta.label}
            </span>
            <div className="relative flex items-center justify-center w-2 h-2 shrink-0">
              {connected && (
                <div
                  className="absolute inset-0 rounded-full animate-ping opacity-40"
                  style={{ background: '#22c55e' }}
                />
              )}
              <div
                className="w-2 h-2 rounded-full"
                style={{ background: connected ? '#22c55e' : '#3f3f46' }}
              />
            </div>
          </div>
          <p className="text-[10px] text-gray-500 mt-0.5 tabular-nums">
            {calls > 0 ? `${calls.toLocaleString()} calls` : 'no activity'}
          </p>
        </div>
      </div>
    </NodeShell>
  );
}

// ── Backend Node ───────────────────────────────────────────────────
function BackendNodeComponent({ data, selected }: NodeProps) {
  const name = String(data.backend_name);
  const transport = String(data.transport);
  const health = String(data.health_status);
  const toolCount = Number(data.tool_count) || 0;
  const expanded = Boolean(data.expanded);
  const healthColor =
    health === 'healthy' ? '#22c55e' : health === 'unhealthy' ? '#ef4444' : '#6b7280';

  return (
    <NodeShell glowColor={health === 'healthy' ? '#22c55e20' : undefined} selected={selected}>
      <Handle
        type="target"
        position={Position.Left}
        className="!w-2.5 !h-2.5 !border-2 !border-[#0a0a0f] !rounded-full"
        style={{ background: '#7c5cfc' }}
      />
      {expanded && (
        <Handle
          type="source"
          position={Position.Right}
          className="!w-2.5 !h-2.5 !border-2 !border-[#0a0a0f] !rounded-full"
          style={{ background: '#7c5cfc' }}
        />
      )}
      <div className="flex items-center gap-3">
        <div className="w-8 h-8 rounded-lg bg-accent/10 flex items-center justify-center shrink-0 border border-accent/15">
          <Server className="w-4 h-4 text-accent" />
        </div>
        <div className="min-w-0">
          <div className="flex items-center gap-2">
            <span className="text-[13px] font-semibold text-white truncate">{name}</span>
            <div className="w-2 h-2 rounded-full shrink-0" style={{ background: healthColor }} />
          </div>
          <div className="flex items-center gap-1.5 mt-0.5">
            <span
              className="text-[9px] uppercase tracking-wider font-medium px-1.5 py-px rounded"
              style={{
                background: '#7c5cfc12',
                color: '#7c5cfc',
                border: '1px solid #7c5cfc18',
              }}
            >
              {transport}
            </span>
            <span className="text-[10px] text-gray-500">{toolCount} tools</span>
          </div>
        </div>
        <ChevronRight
          className="w-4 h-4 text-gray-500 shrink-0 transition-transform duration-300"
          style={{ transform: expanded ? 'rotate(180deg)' : 'rotate(0deg)' }}
        />
      </div>
    </NodeShell>
  );
}

// ── Tool Node ──────────────────────────────────────────────────────
function ToolNodeComponent({ data, selected }: NodeProps) {
  const toolName = String(data.tool_name);
  const risk = data.risk_category ? String(data.risk_category) : null;
  const calls = Number(data.call_count) || 0;
  const rc = getRiskColor(risk);
  const animIndex = Number(data.animIndex) || 0;

  // Strip backend prefix for display (e.g., "arr__radarr_get_tags" → "radarr_get_tags")
  const displayName = toolName.includes('__') ? toolName.split('__').slice(1).join('__') : toolName;

  return (
    <div
      className="tool-node-enter"
      style={{ animationDelay: `${animIndex * 0.06}s` }}
    >
      <NodeShell selected={selected}>
        <Handle
          type="target"
          position={Position.Left}
          className="!w-2.5 !h-2.5 !border-2 !border-[#0a0a0f] !rounded-full"
          style={{ background: rc.dot }}
        />
        <div className="max-w-[180px]">
          <p className="text-[12px] font-medium text-gray-200 truncate" title={toolName}>
            {displayName}
          </p>
          <div className="flex items-center gap-2 mt-1">
            {risk && (
              <span
                className="text-[8px] uppercase tracking-wider font-semibold px-1.5 py-px rounded"
                style={{ background: rc.bg, color: rc.text }}
              >
                {risk}
              </span>
            )}
            {calls > 0 && (
              <span className="text-[10px] text-gray-500 tabular-nums">
                {calls.toLocaleString()}
              </span>
            )}
          </div>
        </div>
      </NodeShell>
    </div>
  );
}

const nodeTypes = {
  appNode: AppNodeComponent,
  backendNode: BackendNodeComponent,
  toolNode: ToolNodeComponent,
};

// ── Relative time helper ───────────────────────────────────────────
function timeAgo(iso: string): string {
  const diff = Date.now() - new Date(iso).getTime();
  const mins = Math.floor(diff / 60000);
  if (mins < 1) return 'just now';
  if (mins < 60) return `${mins}m ago`;
  const hrs = Math.floor(mins / 60);
  if (hrs < 24) return `${hrs}h ago`;
  const days = Math.floor(hrs / 24);
  return `${days}d ago`;
}

// ── Detail Panel (right-side overlay) ──────────────────────────────
interface SelectionInfo {
  type: 'app' | 'backend' | 'tool' | 'app-backend-edge' | 'backend-tool-edge';
  // For nodes
  data?: Record<string, unknown>;
  // For edges
  sourceName?: string;
  targetName?: string;
  callCount?: number;
}

function DetailPanel({
  selection,
  events,
  loadingEvents,
  onClose,
}: {
  selection: SelectionInfo;
  events: AuditEvent[];
  loadingEvents: boolean;
  onClose: () => void;
}) {
  const statusIcon = (status: string) => {
    if (status === 'success') return <CheckCircle className="w-3 h-3 text-emerald-400" />;
    if (status === 'error' || status === 'tool_error')
      return <XCircle className="w-3 h-3 text-red-400" />;
    if (status === 'denied') return <ShieldOff className="w-3 h-3 text-amber-400" />;
    return <Activity className="w-3 h-3 text-gray-400" />;
  };

  const renderHeader = () => {
    switch (selection.type) {
      case 'app': {
        const appKey = String(selection.data?.application ?? '');
        const meta = getAppMeta(appKey);
        const connected = Boolean(selection.data?.is_connected);
        return (
          <div className="flex items-center gap-3">
            <div
              className="w-8 h-8 rounded-lg flex items-center justify-center shrink-0"
              style={{ background: `${meta.color}18`, border: `1px solid ${meta.color}25` }}
            >
              <meta.Icon className="w-4 h-4" style={{ color: meta.color }} />
            </div>
            <div>
              <p className="text-sm font-semibold text-white">{meta.label}</p>
              <div className="flex items-center gap-2 mt-0.5">
                <div
                  className="w-2 h-2 rounded-full"
                  style={{ background: connected ? '#22c55e' : '#3f3f46' }}
                />
                <span className="text-[10px] text-gray-500">
                  {connected ? 'Connected' : 'Disconnected'}
                </span>
              </div>
            </div>
          </div>
        );
      }
      case 'backend': {
        const name = String(selection.data?.backend_name ?? '');
        const transport = String(selection.data?.transport ?? '');
        const health = String(selection.data?.health_status ?? '');
        return (
          <div className="flex items-center gap-3">
            <div className="w-8 h-8 rounded-lg bg-accent/10 flex items-center justify-center shrink-0 border border-accent/15">
              <Server className="w-4 h-4 text-accent" />
            </div>
            <div>
              <p className="text-sm font-semibold text-white">{name}</p>
              <div className="flex items-center gap-1.5 mt-0.5">
                <span
                  className="text-[9px] uppercase tracking-wider font-medium px-1.5 py-px rounded"
                  style={{ background: '#7c5cfc12', color: '#7c5cfc' }}
                >
                  {transport}
                </span>
                <div
                  className="w-2 h-2 rounded-full"
                  style={{
                    background:
                      health === 'healthy'
                        ? '#22c55e'
                        : health === 'unhealthy'
                          ? '#ef4444'
                          : '#6b7280',
                  }}
                />
              </div>
            </div>
          </div>
        );
      }
      case 'tool': {
        const toolName = String(selection.data?.tool_name ?? '');
        const risk = selection.data?.risk_category
          ? String(selection.data.risk_category)
          : null;
        const rc = getRiskColor(risk);
        const displayName = toolName.includes('__')
          ? toolName.split('__').slice(1).join('__')
          : toolName;
        return (
          <div>
            <div className="flex items-center gap-2">
              <Wrench className="w-4 h-4 text-gray-400" />
              <p className="text-sm font-semibold text-white">{displayName}</p>
            </div>
            <div className="flex items-center gap-2 mt-1.5">
              {risk && (
                <span
                  className="text-[9px] uppercase tracking-wider font-semibold px-2 py-0.5 rounded"
                  style={{ background: rc.bg, color: rc.text }}
                >
                  {risk}
                </span>
              )}
              <span className="text-[10px] text-gray-500">
                {Number(selection.data?.call_count ?? 0).toLocaleString()} calls
              </span>
            </div>
            {toolName.includes('__') && (
              <p className="text-[10px] text-gray-600 mt-1 font-mono">{toolName}</p>
            )}
          </div>
        );
      }
      case 'app-backend-edge': {
        const srcMeta = getAppMeta(selection.sourceName ?? '');
        return (
          <div>
            <div className="flex items-center gap-2 text-sm">
              <span className="font-semibold text-white">{srcMeta.label}</span>
              <ArrowRight className="w-3.5 h-3.5 text-gray-600" />
              <span className="font-semibold text-white">{selection.targetName}</span>
            </div>
            <p className="text-[10px] text-gray-500 mt-1">
              {(selection.callCount ?? 0).toLocaleString()} calls in this connection
            </p>
          </div>
        );
      }
      case 'backend-tool-edge': {
        const displayTarget = (selection.targetName ?? '').includes('__')
          ? (selection.targetName ?? '').split('__').slice(1).join('__')
          : (selection.targetName ?? '');
        return (
          <div>
            <div className="flex items-center gap-2 text-sm">
              <span className="font-semibold text-white">{selection.sourceName}</span>
              <ArrowRight className="w-3.5 h-3.5 text-gray-600" />
              <span className="font-semibold text-white">{displayTarget}</span>
            </div>
            <p className="text-[10px] text-gray-500 mt-1">
              {(selection.callCount ?? 0).toLocaleString()} calls in this connection
            </p>
          </div>
        );
      }
    }
  };

  return (
    <div
      className="absolute top-2 right-2 bottom-2 w-80 z-20 rounded-xl border overflow-hidden flex flex-col"
      style={{
        background: 'linear-gradient(180deg, rgba(15,15,23,0.98) 0%, rgba(10,10,15,0.98) 100%)',
        borderColor: 'rgba(30,30,46,0.6)',
        backdropFilter: 'blur(20px)',
        boxShadow: '0 8px 32px rgba(0,0,0,0.6)',
      }}
    >
      {/* Header */}
      <div className="flex items-start justify-between p-4 border-b border-white/5">
        <div className="flex-1 min-w-0">{renderHeader()}</div>
        <button
          onClick={onClose}
          className="p-1 text-gray-600 hover:text-gray-300 rounded transition-colors shrink-0 ml-2"
        >
          <X className="w-4 h-4" />
        </button>
      </div>

      {/* Events */}
      <div className="flex-1 overflow-y-auto">
        <div className="px-4 pt-3 pb-1">
          <p className="text-[10px] uppercase tracking-widest text-gray-600 font-semibold">
            Recent Events
          </p>
        </div>
        {loadingEvents && events.length === 0 ? (
          <div className="flex items-center justify-center py-8">
            <Loader2 className="w-4 h-4 text-accent animate-spin" />
          </div>
        ) : events.length === 0 ? (
          <div className="px-4 py-6 text-center">
            <p className="text-xs text-gray-600">No events found</p>
          </div>
        ) : (
          <div className="px-3 pb-3 space-y-1">
            {events.map((evt, idx) => (
              <div
                key={evt.event_id}
                className="px-3 py-2.5 rounded-lg border border-white/[0.03] hover:border-white/[0.06] transition-all duration-300"
                style={{
                  background: idx === 0 ? 'rgba(124,92,252,0.06)' : 'rgba(255,255,255,0.02)',
                  borderColor: idx === 0 ? 'rgba(124,92,252,0.15)' : undefined,
                  animation: idx === 0 ? 'fadeSlideIn 0.3s ease' : undefined,
                }}
              >
                <div className="flex items-center gap-2">
                  {statusIcon(evt.status)}
                  <span className="text-[11px] font-medium text-gray-300 truncate flex-1">
                    {evt.tool_name.includes('__')
                      ? evt.tool_name.split('__').slice(1).join('__')
                      : evt.tool_name}
                  </span>
                  <span className="text-[9px] text-gray-600 shrink-0 tabular-nums">
                    {evt.duration_ms != null ? `${Math.round(evt.duration_ms)}ms` : ''}
                  </span>
                </div>
                <div className="flex items-center gap-2 mt-1">
                  <Clock className="w-2.5 h-2.5 text-gray-700" />
                  <span className="text-[9px] text-gray-600">{timeAgo(evt.timestamp)}</span>
                  {evt.application && (
                    <>
                      <span className="text-[9px] text-gray-700">via</span>
                      <span className="text-[9px] text-gray-500">{getAppMeta(evt.application).label}</span>
                    </>
                  )}
                </div>
                {evt.error_message && (
                  <p className="text-[9px] text-red-400/70 mt-1 truncate">{evt.error_message}</p>
                )}
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}

// ── Derive WebSocket URL ─────────────────────────────────────────────
function getLiveWsUrl(): string {
  const token = localStorage.getItem('mcpgw_token') ?? '';
  // If a gateway URL is configured, connect directly to the server (bypasses dashboard proxy)
  const gatewayUrl = localStorage.getItem('mcpgw_gateway_url');
  if (gatewayUrl) {
    try {
      const parsed = new URL(gatewayUrl);
      const wsProto = parsed.protocol === 'https:' ? 'wss:' : 'ws:';
      return `${wsProto}//${parsed.host}/api/v1/ws/live?token=${encodeURIComponent(token)}`;
    } catch { /* fall through to default */ }
  }
  // Fallback: same origin as the dashboard
  const proto = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
  const host = window.location.host;
  return `${proto}//${host}/api/v1/ws/live?token=${encodeURIComponent(token)}`;
}

// ── Lightweight event type from the server broadcast ───────────────
interface LiveEvent {
  type: string;
  tool_name?: string;
  backend_name?: string;
  application?: string;
  risk_category?: string;
  timestamp?: string;
  status?: string;
  duration_ms?: number;
  error_message?: string;
  user_id?: string;
}

// ── Main component ─────────────────────────────────────────────────
interface Props {
  isAdmin: boolean;
}

export default function UsageGraph({ isAdmin }: Props) {
  const [graphData, setGraphData] = useState<UsageGraphData | null>(null);
  const [connections, setConnections] = useState<ConnectionStatus[]>([]);
  const [users, setUsers] = useState<User[]>([]);
  const [selectedUserId, setSelectedUserId] = useState<string>('');
  const [range, setRange] = useState<string>('24h');
  const [loading, setLoading] = useState(true);
  const [expandedBackend, setExpandedBackend] = useState<string | null>(null);
  const [livePulse, setLivePulse] = useState(false);
  const [wsConnected, setWsConnected] = useState(false);

  // Detail panel state
  const [selection, setSelection] = useState<SelectionInfo | null>(null);
  const [auditEvents, setAuditEvents] = useState<AuditEvent[]>([]);
  const [loadingAudit, setLoadingAudit] = useState(false);

  // Refs for stable access inside WS callbacks
  const lastAuditParams = useRef<Record<string, string>>({});
  const selectionRef = useRef<SelectionInfo | null>(null);
  useEffect(() => { selectionRef.current = selection; }, [selection]);

  useEffect(() => {
    if (isAdmin) {
      api.getUsers().then(setUsers).catch(() => {});
    }
  }, [isAdmin]);

  const loadGraph = useCallback(async () => {
    setLoading(true);
    try {
      const [data, conn] = await Promise.all([
        api.getUsageGraph(selectedUserId || undefined, range),
        api.getConnections(selectedUserId || undefined),
      ]);
      setGraphData(data);
      setConnections(conn);
    } catch (err) {
      console.error('Failed to load usage graph:', err);
      setGraphData(null);
    } finally {
      setLoading(false);
    }
  }, [selectedUserId, range]);

  useEffect(() => {
    loadGraph();
  }, [loadGraph]);

  // ── Live feed: WebSocket with polling fallback ─────────────────
  const [liveMode, setLiveMode] = useState<'ws' | 'polling' | 'connecting'>('connecting');

  useEffect(() => {
    let ws: WebSocket | null = null;
    let reconnectTimer: ReturnType<typeof setTimeout> | null = null;
    let pollTimer: ReturnType<typeof setInterval> | null = null;
    let dead = false;
    let wsFailCount = 0;
    const WS_MAX_FAILURES = 2; // After 2 failed WS attempts, fall back to polling

    function handleLiveEvent(msg: LiveEvent) {
      if (msg.type !== 'tool_call' || !msg.tool_name) return;

      // Flash the live indicator
      setLivePulse(true);
      setTimeout(() => setLivePulse(false), 500);

      // Incrementally update graph data in-place
      setGraphData((prev) => {
        if (!prev) return prev;

        const ts = msg.timestamp ?? new Date().toISOString();

        // Update or add tool
        let tools = prev.tools;
        const toolIdx = tools.findIndex((t) => t.tool_name === msg.tool_name);
        if (toolIdx >= 0) {
          tools = tools.map((t, i) =>
            i === toolIdx ? { ...t, call_count: t.call_count + 1, last_call: ts } : t,
          );
        } else if (msg.tool_name && msg.backend_name) {
          tools = [...tools, {
            tool_name: msg.tool_name,
            backend_name: msg.backend_name,
            risk_category: msg.risk_category,
            call_count: 1,
            last_call: ts,
          }];
        }

        // Update or add application
        let applications = prev.applications;
        const appIdx = applications.findIndex((a) => a.application === msg.application);
        if (appIdx >= 0) {
          applications = applications.map((a, i) =>
            i === appIdx ? { ...a, call_count: a.call_count + 1 } : a,
          );
        } else if (msg.application) {
          applications = [...applications, {
            application: msg.application,
            call_count: 1,
            is_connected: true,
          }];
        }

        // Update or add app→backend edge
        let app_to_backend = prev.app_to_backend;
        const abIdx = app_to_backend.findIndex(
          (e) => e.source === msg.application && e.target === msg.backend_name,
        );
        if (abIdx >= 0) {
          app_to_backend = app_to_backend.map((e, i) =>
            i === abIdx ? { ...e, call_count: e.call_count + 1, last_call: ts } : e,
          );
        } else if (msg.application && msg.backend_name) {
          app_to_backend = [...app_to_backend, {
            source: msg.application,
            target: msg.backend_name,
            call_count: 1,
            last_call: ts,
          }];
        }

        // Update or add backend→tool edge
        let backend_to_tool = prev.backend_to_tool;
        const btIdx = backend_to_tool.findIndex(
          (e) => e.source === msg.backend_name && e.target === msg.tool_name,
        );
        if (btIdx >= 0) {
          backend_to_tool = backend_to_tool.map((e, i) =>
            i === btIdx ? { ...e, call_count: e.call_count + 1, last_call: ts } : e,
          );
        } else if (msg.backend_name && msg.tool_name) {
          backend_to_tool = [...backend_to_tool, {
            source: msg.backend_name,
            target: msg.tool_name,
            call_count: 1,
            last_call: ts,
          }];
        }

        return { ...prev, tools, applications, app_to_backend, backend_to_tool };
      });

      // Update connection status for this app
      setConnections((prev) =>
        prev.map((c) =>
          c.application === msg.application
            ? { ...c, is_connected: true, last_seen: msg.timestamp }
            : c,
        ),
      );

      // Refresh audit events if the detail panel is open and matches this event
      const sel = selectionRef.current;
      if (sel && Object.keys(lastAuditParams.current).length > 0) {
        const p = lastAuditParams.current;
        const matches =
          (p.tool_name && p.tool_name === msg.tool_name) ||
          (p.backend && p.backend === msg.backend_name) ||
          (p.application && p.application === msg.application);
        if (matches) {
          api.getAuditEvents({ ...p, limit: '15' })
            .then((res) => setAuditEvents(res.events))
            .catch(() => {});
        }
      }
    }

    // ── Polling fallback: re-fetch full graph data every 5s ──────
    function startPolling() {
      if (pollTimer || dead) return;
      setLiveMode('polling');
      setWsConnected(true); // show as connected (polling)
      pollTimer = setInterval(async () => {
        try {
          const [data, conn] = await Promise.all([
            api.getUsageGraph(selectedUserId || undefined, range),
            api.getConnections(selectedUserId || undefined),
          ]);
          setGraphData(data);
          setConnections(conn);
        } catch { /* ignore */ }
      }, 5000);
    }

    function stopPolling() {
      if (pollTimer) { clearInterval(pollTimer); pollTimer = null; }
    }

    // ── WebSocket connection ─────────────────────────────────────
    function connectWs() {
      if (dead) return;
      try {
        ws = new WebSocket(getLiveWsUrl());
      } catch {
        wsFailCount++;
        if (wsFailCount >= WS_MAX_FAILURES) { startPolling(); return; }
        reconnectTimer = setTimeout(connectWs, 3000);
        return;
      }

      ws.onopen = () => {
        wsFailCount = 0;
        stopPolling();
        setWsConnected(true);
        setLiveMode('ws');
      };

      ws.onmessage = (evt) => {
        let msg: LiveEvent;
        try { msg = JSON.parse(evt.data as string); } catch { return; }
        handleLiveEvent(msg);
      };

      ws.onclose = () => {
        setWsConnected(false);
        if (!dead) {
          wsFailCount++;
          if (wsFailCount >= WS_MAX_FAILURES) {
            startPolling();
          } else {
            setLiveMode('connecting');
            reconnectTimer = setTimeout(connectWs, 3000);
          }
        }
      };

      ws.onerror = () => { ws?.close(); };
    }

    connectWs();

    return () => {
      dead = true;
      if (reconnectTimer) clearTimeout(reconnectTimer);
      stopPolling();
      ws?.close();
    };
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selectedUserId, range]);

  // Fetch audit events for the current selection
  const fetchAudit = useCallback(async (params: Record<string, string>) => {
    lastAuditParams.current = params;
    setLoadingAudit(true);
    try {
      const res = await api.getAuditEvents({ ...params, limit: '15' });
      setAuditEvents(res.events);
    } catch {
      setAuditEvents([]);
    } finally {
      setLoadingAudit(false);
    }
  }, []);

  // While the detail panel is open, silently refresh its events every 5s
  // (WS handles instant updates on matching calls; this catches anything missed)
  useEffect(() => {
    if (!selection) return;
    const interval = setInterval(() => {
      if (Object.keys(lastAuditParams.current).length === 0) return;
      api.getAuditEvents({ ...lastAuditParams.current, limit: '15' })
        .then((res) => setAuditEvents(res.events))
        .catch(() => {});
    }, 5000);
    return () => clearInterval(interval);
  }, [selection]);

  // Build graph nodes and edges
  const { nodes, edges } = useMemo(() => {
    if (!graphData)
      return { nodes: [] as Node[], edges: [] as Edge[] };

    const flowNodes: Node[] = [];
    const flowEdges: Edge[] = [];

    const COL_APP = 50;
    const COL_BACKEND = 450;
    const COL_TOOL = 850;
    const ROW_SPACING = 90;

    // All tools with calls for the expanded backend (sorted by most recent, then count)
    const expandedTools = expandedBackend
      ? graphData.tools
          .filter((t) => t.backend_name === expandedBackend && t.call_count > 0)
          .sort((a, b) => {
            if (a.last_call && b.last_call) return b.last_call.localeCompare(a.last_call);
            if (a.last_call) return -1;
            if (b.last_call) return 1;
            return b.call_count - a.call_count;
          })
      : [];

    // Vertical centering — tools only shown when a backend is expanded
    const maxRows = Math.max(
      graphData.applications.length,
      graphData.backends.length,
      expandedTools.length,
      1,
    );
    const getYOffset = (count: number) =>
      ((maxRows - count) * ROW_SPACING) / 2 + 40;

    // App nodes
    const appOffset = getYOffset(graphData.applications.length);
    graphData.applications.forEach((app, i) => {
      const conn = connections.find((c) => c.application === app.application);
      flowNodes.push({
        id: `app-${app.application}`,
        type: 'appNode',
        position: { x: COL_APP, y: i * ROW_SPACING + appOffset },
        data: {
          application: app.application,
          is_connected: conn?.is_connected ?? app.is_connected,
          call_count: app.call_count,
        },
      });
    });

    // Backend nodes
    const backendOffset = getYOffset(graphData.backends.length);
    graphData.backends.forEach((b, i) => {
      flowNodes.push({
        id: `backend-${b.backend_name}`,
        type: 'backendNode',
        position: { x: COL_BACKEND, y: i * ROW_SPACING + backendOffset },
        data: {
          backend_name: b.backend_name,
          transport: b.transport,
          health_status: b.health_status,
          tool_count: b.tool_count,
          expanded: expandedBackend === b.backend_name,
        },
      });
    });

    // Tool nodes — only for expanded backend's called tools
    if (expandedBackend && expandedTools.length > 0) {
      // Position tools vertically centered around the expanded backend node
      const expandedBackendIdx = graphData.backends.findIndex(
        (b) => b.backend_name === expandedBackend,
      );
      const backendY = expandedBackendIdx * ROW_SPACING + backendOffset;
      const toolBlockHeight = (expandedTools.length - 1) * ROW_SPACING;
      const toolStartY = backendY - toolBlockHeight / 2;

      expandedTools.forEach((t, i) => {
        flowNodes.push({
          id: `tool-${t.tool_name}`,
          type: 'toolNode',
          position: { x: COL_TOOL, y: toolStartY + i * ROW_SPACING },
          data: {
            tool_name: t.tool_name,
            backend_name: t.backend_name,
            risk_category: t.risk_category,
            call_count: t.call_count,
            animIndex: i,
          },
        });
      });
    }

    // Edge thickness scaling
    const allCounts = [
      ...graphData.app_to_backend.map((e) => e.call_count),
      ...graphData.backend_to_tool.map((e) => e.call_count),
    ];
    const maxCalls = Math.max(...allCounts, 1);

    // App -> Backend edges
    graphData.app_to_backend.forEach((edge, i) => {
      const meta = getAppMeta(edge.source);
      const thickness = Math.max(1.5, Math.min((edge.call_count / maxCalls) * 5, 5));
      flowEdges.push({
        id: `ab-${i}-${edge.source}-${edge.target}`,
        source: `app-${edge.source}`,
        target: `backend-${edge.target}`,
        animated: true,
        style: { stroke: meta.color, strokeWidth: thickness, opacity: 0.55 },
        data: { sourceName: edge.source, targetName: edge.target, callCount: edge.call_count },
      });
    });

    // Backend -> Tool edges (only for expanded backend's tools)
    if (expandedBackend) {
      graphData.backend_to_tool.forEach((edge, i) => {
        if (edge.source !== expandedBackend) return;
        const toolExists = expandedTools.some((t) => t.tool_name === edge.target);
        if (!toolExists) return;
        const thickness = Math.max(1, Math.min((edge.call_count / maxCalls) * 4, 4));
        flowEdges.push({
          id: `bt-${i}-${edge.source}-${edge.target}`,
          source: `backend-${edge.source}`,
          target: `tool-${edge.target}`,
          animated: true,
          style: { stroke: '#7c5cfc', strokeWidth: thickness, opacity: 0.45 },
          data: { sourceName: edge.source, targetName: edge.target, callCount: edge.call_count },
        });
      });
    }

    return { nodes: flowNodes, edges: flowEdges };
  }, [graphData, connections, expandedBackend]);

  // Event handlers
  const onNodeClick = useCallback(
    (_event: React.MouseEvent, node: Node) => {
      const nodeType =
        node.type === 'appNode' ? 'app' : node.type === 'backendNode' ? 'backend' : 'tool';

      // Toggle backend expansion when clicking a backend node
      if (nodeType === 'backend') {
        const backendName = String(node.data.backend_name);
        setExpandedBackend((prev) => (prev === backendName ? null : backendName));
      }

      setSelection({ type: nodeType as SelectionInfo['type'], data: node.data as Record<string, unknown> });

      // Fetch relevant audit events
      const params: Record<string, string> = {};
      if (nodeType === 'app') params.application = String(node.data.application);
      if (nodeType === 'backend') params.backend = String(node.data.backend_name);
      if (nodeType === 'tool') params.tool_name = String(node.data.tool_name);
      fetchAudit(params);
    },
    [fetchAudit],
  );

  const onEdgeClick = useCallback(
    (_event: React.MouseEvent, edge: Edge) => {
      const isAppBackend = edge.id.startsWith('ab-');
      const edgeData = edge.data as Record<string, unknown> | undefined;
      const sourceName = String(edgeData?.sourceName ?? '');
      const targetName = String(edgeData?.targetName ?? '');
      const callCount = Number(edgeData?.callCount ?? 0);

      setSelection({
        type: isAppBackend ? 'app-backend-edge' : 'backend-tool-edge',
        sourceName,
        targetName,
        callCount,
      });

      // Fetch audit events for this connection
      const params: Record<string, string> = {};
      if (isAppBackend) {
        params.application = sourceName;
        params.backend = targetName;
      } else {
        params.backend = sourceName;
        params.tool_name = targetName;
      }
      fetchAudit(params);
    },
    [fetchAudit],
  );

  const onPaneClick = useCallback(() => {
    setSelection(null);
    setExpandedBackend(null);
    setAuditEvents([]);
    lastAuditParams.current = {};
  }, []);

  return (
    <div className="h-screen flex flex-col -m-8">
      {/* ── Top bar ─────────────────────────────────────────────── */}
      <div
        className="flex items-center gap-3 px-6 py-3 shrink-0 border-b"
        style={{
          background: 'linear-gradient(180deg, #0f0f17 0%, #0a0a0f 100%)',
          borderColor: 'rgba(30,30,46,0.5)',
        }}
      >
        {/* Title + summary */}
        <div className="flex items-center gap-4 mr-auto">
          <div className="flex items-center gap-2">
            <h2 className="text-sm font-semibold text-white tracking-tight">Usage Graph</h2>
            {/* Live status indicator */}
            <div className="flex items-center gap-1.5">
              <div className="relative flex items-center justify-center w-2 h-2">
                {wsConnected && (
                  <div
                    className={clsx(
                      'absolute inset-0 rounded-full transition-opacity duration-300',
                      livePulse ? 'animate-ping opacity-70' : 'opacity-0',
                    )}
                    style={{ background: liveMode === 'ws' ? '#22c55e' : '#3b82f6' }}
                  />
                )}
                <div
                  className="w-1.5 h-1.5 rounded-full transition-colors duration-500"
                  style={{ background: wsConnected ? (liveMode === 'ws' ? '#22c55e' : '#3b82f6') : '#52525b' }}
                />
              </div>
              <span
                className={clsx(
                  'text-[9px] uppercase tracking-widest font-semibold transition-colors duration-500',
                  liveMode === 'ws' ? 'text-emerald-600' : liveMode === 'polling' ? 'text-blue-500' : 'text-zinc-600',
                )}
              >
                {liveMode === 'ws' ? 'live' : liveMode === 'polling' ? 'auto-refresh' : 'connecting…'}
              </span>
            </div>
          </div>
        </div>

        {/* User selector (admin) */}
        {isAdmin && users.length > 0 && (
          <div className="flex items-center gap-1.5">
            <UsersIcon className="w-3.5 h-3.5 text-gray-600" />
            <select
              value={selectedUserId}
              onChange={(e) => setSelectedUserId(e.target.value)}
              className="px-2 py-1 bg-transparent border border-border rounded-lg text-[11px] text-gray-300 focus:outline-none focus:border-accent/40 appearance-none pr-5 cursor-pointer"
            >
              <option value="">Current user</option>
              {users.map((u) => (
                <option key={u.user_id} value={u.user_id}>
                  {u.username}
                </option>
              ))}
            </select>
          </div>
        )}

        {/* Range selector */}
        <div className="flex items-center rounded-lg overflow-hidden border border-border">
          {['24h', '7d', '30d'].map((r) => (
            <button
              key={r}
              onClick={() => setRange(r)}
              className={clsx(
                'px-3 py-1.5 text-[11px] font-medium transition-all',
                range === r
                  ? 'bg-accent/15 text-accent'
                  : 'text-gray-500 hover:text-gray-300 hover:bg-surface-hover',
              )}
            >
              {r}
            </button>
          ))}
        </div>

        {/* Refresh */}
        <button
          onClick={loadGraph}
          disabled={loading}
          className="p-1.5 text-gray-600 hover:text-gray-300 rounded-md transition-colors disabled:opacity-30"
        >
          <RefreshCw className={clsx('w-3.5 h-3.5', loading && 'animate-spin')} />
        </button>
      </div>

      {/* ── Canvas ──────────────────────────────────────────────── */}
      <div className="flex-1 relative">
        {loading ? (
          <div className="flex items-center justify-center h-full">
            <div className="flex flex-col items-center gap-3">
              <div className="w-8 h-8 rounded-xl bg-accent/10 flex items-center justify-center animate-pulse">
                <RefreshCw className="w-4 h-4 text-accent animate-spin" />
              </div>
              <p className="text-xs text-gray-600">Loading graph data...</p>
            </div>
          </div>
        ) : nodes.length === 0 ? (
          <div className="flex items-center justify-center h-full">
            <div className="text-center">
              <div className="w-12 h-12 mx-auto rounded-2xl bg-surface border border-border flex items-center justify-center mb-3">
                <Layers className="w-5 h-5 text-gray-600" />
              </div>
              <p className="text-sm text-gray-400">No usage data</p>
              <p className="text-xs text-gray-600 mt-1">
                Make tool calls to see the graph populate
              </p>
            </div>
          </div>
        ) : (
          <>
            <ReactFlow
              nodes={nodes}
              edges={edges}
              nodeTypes={nodeTypes}
              onNodeClick={onNodeClick}
              onEdgeClick={onEdgeClick}
              onPaneClick={onPaneClick}
              fitView
              fitViewOptions={{ padding: 0.15 }}
              proOptions={{ hideAttribution: true }}
              defaultEdgeOptions={{ type: 'default' }}
              minZoom={0.3}
              maxZoom={1.5}
              style={{ background: '#0a0a0f' }}
            >
              <Background color="#14141f" gap={24} size={1} />
              <Controls
                showInteractive={false}
                className="!bg-surface/80 !border-border !rounded-xl !shadow-2xl [&>button]:!bg-transparent [&>button]:!border-border/50 [&>button]:!text-gray-500 [&>button:hover]:!text-white [&>button:hover]:!bg-surface-hover [&>button]:!rounded-lg [&>button]:!w-8 [&>button]:!h-8"
              />
            </ReactFlow>

            {/* Detail panel overlay */}
            {selection && (
              <DetailPanel
                selection={selection}
                events={auditEvents}
                loadingEvents={loadingAudit}
                onClose={onPaneClick}
              />
            )}
          </>
        )}
      </div>
    </div>
  );
}
