import { useState, useEffect } from 'react';
import { api, Backend, ApiKey, User } from '@/lib/api';
import { Plus, Trash2, Server, Wifi, Terminal, Globe, X, RefreshCw, Link, Copy, Check, RotateCcw, Key, Pencil, Laptop, Boxes } from 'lucide-react';
import clsx from 'clsx';

interface Props {
  isAdmin: boolean;
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
  const [selectedKeyId, setSelectedKeyId] = useState<string>('');
  const [newKeyName, setNewKeyName] = useState('');
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
  const [connectSubmitting, setConnectSubmitting] = useState(false);
  const [connectError, setConnectError] = useState('');
  const [users, setUsers] = useState<User[]>([]);
  const [gatewayUrl, setGatewayUrl] = useState(() => localStorage.getItem('mcpgw_gateway_url') || '');
  const [gatewayUrlSaved, setGatewayUrlSaved] = useState(false);

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
        env: Object.entries(cfg.env || {}).map(([key, value]) => ({ key, value: String(value) })),
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
    setNewKeyName('');
    setNewKeyUserId('');
    setConnectError('');
    setConnectLoading(true);
    try {
      const [keys, userList] = await Promise.all([
        api.getApiKeys(),
        isAdmin ? api.getUsers().catch(() => [] as User[]) : Promise.resolve([] as User[]),
      ]);
      setApiKeys(keys);
      setUsers(userList);
      if (keys.length > 0) {
        setSelectedKeyId(keys[0].key_id);
      }
      if (userList.length > 0) {
        setNewKeyUserId(userList[0].user_id);
      }
    } catch {
      setApiKeys([]);
    } finally {
      setConnectLoading(false);
    }
  };

  const provisionKeys = async () => {
    if (connectSubmitting) return;
    setConnectError('');
    setConnectSubmitting(true);
    try {
      let currentUser: any = {};
      try {
        currentUser = JSON.parse(localStorage.getItem('mcpgw_user') || '{}');
      } catch {
        currentUser = {};
      }
      const targetUserId = (isAdmin && newKeyUserId) ? newKeyUserId : currentUser.user_id;
      if (!targetUserId) return;
      const result = await api.createApiKey({
        name: `${connectTab}-key`,
        user_id: targetUserId,
        application: connectTab,
      });
      setGeneratedKeys(prev => ({ ...prev, [connectTab]: result.raw_key }));
      const keys = await api.getApiKeys();
      setApiKeys(keys);
    } catch (e: any) {
      setConnectError(e.message || 'Failed to generate key');
    } finally {
      setConnectSubmitting(false);
    }
  };

  const generateApiKey = async () => {
    if (connectSubmitting) return;
    const keyName = newKeyName.trim() || 'mcpgw-client-key';
    setConnectError('');
    setConnectSubmitting(true);
    try {
      const result = await api.createApiKey({
        name: keyName,
        user_id: newKeyUserId || undefined,
      });
      setGeneratedKeys(prev => ({ ...prev, [connectTab]: result.raw_key }));
      setSelectedKeyId(result.key_id);
      setNewKeyName('');
      const keys = await api.getApiKeys();
      setApiKeys(keys);
    } catch (e: any) {
      setConnectError(e.message || 'Failed to generate API key');
    } finally {
      setConnectSubmitting(false);
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

  const saveGatewayUrl = () => {
    localStorage.setItem('mcpgw_gateway_url', gatewayUrl);
    setGatewayUrlSaved(true);
    setTimeout(() => setGatewayUrlSaved(false), 2000);
  };

  const getGatewayUrl = () => gatewayUrl || getDefaultGatewayUrl();

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
      const env: Record<string, string> = {};
      httpForm.env.forEach(e => { if (e.key.trim()) env[e.key.trim()] = e.value; });
      return { url: httpForm.url, env };
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

  const HEALTH_COLORS: Record<string, string> = {
    healthy: 'text-success',
    unhealthy: 'text-danger',
    idle: 'text-blue-400',
    unknown: 'text-gray-500',
  };

  const HEALTH_LABELS: Record<string, string> = {
    healthy: 'Healthy',
    unhealthy: 'Unhealthy',
    idle: 'Idle',
    unknown: 'Unknown',
  };

  const renderEnvFields = (form: 'stdio' | 'http') => {
    const entries = form === 'stdio' ? stdioForm.env : httpForm.env;
    return (
      <div>
        <div className="flex items-center justify-between mb-1.5">
          <label className="block text-xs font-medium text-gray-400 uppercase tracking-wider">Environment Variables</label>
          <button type="button" onClick={() => addEnv(form)} className="text-xs text-accent hover:text-accent-hover transition-colors">+ Add Variable</button>
        </div>
        <div className="space-y-2">
          {entries.map((entry, i) => (
            <div key={i} className="flex items-center gap-2">
              <input
                type="text"
                value={entry.key}
                onChange={e => updateEnv(form, i, 'key', e.target.value)}
                placeholder="KEY"
                className="w-[40%] px-2.5 py-1.5 bg-[#0a0a0f] border border-border rounded-lg text-xs text-white font-mono focus:outline-none focus:border-accent/50"
              />
              <span className="text-gray-600 text-xs">=</span>
              <input
                type="text"
                value={entry.value}
                onChange={e => updateEnv(form, i, 'value', e.target.value)}
                placeholder="value"
                className="flex-1 px-2.5 py-1.5 bg-[#0a0a0f] border border-border rounded-lg text-xs text-white font-mono focus:outline-none focus:border-accent/50"
              />
              <button type="button" onClick={() => removeEnv(form, i)} className="p-1 text-gray-600 hover:text-danger transition-colors">
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
      <div className="flex items-center justify-between mb-6">
        <div>
          <h2 className="text-lg font-semibold text-white">Backend MCP Servers</h2>
          <p className="text-sm text-gray-500 mt-1">Connected MCP server backends</p>
        </div>
        <div className="flex items-center gap-2">
          <button
            onClick={openConnectModal}
            className="flex items-center gap-2 px-3 py-2 bg-surface border border-accent/30 rounded-lg text-sm text-accent hover:bg-accent/10 transition-colors"
          >
            <Link className="w-4 h-4" />
            Connect
          </button>
          <button
            onClick={syncAllBackends}
            disabled={syncingAll}
            className="flex items-center gap-2 px-3 py-2 bg-surface border border-border rounded-lg text-sm text-gray-300 hover:border-border-hover transition-colors disabled:opacity-50"
          >
            <RefreshCw className={clsx('w-4 h-4', syncingAll && 'animate-spin')} />
            {syncingAll ? 'Syncing...' : 'Refresh & Sync'}
          </button>
          {isAdmin && (
            <div className="relative">
              <button
                onClick={() => setShowAddMenu(!showAddMenu)}
                className="flex items-center gap-2 px-4 py-2 bg-accent hover:bg-accent-hover text-white text-sm font-medium rounded-lg transition-colors"
              >
                <Plus className="w-4 h-4" />
                Add Backend
              </button>
              {showAddMenu && (
                <div className="absolute right-0 mt-1 w-48 bg-surface border border-border rounded-lg shadow-xl z-10 overflow-hidden">
                  <button
                    onClick={() => { setShowAddMenu(false); openModal(); }}
                    className="w-full flex items-center gap-2 px-4 py-2.5 text-sm text-gray-300 hover:bg-surface-hover hover:text-white transition-colors"
                  >
                    <Plus className="w-4 h-4" />
                    New Backend
                  </button>
                  <button
                    onClick={() => { setShowAddMenu(false); openJsonEditor(); }}
                    className="w-full flex items-center gap-2 px-4 py-2.5 text-sm text-gray-300 hover:bg-surface-hover hover:text-white transition-colors border-t border-border"
                  >
                    <Pencil className="w-4 h-4" />
                    Edit JSON
                  </button>
                </div>
              )}
            </div>
          )}
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

      {/* Backend cards */}
      <div className="grid grid-cols-1 gap-3">
        {loading ? (
          <div className="bg-surface border border-border rounded-xl p-12 text-center text-gray-500 text-sm">Loading backends...</div>
        ) : backends.length === 0 ? (
          <div className="bg-surface border border-border rounded-xl p-12 text-center text-gray-500 text-sm">No backends configured</div>
        ) : (
          backends.map(backend => {
            const isSelected = selectedBackend?.backend_id === backend.backend_id;
            return (
              <div key={backend.backend_id}>
                <div
                  className={clsx(
                    'bg-surface border rounded-xl p-5 transition-all cursor-pointer',
                    isSelected ? 'border-accent/30' : 'border-border hover:border-border-hover',
                    !backend.is_enabled && 'opacity-60'
                  )}
                  onClick={() => setSelectedBackend(isSelected ? null : backend)}
                >
                  <div className="flex items-center justify-between">
                    <div className="flex items-center gap-4">
                      <div className={clsx('w-10 h-10 rounded-xl flex items-center justify-center',
                        backend.health_status === 'healthy' ? 'bg-success/10' : backend.health_status === 'unhealthy' ? 'bg-danger/10' : backend.health_status === 'idle' ? 'bg-blue-500/10' : 'bg-surface-active'
                      )}>
                        {backend.transport === 'agent' ? (
                          <Laptop className={clsx('w-5 h-5',
                            backend.health_status === 'healthy' ? 'text-success' : backend.health_status === 'unhealthy' ? 'text-danger' : backend.health_status === 'idle' ? 'text-blue-400' : 'text-gray-500'
                          )} />
                        ) : (
                          <Server className={clsx('w-5 h-5',
                            backend.health_status === 'healthy' ? 'text-success' : backend.health_status === 'unhealthy' ? 'text-danger' : backend.health_status === 'idle' ? 'text-blue-400' : 'text-gray-500'
                          )} />
                        )}
                      </div>
                      <div>
                        <div className="flex items-center gap-2.5">
                          <h3 className="text-sm font-semibold text-white">{backend.name}</h3>
                          <span className={clsx('inline-flex items-center gap-1 text-xs font-medium', HEALTH_COLORS[backend.health_status] || HEALTH_COLORS.unknown)}>
                            <span className={clsx('w-1.5 h-1.5 rounded-full',
                              backend.health_status === 'healthy' ? 'bg-success' : backend.health_status === 'unhealthy' ? 'bg-danger' : backend.health_status === 'idle' ? 'bg-blue-400' : 'bg-gray-600'
                            )} />
                            {HEALTH_LABELS[backend.health_status] || backend.health_status}
                          </span>
                          {!backend.is_enabled && (
                            <span className="text-xs text-gray-500 bg-surface-active px-1.5 py-0.5 rounded">disabled</span>
                          )}
                        </div>
                        <div className="flex items-center gap-3 mt-1">
                          <span className="text-xs text-gray-500">{backend.tool_count} tools</span>
                        </div>
                      </div>
                    </div>

                    {isAdmin && (
                      <div className="flex items-center gap-2">
                        <button
                          onClick={e => { e.stopPropagation(); openEditModal(backend); }}
                          className="flex items-center gap-1.5 text-xs px-3 py-1.5 rounded-md text-gray-400 hover:bg-surface-hover hover:text-white transition-colors"
                        >
                          <Pencil className="w-3.5 h-3.5" />
                          Edit
                        </button>
                        {backend.is_enabled && (
                          <button
                            onClick={e => { e.stopPropagation(); syncBackend(backend.backend_id); }}
                            disabled={syncingId === backend.backend_id}
                            className="flex items-center gap-1.5 text-xs px-3 py-1.5 rounded-md text-blue-400 hover:bg-blue-500/10 transition-colors disabled:opacity-50"
                          >
                            <RotateCcw className={clsx('w-3.5 h-3.5', syncingId === backend.backend_id && 'animate-spin')} />
                            Sync
                          </button>
                        )}
                        <button
                          onClick={e => { e.stopPropagation(); toggleBackend(backend); }}
                          className={clsx('text-xs px-3 py-1.5 rounded-md transition-colors',
                            backend.is_enabled
                              ? 'text-gray-400 hover:bg-surface-hover'
                              : 'text-success hover:bg-success/10'
                          )}
                        >
                          {backend.is_enabled ? 'Disable' : 'Enable'}
                        </button>
                        <button
                          onClick={e => { e.stopPropagation(); setShowDeleteConfirm(backend.backend_id); }}
                          className="p-1.5 text-gray-500 hover:text-danger hover:bg-danger/10 rounded-md transition-colors"
                        >
                          <Trash2 className="w-4 h-4" />
                        </button>
                      </div>
                    )}
                  </div>
                </div>

                {/* Expanded config */}
                {isSelected && (
                  <div className="mx-2 bg-surface-hover border border-border/50 rounded-b-xl p-4 -mt-2">
                    <h4 className="text-xs font-medium text-gray-400 uppercase tracking-wider mb-2">Configuration</h4>
                    {backend.transport === 'stdio' && (
                      <div className="space-y-2 mb-3">
                        <div className="flex items-center gap-2">
                          <span className="text-xs text-gray-500 w-16 shrink-0">Command:</span>
                          <code className="text-xs text-emerald-400 font-mono">{(backend.config as any).command}</code>
                        </div>
                        {(backend.config as any).args?.length > 0 && (
                          <div className="flex items-start gap-2">
                            <span className="text-xs text-gray-500 w-16 shrink-0 mt-0.5">Args:</span>
                            <div className="flex flex-wrap gap-1">
                              {(backend.config as any).args.map((arg: string, i: number) => (
                                <code key={i} className="text-xs text-blue-400 bg-blue-500/10 px-1.5 py-0.5 rounded font-mono">{arg}</code>
                              ))}
                            </div>
                          </div>
                        )}
                        {Object.keys((backend.config as any).env || {}).length > 0 && (
                          <div className="flex items-start gap-2">
                            <span className="text-xs text-gray-500 w-16 shrink-0 mt-0.5">Env:</span>
                            <div className="space-y-1">
                              {Object.entries((backend.config as any).env || {}).map(([k, v]) => (
                                <div key={k} className="text-xs font-mono">
                                  <span className="text-amber-400">{k}</span>
                                  <span className="text-gray-600">=</span>
                                  <span className="text-gray-400">{String(v).length > 40 ? String(v).slice(0, 40) + '...' : String(v)}</span>
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
                          <Boxes className="w-4 h-4 text-accent" />
                          <span className="text-xs font-medium text-gray-300">Connected MCP Servers</span>
                          {(backend.config as any).sub_backends?.length > 0 && (
                            <span className="text-[10px] text-gray-500 bg-surface-active px-1.5 py-0.5 rounded">
                              {(backend.config as any).sub_backends.length} server{(backend.config as any).sub_backends.length !== 1 ? 's' : ''}
                            </span>
                          )}
                        </div>
                        {(backend.config as any).sub_backends?.length > 0 ? (
                          <div className="space-y-2">
                            {((backend.config as any).sub_backends as any[]).map((sub: any, i: number) => {
                              const SubIcon = TRANSPORT_ICONS[sub.transport] || Globe;
                              return (
                                <div key={i} className="bg-[#0a0a0f] border border-border rounded-lg p-3">
                                  <div className="flex items-center justify-between mb-2">
                                    <div className="flex items-center gap-2">
                                      <SubIcon className="w-3.5 h-3.5 text-accent" />
                                      <span className="text-sm font-medium text-white">{sub.name}</span>
                                      <span className="text-[10px] text-gray-500 bg-surface-active px-1.5 py-0.5 rounded">{sub.transport}</span>
                                    </div>
                                    <span className="text-[10px] text-gray-500">{sub.tool_count} tool{sub.tool_count !== 1 ? 's' : ''}</span>
                                  </div>
                                  <div className="space-y-1.5">
                                    {sub.command && (
                                      <div className="flex items-start gap-2">
                                        <span className="text-[10px] text-gray-500 w-16 shrink-0 mt-0.5 uppercase tracking-wider">Command</span>
                                        <code className="text-xs text-emerald-400 font-mono">{sub.command}</code>
                                      </div>
                                    )}
                                    {sub.args?.length > 0 && (
                                      <div className="flex items-start gap-2">
                                        <span className="text-[10px] text-gray-500 w-16 shrink-0 mt-0.5 uppercase tracking-wider">Args</span>
                                        <div className="flex flex-wrap gap-1">
                                          {sub.args.map((arg: string, j: number) => (
                                            <code key={j} className="text-xs text-blue-400 bg-blue-500/10 px-1.5 py-0.5 rounded font-mono">{arg}</code>
                                          ))}
                                        </div>
                                      </div>
                                    )}
                                    {sub.url && (
                                      <div className="flex items-start gap-2">
                                        <span className="text-[10px] text-gray-500 w-16 shrink-0 mt-0.5 uppercase tracking-wider">URL</span>
                                        <code className="text-xs text-blue-400 font-mono">{sub.url}</code>
                                      </div>
                                    )}
                                    {sub.env_keys?.length > 0 && (
                                      <div className="flex items-start gap-2">
                                        <span className="text-[10px] text-gray-500 w-16 shrink-0 mt-0.5 uppercase tracking-wider">Env</span>
                                        <div className="flex flex-wrap gap-1">
                                          {sub.env_keys.map((key: string) => (
                                            <span key={key} className="text-xs text-amber-400 bg-amber-500/10 px-1.5 py-0.5 rounded font-mono">{key}</span>
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
                          <p className="text-xs text-gray-500">No sub-backend information available. Reconnect the agent to populate.</p>
                        )}
                      </div>
                    )}
                    {backend.transport !== 'stdio' && backend.transport !== 'agent' && (
                      <pre className="text-xs text-gray-400 bg-[#0a0a0f] p-3 rounded-lg overflow-auto max-h-48 font-mono mb-3">
                        {JSON.stringify(backend.config, null, 2)}
                      </pre>
                    )}
                    <div className="grid grid-cols-3 gap-4 text-sm border-t border-border/50 pt-3">
                      <div><span className="text-gray-500">Created:</span> <span className="text-gray-300 text-xs">{new Date(backend.created_at).toLocaleString()}</span></div>
                      <div><span className="text-gray-500">Last Health Check:</span> <span className="text-gray-300 text-xs">{backend.last_health_check ? new Date(backend.last_health_check).toLocaleString() : 'Never'}</span></div>
                      <div><span className="text-gray-500">Backend ID:</span> <span className="text-gray-300 text-xs font-mono">{backend.backend_id.slice(0, 8)}</span></div>
                    </div>
                  </div>
                )}
              </div>
            );
          })
        )}
      </div>

      {showDeleteConfirm && (
        <div className="fixed inset-0 bg-black/60 flex items-center justify-center z-50">
          <div className="bg-surface border border-border rounded-2xl p-6 w-full max-w-sm">
            <h3 className="text-base font-semibold text-white mb-2">Delete Backend</h3>
            <p className="text-sm text-gray-400 mb-4">Are you sure you want to delete this backend? This action cannot be undone.</p>
            <div className="flex gap-3">
              <button onClick={() => setShowDeleteConfirm(null)} className="flex-1 py-2 bg-surface-hover border border-border text-gray-300 text-sm rounded-lg hover:bg-surface-active transition-colors">Cancel</button>
              <button onClick={confirmDeleteBackend} className="flex-1 py-2 bg-danger hover:bg-danger/80 text-white text-sm font-medium rounded-lg transition-colors">Yes, Delete</button>
            </div>
          </div>
        </div>
      )}

      {showJsonEditor && (
        <div className="fixed inset-0 bg-black/60 flex items-center justify-center z-50">
          <div className="bg-surface border border-border rounded-2xl p-6 w-full max-w-2xl max-h-[90vh] overflow-y-auto">
            <div className="flex items-center justify-between mb-4">
              <div>
                <h3 className="text-base font-semibold text-white">Edit Backends JSON</h3>
                <p className="text-xs text-gray-500 mt-0.5">Edit all backend configurations as JSON. Existing backends are matched by name.</p>
              </div>
              <button onClick={() => setShowJsonEditor(false)} className="text-gray-500 hover:text-gray-300">
                <X className="w-5 h-5" />
              </button>
            </div>
            <textarea
              value={jsonContent}
              onChange={e => setJsonContent(e.target.value)}
              className="w-full h-96 px-4 py-3 bg-[#0a0a0f] border border-border rounded-lg text-xs text-gray-300 font-mono focus:outline-none focus:border-accent/50 resize-none"
              spellCheck={false}
            />
            {jsonError && (
              <div className="mt-2 px-3 py-2 bg-danger/10 border border-danger/20 rounded-lg">
                <p className="text-xs text-danger">{jsonError}</p>
              </div>
            )}
            <div className="flex gap-3 mt-4">
              <button onClick={() => setShowJsonEditor(false)} className="flex-1 py-2 bg-surface-hover border border-border text-gray-300 text-sm rounded-lg hover:bg-surface-active transition-colors">Cancel</button>
              <button onClick={saveJsonBackends} disabled={jsonSaving} className="flex-1 py-2 bg-accent hover:bg-accent-hover text-white text-sm font-medium rounded-lg transition-colors disabled:opacity-50">{jsonSaving ? 'Saving…' : 'Save Changes'}</button>
            </div>
          </div>
        </div>
      )}

      {/* Create / Edit modal */}
      {showModal && (
        <div className="fixed inset-0 bg-black/60 flex items-center justify-center z-50">
          <div className="bg-surface border border-border rounded-2xl p-6 w-full max-w-lg max-h-[90vh] overflow-y-auto">
            <div className="flex items-center justify-between mb-5">
              <h3 className="text-base font-semibold text-white">{editingBackend ? 'Edit MCP Server' : 'Add MCP Server'}</h3>
              <button onClick={() => { setShowModal(false); resetForm(); }} className="text-gray-500 hover:text-gray-300">
                <X className="w-5 h-5" />
              </button>
            </div>

            <div className="space-y-4">
              <div>
                <label className="block text-xs font-medium text-gray-400 mb-1.5 uppercase tracking-wider">Server Name</label>
                <input
                  type="text"
                  value={name}
                  onChange={e => setName(e.target.value)}
                  disabled={!!editingBackend}
                  className={clsx(
                    'w-full px-3 py-2 bg-[#0a0a0f] border border-border rounded-lg text-sm text-white focus:outline-none focus:border-accent/50',
                    editingBackend && 'opacity-60 cursor-not-allowed'
                  )}
                  placeholder="e.g., gitea, n8n-mcp, filesystem"
                />
              </div>

              <div className="grid grid-cols-2 gap-4">
                <div>
                  <label className="block text-xs font-medium text-gray-400 mb-1.5 uppercase tracking-wider">Transport</label>
                  <select
                    value={transport}
                    onChange={e => setTransport(e.target.value)}
                    className="w-full px-3 py-2 bg-[#0a0a0f] border border-border rounded-lg text-sm text-gray-300 focus:outline-none focus:border-accent/50"
                  >
                    <option value="stdio">stdio</option>
                    <option value="streamable-http">streamable-http</option>
                    <option value="sse">sse</option>
                  </select>
                </div>
                <div>
                  <label className="block text-xs font-medium text-gray-400 mb-1.5 uppercase tracking-wider">Risk Category</label>
                  <select
                    value={riskCategory}
                    onChange={e => setRiskCategory(e.target.value)}
                    className="w-full px-3 py-2 bg-[#0a0a0f] border border-border rounded-lg text-sm text-gray-300 focus:outline-none focus:border-accent/50"
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
                    <label className="block text-xs font-medium text-gray-400 mb-1.5 uppercase tracking-wider">Command</label>
                    <input
                      type="text"
                      value={stdioForm.command}
                      onChange={e => setStdioForm({ ...stdioForm, command: e.target.value })}
                      className="w-full px-3 py-2 bg-[#0a0a0f] border border-border rounded-lg text-sm text-white font-mono focus:outline-none focus:border-accent/50"
                      placeholder="e.g., npx, go, node, python"
                    />
                  </div>
                  <div>
                    <div className="flex items-center justify-between mb-1.5">
                      <label className="block text-xs font-medium text-gray-400 uppercase tracking-wider">Arguments</label>
                      <button type="button" onClick={addArg} className="text-xs text-accent hover:text-accent-hover transition-colors">+ Add Arg</button>
                    </div>
                    <div className="space-y-2">
                      {stdioForm.args.map((arg, i) => (
                        <div key={i} className="flex items-center gap-2">
                          <span className="text-xs text-gray-600 font-mono w-5 text-right shrink-0">{i}</span>
                          <input
                            type="text"
                            value={arg}
                            onChange={e => updateArg(i, e.target.value)}
                            placeholder={i === 0 ? 'e.g., run, -y, n8n-mcp' : ''}
                            className="flex-1 px-2.5 py-1.5 bg-[#0a0a0f] border border-border rounded-lg text-xs text-white font-mono focus:outline-none focus:border-accent/50"
                          />
                          <button type="button" onClick={() => removeArg(i)} className="p-1 text-gray-600 hover:text-danger transition-colors">
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
                    <label className="block text-xs font-medium text-gray-400 mb-1.5 uppercase tracking-wider">URL</label>
                    <input
                      type="text"
                      value={httpForm.url}
                      onChange={e => setHttpForm({ ...httpForm, url: e.target.value })}
                      className="w-full px-3 py-2 bg-[#0a0a0f] border border-border rounded-lg text-sm text-white font-mono focus:outline-none focus:border-accent/50"
                      placeholder="e.g., http://localhost:8080/mcp"
                    />
                  </div>
                  {renderEnvFields('http')}
                </>
              )}

              {error && (
                <div className="px-3 py-2 bg-danger/10 border border-danger/20 rounded-lg">
                  <p className="text-xs text-danger">{error}</p>
                </div>
              )}

              <div className="flex gap-3 pt-2">
                <button
                  onClick={() => { setShowModal(false); resetForm(); }}
                  className="flex-1 py-2 bg-surface-hover border border-border text-gray-300 text-sm rounded-lg hover:bg-surface-active transition-colors"
                >
                  Cancel
                </button>
                <button
                  onClick={saveBackend}
                  disabled={isSubmitting}
                  className="flex-1 py-2 bg-accent hover:bg-accent-hover text-white text-sm font-medium rounded-lg transition-colors disabled:opacity-50"
                >
                  {isSubmitting ? 'Saving…' : (editingBackend ? 'Save Changes' : 'Add Server')}
                </button>
              </div>
            </div>
          </div>
        </div>
      )}

      {/* Connect / Client Config Modal */}
      {showConnectModal && (
        <div className="fixed inset-0 bg-black/60 flex items-center justify-center z-50">
          <div className="bg-surface border border-border rounded-2xl p-6 w-full max-w-xl max-h-[90vh] overflow-y-auto">
            <div className="flex items-center justify-between mb-5">
              <div>
                <h3 className="text-base font-semibold text-white">Connect AI Client</h3>
                <p className="text-xs text-gray-500 mt-0.5">Add this config to connect your AI client to the gateway</p>
              </div>
              <button onClick={() => setShowConnectModal(false)} className="text-gray-500 hover:text-gray-300">
                <X className="w-5 h-5" />
              </button>
            </div>

            {/* Tab bar */}
            <div className="flex border-b border-border mb-4 overflow-x-auto">
              {CONNECT_TABS.map(tab => (
                <button
                  key={tab.key}
                  onClick={() => { setConnectTab(tab.key); setCopied(false); }}
                  className={clsx('px-3 py-2.5 text-sm font-medium border-b-2 transition-colors -mb-px whitespace-nowrap shrink-0',
                    connectTab === tab.key ? 'border-accent text-white' : 'border-transparent text-gray-500 hover:text-gray-300'
                  )}
                >
                  {tab.label}
                </button>
              ))}
            </div>

            {connectLoading ? (
              <div className="py-8 text-center text-gray-500 text-sm">Loading...</div>
            ) : (
              <>
                {/* Gateway URL */}
                <div className="mb-4">
                  <label className="block text-xs font-medium text-gray-400 mb-1.5 uppercase tracking-wider">Gateway URL</label>
                  <div className="flex gap-2">
                    <input
                      type="text"
                      value={gatewayUrl}
                      onChange={e => { if (isAdmin) setGatewayUrl(e.target.value); }}
                      readOnly={!isAdmin}
                      className={clsx(
                        'flex-1 px-3 py-2 bg-[#0a0a0f] border border-border rounded-lg text-sm font-mono focus:outline-none',
                        isAdmin ? 'text-white focus:border-accent/50' : 'text-gray-500 cursor-not-allowed'
                      )}
                      placeholder={getDefaultGatewayUrl()}
                    />
                    {isAdmin && (
                      <button
                        onClick={saveGatewayUrl}
                        className={clsx(
                          'px-3 py-2 rounded-lg text-xs font-medium transition-colors shrink-0',
                          gatewayUrlSaved
                            ? 'bg-green-500/20 text-green-400 border border-green-500/30'
                            : 'bg-accent/10 text-accent border border-accent/20 hover:bg-accent/20'
                        )}
                      >
                        {gatewayUrlSaved ? 'Saved' : 'Save'}
                      </button>
                    )}
                  </div>
                  <p className="text-[10px] text-gray-600 mt-1">
                    {isAdmin
                      ? 'The public MCP endpoint URL that AI clients will connect to. Click Save to persist.'
                      : 'The public MCP endpoint URL configured by your admin.'}
                  </p>
                </div>

                {/* Per-app API Key display */}
                <div className="mb-4">
                  <label className="block text-xs font-medium text-gray-400 mb-1.5 uppercase tracking-wider">API Key for {connectTab}</label>

                  {isAdmin && users.length > 0 && (
                    <div className="mb-2">
                      <label className="block text-[10px] text-gray-500 mb-1">User</label>
                      <select
                        value={newKeyUserId}
                        onChange={e => setNewKeyUserId(e.target.value)}
                        className="w-full px-2.5 py-1.5 bg-[#0a0a0f] border border-border rounded-lg text-xs text-gray-300 focus:outline-none focus:border-accent/50"
                      >
                        {users.map(u => (
                          <option key={u.user_id} value={u.user_id}>
                            {u.username} ({u.roles?.join(', ') || 'no role'})
                          </option>
                        ))}
                      </select>
                    </div>
                  )}

                  {(() => {
                    const appKey = getAppKey(connectTab);
                    if (appKey) {
                      return (
                        <div className="space-y-2">
                          <div className="flex items-center gap-2 px-3 py-2 bg-[#0a0a0f] border border-border rounded-lg">
                            <Key className="w-3.5 h-3.5 text-gray-500 shrink-0" />
                            <code className="text-xs text-gray-300 font-mono">{appKey.key_prefix}...</code>
                            <span className="text-[10px] text-gray-600 ml-auto">{appKey.name}</span>
                          </div>
                          {generatedKeys[connectTab] && (
                            <div className="px-3 py-2 bg-success/10 border border-success/20 rounded-lg">
                              <p className="text-xs text-success mb-1">New key generated — save it now, it won't be shown again:</p>
                              <code className="text-xs text-success font-mono break-all">{generatedKeys[connectTab]}</code>
                            </div>
                          )}
                        </div>
                      );
                    }
                    return (
                      <div className="space-y-2">
                        <div className="px-3 py-2 bg-amber-500/10 border border-amber-500/20 rounded-lg">
                          <p className="text-xs text-amber-400">No per-app key found for {connectTab}. Provision keys to auto-generate one per application.</p>
                        </div>
                        <button
                          onClick={provisionKeys}
                          disabled={connectSubmitting}
                          className="flex items-center gap-1.5 px-3 py-1.5 bg-accent/10 border border-accent/30 text-accent text-xs rounded-lg hover:bg-accent/20 transition-colors w-full justify-center disabled:opacity-50"
                        >
                          <Key className="w-3.5 h-3.5" />
                          {connectSubmitting ? 'Provisioning…' : 'Provision App Keys'}
                        </button>
                        {generatedKeys[connectTab] && (
                          <div className="px-3 py-2 bg-success/10 border border-success/20 rounded-lg">
                            <p className="text-xs text-success mb-1">New key generated — save it now, it won't be shown again:</p>
                            <code className="text-xs text-success font-mono break-all">{generatedKeys[connectTab]}</code>
                          </div>
                        )}
                      </div>
                    );
                  })()}

                  {connectError && (
                    <div className="mt-2 px-3 py-2 bg-danger/10 border border-danger/20 rounded-lg">
                      <p className="text-xs text-danger">{connectError}</p>
                    </div>
                  )}
                </div>

                {connectTab === 'openwebui' ? (
                  <>
                    <div className="mb-3 space-y-3">
                      <p className="text-xs text-gray-400">
                        Open WebUI (v0.6.31+) connects to MCP servers through the admin UI, not a config file.
                      </p>
                      <ol className="text-xs text-gray-400 space-y-2 list-decimal list-inside">
                        <li>Go to <span className="text-white font-medium">Admin Settings → External Tools</span></li>
                        <li>Click <span className="text-white font-medium">+ (Add Server)</span></li>
                        <li>Set <span className="text-white font-medium">Type</span> to <code className="text-accent bg-accent/10 px-1.5 py-0.5 rounded text-[11px]">MCP (Streamable HTTP)</code></li>
                        <li>Enter the <span className="text-white font-medium">Server URL</span> below</li>
                        <li>Set <span className="text-white font-medium">Auth</span> to <code className="text-accent bg-accent/10 px-1.5 py-0.5 rounded text-[11px]">Bearer</code> and paste the token</li>
                        <li>Save and restart Open WebUI if prompted</li>
                      </ol>
                    </div>

                    <div className="space-y-3">
                      <div>
                        <label className="block text-[10px] font-medium text-gray-500 uppercase tracking-wider mb-1">Server URL</label>
                        <div className="relative">
                          <input
                            readOnly
                            value={getGatewayUrl()}
                            className="w-full px-3 py-2 pr-16 bg-[#0a0a0f] border border-border rounded-lg text-xs text-white font-mono focus:outline-none"
                          />
                          <button
                            onClick={async () => {
                              await navigator.clipboard.writeText(getGatewayUrl());
                              setCopied(true);
                              setTimeout(() => setCopied(false), 2000);
                            }}
                            className={clsx(
                              'absolute right-1.5 top-1/2 -translate-y-1/2 flex items-center gap-1 px-2 py-1 rounded text-[10px] transition-colors',
                              copied ? 'bg-success/20 text-success' : 'bg-surface-hover text-gray-400 hover:text-white'
                            )}
                          >
                            {copied ? <Check className="w-3 h-3" /> : <Copy className="w-3 h-3" />}
                            {copied ? 'Copied' : 'Copy'}
                          </button>
                        </div>
                      </div>

                      <div>
                        <label className="block text-[10px] font-medium text-gray-500 uppercase tracking-wider mb-1">Bearer Token</label>
                        <div className="relative">
                          <input
                            readOnly
                            value={getKeyForDisplay()}
                            className="w-full px-3 py-2 pr-16 bg-[#0a0a0f] border border-border rounded-lg text-xs text-white font-mono focus:outline-none"
                          />
                          <button
                            onClick={async () => {
                              await navigator.clipboard.writeText(getKeyForCopy());
                              setCopied(true);
                              setTimeout(() => setCopied(false), 2000);
                            }}
                            className={clsx(
                              'absolute right-1.5 top-1/2 -translate-y-1/2 flex items-center gap-1 px-2 py-1 rounded text-[10px] transition-colors',
                              copied ? 'bg-success/20 text-success' : 'bg-surface-hover text-gray-400 hover:text-white'
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
                      <p className="text-xs text-gray-500">{getConfigHint()}</p>
                    </div>

                    {connectTab === 'clawbot' && (
                      <div className="mb-3 p-3 bg-surface-hover rounded-lg border border-border/50">
                        <h4 className="text-xs font-medium text-white mb-2">Clawbot Setup Instructions</h4>
                        <ol className="text-xs text-gray-400 space-y-1.5 list-decimal list-inside">
                          <li>Ensure Clawbot v2.0+ is installed with MCP support</li>
                          <li>Add the config below to your Clawbot configuration file</li>
                          <li>Restart Clawbot to load the new MCP gateway connection</li>
                          <li>Verify connection with <code className="text-accent bg-accent/10 px-1 py-0.5 rounded text-[10px]">clawbot mcp list</code></li>
                        </ol>
                      </div>
                    )}

                    {connectTab === 'codex' && (
                      <div className="mb-3 p-3 bg-surface-hover rounded-lg border border-border/50">
                        <h4 className="text-xs font-medium text-white mb-2">Codex Setup Instructions</h4>
                        <ol className="text-xs text-gray-400 space-y-1.5 list-decimal list-inside">
                          <li>Create or edit <code className="text-accent bg-accent/10 px-1 py-0.5 rounded text-[10px]">~/.codex/mcp.json</code></li>
                          <li>Add the configuration block below</li>
                          <li>Restart Codex or run <code className="text-accent bg-accent/10 px-1 py-0.5 rounded text-[10px]">codex mcp refresh</code></li>
                          <li>Tools from the gateway will appear in your Codex session</li>
                        </ol>
                      </div>
                    )}

                    {connectTab === 'lmstudio' && (
                      <div className="mb-3 p-3 bg-surface-hover rounded-lg border border-border/50">
                        <h4 className="text-xs font-medium text-white mb-2">LM Studio Setup Instructions</h4>
                        <ol className="text-xs text-gray-400 space-y-1.5 list-decimal list-inside">
                          <li>Open LM Studio (v0.3.12+) and go to <span className="text-white font-medium">Settings → MCP Servers</span></li>
                          <li>Click <span className="text-white font-medium">Add Server</span> and paste the configuration below</li>
                          <li>Save and restart the chat session to load MCP tools</li>
                        </ol>
                      </div>
                    )}

                    <div className="relative">
                      <pre className="bg-[#0a0a0f] border border-border rounded-lg p-4 text-xs text-gray-300 font-mono overflow-auto max-h-64">
                        {getConfigForDisplay()}
                      </pre>
                      <button
                        onClick={copyConfig}
                        className={clsx(
                          'absolute top-2 right-2 flex items-center gap-1.5 px-2.5 py-1.5 rounded-md text-xs transition-colors',
                          copied
                            ? 'bg-success/20 text-success'
                            : 'bg-surface-hover text-gray-400 hover:text-white'
                        )}
                      >
                        {copied ? <Check className="w-3.5 h-3.5" /> : <Copy className="w-3.5 h-3.5" />}
                        {copied ? 'Copied' : 'Copy'}
                      </button>
                    </div>
                  </>
                )}

                <p className="text-xs text-gray-600 mt-3">{getConfigNote()}</p>
              </>
            )}

            <div className="flex justify-end mt-4">
              <button
                onClick={() => setShowConnectModal(false)}
                className="px-4 py-2 bg-surface-hover border border-border text-gray-300 text-sm rounded-lg hover:bg-surface-active transition-colors"
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
