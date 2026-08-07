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
  Laptop,
} from 'lucide-react';
import clsx from 'clsx';

// ── App metadata with proper names, colors, and icons ──────────────
// color: brand accent — used for glow, handle dot, fallback icon tint. Must be visible on dark bg.
// lineColor: edge stroke color (defaults to color). Use a lighter shade for dark-branded apps.
// iconBg: solid background for the icon box (overrides the default transparent tint).
const APP_META: Record<string, { label: string; color: string; lineColor?: string; iconBg?: string; Icon: LucideIcon }> = {
  claude:        { label: 'Claude Code',    color: '#da7756', Icon: Terminal },                                          // Anthropic terracotta
  claudedesktop: { label: 'Claude Desktop', color: '#da7756', Icon: Monitor },                                          // Anthropic terracotta
  cursor:        { label: 'Cursor',         color: '#9ca3af', lineColor: '#9ca3af', iconBg: '#1a1a1a', Icon: MousePointer2 }, // dark brand → grey lines
  vscode:        { label: 'VS Code',        color: '#007ACC', Icon: Code2 },                                            // official VS Code blue
  openwebui:     { label: 'Open WebUI',     color: '#e0e0e0', lineColor: '#d4d4d4', iconBg: '#111111', Icon: Globe },    // dark icon → white/light lines
  clawbot:       { label: 'Clawbot',        color: '#b07ce8', Icon: Bot },
  codex:         { label: 'Codex',          color: '#10A37F', iconBg: '#0d0d0d', Icon: Terminal },                       // OpenAI Codex
  lmstudio:      { label: 'LM Studio',      color: '#6F42C1', iconBg: '#6F42C1', Icon: Cpu },                           // LM Studio purple
};

const APP_ICON_URLS: Record<string, string> = {
  claude: 'https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons/svg/claude-ai.svg',
  claudedesktop: 'https://cdn.simpleicons.org/anthropic/da7756',
  cursor: 'https://cdn.simpleicons.org/cursor/ffffff',
  vscode: 'https://cdn.jsdelivr.net/gh/devicons/devicon/icons/vscode/vscode-original.svg',
  openwebui: 'https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons/svg/open-webui.svg',
  codex: 'https://unpkg.com/@lobehub/icons-static-png@latest/dark/codex.png',
  lmstudio: 'https://unpkg.com/@lobehub/icons-static-png@latest/dark/lmstudio.png',
  clawbot: 'https://cdn.jsdelivr.net/gh/twitter/twemoji/assets/svg/1f980.svg',
};

const fallbackAppMeta = { label: '', color: '#6b7280', lineColor: undefined as string | undefined, iconBg: undefined as string | undefined, Icon: MessageSquare as LucideIcon };

const getAppMeta = (key: string) => {
  const meta = APP_META[key];
  if (meta) return meta;
  // Try to get label from connectors
  const label = APP_LABELS[key as AppSlug];
  return { ...fallbackAppMeta, label: label || key };
};

// ── Risk category colors & labels ──────────────────────────────────
const RISK_COLORS: Record<string, { bg: string; text: string; dot: string }> = {
  read:         { bg: 'var(--neutral-wash)', text: 'var(--text-4)', dot: 'var(--text-4)' },
  write:        { bg: 'var(--neutral-wash)', text: 'var(--text-3)', dot: 'var(--text-3)' },
  execute:      { bg: 'var(--neutral-wash)', text: 'var(--text-2)', dot: 'var(--text-2)' },
  admin:        { bg: 'var(--warn-wash)', text: 'var(--warn)', dot: 'var(--warn)' },
  destructive:  { bg: 'var(--deny-wash)', text: 'var(--deny)', dot: 'var(--deny)' },
  'external-api': { bg: 'var(--neutral-wash)', text: 'var(--text-3)', dot: 'var(--text-3)' },
  unclassified: { bg: 'transparent', text: 'var(--text-4)', dot: 'var(--line-strong)' },
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
          className="absolute -inset-1 rounded-panel opacity-20 blur-md transition-opacity duration-300 group-hover:opacity-35 pointer-events-none"
          style={{ background: glowColor }}
        />
      )}
      <div
        className={clsx(
          'relative px-4 py-3 rounded-panel min-w-[160px] border transition-all duration-200 group-hover:border-white/10 cursor-pointer',
          selected && 'ring-1 ring-beam-edge/50',
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

// ── Reusable App Icon ──────────────────────────────────────────────
// Renders the actual brand icon with proper background, falling back to Lucide.
function AppIcon({ appKey, size = 8 }: { appKey: string; size?: number }) {
  const meta = getAppMeta(appKey);
  const iconUrl = APP_ICON_URLS[appKey];
  const [imgFailed, setImgFailed] = useState(false);
  const { Icon } = meta;
  const boxPx = size * 4;       // tailwind size units → px (8 = 32px)
  const imgPx = boxPx * 0.625;  // icon inside the box
  const fbPx  = boxPx * 0.5;    // fallback lucide size

  return (
    <div
      className="rounded-row flex items-center justify-center shrink-0 overflow-hidden"
      style={{
        width: boxPx, height: boxPx,
        background: meta.iconBg || `${meta.color}18`,
        border: `1px solid ${meta.iconBg ? meta.iconBg : meta.color + '25'}`,
      }}
    >
      {iconUrl && !imgFailed ? (
        <img
          src={iconUrl}
          alt={meta.label}
          style={{ width: imgPx, height: imgPx }}
          className="object-contain"
          onError={() => setImgFailed(true)}
        />
      ) : (
        <Icon style={{ width: fbPx, height: fbPx, color: meta.color }} />
      )}
    </div>
  );
}

// ── User Node ──────────────────────────────────────────────────────
// Leftmost column: shows which user accessed which application.
const USER_NODE_COLOR = 'var(--text-3)'; // structure, not identity — the apps carry the colour here
function UserNodeComponent({ data, selected }: NodeProps) {
  const username = String(data.username || 'unknown');
  const calls = Number(data.call_count) || 0;
  const initials = username.trim().slice(0, 2).toUpperCase() || '?';

  return (
    <NodeShell glowColor={calls > 0 ? `${USER_NODE_COLOR}20` : undefined} selected={selected}>
      <Handle
        type="source"
        position={Position.Right}
        className="!w-2.5 !h-2.5 !border-2 !border-panel !rounded-full"
        style={{ background: USER_NODE_COLOR }}
      />
      <div className="flex items-center gap-3">
        <div
          className="w-8 h-8 rounded-full flex items-center justify-center shrink-0 text-2xs font-semibold"
          style={{
            background: `${USER_NODE_COLOR}18`,
            border: `1px solid ${USER_NODE_COLOR}30`,
            color: USER_NODE_COLOR,
          }}
        >
          {initials}
        </div>
        <div className="min-w-0">
          <span className="text-sm font-semibold text-ink truncate block">{username}</span>
          <p className="text-micro text-ink-3 mt-0.5 tabular-nums">
            {calls > 0 ? `${calls.toLocaleString()} calls` : 'no activity'}
          </p>
        </div>
      </div>
    </NodeShell>
  );
}

// ── App Node ───────────────────────────────────────────────────────
function AppNodeComponent({ data, selected }: NodeProps) {
  const appKey = String(data.application);
  const meta = getAppMeta(appKey);
  const connected = Boolean(data.is_connected);
  const calls = Number(data.call_count) || 0;

  return (
    <NodeShell glowColor={connected ? meta.color : undefined} selected={selected}>
      {/* Target handle for optional incoming user → app edges. */}
      <Handle
        type="target"
        position={Position.Left}
        className="!w-2 !h-2 !border-2 !border-panel !rounded-full !opacity-60"
        style={{ background: USER_NODE_COLOR }}
      />
      <Handle
        type="source"
        position={Position.Right}
        className="!w-2.5 !h-2.5 !border-2 !border-panel !rounded-full"
        style={{ background: meta.lineColor || meta.color }}
      />
      <div className="flex items-center gap-3">
        <AppIcon appKey={appKey} />
        <div className="min-w-0">
          <div className="flex items-center gap-2">
            <span className="text-sm font-semibold text-ink truncate">
              {meta.label}
            </span>
            <div className="relative flex items-center justify-center w-2 h-2 shrink-0">
              {connected && (
                <div
                  className="absolute inset-0 rounded-full animate-ping opacity-40"
                  style={{ background: 'var(--beam)' }}
                />
              )}
              <div
                className="w-2 h-2 rounded-full"
                style={{ background: connected ? 'var(--beam)' : 'var(--line-strong)' }}
              />
            </div>
          </div>
          <p className="text-micro text-ink-3 mt-0.5 tabular-nums">
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
    health === 'healthy' ? 'var(--beam)' : health === 'unhealthy' ? 'var(--deny)' : 'var(--text-3)';

  return (
    <NodeShell glowColor={health === 'healthy' ? 'var(--beam-wash)' : undefined} selected={selected}>
      <Handle
        type="target"
        position={Position.Left}
        className="!w-2.5 !h-2.5 !border-2 !border-panel !rounded-full"
        style={{ background: 'var(--line-strong)' }}
      />
      {expanded && (
        <Handle
          type="source"
          position={Position.Right}
          className="!w-2.5 !h-2.5 !border-2 !border-panel !rounded-full"
          style={{ background: 'var(--line-strong)' }}
        />
      )}
      <div className="flex items-center gap-3">
        <div className="w-8 h-8 rounded-row bg-beam-wash flex items-center justify-center shrink-0 border border-beam-edge">
          {transport === 'agent' ? <Laptop className="w-4 h-4 text-beam" /> : <Server className="w-4 h-4 text-beam" />}
        </div>
        <div className="min-w-0">
          <div className="flex items-center gap-2">
            <span className="text-sm font-semibold text-ink truncate">{name}</span>
            <div className="w-2 h-2 rounded-full shrink-0" style={{ background: healthColor }} />
          </div>
          <div className="flex items-center gap-1.5 mt-0.5">
            <span
              className="text-micro uppercase tracking-wider font-medium px-1.5 py-px rounded-control"
              style={{
                background: 'var(--neutral-wash)',
                color: 'var(--text-3)',
                border: '1px solid var(--line)',
              }}
            >
              {transport}
            </span>
            <span className="text-micro text-ink-3">{toolCount} tools</span>
          </div>
        </div>
        <ChevronRight
          className="w-4 h-4 text-ink-3 shrink-0 transition-transform duration-300"
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
          className="!w-2.5 !h-2.5 !border-2 !border-panel !rounded-full"
          style={{ background: rc.dot }}
        />
        <div className="max-w-[180px]">
          <p className="text-xs font-medium text-ink truncate" title={toolName}>
            {displayName}
          </p>
          <div className="flex items-center gap-2 mt-1">
            {risk && (
              <span
                className="text-[8px] uppercase tracking-wider font-semibold px-1.5 py-px rounded-control"
                style={{ background: rc.bg, color: rc.text }}
              >
                {risk}
              </span>
            )}
            {calls > 0 && (
              <span className="text-micro text-ink-3 tabular-nums">
                {calls.toLocaleString()}
              </span>
            )}
          </div>
        </div>
      </NodeShell>
    </div>
  );
}

function SubBackendNodeComponent({ data, selected }: NodeProps) {
  const name = String(data.sub_name);
  const toolCount = Number(data.tool_count) || 0;
  const transport = String(data.transport || 'stdio');
  const expanded = Boolean(data.expanded);

  return (
    <div className="tool-node-enter" style={{ animationDelay: `${(Number(data.animIndex) || 0) * 0.06}s` }}>
      <NodeShell selected={selected}>
        <Handle type="target" position={Position.Left} className="!w-2.5 !h-2.5 !border-2 !border-panel !rounded-full" style={{ background: 'var(--line-strong)' }} />
        {expanded && (
          <Handle type="source" position={Position.Right} className="!w-2.5 !h-2.5 !border-2 !border-panel !rounded-full" style={{ background: 'var(--line-strong)' }} />
        )}
        <div className="flex items-center gap-3 max-w-[180px]">
          <div className="w-6 h-6 rounded-control bg-beam-wash flex items-center justify-center shrink-0 border border-beam-edge">
            <Server className="w-3 h-3 text-beam" />
          </div>
          <div className="min-w-0">
            <p className="text-xs font-medium text-ink truncate">{name}</p>
            <span className="text-micro text-ink-3">{toolCount} tools</span>
          </div>
          <ChevronRight
            className="w-3.5 h-3.5 text-ink-3 shrink-0 transition-transform duration-300"
            style={{ transform: expanded ? 'rotate(180deg)' : 'rotate(0deg)' }}
          />
        </div>
      </NodeShell>
    </div>
  );
}

const nodeTypes = {
  userNode: UserNodeComponent,
  appNode: AppNodeComponent,
  backendNode: BackendNodeComponent,
  toolNode: ToolNodeComponent,
  subBackendNode: SubBackendNodeComponent,
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
  type: 'user' | 'app' | 'backend' | 'tool' | 'sub-backend' | 'user-app-edge' | 'app-backend-edge' | 'backend-tool-edge' | 'backend-sub-edge' | 'sub-tool-edge';
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
    if (status === 'success') return <CheckCircle className="w-3 h-3 text-beam" />;
    if (status === 'error' || status === 'tool_error')
      return <XCircle className="w-3 h-3 text-deny" />;
    if (status === 'denied') return <ShieldOff className="w-3 h-3 text-warn" />;
    return <Activity className="w-3 h-3 text-ink-2" />;
  };

  const renderHeader = () => {
    switch (selection.type) {
      case 'user': {
        const username = String(selection.data?.username ?? 'unknown');
        const calls = Number(selection.data?.call_count) || 0;
        const initials = username.trim().slice(0, 2).toUpperCase() || '?';
        return (
          <div className="flex items-center gap-3">
            <div
              className="w-8 h-8 rounded-full flex items-center justify-center shrink-0 text-2xs font-semibold"
              style={{
                background: `${USER_NODE_COLOR}18`,
                border: `1px solid ${USER_NODE_COLOR}30`,
                color: USER_NODE_COLOR,
              }}
            >
              {initials}
            </div>
            <div>
              <p className="text-sm font-semibold text-ink">{username}</p>
              <span className="text-micro text-ink-3">
                {calls > 0 ? `${calls.toLocaleString()} calls` : 'no activity'} in range
              </span>
            </div>
          </div>
        );
      }
      case 'app': {
        const appKey = String(selection.data?.application ?? '');
        const meta = getAppMeta(appKey);
        const connected = Boolean(selection.data?.is_connected);
        return (
          <div className="flex items-center gap-3">
            <AppIcon appKey={appKey} />
            <div>
              <p className="text-sm font-semibold text-ink">{meta.label}</p>
              <div className="flex items-center gap-2 mt-0.5">
                <div
                  className="w-2 h-2 rounded-full"
                  style={{ background: connected ? 'var(--beam)' : 'var(--line-strong)' }}
                />
                <span className="text-micro text-ink-3">
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
            <div className="w-8 h-8 rounded-row bg-beam-wash flex items-center justify-center shrink-0 border border-beam-edge">
              <Server className="w-4 h-4 text-beam" />
            </div>
            <div>
              <p className="text-sm font-semibold text-ink">{name}</p>
              <div className="flex items-center gap-1.5 mt-0.5">
                <span
                  className="text-micro uppercase tracking-wider font-medium px-1.5 py-px rounded-control"
                  style={{ background: 'var(--neutral-wash)', color: 'var(--text-3)' }}
                >
                  {transport}
                </span>
                <div
                  className="w-2 h-2 rounded-full"
                  style={{
                    background:
                      health === 'healthy'
                        ? 'var(--beam)'
                        : health === 'unhealthy'
                          ? 'var(--deny)'
                          : 'var(--text-3)',
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
              <Wrench className="w-4 h-4 text-ink-2" />
              <p className="text-sm font-semibold text-ink">{displayName}</p>
            </div>
            <div className="flex items-center gap-2 mt-1.5">
              {risk && (
                <span
                  className="text-micro uppercase tracking-wider font-semibold px-2 py-0.5 rounded-control"
                  style={{ background: rc.bg, color: rc.text }}
                >
                  {risk}
                </span>
              )}
              <span className="text-micro text-ink-3">
                {Number(selection.data?.call_count ?? 0).toLocaleString()} calls
              </span>
            </div>
            {toolName.includes('__') && (
              <p className="text-micro text-ink-4 mt-1 font-mono">{toolName}</p>
            )}
          </div>
        );
      }
      case 'sub-backend': {
        const subName = String(selection.data?.sub_name ?? '');
        const subToolCount = Number(selection.data?.tool_count ?? 0);
        return (
          <div className="flex items-center gap-3">
            <div className="w-8 h-8 rounded-row bg-beam-wash flex items-center justify-center shrink-0 border border-beam-edge">
              <Server className="w-4 h-4 text-beam" />
            </div>
            <div>
              <p className="text-sm font-semibold text-ink">{subName}</p>
              <span className="text-micro text-ink-3">{subToolCount} tools</span>
            </div>
          </div>
        );
      }
      case 'user-app-edge': {
        const appKey = selection.targetName ?? '';
        const meta = getAppMeta(appKey);
        return (
          <div>
            <div className="flex items-center gap-2 text-sm">
              <UsersIcon className="w-4 h-4" style={{ color: USER_NODE_COLOR }} />
              <span className="font-semibold text-ink">User</span>
              <ArrowRight className="w-3.5 h-3.5 text-ink-4" />
              <AppIcon appKey={appKey} size={6} />
              <span className="font-semibold text-ink">{meta.label}</span>
            </div>
            <p className="text-micro text-ink-3 mt-1">
              {(selection.callCount ?? 0).toLocaleString()} calls in this connection
            </p>
          </div>
        );
      }
      case 'app-backend-edge': {
        const srcAppKey = selection.sourceName ?? '';
        const srcMeta = getAppMeta(srcAppKey);
        return (
          <div>
            <div className="flex items-center gap-2 text-sm">
              <AppIcon appKey={srcAppKey} size={6} />
              <span className="font-semibold text-ink">{srcMeta.label}</span>
              <ArrowRight className="w-3.5 h-3.5 text-ink-4" />
              <span className="font-semibold text-ink">{selection.targetName}</span>
            </div>
            <p className="text-micro text-ink-3 mt-1">
              {(selection.callCount ?? 0).toLocaleString()} calls in this connection
            </p>
          </div>
        );
      }
      case 'backend-sub-edge':
      case 'sub-tool-edge':
      case 'backend-tool-edge': {
        const displayTarget = (selection.targetName ?? '').includes('__')
          ? (selection.targetName ?? '').split('__').slice(1).join('__')
          : (selection.targetName ?? '');
        return (
          <div>
            <div className="flex items-center gap-2 text-sm">
              <span className="font-semibold text-ink">{selection.sourceName}</span>
              <ArrowRight className="w-3.5 h-3.5 text-ink-4" />
              <span className="font-semibold text-ink">{displayTarget}</span>
            </div>
            <p className="text-micro text-ink-3 mt-1">
              {(selection.callCount ?? 0).toLocaleString()} calls in this connection
            </p>
          </div>
        );
      }
    }
  };

  return (
    <div
      className="absolute top-2 right-2 bottom-2 w-80 z-20 rounded-card border overflow-hidden flex flex-col"
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
          className="p-1 text-ink-4 hover:text-ink-2 rounded-control transition-colors shrink-0 ml-2"
        >
          <X className="w-4 h-4" />
        </button>
      </div>

      {/* Events */}
      <div className="flex-1 overflow-y-auto">
        <div className="px-4 pt-3 pb-1">
          <p className="text-micro uppercase tracking-widest text-ink-4 font-semibold">
            Recent events
          </p>
        </div>
        {loadingEvents && events.length === 0 ? (
          <div className="flex items-center justify-center py-8">
            <Loader2 className="w-4 h-4 text-beam animate-spin" />
          </div>
        ) : events.length === 0 ? (
          <div className="px-4 py-6 text-center">
            <p className="text-xs text-ink-4">No recent events</p>
          </div>
        ) : (
          <div className="px-3 pb-3 space-y-1">
            {events.map((evt, idx) => (
              <div
                key={evt.event_id}
                className="px-3 py-2.5 rounded-row border border-white/[0.03] hover:border-white/[0.06] transition-all duration-300"
                style={{
                  background: idx === 0 ? 'rgba(124,92,252,0.06)' : 'rgba(255,255,255,0.02)',
                  borderColor: idx === 0 ? 'rgba(124,92,252,0.15)' : undefined,
                  animation: idx === 0 ? 'fadeSlideIn 0.3s ease' : undefined,
                }}
              >
                <div className="flex items-center gap-2">
                  {statusIcon(evt.status)}
                  <span className="text-2xs font-medium text-ink-2 truncate flex-1">
                    {evt.tool_name.includes('__')
                      ? evt.tool_name.split('__').slice(1).join('__')
                      : evt.tool_name}
                  </span>
                  <span className="text-micro text-ink-4 shrink-0 tabular-nums">
                    {evt.duration_ms != null ? `${Math.round(evt.duration_ms)}ms` : ''}
                  </span>
                </div>
                <div className="flex items-center gap-2 mt-1">
                  <Clock className="w-2.5 h-2.5 text-ink-4" />
                  <span className="text-micro text-ink-4">{timeAgo(evt.timestamp)}</span>
                  {evt.application && (
                    <>
                      <span className="text-micro text-ink-4">via</span>
                      <span className="text-micro text-ink-3">{getAppMeta(evt.application).label}</span>
                    </>
                  )}
                </div>
                {evt.error_message && (
                  <p className="text-micro text-deny mt-1 truncate">{evt.error_message}</p>
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
  const [expandedSubBackend, setExpandedSubBackend] = useState<string | null>(null);
  const [backendConfigs, setBackendConfigs] = useState<any[]>([]);
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

  useEffect(() => {
    api.getBackends().then(setBackendConfigs).catch(() => {});
  }, []);

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
          const prefix = auditToolPrefixRef.current;
          const fetchLimit = prefix ? '100' : '15';
          api.getAuditEvents({ ...p, limit: fetchLimit })
            .then((res) => {
              const filtered = applyToolPrefixFilter(res.events, prefix);
              setAuditEvents(prefix ? filtered.slice(0, 15) : filtered);
            })
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

  // Compute a "from" timestamp based on the selected range for audit queries
  const getAuditFrom = useCallback((): string => {
    const now = new Date();
    if (range === '7d') now.setDate(now.getDate() - 7);
    else if (range === '30d') now.setDate(now.getDate() - 30);
    else now.setHours(now.getHours() - 24);
    return now.toISOString();
  }, [range]);

  // Client-side tool prefix filter for sub-backend selections
  // When viewing a sub-backend, we fetch all events for the parent backend
  // and filter to only those whose tool_name matches the sub-backend prefix.
  const auditToolPrefixRef = useRef<string | null>(null);

  const applyToolPrefixFilter = (events: AuditEvent[], prefix: string | null): AuditEvent[] => {
    if (!prefix) return events;
    return events.filter(evt => {
      const origName = evt.tool_name.includes('__') ? evt.tool_name.split('__').slice(1).join('__') : evt.tool_name;
      return origName.startsWith(prefix + '_') || origName === prefix;
    });
  };

  // Fetch audit events for the current selection
  const fetchAudit = useCallback(async (params: Record<string, string>, toolPrefix?: string | null) => {
    const withRange = { ...params, from: getAuditFrom() };
    lastAuditParams.current = withRange;
    auditToolPrefixRef.current = toolPrefix ?? null;
    setLoadingAudit(true);
    try {
      const fetchLimit = toolPrefix ? '100' : '15';
      const res = await api.getAuditEvents({ ...withRange, limit: fetchLimit });
      const filtered = applyToolPrefixFilter(res.events, toolPrefix ?? null);
      setAuditEvents(toolPrefix ? filtered.slice(0, 15) : filtered);
    } catch {
      setAuditEvents([]);
    } finally {
      setLoadingAudit(false);
    }
  }, [getAuditFrom]);

  // While the detail panel is open, silently refresh its events every 5s
  // (WS handles instant updates on matching calls; this catches anything missed)
  useEffect(() => {
    if (!selection) return;
    const interval = setInterval(() => {
      if (Object.keys(lastAuditParams.current).length === 0) return;
      const prefix = auditToolPrefixRef.current;
      const fetchLimit = prefix ? '100' : '15';
      api.getAuditEvents({ ...lastAuditParams.current, limit: fetchLimit })
        .then((res) => {
          const filtered = applyToolPrefixFilter(res.events, prefix);
          setAuditEvents(prefix ? filtered.slice(0, 15) : filtered);
        })
        .catch(() => {});
    }, 5000);
    return () => clearInterval(interval);
  }, [selection]);

  // Re-fetch audit when range changes and a selection is open
  useEffect(() => {
    if (!selection || Object.keys(lastAuditParams.current).length === 0) return;
    // Update the stored from param and re-fetch
    lastAuditParams.current = { ...lastAuditParams.current, from: getAuditFrom() };
    const prefix = auditToolPrefixRef.current;
    const fetchLimit = prefix ? '100' : '15';
    api.getAuditEvents({ ...lastAuditParams.current, limit: fetchLimit })
      .then((res) => {
        const filtered = applyToolPrefixFilter(res.events, prefix);
        setAuditEvents(prefix ? filtered.slice(0, 15) : filtered);
      })
      .catch(() => {});
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [range]);

  // Build graph nodes and edges
  const { nodes, edges } = useMemo(() => {
    if (!graphData)
      return { nodes: [] as Node[], edges: [] as Edge[] };

    const flowNodes: Node[] = [];
    const flowEdges: Edge[] = [];

    // Optional leftmost Users column. When present, everything else shifts
    // right to make room, so the flow reads Users → Apps → Backends → Tools.
    const usersList = graphData.users ?? [];
    const showUsers = usersList.length > 0;
    const USERS_SHIFT = showUsers ? 300 : 0;
    const COL_USER = 50;
    const COL_APP = 50 + USERS_SHIFT;
    const COL_BACKEND = 450 + USERS_SHIFT;
    const COL_TOOL = 850 + USERS_SHIFT;
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
      usersList.length,
      graphData.applications.length,
      graphData.backends.length,
      expandedTools.length,
      1,
    );
    const getYOffset = (count: number) =>
      ((maxRows - count) * ROW_SPACING) / 2 + 40;

    // User nodes (leftmost column) + user → app edges
    if (showUsers) {
      const userOffset = getYOffset(usersList.length);
      usersList.forEach((u, i) => {
        flowNodes.push({
          id: `user-${u.user_id}`,
          type: 'userNode',
          position: { x: COL_USER, y: i * ROW_SPACING + userOffset },
          data: {
            user_id: u.user_id,
            username: u.username,
            call_count: u.call_count,
            last_seen: u.last_seen,
          },
        });
      });
    }

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

    // Edge thickness scaling
    const allCounts = [
      ...(graphData.user_to_app ?? []).map((e) => e.call_count),
      ...graphData.app_to_backend.map((e) => e.call_count),
      ...graphData.backend_to_tool.map((e) => e.call_count),
    ];
    const maxCalls = Math.max(...allCounts, 1);

    // User -> App edges (colored by the target application for visual continuity)
    if (showUsers) {
      (graphData.user_to_app ?? []).forEach((edge, i) => {
        // Only draw the edge if both endpoints are actually rendered.
        const hasApp = graphData.applications.some((a) => a.application === edge.target);
        const hasUser = usersList.some((u) => u.user_id === edge.source);
        if (!hasApp || !hasUser) return;
        const meta = getAppMeta(edge.target);
        const thickness = Math.max(1.5, Math.min((edge.call_count / maxCalls) * 5, 5));
        const edgeColor = meta.lineColor || meta.color;
        flowEdges.push({
          id: `ua-${i}-${edge.source}-${edge.target}`,
          source: `user-${edge.source}`,
          target: `app-${edge.target}`,
          animated: true,
          style: { stroke: edgeColor, strokeWidth: thickness, opacity: 0.5 },
          data: { sourceName: edge.source, targetName: edge.target, callCount: edge.call_count },
        });
      });
    }

    // App -> Backend edges
    graphData.app_to_backend.forEach((edge, i) => {
      const meta = getAppMeta(edge.source);
      const thickness = Math.max(1.5, Math.min((edge.call_count / maxCalls) * 5, 5));
      const edgeColor = meta.lineColor || meta.color;
      flowEdges.push({
        id: `ab-${i}-${edge.source}-${edge.target}`,
        source: `app-${edge.source}`,
        target: `backend-${edge.target}`,
        animated: true,
        style: { stroke: edgeColor, strokeWidth: thickness, opacity: 0.55 },
        data: { sourceName: edge.source, targetName: edge.target, callCount: edge.call_count },
      });
    });

    // Get the config for the expanded backend to check if it's an agent
    const expandedBackendConfig = expandedBackend
      ? backendConfigs.find(b => b.name === expandedBackend)
      : null;
    const isAgent = expandedBackendConfig?.transport === 'agent';
    const subBackends: any[] = isAgent ? (expandedBackendConfig?.config?.sub_backends || []) : [];

    if (expandedBackend && isAgent && subBackends.length > 0) {
      // Agent backend: show sub-backend nodes
      const expandedBackendIdx = graphData.backends.findIndex(b => b.backend_name === expandedBackend);
      const backendY = expandedBackendIdx * ROW_SPACING + backendOffset;
      const subBlockHeight = (subBackends.length - 1) * ROW_SPACING;
      const subStartY = backendY - subBlockHeight / 2;

      subBackends.forEach((sub: any, i: number) => {
        flowNodes.push({
          id: `sub-${expandedBackend}-${sub.name}`,
          type: 'subBackendNode',
          position: { x: COL_TOOL, y: subStartY + i * ROW_SPACING },
          data: {
            sub_name: sub.name,
            transport: sub.transport,
            tool_count: sub.tool_count || 0,
            expanded: expandedSubBackend === sub.name,
            animIndex: i,
          },
        });

        // Edge from backend to sub-backend
        flowEdges.push({
          id: `bs-${expandedBackend}-${sub.name}`,
          source: `backend-${expandedBackend}`,
          target: `sub-${expandedBackend}-${sub.name}`,
          animated: true,
          style: { stroke: 'var(--line-strong)', strokeWidth: 1.5, opacity: 0.9 },
          data: { sourceName: expandedBackend, targetName: sub.name, callCount: 0 },
        });
      });

      // If a sub-backend is expanded, show its tools
      if (expandedSubBackend) {
        // One column (400px) to the right of the sub-backends. Must be relative
        // to COL_TOOL so it also accounts for USERS_SHIFT — a hardcoded value
        // overlapped the sub-backend column when the Users column was shown,
        // making the sub→tool edges run backwards.
        const COL_TOOL_DETAIL = COL_TOOL + 400;
        const subTools = expandedTools.filter(t => {
          const origName = t.tool_name.includes('__') ? t.tool_name.split('__').slice(1).join('__') : t.tool_name;
          return origName.startsWith(expandedSubBackend + '_') || origName === expandedSubBackend;
        });

        if (subTools.length > 0) {
          const subIdx = subBackends.findIndex((s: any) => s.name === expandedSubBackend);
          const subY = subStartY + subIdx * ROW_SPACING;
          const toolBlockHeight = (subTools.length - 1) * ROW_SPACING;
          const toolStartY = subY - toolBlockHeight / 2;

          subTools.forEach((t, i) => {
            flowNodes.push({
              id: `tool-${t.tool_name}`,
              type: 'toolNode',
              position: { x: COL_TOOL_DETAIL, y: toolStartY + i * ROW_SPACING },
              data: {
                tool_name: t.tool_name,
                backend_name: t.backend_name,
                risk_category: t.risk_category,
                call_count: t.call_count,
                animIndex: i,
              },
            });
          });

          // Edges from sub-backend to tools
          subTools.forEach((t, i) => {
            flowEdges.push({
              id: `st-${expandedSubBackend}-${t.tool_name}`,
              source: `sub-${expandedBackend}-${expandedSubBackend}`,
              target: `tool-${t.tool_name}`,
              animated: true,
              style: { stroke: 'var(--line-strong)', strokeWidth: 1, opacity: 0.7 },
              data: { sourceName: expandedSubBackend, targetName: t.tool_name, callCount: t.call_count },
            });
          });
        }
      }
    } else if (expandedBackend && expandedTools.length > 0) {
      // Non-agent backend: show tools directly (existing behavior)
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

      // Backend -> Tool edges for non-agent
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
          style: { stroke: 'var(--line-strong)', strokeWidth: thickness, opacity: 0.9 },
          data: { sourceName: edge.source, targetName: edge.target, callCount: edge.call_count },
        });
      });
    }

    return { nodes: flowNodes, edges: flowEdges };
  }, [graphData, connections, expandedBackend, expandedSubBackend, backendConfigs]);

  // Event handlers
  const onNodeClick = useCallback(
    (_event: React.MouseEvent, node: Node) => {
      // User node: show that user's recent activity.
      if (node.type === 'userNode') {
        setSelection({ type: 'user', data: node.data as Record<string, unknown> });
        fetchAudit({ user_id: String(node.data.user_id) });
        return;
      }

      const nodeType =
        node.type === 'appNode' ? 'app' : node.type === 'backendNode' ? 'backend' : node.type === 'subBackendNode' ? 'backend' : 'tool';

      // Handle sub-backend node clicks — must come before backend toggle
      if (node.type === 'subBackendNode') {
        const subName = String(node.data.sub_name);
        setExpandedSubBackend((prev) => (prev === subName ? null : subName));
        setSelection({ type: 'sub-backend', data: node.data as Record<string, unknown> });
        const params: Record<string, string> = {};
        if (expandedBackend) params.backend = expandedBackend;
        fetchAudit(params, subName);
        return;
      }

      // Toggle backend expansion when clicking a backend node
      if (node.type === 'backendNode') {
        const backendName = String(node.data.backend_name);
        setExpandedBackend((prev) => {
          if (prev === backendName) {
            setExpandedSubBackend(null);
            return null;
          }
          setExpandedSubBackend(null);
          return backendName;
        });
      }

      setSelection({ type: nodeType as SelectionInfo['type'], data: node.data as Record<string, unknown> });

      // Fetch relevant audit events
      const params: Record<string, string> = {};
      if (nodeType === 'app') params.application = String(node.data.application);
      if (nodeType === 'backend') params.backend = String(node.data.backend_name);
      if (nodeType === 'tool') params.tool_name = String(node.data.tool_name);
      fetchAudit(params);
    },
    [fetchAudit, expandedBackend],
  );

  const onEdgeClick = useCallback(
    (_event: React.MouseEvent, edge: Edge) => {
      const edgeData = edge.data as Record<string, unknown> | undefined;
      const sourceName = String(edgeData?.sourceName ?? '');
      const targetName = String(edgeData?.targetName ?? '');
      const callCount = Number(edgeData?.callCount ?? 0);

      let edgeType: SelectionInfo['type'];
      if (edge.id.startsWith('ua-')) edgeType = 'user-app-edge';
      else if (edge.id.startsWith('ab-')) edgeType = 'app-backend-edge';
      else if (edge.id.startsWith('bs-')) edgeType = 'backend-sub-edge';
      else if (edge.id.startsWith('st-')) edgeType = 'sub-tool-edge';
      else edgeType = 'backend-tool-edge';

      setSelection({ type: edgeType, sourceName, targetName, callCount });

      // Fetch audit events for this connection
      const params: Record<string, string> = {};
      let toolPrefix: string | null = null;
      if (edgeType === 'user-app-edge') {
        params.user_id = sourceName;
        params.application = targetName;
      } else if (edgeType === 'app-backend-edge') {
        params.application = sourceName;
        params.backend = targetName;
      } else if (edgeType === 'backend-sub-edge') {
        params.backend = sourceName;
        toolPrefix = targetName;
      } else if (edgeType === 'sub-tool-edge') {
        params.tool_name = targetName;
      } else {
        params.backend = sourceName;
        params.tool_name = targetName;
      }
      fetchAudit(params, toolPrefix);
    },
    [fetchAudit],
  );

  const onPaneClick = useCallback(() => {
    setSelection(null);
    setExpandedBackend(null);
    setExpandedSubBackend(null);
    setAuditEvents([]);
    lastAuditParams.current = {};
  }, []);

  return (
    <div className="h-full min-h-0 flex flex-col">
      {/* ── Top bar ─────────────────────────────────────────────── */}
      <div
        className="flex items-center gap-x-3 gap-y-2 flex-wrap px-4 sm:px-6 py-3 shrink-0 border-b border-line"
        style={{ background: 'var(--panel)' }}
      >
        {/* Title + summary */}
        <div className="flex items-center gap-3 mr-auto min-w-0">
          <div className="flex items-center gap-2">
            <h2 className="text-sm font-semibold text-ink tracking-tight whitespace-nowrap">Usage graph</h2>
            {/* Live status indicator */}
            <div className="flex items-center gap-1.5">
              <div className="relative flex items-center justify-center w-2 h-2">
                {wsConnected && (
                  <div
                    className={clsx(
                      'absolute inset-0 rounded-full transition-opacity duration-300',
                      livePulse ? 'animate-ping opacity-70' : 'opacity-0',
                    )}
                    style={{ background: liveMode === 'ws' ? 'var(--beam)' : 'var(--text-3)' }}
                  />
                )}
                <div
                  className="w-1.5 h-1.5 rounded-full transition-colors duration-500"
                  style={{ background: wsConnected ? (liveMode === 'ws' ? 'var(--beam)' : 'var(--text-3)') : 'var(--line-strong)' }}
                />
              </div>
              <span
                className={clsx(
                  'text-micro uppercase tracking-widest font-semibold transition-colors duration-500',
                  liveMode === 'ws' ? 'text-beam' : liveMode === 'polling' ? 'text-ink-2' : 'text-ink-4',
                )}
              >
                {liveMode === 'ws' ? 'live' : liveMode === 'polling' ? 'auto-refresh' : 'connecting...'}
              </span>
            </div>
          </div>
        </div>

        {/* User selector (admin) */}
        {isAdmin && users.length > 0 && (
          <div className="flex items-center gap-1.5">
            <UsersIcon className="w-3.5 h-3.5 text-ink-4" />
            <select
              value={selectedUserId}
              onChange={(e) => setSelectedUserId(e.target.value)}
              className="px-2 py-1 bg-transparent border border-line rounded-row text-2xs text-ink-2 focus:outline-none focus:border-beam-edge appearance-none pr-5 cursor-pointer"
            >
              <option value="all">All users</option>
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
        <div className="flex items-center rounded-row overflow-hidden border border-line">
          {['24h', '7d', '30d'].map((r) => (
            <button
              key={r}
              onClick={() => setRange(r)}
              className={clsx(
                'px-3 py-1.5 text-2xs font-medium transition-all',
                range === r
                  ? 'bg-beam-wash text-beam'
                  : 'text-ink-3 hover:text-ink-2 hover:bg-raised',
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
          className="p-1.5 text-ink-4 hover:text-ink-2 rounded-control transition-colors disabled:opacity-30"
        >
          <RefreshCw className={clsx('w-3.5 h-3.5', loading && 'animate-spin')} />
        </button>
      </div>

      {/* ── Canvas ──────────────────────────────────────────────── */}
      <div className="flex-1 relative">
        {loading ? (
          <div className="flex items-center justify-center h-full">
            <div className="flex flex-col items-center gap-3">
              <div className="w-8 h-8 rounded-card bg-beam-wash flex items-center justify-center animate-pulse">
                <RefreshCw className="w-4 h-4 text-beam animate-spin" />
              </div>
              <p className="text-xs text-ink-4">Loading graph data...</p>
            </div>
          </div>
        ) : nodes.length === 0 ? (
          <div className="flex items-center justify-center h-full">
            <div className="text-center">
              <div className="w-12 h-12 mx-auto rounded-panel bg-panel border border-line flex items-center justify-center mb-3">
                <Layers className="w-5 h-5 text-ink-4" />
              </div>
              <p className="text-sm text-ink-2">No usage data yet</p>
              <p className="text-xs text-ink-4 mt-1">
                Once an AI client routes a tool call through the gateway, the graph appears.
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
              style={{ background: 'var(--void)' }}
            >
              <Background color="var(--line-strong)" gap={24} size={1} />
              <Controls
                showInteractive={false}
                className="!bg-panel/80 !border-line !rounded-card !shadow-2xl [&>button]:!bg-transparent [&>button]:!border-line/50 [&>button]:!text-ink-3 [&>button:hover]:!text-ink [&>button:hover]:!bg-raised [&>button]:!rounded-row [&>button]:!w-8 [&>button]:!h-8"
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
