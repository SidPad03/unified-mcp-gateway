import { useState, useEffect } from 'react';
import { api, Backend, ApiKey } from '@/lib/api';
import { Plus, Trash2, Server, Wifi, Terminal, Globe, X, RefreshCw, Link, Copy, Check, RotateCcw, Pencil, Laptop, Boxes, Eye, EyeOff } from 'lucide-react';
import clsx from 'clsx';
import { fmt } from '@/lib/format';
import {
  Badge,
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
  StatusLabel,
  Tone,
} from '@/components/ui';

interface Props {
  isAdmin: boolean;
}

/** A backend's health, as a tone the rail can carry. */
function healthTone(status: string, enabled: boolean): Tone {
  if (!enabled) return 'neutral';
  if (status === 'healthy') return 'ok';
  if (status === 'unhealthy') return 'deny';
  if (status === 'idle' || status === 'unknown') return 'neutral';
  return 'warn';
}

const TRANSPORT_ICONS: Record<string, typeof Terminal> = {
  stdio: Terminal,
  'streamable-http': Globe,
  sse: Wifi,
  agent: Laptop,
};

interface StdioForm {
  command: string;
  args: string[];
  env: { key: string; value: string }[];
}

interface HttpForm {
  url: string;
  env: { key: string; value: string }[];
}

const emptyStdioForm = (): StdioForm => ({ command: '', args: [''], env: [{ key: '', value: '' }] });
const emptyHttpForm = (): HttpForm => ({ url: '', env: [{ key: '', value: '' }] });

export default function BackendConfig({ isAdmin }: Props) {
  const [backends, setBackends] = useState<Backend[]>([]);
  const [loading, setLoading] = useState(true);
  const [showModal, setShowModal] = useState(false);
  const [editingBackend, setEditingBackend] = useState<Backend | null>(null);
  const [name, setName] = useState('');
  const [transport, setTransport] = useState('stdio');
  const [riskCategory, setRiskCategory] = useState('read');
  const [stdioForm, setStdioForm] = useState<StdioForm>(emptyStdioForm());
  const [httpForm, setHttpForm] = useState<HttpForm>(emptyHttpForm());
  const [error, setError] = useState('');
  const [pageError, setPageError] = useState('');
  // Header/env values (e.g. bearer tokens) are masked by default; track which
  // rows the user has explicitly revealed. Keyed by `${form}-${index}`.
  const [revealedValues, setRevealedValues] = useState<Set<string>>(new Set());
  const [selectedBackend, setSelectedBackend] = useState<Backend | null>(null);
  const [showDeleteConfirm, setShowDeleteConfirm] = useState<string | null>(null);
  const [showAddMenu, setShowAddMenu] = useState(false);
  const [showJsonEditor, setShowJsonEditor] = useState(false);
  const [jsonContent, setJsonContent] = useState('');
  const [jsonError, setJsonError] = useState('');
  const [isSubmitting, setIsSubmitting] = useState(false);
  const [jsonSaving, setJsonSaving] = useState(false);

  // Connect modal state
  const [showConnectModal, setShowConnectModal] = useState(false);
  const [connectTab, setConnectTab] = useState<'claude' | 'claudedesktop' | 'cursor' | 'vscode' | 'openwebui' | 'clawbot' | 'codex' | 'lmstudio'>('claude');
  const [apiKeys, setApiKeys] = useState<ApiKey[]>([]);
  const [newKeyUserId, setNewKeyUserId] = useState<string>('');
  const [generatedKeys, _setGeneratedKeys] = useState<Record<string, string>>(() => {
    try { return JSON.parse(localStorage.getItem('mcpgw_raw_keys') || '{}'); } catch { return {}; }
  });
  const setGeneratedKeys = (v: Record<string, string> | ((prev: Record<string, string>) => Record<string, string>)) => {
    _setGeneratedKeys(prev => {
      const next = typeof v === 'function' ? v(prev) : v;
      localStorage.setItem('mcpgw_raw_keys', JSON.stringify(next));
      return next;
    });
  };
  const [copied, setCopied] = useState(false);
  const [connectLoading, setConnectLoading] = useState(false);
  const [connectError, setConnectError] = useState('');
  const [gatewayUrl, setGatewayUrl] = useState(() => localStorage.getItem('mcpgw_gateway_url') || '');
  const [regeneratingKey, setRegeneratingKey] = useState(false);
  const [regenError, setRegenError] = useState('');

  // Sync state
  const [syncingId, setSyncingId] = useState<string | null>(null);
  const [syncingAll, setSyncingAll] = useState(false);

  useEffect(() => {
    loadBackends();
  }, []);

  const loadBackends = async () => {
    try {
      const data = await api.getBackends();
      setBackends(data);
      setPageError('');
    } catch (e: any) {
      setPageError(e.message || 'Failed to load backends');
    } finally {
      setLoading(false);
    }
  };

  const resetForm = () => {
    setName('');
    setTransport('stdio');
    setRiskCategory('read');
    setStdioForm(emptyStdioForm());
    setHttpForm(emptyHttpForm());
    setError('');
    setEditingBackend(null);
  };

  const openModal = () => {
    resetForm();
    setShowModal(true);
  };

  const openEditModal = (backend: Backend) => {
    setEditingBackend(backend);
    setName(backend.name);
    setTransport(backend.transport);
    setRiskCategory(backend.risk_category || 'read');
    if (backend.transport === 'stdio') {
      const cfg = backend.config as any;
      setStdioForm({
        command: cfg.command || '',
        args: cfg.args?.length ? cfg.args : [''],
        env: Object.entries(cfg.env || {}).map(([key, value]) => ({ key, value: String(value) })),
      });
      if (stdioForm.env.length === 0) setStdioForm(prev => ({ ...prev, env: [{ key: '', value: '' }] }));
    } else {
      const cfg = backend.config as any;
      setHttpForm({
        url: cfg.url || '',
        // HTTP KV pairs are stored as `headers`; fall back to legacy `env` records.
        env: Object.entries(cfg.headers || cfg.env || {}).map(([key, value]) => ({ key, value: String(value) })),
      });
      if (httpForm.env.length === 0) setHttpForm(prev => ({ ...prev, env: [{ key: '', value: '' }] }));
    }
    setError('');
    setShowModal(true);
  };

  const openConnectModal = async () => {
    setShowConnectModal(true);
    setConnectTab('claude');
    setCopied(false);
    setConnectError('');
    setConnectLoading(true);
    try {
      // Reveal the logged-in user's full per-app keys so the config box can copy
      // a working key. This runs first because it may rotate any legacy
      // hash-only keys (changing their prefix), so we list keys afterwards.
      let currentUserId: string | undefined;
      try {
        currentUserId = JSON.parse(localStorage.getItem('mcpgw_user') || '{}').user_id;
      } catch {
        currentUserId = undefined;
      }
      if (currentUserId) {
        try {
          const revealed = await api.revealAppKeys(currentUserId);
          const raw: Record<string, string> = {};
          revealed.forEach(r => { if (r.application) raw[r.application] = r.raw_key; });
          if (Object.keys(raw).length) setGeneratedKeys(prev => ({ ...prev, ...raw }));
        } catch {
          /* fall back to prefix-only display if reveal fails */
        }
      }
      const keys = await api.getApiKeys();
      setApiKeys(keys);
    } catch {
      setApiKeys([]);
    } finally {
      setConnectLoading(false);
    }
  };

  const getAppKey = (app: string): ApiKey | undefined => {
    let currentUserId: string | undefined;
    try {
      currentUserId = JSON.parse(localStorage.getItem('mcpgw_user') || '{}').user_id;
    } catch {
      currentUserId = undefined;
    }
    const targetUserId = (isAdmin && newKeyUserId) ? newKeyUserId : currentUserId;
    return apiKeys.find(k => k.application === app && (!targetUserId || k.user_id === targetUserId));
  };

  const getDefaultGatewayUrl = () => 'https://localhost:8080/mcp';

  const getGatewayUrl = () => gatewayUrl || getDefaultGatewayUrl();

  // Revoke this client's current key and issue a fresh one (creates if none).
  const regenerateAppKey = async () => {
    if (regeneratingKey) return;
    setRegenError('');
    setRegeneratingKey(true);
    try {
      let currentUserId: string | undefined;
      try { currentUserId = JSON.parse(localStorage.getItem('mcpgw_user') || '{}').user_id; } catch { currentUserId = undefined; }
      const targetUserId = (isAdmin && newKeyUserId) ? newKeyUserId : currentUserId;
      const result = await api.rotateAppKey(connectTab, targetUserId);
      setGeneratedKeys(prev => ({ ...prev, [connectTab]: result.raw_key }));
      const keys = await api.getApiKeys();
      setApiKeys(keys);
    } catch (e: any) {
      setRegenError(e.message || 'Failed to generate key');
    } finally {
      setRegeneratingKey(false);
    }
  };

  const getKeyForDisplay = () => {
    const appKey = getAppKey(connectTab);
    if (appKey) return `${appKey.key_prefix}...`;
    return '<paste-your-api-key-here>';
  };

  const getKeyForCopy = () => {
    const raw = generatedKeys[connectTab];
    if (raw) return raw;
    return getKeyForDisplay();
  };

  const buildClientConfig = (key: string) => {
    const url = getGatewayUrl();
    switch (connectTab) {
      case 'claude':
        return JSON.stringify({ mcpServers: { 'mcp-gateway': { type: 'http', url, headers: { Authorization: `Bearer ${key}` } } } }, null, 2);
      case 'claudedesktop':
        return JSON.stringify({ mcpServers: { 'mcp-gateway': {
          command: 'npx',
          args: ['-y', 'mcp-remote', url, '--header', `Authorization: Bearer ${key}`],
        } } }, null, 2);
      case 'cursor':
        return JSON.stringify({ mcpServers: { 'mcp-gateway': { url, headers: { Authorization: `Bearer ${key}` } } } }, null, 2);
      case 'vscode':
        return JSON.stringify({ servers: { 'mcp-gateway': { type: 'http', url, headers: { Authorization: `Bearer ${key}` } } } }, null, 2);
      case 'openwebui':
        return JSON.stringify({ url, type: 'MCP (Streamable HTTP)', auth: 'Bearer', token: key }, null, 2);
      case 'clawbot':
        return JSON.stringify({ mcp: { servers: { 'mcp-gateway': { transport: 'http', url, headers: { Authorization: `Bearer ${key}` } } } } }, null, 2);
      case 'codex':
        return JSON.stringify({ mcpServers: { 'mcp-gateway': { type: 'streamable-http', url, headers: { Authorization: `Bearer ${key}` } } } }, null, 2);
      case 'lmstudio':
        return JSON.stringify({ mcpServers: { 'mcp-gateway': { type: 'streamable-http', url, headers: { Authorization: `Bearer ${key}` } } } }, null, 2);
    }
  };

  const getConfigForDisplay = () => buildClientConfig(getKeyForDisplay());
  const getConfigForCopy = () => buildClientConfig(getKeyForCopy());

  const getConfigHint = () => {
    switch (connectTab) {
      case 'claude': return 'Add to ~/.claude/settings.json (or project .mcp.json):';
      case 'claudedesktop': return 'Add to ~/Library/Application Support/Claude/claude_desktop_config.json:';
      case 'cursor': return 'Add to .cursor/mcp.json in your project:';
      case 'vscode': return 'Add to .vscode/mcp.json in your project (or user settings.json under "mcp"):';
      case 'clawbot': return 'Add to your Clawbot configuration file (clawbot.config.json or ~/.clawbot/config.json):';
      case 'codex': return 'Add to your Codex MCP configuration (~/.codex/mcp.json):';
      case 'lmstudio': return 'Add to LM Studio MCP settings (Settings → MCP Servers):';
      default: return '';
    }
  };

  const getConfigNote = () => {
    switch (connectTab) {
      case 'openwebui': return 'Open WebUI requires v0.6.31+. If tools don\'t appear, try adding a comma (,) to the Function Name Filter List under Admin Settings → External Tools.';
      case 'claudedesktop': return 'Claude Desktop uses mcp-remote to connect via stdio. Requires Node.js/npx installed. Restart Claude Desktop after updating the config.';
      case 'vscode': return 'VS Code requires the GitHub Copilot extension (agent mode). Create .vscode/mcp.json or add to user/workspace settings under "mcp.servers".';
      case 'clawbot': return 'Clawbot connects via MCP over HTTP. Make sure Clawbot is running v2.0+ with MCP support enabled. Restart Clawbot after updating the config.';
      case 'codex': return 'Codex uses streamable-http transport. Place the config in ~/.codex/mcp.json and restart Codex to load the gateway connection.';
      case 'lmstudio': return 'LM Studio 0.3.12+ supports MCP servers. Go to Settings → MCP Servers → Add Server, then paste the configuration below.';
      default: return 'The gateway exposes all backend tools through a single MCP endpoint. Policies and RBAC are enforced based on the API key\'s associated user.';
    }
  };

  const copyConfig = async () => {
    const config = getConfigForCopy();
    try {
      await navigator.clipboard.writeText(config);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch {
      const ta = document.createElement('textarea');
      ta.value = config!;
      document.body.appendChild(ta);
      ta.select();
      document.execCommand('copy');
      document.body.removeChild(ta);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    }
  };

  const buildConfig = (): Record<string, unknown> => {
    if (transport === 'stdio') {
      const env: Record<string, string> = {};
      stdioForm.env.forEach(e => { if (e.key.trim()) env[e.key.trim()] = e.value; });
      return {
        command: stdioForm.command,
        args: stdioForm.args.filter(a => a !== ''),
        env,
      };
    } else {
      // HTTP/SSE backends have no subprocess, so these KV pairs are sent as request
      // headers (e.g. Authorization), not environment variables.
      const headers: Record<string, string> = {};
      httpForm.env.forEach(e => { if (e.key.trim()) headers[e.key.trim()] = e.value; });
      return { url: httpForm.url, headers };
    }
  };

  const saveBackend = async () => {
    if (isSubmitting) return;
    setError('');
    if (!name.trim()) { setError('Name is required'); return; }
    if (transport === 'stdio' && !stdioForm.command.trim()) { setError('Command is required'); return; }
    if (transport !== 'stdio' && !httpForm.url.trim()) { setError('URL is required'); return; }

    setIsSubmitting(true);
    try {
      if (editingBackend) {
        await api.updateBackend(editingBackend.backend_id, {
          name: name.trim(),
          transport,
          config: buildConfig(),
          risk_category: riskCategory,
        });
      } else {
        await api.createBackend({
          name: name.trim(),
          transport,
          config: buildConfig(),
          risk_category: riskCategory,
        });
      }
      setShowModal(false);
      resetForm();
      loadBackends();
    } catch (e: any) {
      setError(e.message || 'Failed to save backend');
    } finally {
      setIsSubmitting(false);
    }
  };

  const toggleBackend = async (backend: Backend) => {
    try {
      await api.updateBackend(backend.backend_id, { is_enabled: !backend.is_enabled });
      loadBackends();
    } catch (e: any) {
      setPageError(e.message || 'Failed to toggle backend');
    }
  };

  const confirmDeleteBackend = async () => {
    if (!showDeleteConfirm) return;
    try {
      await api.deleteBackend(showDeleteConfirm);
      setShowDeleteConfirm(null);
      loadBackends();
    } catch (e: any) {
      setPageError(e.message || 'Failed to delete backend');
      setShowDeleteConfirm(null);
    }
  };

  const syncBackend = async (id: string) => {
    setSyncingId(id);
    try {
      await api.syncBackend(id);
      setPageError('');
      loadBackends();
    } catch (e: any) {
      setPageError(e.message || 'Failed to sync backend');
    } finally {
      setSyncingId(null);
    }
  };

  const syncAllBackends = async () => {
    setSyncingAll(true);
    const enabled = backends.filter(b => b.is_enabled);
    const errors: string[] = [];
    for (const backend of enabled) {
      try {
        await api.syncBackend(backend.backend_id);
      } catch (e: any) {
        errors.push(`${backend.name}: ${e.message}`);
      }
    }
    if (errors.length > 0) {
      setPageError(`Sync errors: ${errors.join('; ')}`);
    } else {
      setPageError('');
    }
    loadBackends();
    setSyncingAll(false);
  };

  const openJsonEditor = () => {
    const json = backends.map(b => ({
      name: b.name,
      transport: b.transport,
      config: b.config,
      risk_category: b.risk_category,
      is_enabled: b.is_enabled,
    }));
    setJsonContent(JSON.stringify(json, null, 2));
    setJsonError('');
    setShowJsonEditor(true);
  };

  const saveJsonBackends = async () => {
    if (jsonSaving) return;
    setJsonError('');
    setJsonSaving(true);
    try {
      const parsed = JSON.parse(jsonContent);
      if (!Array.isArray(parsed)) {
        setJsonError('JSON must be an array of backend objects');
        return;
      }
      for (const entry of parsed) {
        if (!entry.name || !entry.transport) {
          setJsonError('Each backend must have "name" and "transport" fields');
          return;
        }
        const existing = backends.find(b => b.name === entry.name);
        if (existing) {
          await api.updateBackend(existing.backend_id, {
            transport: entry.transport,
            config: entry.config || {},
            risk_category: entry.risk_category,
          });
        } else {
          await api.createBackend({
            name: entry.name,
            transport: entry.transport,
            config: entry.config || {},
            risk_category: entry.risk_category,
          });
        }
      }
      setShowJsonEditor(false);
      loadBackends();
    } catch (e: any) {
      if (e instanceof SyntaxError) {
        setJsonError('Invalid JSON syntax');
      } else {
        setJsonError(e.message || 'Failed to save backends');
      }
    } finally {
      setJsonSaving(false);
    }
  };

  // --- Stdio form helpers ---
  const updateArg = (idx: number, val: string) => {
    const next = [...stdioForm.args];
    next[idx] = val;
    setStdioForm({ ...stdioForm, args: next });
  };
  const addArg = () => setStdioForm({ ...stdioForm, args: [...stdioForm.args, ''] });
  const removeArg = (idx: number) => {
    const next = stdioForm.args.filter((_, i) => i !== idx);
    setStdioForm({ ...stdioForm, args: next.length ? next : [''] });
  };

  const toggleReveal = (form: 'stdio' | 'http', idx: number) => {
    const key = `${form}-${idx}`;
    setRevealedValues(prev => {
      const next = new Set(prev);
      next.has(key) ? next.delete(key) : next.add(key);
      return next;
    });
  };

  const updateEnv = (form: 'stdio' | 'http', idx: number, field: 'key' | 'value', val: string) => {
    if (form === 'stdio') {
      const next = [...stdioForm.env];
      next[idx] = { ...next[idx], [field]: val };
      setStdioForm({ ...stdioForm, env: next });
    } else {
      const next = [...httpForm.env];
      next[idx] = { ...next[idx], [field]: val };
      setHttpForm({ ...httpForm, env: next });
    }
  };
  const addEnv = (form: 'stdio' | 'http') => {
    if (form === 'stdio') setStdioForm({ ...stdioForm, env: [...stdioForm.env, { key: '', value: '' }] });
    else setHttpForm({ ...httpForm, env: [...httpForm.env, { key: '', value: '' }] });
  };
  const removeEnv = (form: 'stdio' | 'http', idx: number) => {
    if (form === 'stdio') {
      const next = stdioForm.env.filter((_, i) => i !== idx);
      setStdioForm({ ...stdioForm, env: next.length ? next : [{ key: '', value: '' }] });
    } else {
      const next = httpForm.env.filter((_, i) => i !== idx);
      setHttpForm({ ...httpForm, env: next.length ? next : [{ key: '', value: '' }] });
    }
  };

  const HEALTH_LABELS: Record<string, string> = {
    healthy: 'Healthy',
    unhealthy: 'Unhealthy',
    degraded: 'Degraded',
    idle: 'Idle',
    unknown: 'Unknown',
  };

  const renderEnvFields = (form: 'stdio' | 'http') => {
    const isHttp = form === 'http';
    const entries = isHttp ? httpForm.env : stdioForm.env;
    const label = isHttp ? 'HTTP headers' : 'Environment variables';
    const addLabel = isHttp ? '+ Add header' : '+ Add variable';
    const keyPlaceholder = isHttp ? 'Authorization' : 'KEY';
    const valuePlaceholder = isHttp ? 'Bearer <token>' : 'value';
    return (
      <div>
        <div className="flex items-center justify-between mb-1.5">
          <label className="block text-xs font-medium text-ink-2 uppercase tracking-wider">{label}</label>
          <button type="button" onClick={() => addEnv(form)} className="text-xs text-beam hover:text-beam transition-colors">{addLabel}</button>
        </div>
        {isHttp && (
          <p className="text-2xs text-ink-3 mb-2">Sent as request headers on every call to this backend (e.g. <span className="font-mono">Authorization = Bearer &lt;token&gt;</span>).</p>
        )}
        <div className="space-y-2">
          {entries.map((entry, i) => (
            <div key={i} className="flex items-center gap-2">
              <input
                type="text"
                value={entry.key}
                onChange={e => updateEnv(form, i, 'key', e.target.value)}
                placeholder={keyPlaceholder}
                className="w-[40%] px-2.5 py-1.5 bg-inset border border-line rounded-row text-xs text-ink font-mono focus:outline-none focus:border-beam-edge"
              />
              <span className="text-ink-4 text-xs">=</span>
              <div className="flex-1 relative">
                <input
                  type={revealedValues.has(`${form}-${i}`) ? 'text' : 'password'}
                  value={entry.value}
                  onChange={e => updateEnv(form, i, 'value', e.target.value)}
                  placeholder={valuePlaceholder}
                  autoComplete="off"
                  className="w-full px-2.5 py-1.5 pr-8 bg-inset border border-line rounded-row text-xs text-ink font-mono focus:outline-none focus:border-beam-edge"
                />
                <button
                  type="button"
                  onClick={() => toggleReveal(form, i)}
                  tabIndex={-1}
                  aria-label={revealedValues.has(`${form}-${i}`) ? 'Hide value' : 'Show value'}
                  className="absolute right-2 top-1/2 -translate-y-1/2 text-ink-4 hover:text-ink-2 transition-colors"
                >
                  {revealedValues.has(`${form}-${i}`) ? <EyeOff className="w-3.5 h-3.5" /> : <Eye className="w-3.5 h-3.5" />}
                </button>
              </div>
              <button type="button" onClick={() => removeEnv(form, i)} className="p-1 text-ink-4 hover:text-deny transition-colors">
                <X className="w-3.5 h-3.5" />
              </button>
            </div>
          ))}
        </div>
      </div>
    );
  };

  const CONNECT_TABS = [
    { key: 'claude' as const, label: 'Claude Code' },
    { key: 'claudedesktop' as const, label: 'Claude Desktop' },
    { key: 'cursor' as const, label: 'Cursor' },
    { key: 'vscode' as const, label: 'VS Code' },
    { key: 'openwebui' as const, label: 'Open WebUI' },
    { key: 'clawbot' as const, label: 'Clawbot' },
    { key: 'codex' as const, label: 'Codex' },
    { key: 'lmstudio' as const, label: 'LM Studio' },
  ];

  return (
    <div>
      <PageHeader
        title="Backends"
        description="The MCP servers this gateway aggregates. A backend can be a local process, a remote HTTP endpoint, or a Mac running the agent."
        actions={
          <>
            <Button icon={Link} onClick={openConnectModal} className="text-beam border-beam-edge">
              Connect a client
            </Button>
            <Button
              icon={RefreshCw}
              onClick={syncAllBackends}
              loading={syncingAll}
            >
              {syncingAll ? 'Syncing...' : 'Refresh and sync'}
            </Button>
            {isAdmin && (
              <div className="relative">
                <Button variant="primary" icon={Plus} onClick={() => setShowAddMenu(!showAddMenu)}>
                  Add backend
                </Button>
                {showAddMenu && (
                  <>
                    {/* Click-away, so the menu cannot be left open behind a
                        dialog the way the old one could. */}
                    <div className="fixed inset-0 z-10" onClick={() => setShowAddMenu(false)} />
                    <div className="absolute right-0 mt-1.5 w-44 bg-high border border-line rounded-row shadow-[var(--shadow-pop)] z-20 overflow-hidden p-1 animate-pop origin-top-right">
                      <button
                        onClick={() => { setShowAddMenu(false); openModal(); }}
                        className="w-full flex items-center gap-2 px-2.5 h-8 rounded-control text-xs text-ink-2 hover:bg-raised hover:text-ink transition-colors"
                      >
                        <Plus className="w-3.5 h-3.5" />
                        New backend
                      </button>
                      <button
                        onClick={() => { setShowAddMenu(false); openJsonEditor(); }}
                        className="w-full flex items-center gap-2 px-2.5 h-8 rounded-control text-xs text-ink-2 hover:bg-raised hover:text-ink transition-colors"
                      >
                        <Pencil className="w-3.5 h-3.5" />
                        Edit as JSON
                      </button>
                    </div>
                  </>
                )}
              </div>
            )}
          </>
        }
      />

      {pageError && (
        <Banner tone="deny" onDismiss={() => setPageError('')} className="mb-4">
          {pageError}
        </Banner>
      )}

      {backends.length > 0 && (
        <Card className="mb-4">
          <div className="flex items-end justify-between gap-8 flex-wrap">
            <div>
              <Label>Tools behind the gate</Label>
              <div className="text-2xl font-semibold tracking-[-0.02em] tabular-nums text-ink mt-1.5">
                {fmt.count(backends.reduce((s, b) => s + b.tool_count, 0))}
              </div>
            </div>
            <div className="flex items-end gap-7 flex-wrap">
              <MiniStat label="Backends" value={fmt.count(backends.length)} />
              <MiniStat
                label="Healthy"
                value={fmt.count(backends.filter(b => b.health_status === 'healthy').length)}
                tone="ok"
              />
              {backends.filter(b => b.is_enabled && b.health_status !== 'healthy' && b.health_status !== 'idle').length > 0 && (
                <MiniStat
                  label="Needs attention"
                  value={fmt.count(
                    backends.filter(
                      b => b.is_enabled && b.health_status !== 'healthy' && b.health_status !== 'idle'
                    ).length
                  )}
                  tone="warn"
                />
              )}
              <MiniStat
                label="Disabled"
                value={fmt.count(backends.filter(b => !b.is_enabled).length)}
              />
            </div>
          </div>
        </Card>
      )}

      {loading ? (
        <Card>
          <Loading label="Loading backends..." />
        </Card>
      ) : backends.length === 0 ? (
        <Card>
          <EmptyState
            icon={Server}
            title="No backends configured"
            message="Add an MCP server and the gateway will start it, register its tools, and put them behind the same endpoint as everything else."
            action={
              isAdmin ? (
                <Button variant="primary" icon={Plus} onClick={openModal}>
                  Add your first backend
                </Button>
              ) : undefined
            }
          />
        </Card>
      ) : (
        <RailList>
          {backends.map(backend => {
            const isSelected = selectedBackend?.backend_id === backend.backend_id;
            const tone = healthTone(backend.health_status, backend.is_enabled);
            return (
              <div key={backend.backend_id}>
                <RailRow
                  tone={tone}
                  active={isSelected}
                  onClick={() => setSelectedBackend(isSelected ? null : backend)}
                  className={clsx(!backend.is_enabled && 'opacity-60')}
                  trailing={
                    isAdmin ? (
                      <div
                        className="flex items-center gap-1"
                        onClick={e => e.stopPropagation()}
                      >
                        <Button size="sm" variant="ghost" icon={Pencil} onClick={() => openEditModal(backend)}>
                          Edit
                        </Button>
                        {backend.is_enabled && (
                          <Button
                            size="sm"
                            variant="ghost"
                            icon={RotateCcw}
                            loading={syncingId === backend.backend_id}
                            onClick={() => syncBackend(backend.backend_id)}
                          >
                            Sync
                          </Button>
                        )}
                        <Button
                          size="sm"
                          variant="ghost"
                          onClick={() => toggleBackend(backend)}
                          className={backend.is_enabled ? undefined : 'text-beam'}
                        >
                          {backend.is_enabled ? 'Disable' : 'Enable'}
                        </Button>
                        <IconButton
                          icon={Trash2}
                          label={`Delete ${backend.name}`}
                          onClick={() => setShowDeleteConfirm(backend.backend_id)}
                          className="hover:text-deny"
                        />
                      </div>
                    ) : (
                      <StatusLabel tone={tone} pulsing={tone === 'ok'}>
                        {HEALTH_LABELS[backend.health_status] || backend.health_status}
                      </StatusLabel>
                    )
                  }
                >
                  <div className="flex items-center gap-3 min-w-0 flex-wrap">
                    <Mono className="text-xs font-medium text-ink">{backend.name}</Mono>
                    {isAdmin && (
                      <StatusLabel tone={tone} pulsing={tone === 'ok'}>
                        {HEALTH_LABELS[backend.health_status] || backend.health_status}
                      </StatusLabel>
                    )}
                    <Badge>{backend.transport}</Badge>
                    <span className="text-2xs text-ink-4 tabular-nums">
                      {backend.tool_count} tools
                    </span>
                    {!backend.is_enabled && <Badge>disabled</Badge>}
                  </div>
                </RailRow>

                {/* Expanded config */}
                {isSelected && (
                  <div className="bg-inset border-y border-line-soft px-4 py-3.5">
                    <h4 className="text-xs font-medium text-ink-2 uppercase tracking-wider mb-2">Configuration</h4>
                    {backend.transport === 'stdio' && (
                      <div className="space-y-2 mb-3">
                        <div className="flex items-center gap-2">
                          <span className="text-xs text-ink-3 w-16 shrink-0">Command:</span>
                          <code className="text-xs text-beam font-mono">{(backend.config as any).command}</code>
                        </div>
                        {(backend.config as any).args?.length > 0 && (
                          <div className="flex items-start gap-2">
                            <span className="text-xs text-ink-3 w-16 shrink-0 mt-0.5">Args:</span>
                            <div className="flex flex-wrap gap-1">
                              {(backend.config as any).args.map((arg: string, i: number) => (
                                <code key={i} className="text-xs text-ink-2 bg-neutral-wash px-1.5 py-0.5 rounded-control font-mono">{arg}</code>
                              ))}
                            </div>
                          </div>
                        )}
                        {Object.keys((backend.config as any).env || {}).length > 0 && (
                          <div className="flex items-start gap-2">
                            <span className="text-xs text-ink-3 w-16 shrink-0 mt-0.5">Env:</span>
                            <div className="space-y-1">
                              {Object.entries((backend.config as any).env || {}).map(([k, v]) => (
                                <div key={k} className="text-xs font-mono">
                                  <span className="text-warn">{k}</span>
                                  <span className="text-ink-4">=</span>
                                  <span className="text-ink-2">{String(v).length > 40 ? String(v).slice(0, 40) + '...' : String(v)}</span>
                                </div>
                              ))}
                            </div>
                          </div>
                        )}
                      </div>
                    )}
                    {backend.transport === 'agent' && (
                      <div className="mb-3">
                        <div className="flex items-center gap-2 mb-3">
                          <Boxes className="w-4 h-4 text-beam" />
                          <span className="text-xs font-medium text-ink-2">Connected MCP servers</span>
                          {(backend.config as any).sub_backends?.length > 0 && (
                            <span className="text-micro text-ink-3 bg-high px-1.5 py-0.5 rounded-control">
                              {(backend.config as any).sub_backends.length} server{(backend.config as any).sub_backends.length !== 1 ? 's' : ''}
                            </span>
                          )}
                        </div>
                        {(backend.config as any).sub_backends?.length > 0 ? (
                          <div className="space-y-2">
                            {((backend.config as any).sub_backends as any[]).map((sub: any, i: number) => {
                              const SubIcon = TRANSPORT_ICONS[sub.transport] || Globe;
                              return (
                                <div key={i} className="bg-inset border border-line rounded-row p-3">
                                  <div className="flex items-center justify-between mb-2">
                                    <div className="flex items-center gap-2">
                                      <SubIcon className="w-3.5 h-3.5 text-beam" />
                                      <span className="text-sm font-medium text-ink">{sub.name}</span>
                                      <span className="text-micro text-ink-3 bg-high px-1.5 py-0.5 rounded-control">{sub.transport}</span>
                                    </div>
                                    <span className="text-micro text-ink-3">{sub.tool_count} tool{sub.tool_count !== 1 ? 's' : ''}</span>
                                  </div>
                                  <div className="space-y-1.5">
                                    {sub.command && (
                                      <div className="flex items-start gap-2">
                                        <span className="text-micro text-ink-3 w-16 shrink-0 mt-0.5 uppercase tracking-wider">Command</span>
                                        <code className="text-xs text-beam font-mono">{sub.command}</code>
                                      </div>
                                    )}
                                    {sub.args?.length > 0 && (
                                      <div className="flex items-start gap-2">
                                        <span className="text-micro text-ink-3 w-16 shrink-0 mt-0.5 uppercase tracking-wider">Args</span>
                                        <div className="flex flex-wrap gap-1">
                                          {sub.args.map((arg: string, j: number) => (
                                            <code key={j} className="text-xs text-ink-2 bg-neutral-wash px-1.5 py-0.5 rounded-control font-mono">{arg}</code>
                                          ))}
                                        </div>
                                      </div>
                                    )}
                                    {sub.url && (
                                      <div className="flex items-start gap-2">
                                        <span className="text-micro text-ink-3 w-16 shrink-0 mt-0.5 uppercase tracking-wider">URL</span>
                                        <code className="text-xs text-ink-2 font-mono">{sub.url}</code>
                                      </div>
                                    )}
                                    {sub.env_keys?.length > 0 && (
                                      <div className="flex items-start gap-2">
                                        <span className="text-micro text-ink-3 w-16 shrink-0 mt-0.5 uppercase tracking-wider">Env</span>
                                        <div className="flex flex-wrap gap-1">
                                          {sub.env_keys.map((key: string) => (
                                            <span key={key} className="text-xs text-warn bg-warn-wash px-1.5 py-0.5 rounded-control font-mono">{key}</span>
                                          ))}
                                        </div>
                                      </div>
                                    )}
                                  </div>
                                </div>
                              );
                            })}
                          </div>
                        ) : (
                          <p className="text-xs text-ink-3">No sub-backend information available. Reconnect the agent to refresh it.</p>
                        )}
                      </div>
                    )}
                    {backend.transport !== 'stdio' && backend.transport !== 'agent' && (
                      <pre className="text-xs text-ink-2 bg-inset p-3 rounded-row overflow-auto max-h-48 font-mono mb-3">
                        {JSON.stringify(backend.config, null, 2)}
                      </pre>
                    )}
                    <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-x-8 gap-y-3 border-t border-line-soft pt-3">
                      <div>
                        <Label className="block">Created</Label>
                        <span className="text-2xs text-ink-2">{fmt.dateTime(backend.created_at)}</span>
                      </div>
                      <div>
                        <Label className="block">Last health check</Label>
                        <span className="text-2xs text-ink-2">
                          {backend.last_health_check ? fmt.relative(backend.last_health_check) : 'Never'}
                        </span>
                      </div>
                      <div>
                        <Label className="block">Backend ID</Label>
                        <Mono className="text-2xs text-ink-2">{backend.backend_id.slice(0, 8)}</Mono>
                      </div>
                    </div>
                  </div>
                )}
              </div>
            );
          })}
        </RailList>
      )}

      {showDeleteConfirm && (
        <div className="fixed inset-0 bg-scrim flex items-center justify-center z-50">
          <div className="bg-panel border border-line rounded-panel p-6 w-full max-w-sm">
            <h3 className="text-md font-semibold text-ink mb-2">Delete this backend?</h3>
            <p className="text-sm text-ink-2 mb-4">
              <span className="font-medium text-ink">
                {backends.find(b => b.backend_id === showDeleteConfirm)?.name}
              </span>{' '}
              and its tools are withdrawn from the gateway. This cannot be undone.
            </p>
            <div className="flex gap-3">
              <button onClick={() => setShowDeleteConfirm(null)} className="flex-1 py-2 bg-raised border border-line text-ink-2 text-sm rounded-row hover:bg-high transition-colors">Cancel</button>
              <button onClick={confirmDeleteBackend} className="flex-1 py-2 bg-deny hover:opacity-90 text-on-solid text-sm font-medium rounded-row transition-colors">Delete backend</button>
            </div>
          </div>
        </div>
      )}

      {showJsonEditor && (
        <div className="fixed inset-0 bg-scrim flex items-center justify-center z-50">
          <div className="bg-panel border border-line rounded-panel p-6 w-full max-w-2xl max-h-[90vh] overflow-y-auto">
            <div className="flex items-center justify-between mb-4">
              <div>
                <h3 className="text-md font-semibold text-ink">Edit backends as JSON</h3>
                <p className="text-xs text-ink-3 mt-0.5">Edit all backend configurations as JSON. Existing backends are matched by name.</p>
              </div>
              <button onClick={() => setShowJsonEditor(false)} className="text-ink-3 hover:text-ink-2">
                <X className="w-5 h-5" />
              </button>
            </div>
            <textarea
              value={jsonContent}
              onChange={e => setJsonContent(e.target.value)}
              className="w-full h-96 px-4 py-3 bg-inset border border-line rounded-row text-xs text-ink-2 font-mono focus:outline-none focus:border-beam-edge resize-none"
              spellCheck={false}
            />
            {jsonError && (
              <div className="mt-2 px-3 py-2 bg-deny-wash border border-deny-edge rounded-row">
                <p className="text-xs text-deny">{jsonError}</p>
              </div>
            )}
            <div className="flex gap-3 mt-4">
              <button onClick={() => setShowJsonEditor(false)} className="flex-1 py-2 bg-raised border border-line text-ink-2 text-sm rounded-row hover:bg-high transition-colors">Cancel</button>
              <button onClick={saveJsonBackends} disabled={jsonSaving} className="flex-1 py-2 bg-solid hover:bg-solid-hover text-on-solid text-sm font-medium rounded-row transition-colors disabled:opacity-50">{jsonSaving ? 'Saving...' : 'Save changes'}</button>
            </div>
          </div>
        </div>
      )}

      {/* Create / Edit modal */}
      {showModal && (
        <div className="fixed inset-0 bg-scrim flex items-center justify-center z-50">
          <div className="bg-panel border border-line rounded-panel p-6 w-full max-w-lg max-h-[90vh] overflow-y-auto">
            <div className="flex items-center justify-between mb-5">
              <h3 className="text-md font-semibold text-ink">{editingBackend ? 'Edit backend' : 'Add backend'}</h3>
              <button onClick={() => { setShowModal(false); resetForm(); }} className="text-ink-3 hover:text-ink-2">
                <X className="w-5 h-5" />
              </button>
            </div>

            <div className="space-y-4">
              <div>
                <label className="block text-xs font-medium text-ink-2 mb-1.5 uppercase tracking-wider">Backend name</label>
                <input
                  type="text"
                  value={name}
                  onChange={e => setName(e.target.value)}
                  disabled={!!editingBackend}
                  className={clsx(
                    'w-full px-3 py-2 bg-inset border border-line rounded-row text-sm text-ink focus:outline-none focus:border-beam-edge',
                    editingBackend && 'opacity-60 cursor-not-allowed'
                  )}
                  placeholder="e.g. gitea, n8n-mcp, filesystem"
                />
              </div>

              <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
                <div>
                  <label className="block text-xs font-medium text-ink-2 mb-1.5 uppercase tracking-wider">Transport</label>
                  <select
                    value={transport}
                    onChange={e => setTransport(e.target.value)}
                    className="w-full px-3 py-2 bg-inset border border-line rounded-row text-sm text-ink-2 focus:outline-none focus:border-beam-edge"
                  >
                    <option value="stdio">stdio</option>
                    <option value="streamable-http">streamable-http</option>
                    <option value="sse">sse</option>
                  </select>
                </div>
                <div>
                  <label className="block text-xs font-medium text-ink-2 mb-1.5 uppercase tracking-wider">Risk category</label>
                  <select
                    value={riskCategory}
                    onChange={e => setRiskCategory(e.target.value)}
                    className="w-full px-3 py-2 bg-inset border border-line rounded-row text-sm text-ink-2 focus:outline-none focus:border-beam-edge"
                  >
                    <option value="read">read</option>
                    <option value="write">write</option>
                    <option value="filesystem">filesystem</option>
                    <option value="network">network</option>
                    <option value="external-api">external-api</option>
                    <option value="high-risk">high-risk</option>
                    <option value="admin">admin</option>
                  </select>
                </div>
              </div>

              {transport === 'stdio' && (
                <>
                  <div>
                    <label className="block text-xs font-medium text-ink-2 mb-1.5 uppercase tracking-wider">Command</label>
                    <input
                      type="text"
                      value={stdioForm.command}
                      onChange={e => setStdioForm({ ...stdioForm, command: e.target.value })}
                      className="w-full px-3 py-2 bg-inset border border-line rounded-row text-sm text-ink font-mono focus:outline-none focus:border-beam-edge"
                      placeholder="e.g. npx, go, node, python"
                    />
                  </div>
                  <div>
                    <div className="flex items-center justify-between mb-1.5">
                      <label className="block text-xs font-medium text-ink-2 uppercase tracking-wider">Arguments</label>
                      <button type="button" onClick={addArg} className="text-xs text-beam hover:text-beam transition-colors">+ Add argument</button>
                    </div>
                    <div className="space-y-2">
                      {stdioForm.args.map((arg, i) => (
                        <div key={i} className="flex items-center gap-2">
                          <span className="text-xs text-ink-4 font-mono w-5 text-right shrink-0">{i}</span>
                          <input
                            type="text"
                            value={arg}
                            onChange={e => updateArg(i, e.target.value)}
                            placeholder={i === 0 ? 'e.g. run, -y, n8n-mcp' : ''}
                            className="flex-1 px-2.5 py-1.5 bg-inset border border-line rounded-row text-xs text-ink font-mono focus:outline-none focus:border-beam-edge"
                          />
                          <button type="button" onClick={() => removeArg(i)} className="p-1 text-ink-4 hover:text-deny transition-colors">
                            <X className="w-3.5 h-3.5" />
                          </button>
                        </div>
                      ))}
                    </div>
                  </div>
                  {renderEnvFields('stdio')}
                </>
              )}

              {transport !== 'stdio' && (
                <>
                  <div>
                    <label className="block text-xs font-medium text-ink-2 mb-1.5 uppercase tracking-wider">URL</label>
                    <input
                      type="text"
                      value={httpForm.url}
                      onChange={e => setHttpForm({ ...httpForm, url: e.target.value })}
                      className="w-full px-3 py-2 bg-inset border border-line rounded-row text-sm text-ink font-mono focus:outline-none focus:border-beam-edge"
                      placeholder="e.g. http://localhost:8080/mcp"
                    />
                  </div>
                  {renderEnvFields('http')}
                </>
              )}

              {error && (
                <div className="px-3 py-2 bg-deny-wash border border-deny-edge rounded-row">
                  <p className="text-xs text-deny">{error}</p>
                </div>
              )}

              <div className="flex gap-3 pt-2">
                <button
                  onClick={() => { setShowModal(false); resetForm(); }}
                  className="flex-1 py-2 bg-raised border border-line text-ink-2 text-sm rounded-row hover:bg-high transition-colors"
                >
                  Cancel
                </button>
                <button
                  onClick={saveBackend}
                  disabled={isSubmitting}
                  className="flex-1 py-2 bg-solid hover:bg-solid-hover text-on-solid text-sm font-medium rounded-row transition-colors disabled:opacity-50"
                >
                  {isSubmitting ? 'Saving...' : (editingBackend ? 'Save changes' : 'Add backend')}
                </button>
              </div>
            </div>
          </div>
        </div>
      )}

      {/* Connect / Client Config Modal */}
      {showConnectModal && (
        <div className="fixed inset-0 bg-scrim flex items-center justify-center z-50">
          <div className="bg-panel border border-line rounded-panel p-6 w-full max-w-xl max-h-[90vh] overflow-y-auto">
            <div className="flex items-center justify-between mb-5">
              <div>
                <h3 className="text-md font-semibold text-ink">Connect a client</h3>
                <p className="text-xs text-ink-3 mt-0.5">Add this config to connect your AI client to the gateway.</p>
              </div>
              <button onClick={() => setShowConnectModal(false)} className="text-ink-3 hover:text-ink-2">
                <X className="w-5 h-5" />
              </button>
            </div>

            {/* Tab bar */}
            <div className="flex border-b border-line mb-4 overflow-x-auto">
              {CONNECT_TABS.map(tab => (
                <button
                  key={tab.key}
                  onClick={() => { setConnectTab(tab.key); setCopied(false); }}
                  className={clsx('px-3 py-2.5 text-sm font-medium border-b-2 transition-colors -mb-px whitespace-nowrap shrink-0',
                    connectTab === tab.key ? 'border-beam-edge text-ink' : 'border-transparent text-ink-3 hover:text-ink-2'
                  )}
                >
                  {tab.label}
                </button>
              ))}
            </div>

            {connectLoading ? (
              <div className="py-8 text-center text-ink-3 text-sm">Loading...</div>
            ) : (
              <>
                {/* Generate / rotate this client's key */}
                <div className="flex items-center justify-between gap-3 mb-3">
                  <p className="text-2xs text-ink-3">One key per client. Generating a new key revokes the old one.</p>
                  <button
                    onClick={regenerateAppKey}
                    disabled={regeneratingKey}
                    className="flex items-center gap-1.5 px-2.5 py-1.5 bg-beam-wash border border-beam-edge text-beam text-xs rounded-row hover:bg-beam-wash/20 transition-colors disabled:opacity-50 shrink-0"
                  >
                    <RotateCcw className={clsx('w-3.5 h-3.5', regeneratingKey && 'animate-spin')} />
                    {regeneratingKey ? 'Generating...' : 'Generate new key'}
                  </button>
                </div>
                {regenError && (
                  <div className="mb-3 px-3 py-2 bg-deny-wash border border-deny-edge rounded-row">
                    <p className="text-xs text-deny">{regenError}</p>
                  </div>
                )}

                {connectTab === 'openwebui' ? (
                  <>
                    <div className="mb-3 space-y-3">
                      <p className="text-xs text-ink-2">
                        Open WebUI (v0.6.31+) connects to MCP servers through the admin UI, not a config file.
                      </p>
                      <ol className="text-xs text-ink-2 space-y-2 list-decimal list-inside">
                        <li>Go to <span className="text-ink font-medium">Admin Settings → External Tools</span></li>
                        <li>Click <span className="text-ink font-medium">+ (Add Server)</span></li>
                        <li>Set <span className="text-ink font-medium">Type</span> to <code className="text-beam bg-beam-wash px-1.5 py-0.5 rounded-control text-2xs">MCP (Streamable HTTP)</code></li>
                        <li>Enter the <span className="text-ink font-medium">Server URL</span> below</li>
                        <li>Set <span className="text-ink font-medium">Auth</span> to <code className="text-beam bg-beam-wash px-1.5 py-0.5 rounded-control text-2xs">Bearer</code> and paste the token</li>
                        <li>Save and restart Open WebUI if prompted</li>
                      </ol>
                    </div>

                    <div className="space-y-3">
                      <div>
                        <label className="block text-micro font-medium text-ink-3 uppercase tracking-wider mb-1">Server URL</label>
                        <div className="relative">
                          <input
                            readOnly
                            value={getGatewayUrl()}
                            className="w-full px-3 py-2 pr-16 bg-inset border border-line rounded-row text-xs text-ink font-mono focus:outline-none"
                          />
                          <button
                            onClick={async () => {
                              await navigator.clipboard.writeText(getGatewayUrl());
                              setCopied(true);
                              setTimeout(() => setCopied(false), 2000);
                            }}
                            className={clsx(
                              'absolute right-1.5 top-1/2 -translate-y-1/2 flex items-center gap-1 px-2 py-1 rounded-control text-micro transition-colors',
                              copied ? 'bg-beam-wash text-beam' : 'bg-raised text-ink-2 hover:text-ink'
                            )}
                          >
                            {copied ? <Check className="w-3 h-3" /> : <Copy className="w-3 h-3" />}
                            {copied ? 'Copied' : 'Copy'}
                          </button>
                        </div>
                      </div>

                      <div>
                        <label className="block text-micro font-medium text-ink-3 uppercase tracking-wider mb-1">Bearer token</label>
                        <div className="relative">
                          <input
                            readOnly
                            value={getKeyForDisplay()}
                            className="w-full px-3 py-2 pr-16 bg-inset border border-line rounded-row text-xs text-ink font-mono focus:outline-none"
                          />
                          <button
                            onClick={async () => {
                              await navigator.clipboard.writeText(getKeyForCopy());
                              setCopied(true);
                              setTimeout(() => setCopied(false), 2000);
                            }}
                            className={clsx(
                              'absolute right-1.5 top-1/2 -translate-y-1/2 flex items-center gap-1 px-2 py-1 rounded-control text-micro transition-colors',
                              copied ? 'bg-beam-wash text-beam' : 'bg-raised text-ink-2 hover:text-ink'
                            )}
                          >
                            {copied ? <Check className="w-3 h-3" /> : <Copy className="w-3 h-3" />}
                            {copied ? 'Copied' : 'Copy'}
                          </button>
                        </div>
                      </div>
                    </div>
                  </>
                ) : (
                  <>
                    <div className="mb-2">
                      <p className="text-xs text-ink-3">{getConfigHint()}</p>
                    </div>

                    {connectTab === 'clawbot' && (
                      <div className="mb-3 p-3 bg-raised rounded-row border border-line-soft">
                        <h4 className="text-xs font-medium text-ink mb-2">Clawbot setup instructions</h4>
                        <ol className="text-xs text-ink-2 space-y-1.5 list-decimal list-inside">
                          <li>Ensure Clawbot v2.0+ is installed with MCP support</li>
                          <li>Add the config below to your Clawbot configuration file</li>
                          <li>Restart Clawbot to load the new MCP gateway connection</li>
                          <li>Verify connection with <code className="text-beam bg-beam-wash px-1 py-0.5 rounded-control text-micro">clawbot mcp list</code></li>
                        </ol>
                      </div>
                    )}

                    {connectTab === 'codex' && (
                      <div className="mb-3 p-3 bg-raised rounded-row border border-line-soft">
                        <h4 className="text-xs font-medium text-ink mb-2">Codex setup instructions</h4>
                        <ol className="text-xs text-ink-2 space-y-1.5 list-decimal list-inside">
                          <li>Create or edit <code className="text-beam bg-beam-wash px-1 py-0.5 rounded-control text-micro">~/.codex/mcp.json</code></li>
                          <li>Add the configuration block below</li>
                          <li>Restart Codex or run <code className="text-beam bg-beam-wash px-1 py-0.5 rounded-control text-micro">codex mcp refresh</code></li>
                          <li>Tools from the gateway will appear in your Codex session</li>
                        </ol>
                      </div>
                    )}

                    {connectTab === 'lmstudio' && (
                      <div className="mb-3 p-3 bg-raised rounded-row border border-line-soft">
                        <h4 className="text-xs font-medium text-ink mb-2">LM Studio setup instructions</h4>
                        <ol className="text-xs text-ink-2 space-y-1.5 list-decimal list-inside">
                          <li>Open LM Studio (v0.3.12+) and go to <span className="text-ink font-medium">Settings → MCP Servers</span></li>
                          <li>Click <span className="text-ink font-medium">Add Server</span> and paste the configuration below</li>
                          <li>Save and restart the chat session to load MCP tools</li>
                        </ol>
                      </div>
                    )}

                    <div className="relative">
                      <pre className="bg-inset border border-line rounded-row p-4 text-xs text-ink-2 font-mono overflow-auto max-h-64">
                        {getConfigForDisplay()}
                      </pre>
                      <button
                        onClick={copyConfig}
                        className={clsx(
                          'absolute top-2 right-2 flex items-center gap-1.5 px-2.5 py-1.5 rounded-control text-xs transition-colors',
                          copied
                            ? 'bg-beam-wash text-beam'
                            : 'bg-raised text-ink-2 hover:text-ink'
                        )}
                      >
                        {copied ? <Check className="w-3.5 h-3.5" /> : <Copy className="w-3.5 h-3.5" />}
                        {copied ? 'Copied' : 'Copy'}
                      </button>
                    </div>
                  </>
                )}

                <p className="text-xs text-ink-4 mt-3">{getConfigNote()}</p>
              </>
            )}

            <div className="flex justify-end mt-4">
              <button
                onClick={() => setShowConnectModal(false)}
                className="px-4 py-2 bg-raised border border-line text-ink-2 text-sm rounded-row hover:bg-high transition-colors"
              >
                Done
              </button>
            </div>
          </div>
        </div>
      )}

    </div>
  );
}
