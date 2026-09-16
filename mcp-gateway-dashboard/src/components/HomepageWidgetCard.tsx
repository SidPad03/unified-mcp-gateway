import { useEffect, useMemo, useState } from 'react';
import { LayoutDashboard, Copy, Check, KeyRound, RotateCw, Trash2 } from 'lucide-react';
import clsx from 'clsx';
import { api, GatewayStats, StatsTokenStatus } from '@/lib/api';
import { fmt } from '@/lib/format';
import { Banner, Button, ConfirmModal, Field, Input, MiniStat } from '@/components/ui';

/**
 * A Homepage (gethomepage.dev) tile for this gateway: a read-only token and the
 * `services.yaml` entry that uses it.
 *
 * The token is the point of the card. The key an operator would otherwise
 * paste into a dashboard's YAML is their own `mcpgw_` key, which calls every
 * tool behind the gateway; this one reads `/api/v1/stats` and is refused
 * everywhere else. It is shown once — the server keeps only a hash — so the
 * snippet below is built with it filled in while it is still on screen.
 */

interface Mapping {
  field: keyof GatewayStats;
  label: string;
  format: 'number' | 'float' | 'percent' | 'relativeDate';
  /** Homepage's `scale` and `suffix` transformations. */
  scale?: number;
  suffix?: string;
}

/** Every field worth a tile, in the order they are offered. */
const MAPPINGS: Mapping[] = [
  { field: 'tools', label: 'Tools', format: 'number' },
  { field: 'backends', label: 'Backends', format: 'number' },
  { field: 'calls_24h', label: 'Calls (24h)', format: 'number' },
  { field: 'backends_healthy', label: 'Healthy', format: 'number' },
  { field: 'backends_unhealthy', label: 'Unhealthy', format: 'number' },
  { field: 'errors_24h', label: 'Errors (24h)', format: 'number' },
  { field: 'denied_24h', label: 'Denied (24h)', format: 'number' },
  // A fraction on the wire; Homepage's `percent` expects a percentage.
  { field: 'error_rate_24h', label: 'Error rate', format: 'percent', scale: 100 },
  { field: 'avg_latency_ms_24h', label: 'Latency', format: 'float', suffix: ' ms' },
  { field: 'agents_connected', label: 'Macs', format: 'number' },
  { field: 'last_call_at', label: 'Last call', format: 'relativeDate' },
];

/** What was asked for: tools, backends, and calls in the last day. */
const DEFAULT_FIELDS: Mapping['field'][] = ['tools', 'backends', 'calls_24h'];

const TOKEN_PLACEHOLDER = '<your stats token>';

/** Homepage's `customapi` block view shows `mappings.slice(0, 4)`. */
const BLOCK_VIEW_FIELDS = 4;

/** The brand mark, from this repository, so the tile matches the dashboard. */
const ICON_URL =
  'https://raw.githubusercontent.com/SidPad03/unified-mcp-gateway/main/mcp-gateway.svg';

function buildServicesYaml({
  dashboardUrl,
  apiUrl,
  token,
  fields,
}: {
  dashboardUrl: string;
  apiUrl: string;
  token: string;
  fields: Mapping['field'][];
}): string {
  const base = apiUrl.trim().replace(/\/+$/, '') || dashboardUrl;
  const mappings = MAPPINGS.filter(m => fields.includes(m.field)).flatMap(m => [
    `        - field: ${m.field}`,
    `          label: ${m.label}`,
    `          format: ${m.format}`,
    ...(m.scale != null ? [`          scale: ${m.scale}`] : []),
    ...(m.suffix != null ? [`          suffix: "${m.suffix}"`] : []),
  ]);
  return [
    '# services.yaml — under one of your groups',
    '- MCP Gateway:',
    `    href: ${dashboardUrl}`,
    '    description: MCP tools behind the gateway',
    `    icon: ${ICON_URL}`,
    '    widget:',
    '      type: customapi',
    `      url: ${base}/api/v1/stats`,
    '      refreshInterval: 60000',
    // The default block view renders the first four mappings and drops the
    // rest without a word; the list view renders every one of them.
    ...(fields.length > BLOCK_VIEW_FIELDS ? ['      display: list'] : []),
    '      headers:',
    `        Authorization: Bearer ${token}`,
    '      mappings:',
    ...mappings,
    '',
  ].join('\n');
}

/** A figure as the widget will show it, so the preview and the tile agree. */
function preview(m: Mapping, stats: GatewayStats): string {
  const value = stats[m.field];
  if (value == null) return '—';
  switch (m.format) {
    case 'percent':
      return fmt.percent(value as number);
    case 'float':
      return `${(value as number).toFixed(1)}${m.suffix ?? ''}`;
    case 'relativeDate':
      return fmt.relative(value as string);
    default:
      return fmt.count(value as number);
  }
}

async function copyText(text: string) {
  try {
    await navigator.clipboard.writeText(text);
  } catch {
    // Plain-http deployments have no async clipboard.
    const area = document.createElement('textarea');
    area.value = text;
    document.body.appendChild(area);
    area.select();
    document.execCommand('copy');
    document.body.removeChild(area);
  }
}

function CopyButton({ text, label = 'Copy' }: { text: string; label?: string }) {
  const [copied, setCopied] = useState(false);
  return (
    <Button
      size="sm"
      icon={copied ? Check : Copy}
      onClick={async () => {
        await copyText(text);
        setCopied(true);
        setTimeout(() => setCopied(false), 2000);
      }}
      className={clsx(copied && 'text-beam border-beam-edge')}
    >
      {copied ? 'Copied' : label}
    </Button>
  );
}

export default function HomepageWidgetCard() {
  const [status, setStatus] = useState<StatsTokenStatus | null>(null);
  const [stats, setStats] = useState<GatewayStats | null>(null);
  const [error, setError] = useState('');
  /** The token status could not be read — offer a retry, not a spinner forever. */
  const [loadFailed, setLoadFailed] = useState(false);
  const [busy, setBusy] = useState(false);
  /** Only for as long as this page is open. It is never stored anywhere. */
  const [freshToken, setFreshToken] = useState<string | null>(null);
  const [confirm, setConfirm] = useState<'regenerate' | 'revoke' | null>(null);
  const [fields, setFields] = useState<Mapping['field'][]>(DEFAULT_FIELDS);
  // Homepage fetches the widget from its own server, which often reaches the
  // gateway by a different name than your browser does — a container name, a
  // LAN address. The link on the tile is for the browser.
  const [apiUrl, setApiUrl] = useState(() => window.location.origin);

  const load = () => {
    setLoadFailed(false);
    api
      .getStatsToken()
      .then(setStatus)
      .catch(() => setLoadFailed(true));
    api.getStats().then(setStats).catch(() => {
      /* the preview is a nicety; the token controls still work */
    });
  };

  useEffect(load, []);

  const yaml = useMemo(
    () =>
      buildServicesYaml({
        dashboardUrl: window.location.origin,
        apiUrl,
        token: freshToken ?? TOKEN_PLACEHOLDER,
        fields,
      }),
    [apiUrl, freshToken, fields]
  );

  const issue = async () => {
    setBusy(true);
    setError('');
    try {
      const created = await api.createStatsToken();
      setFreshToken(created.token);
      setStatus({
        configured: true,
        prefix: created.prefix,
        created_at: created.created_at,
        created_by: JSON.parse(localStorage.getItem('mcpgw_user') || '{}').username,
        last_used_at: null,
      });
    } catch (e: any) {
      setError(e.message || 'Could not create a token');
    } finally {
      setBusy(false);
      setConfirm(null);
    }
  };

  const revoke = async () => {
    setBusy(true);
    setError('');
    try {
      await api.revokeStatsToken();
      setFreshToken(null);
      setStatus({ configured: false });
    } catch (e: any) {
      setError(e.message || 'Could not revoke the token');
    } finally {
      setBusy(false);
      setConfirm(null);
    }
  };

  const toggleField = (field: Mapping['field']) =>
    setFields(current =>
      current.includes(field) ? current.filter(f => f !== field) : [...current, field]
    );

  return (
    <div className="bg-panel border border-line rounded-card p-6 mb-6">
      <div className="flex items-start gap-4 mb-5">
        <div className="w-10 h-10 bg-beam-wash rounded-card flex items-center justify-center shrink-0">
          <LayoutDashboard className="w-5 h-5 text-beam" />
        </div>
        <div className="min-w-0">
          <h3 className="text-sm font-semibold text-ink">Homepage widget</h3>
          <p className="text-xs text-ink-3 mt-1">
            Show this gateway's tools, backends and calls on a{' '}
            <a
              href="https://gethomepage.dev/widgets/services/customapi/"
              target="_blank"
              rel="noopener noreferrer"
              className="text-beam hover:underline"
            >
              Homepage
            </a>{' '}
            dashboard, or anything else that reads JSON. The widget gets its own token, which
            reads these figures and is refused by every other endpoint — not an API key that
            could call your tools.
          </p>
        </div>
      </div>

      {/* The selected fields, as the tile will show them. */}
      {stats && fields.length > 0 && (
        <div className="flex items-end gap-7 flex-wrap mb-5 px-4 py-3 bg-inset border border-line rounded-row">
          {MAPPINGS.filter(m => fields.includes(m.field)).map(m => (
            <MiniStat key={m.field} label={m.label} value={preview(m, stats)} />
          ))}
          <span className="text-micro text-ink-4 ml-auto self-center">
            What the widget will show now
          </span>
        </div>
      )}

      {error && (
        <Banner tone="deny" onDismiss={() => setError('')} className="mb-4">
          {error}
        </Banner>
      )}

      {/* Token */}
      <div className="flex items-center justify-between gap-4 flex-wrap p-4 rounded-card border border-line">
        <div className="min-w-0">
          <div className="flex items-center gap-2">
            <KeyRound className="w-3.5 h-3.5 text-ink-3 shrink-0" />
            <span className="text-sm font-medium text-ink">Stats token</span>
          </div>
          <p className="text-xs text-ink-3 mt-1">
            {status == null
              ? loadFailed
                ? 'Could not read the token status.'
                : 'Loading…'
              : status.configured
                ? <>
                    <span className="font-mono text-ink-2">{status.prefix}…</span>
                    {status.created_at && <> · created {fmt.relative(status.created_at)}</>}
                    {status.created_by && <> by {status.created_by}</>}
                    {' · '}
                    {status.last_used_at ? `last read ${fmt.relative(status.last_used_at)}` : 'not used yet'}
                  </>
                : 'None yet. Create one to set up the widget.'}
          </p>
        </div>
        {status?.configured ? (
          <div className="flex items-center gap-2">
            <Button icon={RotateCw} onClick={() => setConfirm('regenerate')} disabled={busy}>
              Regenerate
            </Button>
            <Button icon={Trash2} variant="danger" onClick={() => setConfirm('revoke')} disabled={busy}>
              Revoke
            </Button>
          </div>
        ) : status == null && loadFailed ? (
          <Button icon={RotateCw} onClick={load}>
            Retry
          </Button>
        ) : (
          <Button variant="primary" icon={KeyRound} onClick={issue} loading={busy} disabled={status == null}>
            Create token
          </Button>
        )}
      </div>

      {freshToken && (
        <div className="mt-3 p-4 rounded-card border border-beam-edge bg-beam-wash">
          <p className="text-xs text-ink-2 mb-2">
            Copy it now — the gateway keeps only a hash, so this is the one time it can be shown.
          </p>
          <div className="flex items-center gap-2">
            <code className="flex-1 min-w-0 truncate px-3 py-2 bg-inset border border-line rounded-row text-xs font-mono text-ink">
              {freshToken}
            </code>
            <CopyButton text={freshToken} />
          </div>
        </div>
      )}

      {/* Snippet */}
      <div className="mt-5 grid gap-4">
        <Field
          label="Address Homepage uses to reach the gateway"
          hint="Homepage requests the widget from its own server. If it runs in the same Docker network, that is often the dashboard container's name rather than this page's address."
        >
          <Input
            value={apiUrl}
            onChange={e => setApiUrl(e.target.value)}
            placeholder="http://mcp-gateway-dashboard"
            className="w-full h-9 px-2.5 text-xs font-mono"
            spellCheck={false}
          />
        </Field>

        <div>
          <div className="text-micro font-semibold uppercase tracking-[0.14em] text-ink-4 mb-2">Fields</div>
          <div className="flex flex-wrap gap-1.5">
            {MAPPINGS.map(m => {
              const on = fields.includes(m.field);
              return (
                <button
                  key={m.field}
                  type="button"
                  aria-pressed={on}
                  onClick={() => toggleField(m.field)}
                  className={clsx(
                    'px-2.5 py-1 rounded-control border text-2xs transition-colors',
                    on
                      ? 'border-beam-edge bg-beam-wash text-beam'
                      : 'border-line text-ink-3 hover:text-ink-2 hover:border-line-strong'
                  )}
                >
                  {m.label}
                </button>
              );
            })}
          </div>
          {fields.length > BLOCK_VIEW_FIELDS && (
            <p className="text-2xs text-ink-3 mt-2">
              Homepage's block view shows four fields, so with more selected the widget uses{' '}
              <code className="font-mono">display: list</code> — one row per field.
            </p>
          )}
        </div>

        <div>
          <div className="flex items-center justify-between mb-2">
            <div className="text-micro font-semibold uppercase tracking-[0.14em] text-ink-4">
              services.yaml
            </div>
            <CopyButton text={yaml} label="Copy YAML" />
          </div>
          <pre className="p-3 bg-inset border border-line rounded-row text-2xs font-mono text-ink-2 overflow-x-auto whitespace-pre">
            {yaml}
          </pre>
          {!freshToken && (
            <p className="text-2xs text-ink-4 mt-2">
              {status?.configured
                ? 'The token is not shown again. Regenerate to get a new one filled in here, or paste the one you saved.'
                : 'Create a token and it is filled in here.'}{' '}
              To keep it out of the file, set <code className="font-mono">HOMEPAGE_VAR_MCPGW_TOKEN</code> on
              the Homepage container and write <code className="font-mono">{'{{HOMEPAGE_VAR_MCPGW_TOKEN}}'}</code> instead.
            </p>
          )}
        </div>
      </div>

      <ConfirmModal
        open={confirm === 'regenerate'}
        onClose={() => setConfirm(null)}
        onConfirm={issue}
        loading={busy}
        tone="primary"
        title="Regenerate the stats token?"
        description="The current token stops working immediately. Update your Homepage config with the new one."
        confirmLabel="Regenerate"
      />
      <ConfirmModal
        open={confirm === 'revoke'}
        onClose={() => setConfirm(null)}
        onConfirm={revoke}
        loading={busy}
        title="Revoke the stats token?"
        description="Any widget using it starts getting 401 Unauthorized."
        confirmLabel="Revoke"
      />
    </div>
  );
}
