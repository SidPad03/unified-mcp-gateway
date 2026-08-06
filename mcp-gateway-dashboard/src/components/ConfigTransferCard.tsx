import { useRef, useState } from 'react';
import { api, ConfigBundle, ImportSummary } from '@/lib/api';
import { Download, Upload, Loader2, AlertTriangle, CheckCircle, Database } from 'lucide-react';

/** Mirrors the server's MIN_PASSPHRASE_LEN so the UI fails fast and locally. */
const MIN_PASSPHRASE = 12;
/** Typed verbatim before an import will run. Import replaces everything. */
const CONFIRM_PHRASE = 'REPLACE';

type Mode = 'idle' | 'exporting' | 'importing';

/**
 * Whole-deployment export/import.
 *
 * Owner-only, and self-hides for everyone else — the endpoints are owner-gated
 * server-side regardless, this just avoids showing a control that would 403.
 */
export default function ConfigTransferCard({ isOwner }: { isOwner: boolean }) {
  const [mode, setMode] = useState<Mode>('idle');
  const [exportPass, setExportPass] = useState('');
  const [includeAudit, setIncludeAudit] = useState(true);
  const [importPass, setImportPass] = useState('');
  const [confirm, setConfirm] = useState('');
  const [pendingBundle, setPendingBundle] = useState<ConfigBundle | null>(null);
  const [fileName, setFileName] = useState('');
  const [error, setError] = useState('');
  const [notice, setNotice] = useState('');
  const [summary, setSummary] = useState<ImportSummary | null>(null);
  const fileInput = useRef<HTMLInputElement>(null);

  if (!isOwner) return null;

  const busy = mode !== 'idle';

  const handleExport = async () => {
    setError('');
    setNotice('');
    if (exportPass.length < MIN_PASSPHRASE) {
      setError(`Passphrase must be at least ${MIN_PASSPHRASE} characters.`);
      return;
    }
    setMode('exporting');
    try {
      const bundle = await api.exportConfig(exportPass, includeAudit);
      // Hand the file straight to the browser; it never touches disk server-side.
      const blob = new Blob([JSON.stringify(bundle, null, 2)], { type: 'application/json' });
      const url = URL.createObjectURL(blob);
      const a = document.createElement('a');
      a.href = url;
      a.download = `mcp-gateway-export-${new Date().toISOString().slice(0, 10)}.mcpgw.json`;
      a.click();
      URL.revokeObjectURL(url);
      setExportPass('');
      setNotice('Export downloaded. Store it somewhere safe — it contains credentials.');
    } catch (e: any) {
      setError(e.message || 'Export failed');
    } finally {
      setMode('idle');
    }
  };

  const handleFile = async (file: File) => {
    setError('');
    setNotice('');
    setSummary(null);
    try {
      const parsed = JSON.parse(await file.text());
      if (parsed?.format !== 'mcp-gateway-config-export') {
        setError('That file is not an MCP Gateway configuration bundle.');
        return;
      }
      setPendingBundle(parsed);
      setFileName(file.name);
    } catch {
      setError('That file is not valid JSON.');
    }
  };

  const handleImport = async () => {
    if (!pendingBundle) return;
    setError('');
    setNotice('');
    if (confirm !== CONFIRM_PHRASE) {
      setError(`Type ${CONFIRM_PHRASE} to confirm.`);
      return;
    }
    setMode('importing');
    try {
      const result = await api.importConfig(importPass, pendingBundle);
      setSummary(result);
      setPendingBundle(null);
      setImportPass('');
      setConfirm('');
      setFileName('');
      if (fileInput.current) fileInput.current.value = '';
      setNotice('Import complete. Sign out and back in — your session belongs to the replaced data.');
    } catch (e: any) {
      setError(e.message || 'Import failed');
    } finally {
      setMode('idle');
    }
  };

  return (
    <div className="bg-surface border border-border rounded-xl p-6">
      <div className="flex items-start gap-4 mb-5">
        <div className="w-10 h-10 bg-surface-hover rounded-xl flex items-center justify-center shrink-0">
          <Database className="w-5 h-5 text-gray-400" />
        </div>
        <div>
          <h3 className="text-sm font-semibold text-white">Configuration transfer</h3>
          <p className="text-xs text-gray-500 mt-1">
            Move a whole deployment — users, roles, policies, backends, tools, API keys, and
            audit history — to another install.
          </p>
        </div>
      </div>

      {error && (
        <div className="mb-4 px-3 py-2 bg-danger/10 border border-danger/20 rounded-lg flex items-start gap-2">
          <AlertTriangle className="w-3.5 h-3.5 text-danger mt-0.5 shrink-0" />
          <p className="text-xs text-danger">{error}</p>
        </div>
      )}
      {notice && (
        <div className="mb-4 px-3 py-2 bg-accent/10 border border-accent/20 rounded-lg flex items-start gap-2">
          <CheckCircle className="w-3.5 h-3.5 text-accent mt-0.5 shrink-0" />
          <p className="text-xs text-accent">{notice}</p>
        </div>
      )}

      <div className="grid gap-6 md:grid-cols-2">
        {/* ── Export ── */}
        <div className="space-y-3">
          <h4 className="text-xs font-medium text-gray-400 uppercase tracking-wider">Export</h4>
          <p className="text-[11px] text-gray-500 leading-relaxed">
            The bundle is encrypted with a passphrase you choose. Without that passphrase it
            cannot be imported — there is no recovery, so store it with the file.
          </p>
          <input
            type="password"
            value={exportPass}
            onChange={e => setExportPass(e.target.value)}
            placeholder={`Encryption passphrase (min ${MIN_PASSPHRASE} chars)`}
            autoComplete="new-password"
            disabled={busy}
            className="w-full px-2.5 py-1.5 bg-[#0a0a0f] border border-border rounded-lg text-xs text-white focus:outline-none focus:border-accent/50 disabled:opacity-50"
          />
          <label className="flex items-center gap-2 text-xs text-gray-400">
            <input
              type="checkbox"
              checked={includeAudit}
              onChange={e => setIncludeAudit(e.target.checked)}
              disabled={busy}
              className="accent-accent"
            />
            Include audit history
            <span className="text-gray-600">(largest part of the bundle)</span>
          </label>
          <button
            onClick={handleExport}
            disabled={busy}
            className="w-full flex items-center justify-center gap-2 px-3 py-2 bg-accent/15 hover:bg-accent/25 border border-accent/20 rounded-lg text-xs text-accent transition-colors disabled:opacity-50"
          >
            {mode === 'exporting' ? (
              <><Loader2 className="w-3.5 h-3.5 animate-spin" /> Preparing…</>
            ) : (
              <><Download className="w-3.5 h-3.5" /> Export configuration</>
            )}
          </button>
        </div>

        {/* ── Import ── */}
        <div className="space-y-3">
          <h4 className="text-xs font-medium text-gray-400 uppercase tracking-wider">Import</h4>
          <div className="px-3 py-2 bg-danger/5 border border-danger/20 rounded-lg">
            <p className="text-[11px] text-danger leading-relaxed">
              <strong>Replaces everything.</strong> Every user, key, backend, policy, and audit
              event on this deployment is deleted first, so the result matches the bundle exactly.
              It runs in one transaction — if it fails, nothing changes.
            </p>
          </div>
          <input
            ref={fileInput}
            type="file"
            accept=".json,.mcpgw,application/json"
            disabled={busy}
            onChange={e => e.target.files?.[0] && handleFile(e.target.files[0])}
            className="w-full text-xs text-gray-400 file:mr-2 file:px-2.5 file:py-1.5 file:rounded-lg file:border-0 file:bg-surface-hover file:text-xs file:text-gray-300 hover:file:bg-border disabled:opacity-50"
          />
          {pendingBundle && (
            <div className="px-3 py-2 bg-surface-hover rounded-lg space-y-0.5">
              <p className="text-[11px] text-gray-300 font-mono truncate">{fileName}</p>
              <p className="text-[11px] text-gray-500">
                From v{pendingBundle.source_version} ·{' '}
                {new Date(pendingBundle.created_at).toLocaleString()} ·{' '}
                {pendingBundle.includes_audit ? 'with' : 'without'} audit history
              </p>
            </div>
          )}
          <input
            type="password"
            value={importPass}
            onChange={e => setImportPass(e.target.value)}
            placeholder="Bundle passphrase"
            autoComplete="off"
            disabled={busy || !pendingBundle}
            className="w-full px-2.5 py-1.5 bg-[#0a0a0f] border border-border rounded-lg text-xs text-white focus:outline-none focus:border-accent/50 disabled:opacity-50"
          />
          <input
            type="text"
            value={confirm}
            onChange={e => setConfirm(e.target.value)}
            placeholder={`Type ${CONFIRM_PHRASE} to confirm`}
            autoComplete="off"
            disabled={busy || !pendingBundle}
            className="w-full px-2.5 py-1.5 bg-[#0a0a0f] border border-border rounded-lg text-xs text-white font-mono focus:outline-none focus:border-danger/50 disabled:opacity-50"
          />
          <button
            onClick={handleImport}
            disabled={busy || !pendingBundle || confirm !== CONFIRM_PHRASE}
            className="w-full flex items-center justify-center gap-2 px-3 py-2 bg-danger/15 hover:bg-danger/25 border border-danger/20 rounded-lg text-xs text-danger transition-colors disabled:opacity-40"
          >
            {mode === 'importing' ? (
              <><Loader2 className="w-3.5 h-3.5 animate-spin" /> Importing…</>
            ) : (
              <><Upload className="w-3.5 h-3.5" /> Replace this deployment</>
            )}
          </button>
        </div>
      </div>

      {summary && (
        <div className="mt-5 pt-4 border-t border-border">
          <p className="text-xs text-gray-400 mb-2">
            Restored from v{summary.source_version}:
          </p>
          <div className="flex flex-wrap gap-x-4 gap-y-1">
            {summary.imported.map(t => (
              <span key={t.table} className="text-[11px] text-gray-500">
                <span className="text-gray-300 tabular-nums">{t.rows}</span> {t.table}
              </span>
            ))}
          </div>
        </div>
      )}
    </div>
  );
}
