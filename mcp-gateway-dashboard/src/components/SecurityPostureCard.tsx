import { useEffect, useState } from 'react';
import { api, SecurityPosture } from '@/lib/api';
import { Banner, RailList, RailRow, SectionHeader, Tone } from '@/components/ui';

interface Check {
  label: string;
  tone: Tone;
  detail: string;
}

// Deliberately a CHECKLIST, never an aggregate "you are secure" verdict — the
// card can only test the handful of signals the endpoint exposes, so a green
// shield would manufacture false assurance. Each row is a named check + how to fix.
function buildChecks(p: SecurityPosture): Check[] {
  return [
    {
      label: 'Listener binding',
      tone: p.listen_addr_public ? 'warn' : 'ok',
      detail: p.listen_addr_public
        ? `Public interface (${p.listen_addr}). Front it with a proxy or VPN, or bind loopback if it shouldn't be internet-facing.`
        : `Bound to ${p.listen_addr}.`,
    },
    {
      label: 'Admin password rotated',
      tone: p.admin_password_change_pending ? 'deny' : 'ok',
      detail: p.admin_password_change_pending
        ? 'A seeded admin still owes a first-login password change.'
        : 'No seeded default awaiting rotation.',
    },
    {
      label: 'Active owners',
      tone: p.active_owner_count === 0 ? 'deny' : 'ok',
      detail:
        p.active_owner_count === 0
          ? 'No active owner. Assign the owner role to a user.'
          : `${p.active_owner_count} active: ${p.owners.map(o => o.username).join(', ')}.`,
    },
  ];
}

export default function SecurityPostureCard() {
  const [posture, setPosture] = useState<SecurityPosture | null>(null);
  const [error, setError] = useState('');
  const [forbidden, setForbidden] = useState(false);

  useEffect(() => {
    let current = true;
    api
      .getSecurityPosture()
      .then(p => {
        if (current) setPosture(p);
      })
      .catch((e: Error) => {
        if (!current) return;
        // A non-owner is not entitled to the posture, so there is nothing to
        // say to them. Anything else is a failure to read it, and a security
        // checklist that deletes itself on failure is indistinguishable from
        // one that checked and found nothing — which is the worst thing this
        // particular card can do.
        if (/forbidden|owner role/i.test(e.message)) setForbidden(true);
        else setError(e.message || 'Could not read the security posture');
      });
    return () => {
      current = false;
    };
  }, []);

  if (forbidden) return null;

  if (error) {
    return (
      <section className="mb-3.5">
        <SectionHeader className="mb-2">Security posture</SectionHeader>
        <Banner tone="warn">
          The security checks could not be read, so nothing here is a verdict. {error}
        </Banner>
      </section>
    );
  }

  // Still loading. The card appears when it has something true to say.
  if (!posture) return null;

  const checks = buildChecks(posture);
  const failing = checks.filter(c => c.tone !== 'ok').length;

  return (
    <section className="mb-3.5">
      <SectionHeader
        className="mb-2"
        trailing={
          <span className="text-2xs text-ink-4 tabular-nums">
            {failing > 0
              ? `${checks.length - failing}/${checks.length} passing`
              : 'all configured checks passing'}
          </span>
        }
      >
        Security posture
      </SectionHeader>
      {/* Each check carries its own rail, so a page of green with one amber
          notch is readable before any of the words are. */}
      <RailList>
        {checks.map(c => (
          <RailRow key={c.label} tone={c.tone}>
            <div className="flex items-baseline gap-3 min-w-0 flex-wrap">
              <span className="text-xs font-medium text-ink w-44 shrink-0">{c.label}</span>
              <span className="text-2xs text-ink-3 min-w-0">{c.detail}</span>
            </div>
          </RailRow>
        ))}
      </RailList>
    </section>
  );
}
