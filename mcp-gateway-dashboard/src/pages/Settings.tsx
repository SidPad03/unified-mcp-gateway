import { useState, useEffect } from 'react';
import { api, Tool, Backend } from '@/lib/api';
import { Settings as SettingsIcon, Sparkles, Key, Eye, EyeOff, Loader2, CheckCircle, AlertTriangle, X, Info, Tag, ChevronDown, Link } from 'lucide-react';
import clsx from 'clsx';

const RISK_COLORS: Record<string, string> = {
  read: 'text-emerald-400',
  write: 'text-blue-400',
  admin: 'text-orange-400',
  destructive: 'text-red-400',
  execute: 'text-purple-400',
  unclassified: 'text-gray-400',
};

const RISK_BG: Record<string, string> = {
  read: 'bg-emerald-500/10 border-emerald-500/20',
  write: 'bg-blue-500/10 border-blue-500/20',
  admin: 'bg-orange-500/10 border-orange-500/20',
  destructive: 'bg-red-500/10 border-red-500/20',
  execute: 'bg-purple-500/10 border-purple-500/20',
  unclassified: 'bg-gray-500/10 border-gray-500/20',
};

const RISK_CATEGORIES = ['read', 'write', 'admin', 'destructive', 'execute', 'unclassified'];

export default function Settings() {
  // GPT-5 Classification state
  const [apiToken, setApiToken] = useState(() => sessionStorage.getItem('mcpgw_openai_token') || '');
  const [showToken, setShowToken] = useState(false);
  const [classifyMode, setClassifyMode] = useState<'all' | 'unclassified'>('unclassified');
  const [classifying, setClassifying] = useState(false);
  const [classifyProgress, setClassifyProgress] = useState<{ done: number; total: number } | null>(null);
  const [classifyResults, setClassifyResults] = useState<{ tool: string; from: string; to: string }[]>([]);
  const [classifyError, setClassifyError] = useState('');
  const [tokenSaved, setTokenSaved] = useState(false);
  const [tools, setTools] = useState<Tool[]>([]);
  const [batchSize, setBatchSize] = useState(10);

  // Gateway URL: the public MCP endpoint AI clients connect to. Stored in the
  // browser and reflected in the "Connect AI Client" config on the AI Client page.
  const [gatewayUrl, setGatewayUrl] = useState(() => localStorage.getItem('mcpgw_gateway_url') || '');
  const [gatewayUrlSaved, setGatewayUrlSaved] = useState(false);

  // Bulk reclassify state
  const [backends, setBackends] = useState<Backend[]>([]);
  const [selectedBackend, setSelectedBackend] = useState('');
  const [bulkCategory, setBulkCategory] = useState('');
  const [bulkSelections, setBulkSelections] = useState<Record<string, string>>({});
  const [bulkSaving, setBulkSaving] = useState(false);
  const [bulkSuccess, setBulkSuccess] = useState('');
  const [bulkError, setBulkError] = useState('');
  const [bulkAiRunning, setBulkAiRunning] = useState(false);
  const [bulkAiProgress, setBulkAiProgress] = useState<{ done: number; total: number } | null>(null);

  useEffect(() => {
    loadTools();
    loadBackends();
  }, []);

  const loadTools = async () => {
    try {
      const data = await api.getTools();
      setTools(data);
    } catch {}
  };

  const loadBackends = async () => {
    try {
      const data = await api.getBackends();
      setBackends(data);
    } catch {}
  };

  const backendTools = selectedBackend
    ? tools.filter(t => t.backend_name === selectedBackend)
    : [];

  const handleBackendSelect = (name: string) => {
    setSelectedBackend(name);
    setBulkCategory('');
    setBulkSelections({});
    setBulkSuccess('');
    setBulkError('');
  };

  const applyBulkCategory = () => {
    if (!bulkCategory) return;
    const updated: Record<string, string> = {};
    backendTools.forEach(t => { updated[t.tool_id] = bulkCategory; });
    setBulkSelections(updated);
  };

  const saveBulkReclassify = async () => {
    const entries = Object.entries(bulkSelections).filter(
      ([id, cat]) => {
        const tool = tools.find(t => t.tool_id === id);
        return tool && cat && cat !== (tool.risk_category || 'unclassified');
      }
    );
    if (entries.length === 0) {
      setBulkError('No changes to save');
      return;
    }
    setBulkSaving(true);
    setBulkError('');
    setBulkSuccess('');
    let saved = 0;
    for (const [id, cat] of entries) {
      try {
        await api.updateTool(id, { risk_category: cat });
        saved++;
      } catch {}
    }
    setBulkSaving(false);
    setBulkSuccess(`Updated ${saved} tool${saved !== 1 ? 's' : ''}`);
    setBulkSelections({});
    loadTools();
  };

  const classifyToolsWithAi = async (targetTools: Tool[]): Promise<{ name: string; risk: string }[]> => {
    const results: { name: string; risk: string }[] = [];
    const batches: Tool[][] = [];
    for (let i = 0; i < targetTools.length; i += batchSize) {
      batches.push(targetTools.slice(i, i + batchSize));
    }
    for (const batch of batches) {
      const toolDescriptions = batch.map(t =>
        `Tool: "${t.original_name}" (full: "${t.tool_name}")\nDescription: ${t.description || 'No description'}`
      ).join('\n\n');

      const response = await fetch('https://api.openai.com/v1/chat/completions', {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
          'Authorization': `Bearer ${apiToken}`,
        },
        body: JSON.stringify({
          model: 'gpt-5',
          messages: [
            {
              role: 'system',
              content: `You are a security classification system for MCP (Model Context Protocol) tools. Classify each tool into exactly one risk category:

- "read": Tools that only read/query data without modifications (search, list, get, fetch, query)
- "write": Tools that create or modify data (create, update, edit, write, set, post)
- "admin": Tools that manage system configuration, permissions, or infrastructure (configure, manage, deploy, admin)
- "destructive": Tools that delete, destroy, or irreversibly modify data (delete, drop, remove, truncate, purge)
- "execute": Tools that execute code, commands, or run processes (exec, run, shell, evaluate, spawn)
- "unclassified": Only if the tool truly cannot be categorized

Respond with ONLY a JSON array of objects: [{"name": "tool_name", "risk": "category"}]
No other text.`
            },
            {
              role: 'user',
              content: `Classify these tools:\n\n${toolDescriptions}`
            }
          ],
        }),
      });

      if (!response.ok) {
        const err = await response.json().catch(() => ({}));
        throw new Error(err.error?.message || `OpenAI API error: ${response.status}`);
      }

      const data = await response.json();
      const content = data.choices?.[0]?.message?.content || '[]';
      let classifications: { name: string; risk: string }[];
      try {
        const jsonMatch = content.match(/\[[\s\S]*\]/);
        classifications = jsonMatch ? JSON.parse(jsonMatch[0]) : [];
      } catch {
        classifications = [];
      }
      results.push(...classifications);
    }
    return results;
  };

  const bulkAiClassify = async () => {
    if (!apiToken.trim()) {
      setBulkError('Set your OpenAI API token in the section above first');
      return;
    }
    if (backendTools.length === 0) return;

    setBulkAiRunning(true);
    setBulkError('');
    setBulkSuccess('');
    setBulkAiProgress({ done: 0, total: backendTools.length });

    try {
      const classifications = await classifyToolsWithAi(backendTools);
      const updated: Record<string, string> = { ...bulkSelections };
      const validCategories = ['read', 'write', 'admin', 'destructive', 'execute', 'unclassified'];
      let matched = 0;

      for (const tool of backendTools) {
        const classification = classifications.find(c =>
          c.name === tool.tool_name || c.name === tool.original_name
        );
        if (classification && classification.risk && validCategories.includes(classification.risk)) {
          updated[tool.tool_id] = classification.risk;
          matched++;
        }
        setBulkAiProgress(prev => prev ? { ...prev, done: prev.done + 1 } : null);
      }

      setBulkSelections(updated);
      setBulkSuccess(`AI classified ${matched} of ${backendTools.length} tools — review and save`);
    } catch (e: any) {
      setBulkError(e.message || 'AI classification failed');
    } finally {
      setBulkAiRunning(false);
      setBulkAiProgress(null);
    }
  };

  const saveToken = () => {
    sessionStorage.setItem('mcpgw_openai_token', apiToken);
    setTokenSaved(true);
    setTimeout(() => setTokenSaved(false), 2000);
  };

  const clearToken = () => {
    setApiToken('');
    sessionStorage.removeItem('mcpgw_openai_token');
  };

  const classifyTools = async () => {
    if (!apiToken.trim()) {
      setClassifyError('Please enter your OpenAI API token first');
      return;
    }

    setClassifying(true);
    setClassifyError('');
    setClassifyResults([]);

    const targetTools = classifyMode === 'unclassified'
      ? tools.filter(t => !t.risk_category || t.risk_category === 'unclassified')
      : tools;

    if (targetTools.length === 0) {
      setClassifyError(classifyMode === 'unclassified' ? 'No unclassified tools found' : 'No tools found');
      setClassifying(false);
      return;
    }

    setClassifyProgress({ done: 0, total: targetTools.length });
    const results: { tool: string; from: string; to: string }[] = [];

    const batches: Tool[][] = [];
    for (let i = 0; i < targetTools.length; i += batchSize) {
      batches.push(targetTools.slice(i, i + batchSize));
    }

    for (const batch of batches) {
      try {
        const toolDescriptions = batch.map(t =>
          `Tool: "${t.original_name}" (full: "${t.tool_name}")\nDescription: ${t.description || 'No description'}`
        ).join('\n\n');

        const response = await fetch('https://api.openai.com/v1/chat/completions', {
          method: 'POST',
          headers: {
            'Content-Type': 'application/json',
            'Authorization': `Bearer ${apiToken}`,
          },
          body: JSON.stringify({
            model: 'gpt-5',
            messages: [
              {
                role: 'system',
                content: `You are a security classification system for MCP (Model Context Protocol) tools. Classify each tool into exactly one risk category:

- "read": Tools that only read/query data without modifications (search, list, get, fetch, query)
- "write": Tools that create or modify data (create, update, edit, write, set, post)
- "admin": Tools that manage system configuration, permissions, or infrastructure (configure, manage, deploy, admin)
- "destructive": Tools that delete, destroy, or irreversibly modify data (delete, drop, remove, truncate, purge)
- "execute": Tools that execute code, commands, or run processes (exec, run, shell, evaluate, spawn)
- "unclassified": Only if the tool truly cannot be categorized

Respond with ONLY a JSON array of objects: [{"name": "tool_name", "risk": "category"}]
No other text.`
              },
              {
                role: 'user',
                content: `Classify these tools:\n\n${toolDescriptions}`
              }
            ],
          }),
        });

        if (!response.ok) {
          const err = await response.json().catch(() => ({}));
          throw new Error(err.error?.message || `OpenAI API error: ${response.status}`);
        }

        const data = await response.json();
        const content = data.choices?.[0]?.message?.content || '[]';

        let classifications: { name: string; risk: string }[];
        try {
          const jsonMatch = content.match(/\[[\s\S]*\]/);
          classifications = jsonMatch ? JSON.parse(jsonMatch[0]) : [];
        } catch {
          classifications = [];
        }

        for (const tool of batch) {
          const classification = classifications.find(c =>
            c.name === tool.tool_name || c.name === tool.original_name
          );
          if (classification && classification.risk && classification.risk !== (tool.risk_category || 'unclassified')) {
            const validCategories = ['read', 'write', 'admin', 'destructive', 'execute', 'unclassified'];
            if (validCategories.includes(classification.risk)) {
              try {
                await api.updateTool(tool.tool_id, { risk_category: classification.risk });
                results.push({
                  tool: tool.tool_name,
                  from: tool.risk_category || 'unclassified',
                  to: classification.risk,
                });
              } catch {}
            }
          }
          setClassifyProgress(prev => prev ? { ...prev, done: prev.done + 1 } : null);
        }

        setClassifyResults([...results]);
      } catch (e: any) {
        setClassifyError(e.message || 'Classification failed');
        break;
      }
    }

    setClassifying(false);
    setClassifyProgress(null);
    loadTools();
  };

  const unclassifiedCount = tools.filter(t => !t.risk_category || t.risk_category === 'unclassified').length;

  const saveGatewayUrl = () => {
    localStorage.setItem('mcpgw_gateway_url', gatewayUrl.trim());
    setGatewayUrlSaved(true);
    setTimeout(() => setGatewayUrlSaved(false), 2000);
  };

  return (
    <div>
      <div className="mb-6">
        <h2 className="text-lg font-semibold text-white">Settings</h2>
        <p className="text-sm text-gray-500 mt-1">Application configuration and integrations</p>
      </div>

      {/* Gateway Connection Section */}
      <div className="bg-surface border border-border rounded-xl p-6 mb-6">
        <div className="flex items-start gap-4 mb-6">
          <div className="w-10 h-10 bg-accent/10 rounded-xl flex items-center justify-center shrink-0">
            <Link className="w-5 h-5 text-accent" />
          </div>
          <div>
            <h3 className="text-sm font-semibold text-white">Gateway URL</h3>
            <p className="text-xs text-gray-500 mt-1">
              The public MCP endpoint URL that AI clients connect to. Used to build the
              ready-to-paste configuration on the AI Client page.
            </p>
          </div>
        </div>

        <div>
          <label className="block text-xs font-medium text-gray-400 mb-2 uppercase tracking-wider">Public MCP Endpoint</label>
          <div className="flex gap-2">
            <div className="relative flex-1">
              <Link className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-gray-500" />
              <input
                type="text"
                value={gatewayUrl}
                onChange={e => setGatewayUrl(e.target.value)}
                placeholder="https://mcp-gateway.example.com/mcp"
                className="w-full pl-9 pr-3 py-2.5 bg-[#0a0a0f] border border-border rounded-lg text-sm text-white font-mono focus:outline-none focus:border-accent/50 transition-colors"
              />
            </div>
            <button
              onClick={saveGatewayUrl}
              className={clsx(
                'px-4 py-2 text-sm font-medium rounded-lg transition-colors',
                gatewayUrlSaved
                  ? 'bg-success/20 text-success border border-success/30'
                  : 'bg-accent hover:bg-accent-hover text-white'
              )}
            >
              {gatewayUrlSaved ? 'Saved' : 'Save'}
            </button>
          </div>
          <p className="text-[10px] text-gray-600 mt-1.5">Stored in your browser and reflected in the AI Client connection config.</p>
        </div>
      </div>

      {/* AI Risk Classification Section */}
      <div className="bg-surface border border-border rounded-xl p-6 mb-6">
        <div className="flex items-start gap-4 mb-6">
          <div className="w-10 h-10 bg-accent/10 rounded-xl flex items-center justify-center shrink-0">
            <Sparkles className="w-5 h-5 text-accent" />
          </div>
          <div>
            <h3 className="text-sm font-semibold text-white">AI-Powered Risk Classification</h3>
            <p className="text-xs text-gray-500 mt-1">
              Use OpenAI GPT-5 to automatically classify tool risk levels. This analyzes tool names and descriptions
              to assign appropriate risk categories (read, write, admin, destructive, execute).
            </p>
          </div>
        </div>

        {/* API Token */}
        <div className="mb-6">
          <label className="block text-xs font-medium text-gray-400 mb-2 uppercase tracking-wider">OpenAI API Token</label>
          <div className="flex gap-2">
            <div className="relative flex-1">
              <Key className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-gray-500" />
              <input
                type={showToken ? 'text' : 'password'}
                value={apiToken}
                onChange={e => setApiToken(e.target.value)}
                placeholder="sk-..."
                className="w-full pl-9 pr-10 py-2.5 bg-[#0a0a0f] border border-border rounded-lg text-sm text-white font-mono focus:outline-none focus:border-accent/50 transition-colors"
              />
              <button
                onClick={() => setShowToken(!showToken)}
                className="absolute right-3 top-1/2 -translate-y-1/2 text-gray-500 hover:text-gray-300"
              >
                {showToken ? <EyeOff className="w-4 h-4" /> : <Eye className="w-4 h-4" />}
              </button>
            </div>
            <button
              onClick={saveToken}
              className={clsx(
                'px-4 py-2 text-sm font-medium rounded-lg transition-colors',
                tokenSaved
                  ? 'bg-success/20 text-success border border-success/30'
                  : 'bg-accent hover:bg-accent-hover text-white'
              )}
            >
              {tokenSaved ? 'Saved' : 'Save'}
            </button>
            {apiToken && (
              <button
                onClick={clearToken}
                className="px-3 py-2 bg-surface-hover border border-border text-gray-400 text-sm rounded-lg hover:text-danger hover:border-danger/30 transition-colors"
              >
                Clear
              </button>
            )}
          </div>
          <p className="text-[10px] text-gray-600 mt-1.5">Your API key is stored locally in the browser and sent directly to OpenAI. It is never sent to the gateway server.</p>
        </div>

        {/* Classification Mode */}
        <div className="mb-6">
          <label className="block text-xs font-medium text-gray-400 mb-2 uppercase tracking-wider">Classification Scope</label>
          <div className="flex gap-3">
            <button
              onClick={() => setClassifyMode('unclassified')}
              className={clsx(
                'flex-1 p-4 rounded-xl border text-left transition-all',
                classifyMode === 'unclassified'
                  ? 'border-accent/40 bg-accent/5'
                  : 'border-border hover:border-border-hover'
              )}
            >
              <div className="flex items-center justify-between mb-2">
                <span className="text-sm font-medium text-white">Unclassified Only</span>
                <span className="text-xs text-gray-500 bg-surface-active px-2 py-0.5 rounded">{unclassifiedCount} tools</span>
              </div>
              <p className="text-xs text-gray-500">Classify only tools that currently have no risk label or are marked as "unclassified"</p>
            </button>
            <button
              onClick={() => setClassifyMode('all')}
              className={clsx(
                'flex-1 p-4 rounded-xl border text-left transition-all',
                classifyMode === 'all'
                  ? 'border-accent/40 bg-accent/5'
                  : 'border-border hover:border-border-hover'
              )}
            >
              <div className="flex items-center justify-between mb-2">
                <span className="text-sm font-medium text-white">All Tools</span>
                <span className="text-xs text-gray-500 bg-surface-active px-2 py-0.5 rounded">{tools.length} tools</span>
              </div>
              <p className="text-xs text-gray-500">Re-classify all tools, overriding any existing risk labels with AI suggestions</p>
            </button>
          </div>
        </div>

        {/* Batch Size */}
        <div className="mb-6">
          <label className="block text-xs font-medium text-gray-400 mb-2 uppercase tracking-wider">Batch Size</label>
          <p className="text-xs text-gray-500 mb-3">Number of tools to classify per API request. Larger batches are faster but may hit token limits.</p>
          <div className="flex items-center gap-3">
            {[1, 5, 10, 20, 50].map(size => (
              <button
                key={size}
                onClick={() => setBatchSize(size)}
                className={clsx(
                  'px-4 py-2 rounded-lg text-sm font-medium border transition-colors',
                  batchSize === size
                    ? 'border-accent/40 bg-accent/10 text-accent'
                    : 'border-border bg-surface text-gray-400 hover:border-border-hover hover:text-gray-300'
                )}
              >
                {size}
              </button>
            ))}
            <span className="text-xs text-gray-600 ml-2">{batchSize === 1 ? '1 tool per request (most accurate)' : `${batchSize} tools per request`}</span>
          </div>
        </div>

        {/* Classification Action */}
        <div className="flex items-center gap-3">
          <button
            onClick={classifyTools}
            disabled={classifying || !apiToken.trim()}
            className={clsx(
              'flex items-center gap-2 px-5 py-2.5 text-sm font-medium rounded-lg transition-colors',
              classifying || !apiToken.trim()
                ? 'bg-gray-700 text-gray-500 cursor-not-allowed'
                : 'bg-accent hover:bg-accent-hover text-white'
            )}
          >
            {classifying ? (
              <>
                <Loader2 className="w-4 h-4 animate-spin" />
                Classifying...
              </>
            ) : (
              <>
                <Sparkles className="w-4 h-4" />
                Run Classification
              </>
            )}
          </button>

          {classifyProgress && (
            <div className="flex items-center gap-3">
              <div className="w-48 h-2 bg-surface-active rounded-full overflow-hidden">
                <div
                  className="h-full bg-accent rounded-full transition-all duration-300"
                  style={{ width: `${(classifyProgress.done / classifyProgress.total) * 100}%` }}
                />
              </div>
              <span className="text-xs text-gray-400">{classifyProgress.done}/{classifyProgress.total}</span>
            </div>
          )}
        </div>

        {classifyError && (
          <div className="mt-4 px-4 py-3 bg-danger/10 border border-danger/20 rounded-lg flex items-center justify-between">
            <div className="flex items-center gap-2">
              <AlertTriangle className="w-4 h-4 text-danger shrink-0" />
              <p className="text-xs text-danger">{classifyError}</p>
            </div>
            <button onClick={() => setClassifyError('')} className="text-danger/60 hover:text-danger">
              <X className="w-3.5 h-3.5" />
            </button>
          </div>
        )}

        {/* Results */}
        {classifyResults.length > 0 && (
          <div className="mt-4">
            <div className="flex items-center gap-2 mb-3">
              <CheckCircle className="w-4 h-4 text-success" />
              <span className="text-sm font-medium text-success">{classifyResults.length} tools reclassified</span>
            </div>
            <div className="bg-[#0a0a0f] border border-border rounded-lg overflow-hidden max-h-60 overflow-y-auto">
              <table className="w-full">
                <thead>
                  <tr className="border-b border-border">
                    <th className="text-left px-4 py-2 text-[10px] font-medium text-gray-500 uppercase tracking-wider">Tool</th>
                    <th className="text-left px-4 py-2 text-[10px] font-medium text-gray-500 uppercase tracking-wider">From</th>
                    <th className="text-left px-4 py-2 text-[10px] font-medium text-gray-500 uppercase tracking-wider">To</th>
                  </tr>
                </thead>
                <tbody>
                  {classifyResults.map((r, i) => (
                    <tr key={i} className="border-b border-border/30">
                      <td className="px-4 py-2 text-xs text-gray-300 font-mono">{r.tool}</td>
                      <td className="px-4 py-2">
                        <span className={clsx('text-xs', RISK_COLORS[r.from] || 'text-gray-400')}>{r.from}</span>
                      </td>
                      <td className="px-4 py-2">
                        <span className={clsx('text-xs font-medium', RISK_COLORS[r.to] || 'text-gray-400')}>{r.to}</span>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          </div>
        )}
      </div>

      {/* Bulk Reclassify by Backend */}
      <div className="bg-surface border border-border rounded-xl p-6 mb-6">
        <div className="flex items-start gap-4 mb-6">
          <div className="w-10 h-10 bg-accent/10 rounded-xl flex items-center justify-center shrink-0">
            <Tag className="w-5 h-5 text-accent" />
          </div>
          <div>
            <h3 className="text-sm font-semibold text-white">Bulk Reclassify by Backend</h3>
            <p className="text-xs text-gray-500 mt-1">
              Select a backend to view its tools and manually assign or bulk-update risk categories.
            </p>
          </div>
        </div>

        {/* Backend Selector */}
        <div className="mb-5">
          <label className="block text-xs font-medium text-gray-400 mb-2 uppercase tracking-wider">Backend</label>
          <div className="relative w-72">
            <select
              value={selectedBackend}
              onChange={e => handleBackendSelect(e.target.value)}
              className="w-full appearance-none pl-3 pr-9 py-2.5 bg-[#0a0a0f] border border-border rounded-lg text-sm text-white focus:outline-none focus:border-accent/50 transition-colors cursor-pointer"
            >
              <option value="">Select a backend...</option>
              {backends.map(b => (
                <option key={b.backend_id} value={b.name}>
                  {b.name} ({b.tool_count} tools)
                </option>
              ))}
            </select>
            <ChevronDown className="absolute right-3 top-1/2 -translate-y-1/2 w-4 h-4 text-gray-500 pointer-events-none" />
          </div>
        </div>

        {selectedBackend && backendTools.length > 0 && (
          <>
            {/* Bulk Actions Row */}
            <div className="flex items-center gap-3 mb-4 flex-wrap">
              <span className="text-xs text-gray-400">Set all to:</span>
              <div className="flex gap-1.5">
                {RISK_CATEGORIES.map(cat => (
                  <button
                    key={cat}
                    onClick={() => { setBulkCategory(cat); }}
                    className={clsx(
                      'px-2.5 py-1 text-xs rounded-md border transition-colors capitalize',
                      bulkCategory === cat
                        ? RISK_BG[cat]
                        : 'border-border bg-surface text-gray-400 hover:border-border-hover'
                    )}
                  >
                    {cat}
                  </button>
                ))}
              </div>
              <button
                onClick={applyBulkCategory}
                disabled={!bulkCategory}
                className={clsx(
                  'px-3 py-1 text-xs font-medium rounded-md transition-colors',
                  bulkCategory
                    ? 'bg-accent/20 text-accent hover:bg-accent/30'
                    : 'bg-surface text-gray-600 cursor-not-allowed'
                )}
              >
                Apply
              </button>

              <div className="w-px h-5 bg-border mx-1" />

              <button
                onClick={bulkAiClassify}
                disabled={bulkAiRunning || !apiToken.trim()}
                className={clsx(
                  'flex items-center gap-1.5 px-3 py-1 text-xs font-medium rounded-md border transition-colors',
                  bulkAiRunning || !apiToken.trim()
                    ? 'border-border bg-surface text-gray-600 cursor-not-allowed'
                    : 'border-accent/30 bg-accent/10 text-accent hover:bg-accent/20'
                )}
                title={!apiToken.trim() ? 'Set your OpenAI API token above first' : 'Classify with AI'}
              >
                {bulkAiRunning ? (
                  <Loader2 className="w-3 h-3 animate-spin" />
                ) : (
                  <Sparkles className="w-3 h-3" />
                )}
                {bulkAiRunning ? 'Classifying...' : 'AI Classify'}
              </button>

              {bulkAiProgress && (
                <div className="flex items-center gap-2">
                  <div className="w-24 h-1.5 bg-surface-active rounded-full overflow-hidden">
                    <div
                      className="h-full bg-accent rounded-full transition-all duration-300"
                      style={{ width: `${(bulkAiProgress.done / bulkAiProgress.total) * 100}%` }}
                    />
                  </div>
                  <span className="text-[10px] text-gray-500">{bulkAiProgress.done}/{bulkAiProgress.total}</span>
                </div>
              )}
            </div>

            {/* Tools Table */}
            <div className="bg-[#0a0a0f] border border-border rounded-lg overflow-hidden max-h-80 overflow-y-auto mb-4">
              <table className="w-full">
                <thead className="sticky top-0 bg-[#0a0a0f]">
                  <tr className="border-b border-border">
                    <th className="text-left px-4 py-2 text-[10px] font-medium text-gray-500 uppercase tracking-wider">Tool</th>
                    <th className="text-left px-4 py-2 text-[10px] font-medium text-gray-500 uppercase tracking-wider w-36">Current</th>
                    <th className="text-left px-4 py-2 text-[10px] font-medium text-gray-500 uppercase tracking-wider w-44">New Category</th>
                  </tr>
                </thead>
                <tbody>
                  {backendTools.map(tool => {
                    const current = tool.risk_category || 'unclassified';
                    const selected = bulkSelections[tool.tool_id] || '';
                    const changed = selected && selected !== current;
                    return (
                      <tr key={tool.tool_id} className={clsx('border-b border-border/30', changed && 'bg-accent/5')}>
                        <td className="px-4 py-2">
                          <span className="text-xs text-gray-300 font-mono">{tool.original_name}</span>
                        </td>
                        <td className="px-4 py-2">
                          <span className={clsx('text-xs capitalize', RISK_COLORS[current])}>{current}</span>
                        </td>
                        <td className="px-4 py-2">
                          <div className="relative">
                            <select
                              value={selected || current}
                              onChange={e => setBulkSelections(prev => ({ ...prev, [tool.tool_id]: e.target.value }))}
                              className={clsx(
                                'appearance-none w-full pl-2 pr-7 py-1 text-xs rounded-md border bg-transparent focus:outline-none focus:border-accent/50 transition-colors cursor-pointer capitalize',
                                changed ? 'border-accent/40 text-accent' : 'border-border text-gray-400'
                              )}
                            >
                              {RISK_CATEGORIES.map(cat => (
                                <option key={cat} value={cat}>{cat}</option>
                              ))}
                            </select>
                            <ChevronDown className="absolute right-2 top-1/2 -translate-y-1/2 w-3 h-3 text-gray-600 pointer-events-none" />
                          </div>
                        </td>
                      </tr>
                    );
                  })}
                </tbody>
              </table>
            </div>

            {/* Save Button & Feedback */}
            <div className="flex items-center gap-3">
              <button
                onClick={saveBulkReclassify}
                disabled={bulkSaving || Object.keys(bulkSelections).length === 0}
                className={clsx(
                  'flex items-center gap-2 px-5 py-2.5 text-sm font-medium rounded-lg transition-colors',
                  bulkSaving || Object.keys(bulkSelections).length === 0
                    ? 'bg-gray-700 text-gray-500 cursor-not-allowed'
                    : 'bg-accent hover:bg-accent-hover text-white'
                )}
              >
                {bulkSaving ? (
                  <>
                    <Loader2 className="w-4 h-4 animate-spin" />
                    Saving...
                  </>
                ) : (
                  'Save Changes'
                )}
              </button>
              {bulkSuccess && (
                <div className="flex items-center gap-1.5 text-xs text-success">
                  <CheckCircle className="w-3.5 h-3.5" />
                  {bulkSuccess}
                </div>
              )}
              {bulkError && (
                <div className="flex items-center gap-1.5 text-xs text-danger">
                  <AlertTriangle className="w-3.5 h-3.5" />
                  {bulkError}
                </div>
              )}
            </div>
          </>
        )}

        {selectedBackend && backendTools.length === 0 && (
          <p className="text-xs text-gray-500">No tools found for this backend.</p>
        )}
      </div>

      {/* About Section */}
      <div className="bg-surface border border-border rounded-xl p-6">
        <div className="flex items-start gap-4">
          <div className="w-10 h-10 bg-surface-hover rounded-xl flex items-center justify-center shrink-0">
            <Info className="w-5 h-5 text-gray-400" />
          </div>
          <div>
            <h3 className="text-sm font-semibold text-white">About MCP Gateway</h3>
            <p className="text-xs text-gray-500 mt-1">Version 0.1.0</p>
            <p className="text-xs text-gray-500 mt-2">
              A unified MCP gateway that aggregates tools from multiple MCP backends, enforcing RBAC policies
              and providing audit logging for all tool calls made by AI agents.
            </p>
          </div>
        </div>
      </div>
    </div>
  );
}
