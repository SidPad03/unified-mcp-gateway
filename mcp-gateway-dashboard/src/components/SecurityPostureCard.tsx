import { useEffect, useState } from 'react';
import { api, SecurityPosture } from '@/lib/api';
import { ShieldCheck, ShieldAlert } from 'lucide-react';

type CheckStatus = 'pass' | 'warn' | 'fail';
interface Check {
  label: string;
  status: CheckStatus;
  detail: string;
}

// Deliberately a CHECKLIST, never an aggregate "you are secure" verdict — the
// card can only test the handful of signals the endpoint exposes, so a green
// shield would manufacture false assurance. Each row is a named check + how to fix.
function buildChecks(p: SecurityPosture): Check[] {
  return [
    {
      label: 'Listener binding',
      status: p.listen_addr_public ? 'warn' : 'pass',
      detail: p.listen_addr_public
        ? `Public interface (${p.listen_addr}). Front it with a proxy/VPN or bind loopback if it shouldn't be internet-facing.`
        : `Bound to ${p.listen_addr}.`,
    },
    {
      label: 'Admin password rotated',
      status: p.admin_password_change_pending ? 'fail' : 'pass',
      detail: p.admin_password_change_pending
        ? 'A seeded admin still owes a first-login password change.'
        : 'No seeded default awaiting rotation.',
    },
    {
      label: 'Agent backends owned',
      status: p.unowned_agent_backends.length > 0 ? 'warn' : 'pass',
      detail: p.unowned_agent_backends.length > 0
        ? `Unowned: ${p.unowned_agent_backends.join(', ')}. Assign an owner.`
        : 'Every agent backend has an owner.',
    },
    {
      label: 'Active owners',
      status: p.active_owner_count === 0 ? 'fail' : 'pass',
      detail: p.active_owner_count === 0
        ? 'No active owner — the system has no administrator.'
        : `${p.active_owner_count} active: ${p.owners.map(o => o.username).join(', ')}.`,
    },
  ];
}

export default function SecurityPostureCard() {
  const [posture, setPosture] = useState<SecurityPosture | null>(null);
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    api.getSecurityPosture().then(setPosture).catch(() => setFailed(true));
  }, []);

  // Never claim a posture we couldn't read: hide on error (older server without
  // the endpoint, or a non-owner who gets 403) and while loading.
  if (failed || !posture) return null;

  const checks = buildChecks(posture);
  const failing = checks.filter(c => c.status !== 'pass').length;

  return (
    <div className="mb-6 rounded-xl border border-border bg-surface p-4">
      <div className="flex items-center gap-2 mb-3">
        {failing > 0 ? (
          <ShieldAlert className="w-4 h-4 text-amber-400" />
        ) : (
          <ShieldCheck className="w-4 h-4 text-emerald-400" />
        )}
        <h3 className="text-sm font-semibold text-white">Security posture</h3>
        <span className="text-xs text-gray-500">
          {failing > 0
            ? `${checks.length - failing}/${checks.length} checks passing`
            : 'all configured checks passing'}
        </span>
      </div>
      <ul className="space-y-1.5">
        {checks.map((c, i) => (
          <li key={i} className="flex items-start gap-2 text-xs">
            <span
              className={
                c.status === 'fail'
                  ? 'text-red-400'
                  : c.status === 'warn'
                  ? 'text-amber-400'
                  : 'text-emerald-400'
              }
            >
              {c.status === 'fail' ? '✕' : c.status === 'warn' ? '!' : '✓'}
            </span>
            <span className="text-gray-300 w-40 shrink-0">{c.label}</span>
            <span className="text-gray-500">{c.detail}</span>
          </li>
        ))}
      </ul>
    </div>
  );
}
