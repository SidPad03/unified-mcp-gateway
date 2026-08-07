import { ReactNode } from 'react';
import clsx from 'clsx';

/* ══════════════════════════════════════════════════════════════════════════
   Chart chrome, bound to the tokens.

   recharts takes colours as literal strings, which is how the old build ended
   up with `#7c5cfc` and `#0a0a0f` hard-coded at a dozen call sites and no way
   to theme them. CSS variables work fine in those props — the values are
   resolved by the browser against the real DOM node — so everything here goes
   through `var(--…)` and follows light/dark for free.
   ══════════════════════════════════════════════════════════════════════════ */

/** Spread onto a recharts <Tooltip>. */
export const tooltipProps = {
  contentStyle: {
    background: 'var(--high)',
    border: '1px solid var(--line)',
    borderRadius: '8px',
    fontSize: '11px',
    padding: '7px 10px',
    boxShadow: 'var(--shadow-pop)',
    color: 'var(--text)',
  },
  labelStyle: {
    color: 'var(--text-3)',
    fontSize: '10px',
    marginBottom: '3px',
    textTransform: 'uppercase' as const,
    letterSpacing: '0.1em',
  },
  itemStyle: { color: 'var(--text)', fontSize: '11px', padding: 0 },
  cursor: { stroke: 'var(--beam)', strokeWidth: 1, strokeDasharray: '3 3' },
};

export const axisProps = {
  tick: { fill: 'var(--text-4)', fontSize: 10 },
  axisLine: false,
  tickLine: false,
};

/**
 * Risk is *ordinal*, so it gets a sequential ramp rather than six unrelated
 * hues: the safe end climbs through neutral ink, and only the two levels that
 * warrant action take a colour. Encoding an ordered variable categorically is
 * what made the old donut unreadable — six equally-loud slices with no sense of
 * which end was bad.
 */
export const RISK_FILL: Record<string, string> = {
  read: 'var(--text-4)',
  write: 'var(--text-3)',
  execute: 'var(--text-2)',
  admin: 'var(--warn)',
  destructive: 'var(--deny)',
  unclassified: 'var(--line-strong)',
};

export function ChartCard({
  title,
  trailing,
  children,
  className,
  bleed = false,
}: {
  title: string;
  trailing?: ReactNode;
  children: ReactNode;
  className?: string;
  /** Charts run to the card's edge; lists keep the padding. */
  bleed?: boolean;
}) {
  return (
    <section
      className={clsx(
        'bg-panel border border-line rounded-card shadow-[var(--shadow-card)] flex flex-col',
        className
      )}
    >
      <div className="flex items-center justify-between gap-3 px-4 pt-3.5 pb-3 min-h-[20px]">
        <h2 className="text-micro font-semibold uppercase tracking-[0.16em] text-ink-3">{title}</h2>
        {trailing}
      </div>
      <div className={clsx('flex-1 min-h-0', bleed ? 'pb-2' : 'px-4 pb-4')}>{children}</div>
    </section>
  );
}

/**
 * A value against its share of the maximum. Used for "top tools" and latency —
 * a bar per row reads faster than a column chart when the labels are long
 * identifiers, which here they always are.
 */
export function BarRow({
  label,
  value,
  fraction,
  tone = 'var(--beam)',
  trailing,
  rank,
}: {
  label: ReactNode;
  value: ReactNode;
  fraction: number;
  tone?: string;
  trailing?: ReactNode;
  rank?: number;
}) {
  return (
    <div className="group">
      <div className="flex items-center justify-between gap-3 mb-1">
        <div className="flex items-center gap-2 min-w-0">
          {rank !== undefined && (
            <span className="text-micro text-ink-4 tabular-nums w-3 text-right shrink-0">
              {rank}
            </span>
          )}
          <span className="text-2xs font-mono text-ink-2 truncate">{label}</span>
        </div>
        <div className="flex items-center gap-2.5 shrink-0">
          {trailing}
          <span className="text-2xs font-medium text-ink tabular-nums">{value}</span>
        </div>
      </div>
      <div
        className={clsx('h-[3px] rounded-full bg-neutral-wash overflow-hidden', rank !== undefined && 'ml-5')}
      >
        <div
          className="h-full rounded-full transition-[width] duration-500 ease-[var(--ease-out-quint)]"
          style={{ width: `${Math.max(fraction * 100, 1.5)}%`, background: tone }}
        />
      </div>
    </div>
  );
}

/** One 100%-wide bar split by category — a legible replacement for a donut. */
export function StackedBar({
  segments,
  className,
}: {
  segments: { key: string; value: number; fill: string }[];
  className?: string;
}) {
  const total = segments.reduce((sum, s) => sum + s.value, 0) || 1;
  return (
    <div className={clsx('flex h-2 rounded-full overflow-hidden bg-neutral-wash gap-px', className)}>
      {segments.map(s => (
        <div
          key={s.key}
          title={`${s.key}: ${s.value.toLocaleString()}`}
          style={{ width: `${(s.value / total) * 100}%`, background: s.fill }}
          className="first:rounded-l-full last:rounded-r-full transition-[width] duration-500 ease-[var(--ease-out-quint)]"
        />
      ))}
    </div>
  );
}
