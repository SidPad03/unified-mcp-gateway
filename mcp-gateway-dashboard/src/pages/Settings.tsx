import { useState, useEffect } from 'react';
import { api, Tool } from '@/lib/api';
import { useUpdateCheck } from '@/hooks/useUpdateCheck';
import { Sparkles, Key, Eye, EyeOff, Loader2, CheckCircle, AlertTriangle, X, Info, Link, RefreshCw } from 'lucide-react';
import clsx from 'clsx';
import { PageHeader } from '@/components/ui';

const RISK_COLORS: Record<string, string> = {
  read: 'text-beam',
  write: 'text-ink-2',
  admin: 'text-warn',
  destructive: 'text-deny',
  execute: 'text-ink-2',
  unclassified: 'text-ink-2',
};

const RISK_BG: Record<string, string> = {
  read: 'bg-beam-wash border-beam-edge',
  write: 'bg-neutral-wash border-line',
  admin: 'bg-warn-wash border-warn-edge',
  destructive: 'bg-deny-wash border-deny-edge',
  execute: 'bg-neutral-wash border-line',
  unclassified: 'bg-neutral-wash border-line',
};

const RISK_CATEGORIES = ['read', 'write', 'admin', 'destructive', 'execute', 'unclassified'];

/** Owner check from the cached session, matching useAuth's `isAdmin`. */
function useIsOwner(): boolean {
  try {
    const raw = localStorage.getItem('mcpgw_user');
    return raw ? (JSON.parse(raw).roles ?? []).includes('owner') : false;
  } catch {
    return false;
  }
}

export default function Settings() {
  const isOwner = useIsOwner();
  // Shared with the sidebar's passive check, so pressing this button also
  // settles the footer notice instead of leaving the two disagreeing. It forces
  // past the server's 30-minute cache, which is what someone who just published
  // a release expects from pressing it.
  const {
    status: updateStatus,
    checking: updateChecking,
    refresh: runUpdateCheck,
  } = useUpdateCheck();

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

  useEffect(() => {
    loadTools();
  }, []);

  const loadTools = async () => {
    try {
      const data = await api.getTools();
      setTools(data);
    } catch {}
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
      setClassifyError('Enter your OpenAI API token first');
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
      <PageHeader
        title="Settings"
        description="How this gateway is reached, which AI clients are wired to it, and where its configuration lives."
      />

      {/* Gateway Connection Section */}
      <div className="bg-panel border border-line rounded-card p-6 mb-6">
        <div className="flex items-start gap-4 mb-6">
          <div className="w-10 h-10 bg-beam-wash rounded-card flex items-center justify-center shrink-0">
            <Link className="w-5 h-5 text-beam" />
          </div>
          <div>
            <h3 className="text-sm font-semibold text-ink">Gateway URL</h3>
            <p className="text-xs text-ink-3 mt-1">
              The public MCP endpoint URL that AI clients connect to. Used to build the
              ready-to-paste configuration on the Backends page.
            </p>
          </div>
        </div>

        <div>
          <label className="block text-xs font-medium text-ink-2 mb-2 uppercase tracking-wider">Public MCP endpoint</label>
          <div className="flex gap-2">
            <div className="relative flex-1">
              <Link className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-ink-3" />
              <input
                type="text"
                value={gatewayUrl}
                onChange={e => setGatewayUrl(e.target.value)}
                placeholder="https://mcp-gateway.example.com/mcp"
                className="w-full pl-9 pr-3 py-2.5 bg-inset border border-line rounded-row text-sm text-ink font-mono focus:outline-none focus:border-beam-edge transition-colors"
              />
            </div>
            <button
              onClick={saveGatewayUrl}
              className={clsx(
                'px-4 py-2 text-sm font-medium rounded-row transition-colors',
                gatewayUrlSaved
                  ? 'bg-beam-wash text-beam border border-beam-edge/30'
                  : 'bg-solid hover:bg-solid-hover text-on-solid'
              )}
            >
              {gatewayUrlSaved ? 'Saved' : 'Save'}
            </button>
          </div>
          <p className="text-micro text-ink-4 mt-1.5">Stored in your browser and used to build the client configuration.</p>
        </div>
      </div>

      {/* AI Risk Classification Section */}
      <div className="bg-panel border border-line rounded-card p-6 mb-6">
        <div className="flex items-start gap-4 mb-6">
          <div className="w-10 h-10 bg-beam-wash rounded-card flex items-center justify-center shrink-0">
            <Sparkles className="w-5 h-5 text-beam" />
          </div>
          <div>
            <h3 className="text-sm font-semibold text-ink">AI risk classification</h3>
            <p className="text-xs text-ink-3 mt-1">
              Use OpenAI GPT-5 to classify tool risk levels from their names and descriptions
              (read, write, admin, destructive, execute).
            </p>
          </div>
        </div>

        {/* API Token */}
        <div className="mb-6">
          <label className="block text-xs font-medium text-ink-2 mb-2 uppercase tracking-wider">OpenAI API token</label>
          <div className="flex gap-2">
            <div className="relative flex-1">
              <Key className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-ink-3" />
              <input
                type={showToken ? 'text' : 'password'}
                value={apiToken}
                onChange={e => setApiToken(e.target.value)}
                placeholder="sk-..."
                className="w-full pl-9 pr-10 py-2.5 bg-inset border border-line rounded-row text-sm text-ink font-mono focus:outline-none focus:border-beam-edge transition-colors"
              />
              <button
                onClick={() => setShowToken(!showToken)}
                className="absolute right-3 top-1/2 -translate-y-1/2 text-ink-3 hover:text-ink-2"
              >
                {showToken ? <EyeOff className="w-4 h-4" /> : <Eye className="w-4 h-4" />}
              </button>
            </div>
            <button
              onClick={saveToken}
              className={clsx(
                'px-4 py-2 text-sm font-medium rounded-row transition-colors',
                tokenSaved
                  ? 'bg-beam-wash text-beam border border-beam-edge/30'
                  : 'bg-solid hover:bg-solid-hover text-on-solid'
              )}
            >
              {tokenSaved ? 'Saved' : 'Save'}
            </button>
            {apiToken && (
              <button
                onClick={clearToken}
                className="px-3 py-2 bg-raised border border-line text-ink-2 text-sm rounded-row hover:text-deny hover:border-deny-edge/30 transition-colors"
              >
                Clear
              </button>
            )}
          </div>
          <p className="text-micro text-ink-4 mt-1.5">Your API key is stored locally in the browser and sent directly to OpenAI. It is never sent to the gateway server.</p>
        </div>

        {/* Classification Mode */}
        <div className="mb-6">
          <label className="block text-xs font-medium text-ink-2 mb-2 uppercase tracking-wider">Classification scope</label>
          <div className="flex flex-col sm:flex-row gap-3">
            <button
              onClick={() => setClassifyMode('unclassified')}
              className={clsx(
                'flex-1 p-4 rounded-card border text-left transition-all',
                classifyMode === 'unclassified'
                  ? 'border-beam-edge bg-beam-wash'
                  : 'border-line hover:border-line-strong'
              )}
            >
              <div className="flex items-center justify-between mb-2">
                <span className="text-sm font-medium text-ink">Unclassified only</span>
                <span className="text-xs text-ink-3 bg-high px-2 py-0.5 rounded-control whitespace-nowrap shrink-0">{unclassifiedCount} tools</span>
              </div>
              <p className="text-xs text-ink-3">Classify only tools that have no risk label or are marked as "unclassified".</p>
            </button>
            <button
              onClick={() => setClassifyMode('all')}
              className={clsx(
                'flex-1 p-4 rounded-card border text-left transition-all',
                classifyMode === 'all'
                  ? 'border-beam-edge bg-beam-wash'
                  : 'border-line hover:border-line-strong'
              )}
            >
              <div className="flex items-center justify-between mb-2">
                <span className="text-sm font-medium text-ink">All tools</span>
                <span className="text-xs text-ink-3 bg-high px-2 py-0.5 rounded-control whitespace-nowrap shrink-0">{tools.length} tools</span>
              </div>
              <p className="text-xs text-ink-3">Re-classify all tools, overriding any existing risk labels with AI suggestions.</p>
            </button>
          </div>
        </div>

        {/* Batch Size */}
        <div className="mb-6">
          <label className="block text-xs font-medium text-ink-2 mb-2 uppercase tracking-wider">Batch size</label>
          <p className="text-xs text-ink-3 mb-3">Number of tools to classify per API request. Larger batches are faster but may hit token limits.</p>
          <div className="flex items-center gap-3">
            {[1, 5, 10, 20, 50].map(size => (
              <button
                key={size}
                onClick={() => setBatchSize(size)}
                className={clsx(
                  'px-4 py-2 rounded-row text-sm font-medium border transition-colors',
                  batchSize === size
                    ? 'border-beam-edge bg-beam-wash text-beam'
                    : 'border-line bg-panel text-ink-2 hover:border-line-strong hover:text-ink-2'
                )}
              >
                {size}
              </button>
            ))}
            <span className="text-xs text-ink-4 ml-2">{batchSize === 1 ? '1 tool per request (most accurate)' : `${batchSize} tools per request`}</span>
          </div>
        </div>

        {/* Classification Action */}
        <div className="flex items-center gap-3">
          <button
            onClick={classifyTools}
            disabled={classifying || !apiToken.trim()}
            className={clsx(
              'flex items-center gap-2 px-5 py-2.5 text-sm font-medium rounded-row transition-colors',
              classifying || !apiToken.trim()
                ? 'bg-neutral-wash text-ink-3 cursor-not-allowed'
                : 'bg-solid hover:bg-solid-hover text-on-solid'
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
                Run classification
              </>
            )}
          </button>

          {classifyProgress && (
            <div className="flex items-center gap-3">
              <div className="w-48 h-2 bg-high rounded-full overflow-hidden">
                <div
                  className="h-full bg-solid rounded-full transition-all duration-300"
                  style={{ width: `${(classifyProgress.done / classifyProgress.total) * 100}%` }}
                />
              </div>
              <span className="text-xs text-ink-2">{classifyProgress.done}/{classifyProgress.total}</span>
            </div>
          )}
        </div>

        {classifyError && (
          <div className="mt-4 px-4 py-3 bg-deny-wash border border-deny-edge rounded-row flex items-center justify-between">
            <div className="flex items-center gap-2">
              <AlertTriangle className="w-4 h-4 text-deny shrink-0" />
              <p className="text-xs text-deny">{classifyError}</p>
            </div>
            <button onClick={() => setClassifyError('')} className="text-deny/60 hover:text-deny">
              <X className="w-3.5 h-3.5" />
            </button>
          </div>
        )}

        {/* Results */}
        {classifyResults.length > 0 && (
          <div className="mt-4">
            <div className="flex items-center gap-2 mb-3">
              <CheckCircle className="w-4 h-4 text-beam" />
              <span className="text-sm font-medium text-beam">{classifyResults.length} tools reclassified</span>
            </div>
            <div className="bg-inset border border-line rounded-row overflow-hidden max-h-60 overflow-y-auto">
              <table className="w-full">
                <thead>
                  <tr className="border-b border-line">
                    <th className="text-left px-4 py-2 text-micro font-medium text-ink-3 uppercase tracking-wider">Tool</th>
                    <th className="text-left px-4 py-2 text-micro font-medium text-ink-3 uppercase tracking-wider">From</th>
                    <th className="text-left px-4 py-2 text-micro font-medium text-ink-3 uppercase tracking-wider">To</th>
                  </tr>
                </thead>
                <tbody>
                  {classifyResults.map((r, i) => (
                    <tr key={i} className="border-b border-line-soft">
                      <td className="px-4 py-2 text-xs text-ink-2 font-mono">{r.tool}</td>
                      <td className="px-4 py-2">
                        <span className={clsx('text-xs', RISK_COLORS[r.from] || 'text-ink-2')}>{r.from}</span>
                      </td>
                      <td className="px-4 py-2">
                        <span className={clsx('text-xs font-medium', RISK_COLORS[r.to] || 'text-ink-2')}>{r.to}</span>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          </div>
        )}
      </div>

      {/* About Section */}
      <div className="bg-panel border border-line rounded-card p-6">
        <div className="flex items-start gap-4">
          <div className="w-10 h-10 bg-raised rounded-card flex items-center justify-center shrink-0">
            <Info className="w-5 h-5 text-ink-2" />
          </div>
          <div className="flex-1 min-w-0">
            <h3 className="text-sm font-semibold text-ink">About MCP Gateway</h3>
            {/* Same build-time define the sidebar uses (vite.config.ts), fed by
                the APP_VERSION build-arg in CI. Hardcoding it here meant /settings
                reported 0.1.0 forever while the app was on a much later release. */}
            <p className="text-xs text-ink-3 mt-1">Version {__APP_VERSION__}</p>
            <p className="text-xs text-ink-3 mt-2">
              A unified MCP gateway that aggregates tools from multiple MCP backends, enforcing RBAC policies
              and providing audit logging for all tool calls made by AI agents.
            </p>

            <div className="mt-4 flex items-center gap-3 flex-wrap">
              <button
                onClick={runUpdateCheck}
                disabled={updateChecking}
                className="flex items-center gap-1.5 px-2.5 py-1.5 bg-raised hover:bg-line-strong rounded-row text-xs text-ink-2 transition-colors disabled:opacity-50"
              >
                {updateChecking
                  ? <><Loader2 className="w-3.5 h-3.5 animate-spin" /> Checking...</>
                  : <><RefreshCw className="w-3.5 h-3.5" /> Check for updates</>}
              </button>

              {/* Three outcomes worth distinguishing: an update exists, you are
                  current, or the check itself failed. Collapsing the third into
                  "up to date" would quietly hide a stale deployment. */}
              {updateStatus?.error && (
                <span className="text-xs text-warn">{updateStatus.error}</span>
              )}
              {updateStatus && !updateStatus.error && updateStatus.update_available && (
                <a
                  href={updateStatus.release_url ?? '#'}
                  target="_blank"
                  rel="noopener noreferrer"
                  className="text-xs text-beam hover:underline"
                >
                  v{updateStatus.latest_version} is available →
                </a>
              )}
              {updateStatus && !updateStatus.error && !updateStatus.update_available && (
                <span className="text-xs text-beam">
                  Up to date{updateStatus.latest_version ? ` (latest: v${updateStatus.latest_version})` : ''}
                </span>
              )}
            </div>

            {updateStatus?.update_available && updateStatus.release_notes && (
              <details className="mt-3">
                <summary className="text-xs text-ink-3 cursor-pointer hover:text-ink-2">
                  Release notes for {updateStatus.release_name || `v${updateStatus.latest_version}`}
                </summary>
                <pre className="mt-2 p-3 bg-inset border border-line rounded-row text-2xs text-ink-2 whitespace-pre-wrap max-h-56 overflow-y-auto">
                  {updateStatus.release_notes}
                </pre>
              </details>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}
