/**
 * One place that decides how a number, a duration or a timestamp is written.
 *
 * The agent's `Format` enum in `Design/Components.swift` is the same set with
 * the same rules, so a latency reads identically in the Mac app and here.
 */
export const fmt = {
  /** 1284 → "1,284". Anything that can change is rendered with tabular figures. */
  count(value: number | null | undefined): string {
    if (value == null || !Number.isFinite(value)) return '—';
    return value.toLocaleString();
  },

  /** 12840 → "12.8k". For axis ticks and tight cells only; a headline gets the real number. */
  compact(value: number | null | undefined): string {
    if (value == null || !Number.isFinite(value)) return '—';
    if (Math.abs(value) < 1_000) return String(value);
    return Intl.NumberFormat(undefined, { notation: 'compact', maximumFractionDigits: 1 }).format(
      value
    );
  },

  /** Sub-second stays in ms; past that, seconds with two decimals. */
  duration(ms: number | null | undefined): string {
    if (ms == null || !Number.isFinite(ms)) return '—';
    if (ms < 1_000) return `${Math.round(ms)} ms`;
    return `${(ms / 1_000).toFixed(2)} s`;
  },

  /** `3d 4h`, `4h 12m`, `12m`, `45s` — one unit of precision past the first. */
  uptime(seconds: number | null | undefined): string {
    if (seconds == null || seconds < 0 || !Number.isFinite(seconds)) return '—';
    const d = Math.floor(seconds / 86_400);
    const h = Math.floor((seconds % 86_400) / 3_600);
    const m = Math.floor((seconds % 3_600) / 60);
    if (d > 0) return `${d}d ${h}h`;
    if (h > 0) return `${h}h ${m}m`;
    if (m > 0) return `${m}m`;
    return `${Math.floor(seconds)}s`;
  },

  percent(fraction: number | null | undefined, digits = 1): string {
    if (fraction == null || !Number.isFinite(fraction)) return '—';
    return `${(fraction * 100).toFixed(digits).replace(/\.0$/, '')}%`;
  },

  /** Local time-of-day. The events happened somewhere with a clock. */
  time(value: string | number | Date): string {
    const d = new Date(value);
    if (Number.isNaN(d.getTime())) return '—';
    return d.toLocaleTimeString(undefined, { hour12: false });
  },

  dateTime(value: string | number | Date): string {
    const d = new Date(value);
    if (Number.isNaN(d.getTime())) return '—';
    return d.toLocaleString(undefined, {
      month: 'short',
      day: 'numeric',
      hour: '2-digit',
      minute: '2-digit',
      second: '2-digit',
      hour12: false,
    });
  },

  /** An hourly bucket: `6 Aug 09:00`. Seconds on a figure that covers an hour
   *  are three characters that can only ever read `00`. */
  hour(value: string | number | Date): string {
    const d = new Date(value);
    if (Number.isNaN(d.getTime())) return '—';
    return d.toLocaleString(undefined, {
      month: 'short',
      day: 'numeric',
      hour: '2-digit',
      minute: '2-digit',
      hour12: false,
    });
  },

  /** "4m ago". Falls back to an absolute date past a week, where "9d ago" stops helping. */
  relative(value: string | number | Date): string {
    const d = new Date(value);
    if (Number.isNaN(d.getTime())) return '—';
    const secs = Math.round((Date.now() - d.getTime()) / 1_000);
    if (secs < 45) return 'just now';
    if (secs < 3_600) return `${Math.round(secs / 60)}m ago`;
    if (secs < 86_400) return `${Math.round(secs / 3_600)}h ago`;
    if (secs < 604_800) return `${Math.round(secs / 86_400)}d ago`;
    return d.toLocaleDateString(undefined, { month: 'short', day: 'numeric' });
  },

  bytes(value: number | null | undefined): string {
    if (value == null || !Number.isFinite(value)) return '—';
    const units = ['B', 'KB', 'MB', 'GB'];
    let n = value;
    let i = 0;
    while (n >= 1024 && i < units.length - 1) {
      n /= 1024;
      i += 1;
    }
    return `${i === 0 ? n : n.toFixed(1)} ${units[i]}`;
  },
};
