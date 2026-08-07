import {
  ButtonHTMLAttributes,
  InputHTMLAttributes,
  ReactNode,
  SelectHTMLAttributes,
  TextareaHTMLAttributes,
  useEffect,
  useId,
  useRef,
  useState,
} from 'react';
import { ChevronDown, ChevronUp, ChevronsUpDown, Eye, EyeOff, Loader2, X } from 'lucide-react';
import clsx from 'clsx';

/* ══════════════════════════════════════════════════════════════════════════
   The shared vocabulary.

   Everything the dashboard draws comes from here. The agent's
   `Design/Components.swift` has the same set under the same names — a Card is a
   Card, a Stat is a Stat, a Rail is a Rail — and keeping the two in step is what
   makes the web app and the Mac app read as one product.

   Two rules run through all of it:

   1. **Colour means something.** Tone is `ok | warn | deny | neutral` and
      nothing else. There is no "primary blue", no decorative gradient, and the
      accent is never a button fill — a solid button is near-white on dark and
      near-black on light, which is what keeps the accent scarce enough to read
      as "alive".
   2. **Hierarchy is weight and colour before size.** A 13px value at 600 in
      primary ink separates from a 13px label at 500 in secondary ink more
      cleanly than two regular weights two points apart.
   ══════════════════════════════════════════════════════════════════════════ */

export type Tone = 'ok' | 'warn' | 'deny' | 'neutral';

/** The raw CSS variable for a tone — for rails, dots, chart series, SVG fills. */
export const toneColor = (tone: Tone): string =>
  tone === 'ok'
    ? 'var(--beam)'
    : tone === 'warn'
      ? 'var(--warn)'
      : tone === 'deny'
        ? 'var(--deny)'
        : 'var(--line-strong)';

const TONE_FG: Record<Tone, string> = {
  ok: 'text-beam',
  warn: 'text-warn',
  deny: 'text-deny',
  neutral: 'text-ink-3',
};

const TONE_CHIP: Record<Tone, string> = {
  ok: 'text-beam bg-beam-wash border-beam-edge',
  warn: 'text-warn bg-warn-wash border-warn-edge',
  deny: 'text-deny bg-deny-wash border-deny-edge',
  neutral: 'text-ink-2 bg-neutral-wash border-line',
};

/* ── Surfaces ──────────────────────────────────────────────────────────── */

/**
 * Content on its own plane, above the page.
 *
 * On dark the lift is a hairline border and a 3% lightness step; on light it is
 * a white panel over a cold-grey canvas plus a three-layer shadow. Same
 * hierarchy, different physics — depth shadows simply do not read on near-black.
 */
export function Card({
  children,
  className,
  padding = 'p-4',
  onClick,
  as: As = 'div',
}: {
  children: ReactNode;
  className?: string;
  /** Density is a decision: 16px is the house value. Drop to p-3 inside a card. */
  padding?: string;
  onClick?: () => void;
  as?: 'div' | 'section' | 'article' | 'li';
}) {
  return (
    <As
      onClick={onClick}
      className={clsx(
        'bg-panel border border-line rounded-card shadow-[var(--shadow-card)]',
        padding,
        onClick && 'cursor-pointer transition-colors duration-150 hover:border-line-strong',
        className
      )}
    >
      {children}
    </As>
  );
}

/* ── Type ──────────────────────────────────────────────────────────────── */

/**
 * The one focal element of a page: what you came here to do, and the actions
 * that do it. Nothing else on the page competes at this size.
 */
export function PageHeader({
  title,
  description,
  actions,
  children,
}: {
  title: string;
  description?: string;
  actions?: ReactNode;
  children?: ReactNode;
}) {
  return (
    <header className="mb-6 flex items-start justify-between gap-6 flex-wrap">
      <div className="min-w-0">
        <h1 className="text-xl font-semibold text-ink">{title}</h1>
        {description && <p className="text-xs text-ink-3 mt-1 max-w-[68ch]">{description}</p>}
        {children}
      </div>
      {actions && <div className="flex items-center gap-2 shrink-0">{actions}</div>}
    </header>
  );
}

/** `10px / 600 / .16em / uppercase` — the label voice, used everywhere. */
export function SectionHeader({
  children,
  trailing,
  className,
}: {
  children: ReactNode;
  trailing?: ReactNode;
  className?: string;
}) {
  return (
    <div className={clsx('flex items-center justify-between gap-3 min-h-[20px]', className)}>
      <h2 className="text-micro font-semibold uppercase tracking-[0.16em] text-ink-3">
        {children}
      </h2>
      {trailing}
    </div>
  );
}

export function Label({ children, className }: { children: ReactNode; className?: string }) {
  return (
    <span
      className={clsx(
        'text-micro font-semibold uppercase tracking-[0.14em] text-ink-3',
        className
      )}
    >
      {children}
    </span>
  );
}

/** An identifier — a tool name, an agent id, a host, a hash. Always mono. */
export function Mono({ children, className }: { children: ReactNode; className?: string }) {
  return <span className={clsx('font-mono tabular-nums', className)}>{children}</span>;
}

/* ── Numbers ───────────────────────────────────────────────────────────── */

/**
 * The figure you came for. One of these per view, at most.
 *
 * Three levers at once — 26px, 600, primary ink — against a 10px/600/muted
 * label. Size alone would not separate them.
 */
export function Stat({
  label,
  value,
  tone,
  delta,
  detail,
  className,
}: {
  label: string;
  value: ReactNode;
  tone?: Tone;
  delta?: { value: string; tone: Tone };
  detail?: ReactNode;
  className?: string;
}) {
  return (
    <div className={clsx('min-w-0', className)}>
      <Label>{label}</Label>
      <div
        className={clsx(
          'text-2xl font-semibold tracking-[-0.02em] tabular-nums mt-1.5',
          tone ? TONE_FG[tone] : 'text-ink'
        )}
      >
        {value}
      </div>
      {delta && (
        <div className={clsx('text-2xs font-medium mt-1.5', TONE_FG[delta.tone])}>
          {delta.value}
          {detail && <span className="text-ink-4 font-normal ml-1.5">{detail}</span>}
        </div>
      )}
      {!delta && detail && <div className="text-2xs text-ink-4 mt-1.5">{detail}</div>}
    </div>
  );
}

/** The supporting tier under a Stat — deliberately a third of its size. */
export function MiniStat({
  label,
  value,
  tone,
}: {
  label: string;
  value: ReactNode;
  tone?: Tone;
}) {
  return (
    <div className="min-w-0">
      <div className="text-micro font-semibold uppercase tracking-[0.14em] text-ink-4">{label}</div>
      <div
        className={clsx(
          'text-md font-semibold tabular-nums mt-0.5',
          tone ? TONE_FG[tone] : 'text-ink'
        )}
      >
        {value}
      </div>
    </div>
  );
}

/* ── Status ────────────────────────────────────────────────────────────── */

/**
 * A status dot. `pulsing` is the one animation in the product that carries
 * meaning rather than polish — it is how "connected" differs from a screenshot
 * of connected.
 *
 * Never the only signal: every dot is paired with a word, because a colour
 * difference is not information to everyone.
 */
export function Dot({ tone, pulsing = false }: { tone: Tone; pulsing?: boolean }) {
  return (
    <span className="relative inline-flex shrink-0 h-[6px] w-[6px]" aria-hidden="true">
      {pulsing && (
        <span
          className="absolute inset-0 rounded-full motion-safe:animate-pulse-ring"
          style={{ background: toneColor(tone) }}
        />
      )}
      <span className="relative h-full w-full rounded-full" style={{ background: toneColor(tone) }} />
    </span>
  );
}

export function Badge({
  children,
  tone = 'neutral',
  dot = false,
  className,
  title,
  onClick,
}: {
  children: ReactNode;
  tone?: Tone;
  dot?: boolean;
  className?: string;
  title?: string;
  onClick?: () => void;
}) {
  const Tag = onClick ? 'button' : 'span';
  return (
    <Tag
      type={onClick ? 'button' : undefined}
      onClick={onClick}
      title={title}
      className={clsx(
        'inline-flex items-center gap-1.5 h-5 px-2 rounded-control border',
        'text-micro font-medium tracking-[0.02em] whitespace-nowrap',
        TONE_CHIP[tone],
        onClick && 'transition-transform duration-100 active:scale-[0.97] cursor-pointer',
        className
      )}
    >
      {dot && <Dot tone={tone} />}
      {children}
    </Tag>
  );
}

/** Status as a word plus a dot — the pair, never one alone. */
export function StatusLabel({
  tone,
  children,
  pulsing,
  className,
}: {
  tone: Tone;
  children: ReactNode;
  pulsing?: boolean;
  className?: string;
}) {
  return (
    <span className={clsx('inline-flex items-center gap-2 text-xs font-medium', TONE_FG[tone], className)}>
      <Dot tone={tone} pulsing={pulsing} />
      {children}
    </span>
  );
}

/* ── Risk ──────────────────────────────────────────────────────────────────
   Risk classification is a *ramp*, not six unrelated colours.

   The old build gave read/write/admin/destructive/execute/unclassified their own
   hue — emerald, blue, orange, red, purple, grey — which made a page of tools
   look like a paint chart and told you nothing about which ones to worry about.
   Here emphasis climbs with severity: the safe categories sit quietly in
   neutral ink at increasing weight, admin lifts to amber, destructive to red.
   Unclassified is drawn as a dashed outline because it is a *gap*, not a level.
   ───────────────────────────────────────────────────────────────────────── */

export const RISK_LEVELS = [
  'read',
  'write',
  'execute',
  'admin',
  'destructive',
  'unclassified',
] as const;

const RISK_STYLE: Record<string, string> = {
  read: 'text-ink-3 bg-neutral-wash border-line',
  write: 'text-ink-2 bg-neutral-wash border-line',
  execute: 'text-ink bg-neutral-wash border-line-strong',
  admin: 'text-warn bg-warn-wash border-warn-edge',
  destructive: 'text-deny bg-deny-wash border-deny-edge',
  unclassified: 'text-ink-4 bg-transparent border-line border-dashed',
};

/** The tone a risk category contributes to a row's rail. */
export function riskTone(risk?: string | null): Tone {
  if (risk === 'destructive') return 'deny';
  if (risk === 'admin') return 'warn';
  return 'neutral';
}

export function RiskBadge({
  risk,
  count,
  active,
  onClick,
  className,
}: {
  risk?: string | null;
  count?: number;
  active?: boolean;
  onClick?: () => void;
  className?: string;
}) {
  const level = risk || 'unclassified';
  const Tag = onClick ? 'button' : 'span';
  return (
    <Tag
      type={onClick ? 'button' : undefined}
      onClick={onClick}
      aria-pressed={onClick ? !!active : undefined}
      className={clsx(
        'inline-flex items-center gap-1.5 h-5 px-2 rounded-control border',
        'text-micro font-medium tracking-[0.02em] whitespace-nowrap',
        RISK_STYLE[level] ?? RISK_STYLE.unclassified,
        onClick && 'transition-transform duration-100 active:scale-[0.97] cursor-pointer',
        active && 'ring-2 ring-beam-edge',
        className
      )}
    >
      {level}
      {count !== undefined && <span className="opacity-55 tabular-nums">{count}</span>}
    </Tag>
  );
}

/* ── The gate rail ─────────────────────────────────────────────────────────
   The signature. A gateway is a channel things pass through, so the interface
   has one: a 3px rail down the left of every row, carrying that row's verdict.
   Scroll a thousand audit events and the system's health is readable
   peripherally, as a column of green with red notches in it.
   ───────────────────────────────────────────────────────────────────────── */

/** For a table cell — the rail as an inset shadow, so it does not cost a column. */
export const railStyle = (tone?: Tone) => ({
  boxShadow: `inset 3px 0 0 0 ${tone ? toneColor(tone) : 'transparent'}`,
});

/** A list of railed rows. */
export function RailList({ children, className }: { children: ReactNode; className?: string }) {
  return (
    <div className={clsx('bg-panel border border-line rounded-card overflow-hidden shadow-[var(--shadow-card)]', className)}>
      {children}
    </div>
  );
}

export function RailRow({
  tone = 'neutral',
  children,
  trailing,
  onClick,
  className,
  active,
}: {
  tone?: Tone;
  children: ReactNode;
  trailing?: ReactNode;
  onClick?: () => void;
  className?: string;
  active?: boolean;
}) {
  return (
    <div
      onClick={onClick}
      role={onClick ? 'button' : undefined}
      tabIndex={onClick ? 0 : undefined}
      onKeyDown={
        onClick
          ? e => {
              if (e.key === 'Enter' || e.key === ' ') {
                e.preventDefault();
                onClick();
              }
            }
          : undefined
      }
      className={clsx(
        'grid grid-cols-[3px_1fr_auto] items-stretch',
        'border-b border-line-soft last:border-b-0',
        'transition-colors duration-150',
        onClick && 'cursor-pointer hover:bg-raised',
        active && 'bg-raised',
        className
      )}
    >
      <div style={{ background: toneColor(tone) }} />
      <div className="px-3.5 py-2.5 min-w-0 flex items-center">{children}</div>
      <div className="px-3.5 py-2.5 flex items-center gap-3.5 shrink-0">{trailing}</div>
    </div>
  );
}

/* ── Controls ──────────────────────────────────────────────────────────── */

type Variant = 'primary' | 'secondary' | 'ghost' | 'danger';
type Size = 'sm' | 'md' | 'lg';

const VARIANT: Record<Variant, string> = {
  // Never the accent. A solid near-white (dark) / near-black (light) fill is
  // unmistakably the primary action and costs no colour budget.
  primary: 'bg-solid text-on-solid hover:bg-solid-hover font-semibold border-transparent',
  secondary: 'bg-raised text-ink-2 border-line hover:text-ink hover:border-line-strong',
  ghost: 'bg-transparent text-ink-3 border-transparent hover:text-ink hover:bg-raised',
  danger: 'bg-deny-wash text-deny border-deny-edge hover:bg-deny hover:text-on-solid',
};

const SIZE: Record<Size, string> = {
  sm: 'h-7 px-2.5 text-2xs gap-1.5 rounded-control',
  md: 'h-8 px-3 text-xs gap-1.5 rounded-control',
  lg: 'h-9 px-4 text-sm gap-2 rounded-control',
};

export function Button({
  variant = 'secondary',
  size = 'md',
  loading = false,
  icon: Icon,
  children,
  className,
  ...rest
}: ButtonHTMLAttributes<HTMLButtonElement> & {
  variant?: Variant;
  size?: Size;
  loading?: boolean;
  icon?: React.ComponentType<{ className?: string }>;
}) {
  return (
    <button
      type="button"
      {...rest}
      disabled={rest.disabled || loading}
      className={clsx(
        'inline-flex items-center justify-center border font-medium whitespace-nowrap select-none',
        // Named properties, never `all` — transitioning `all` animates layout.
        'transition-[background-color,border-color,color,transform,opacity] duration-150 ease-[var(--ease-out-quint)]',
        // Tactile confirmation the UI heard the click. Never below 0.95.
        'active:scale-[0.97]',
        'disabled:opacity-45 disabled:pointer-events-none',
        VARIANT[variant],
        SIZE[size],
        className
      )}
    >
      {loading ? (
        <Loader2 className="w-3.5 h-3.5 animate-spin shrink-0" />
      ) : (
        Icon && <Icon className="w-3.5 h-3.5 shrink-0" />
      )}
      {children}
    </button>
  );
}

/**
 * An icon-only control. 32px visible, but the hit area is pushed to 40 with a
 * pseudo-element — WCAG wants 44 and 40 is the floor for a pointer-first admin
 * tool. Never let two of these overlap.
 */
export function IconButton({
  icon: Icon,
  label,
  className,
  tone,
  ...rest
}: ButtonHTMLAttributes<HTMLButtonElement> & {
  icon: React.ComponentType<{ className?: string }>;
  label: string;
  tone?: Tone;
}) {
  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      {...rest}
      className={clsx(
        'relative inline-flex items-center justify-center h-8 w-8 rounded-control shrink-0',
        'after:absolute after:-inset-1 after:content-[""]',
        'transition-[background-color,color,transform] duration-150 ease-[var(--ease-out-quint)]',
        'active:scale-[0.94] disabled:opacity-40 disabled:pointer-events-none',
        tone ? TONE_FG[tone] : 'text-ink-3 hover:text-ink',
        'hover:bg-raised',
        className
      )}
    >
      <Icon className="w-4 h-4" />
    </button>
  );
}

/* ── Fields ────────────────────────────────────────────────────────────────
   Inputs are painted *darker* than the surface around them, in both themes. An
   input receives content, so it reads as inset. That signals "type here"
   without a heavy border.
   ───────────────────────────────────────────────────────────────────────── */

// No width here on purpose. An Input is nearly always in a form and wants the
// column; a Select is nearly always a filter in a toolbar and wants to be as
// wide as its longest option. Baking w-full into the shared base made every
// filter row wrap onto three lines.
const FIELD =
  'field bg-inset border border-line rounded-control text-ink placeholder:text-ink-4 ' +
  'transition-[border-color,box-shadow] duration-150 ' +
  'focus:outline-none focus:border-beam-edge focus:ring-[3px] focus:ring-beam-wash ' +
  'disabled:opacity-50 disabled:cursor-not-allowed';

export function Input({ className, ...rest }: InputHTMLAttributes<HTMLInputElement>) {
  return <input {...rest} className={clsx(FIELD, 'w-full h-8 px-2.5 text-xs', className)} />;
}

export function Textarea({ className, ...rest }: TextareaHTMLAttributes<HTMLTextAreaElement>) {
  return (
    <textarea {...rest} className={clsx(FIELD, 'w-full px-2.5 py-2 text-xs leading-relaxed', className)} />
  );
}

/**
 * A native `<select>`, styled.
 *
 * The closed state is ours; the popup stays the platform's. That keeps the
 * keyboard handling, the ARIA and the type-ahead the browser already
 * implements, none of which a hand-rolled listbox gets for free. The chevron is
 * a background image — see `select.field` in index.css.
 */
export function Select({ className, children, ...rest }: SelectHTMLAttributes<HTMLSelectElement>) {
  return (
    <select {...rest} className={clsx(FIELD, 'h-8 pl-2.5 text-xs cursor-pointer', className)}>
      {children}
    </select>
  );
}

/** A password field with a reveal. The toggle sits inside the field's padding. */
export function PasswordInput({
  className,
  ...rest
}: Omit<InputHTMLAttributes<HTMLInputElement>, 'type'>) {
  const [shown, setShown] = useState(false);
  return (
    <div className="relative">
      <input
        {...rest}
        type={shown ? 'text' : 'password'}
        // `w-full` like Input and Textarea. Without it the input keeps its
        // intrinsic size attribute, so on the sign-in form the password box came
        // out narrower than the username box above it.
        className={clsx(FIELD, 'w-full h-9 pl-2.5 pr-9 text-xs', className)}
      />
      <button
        type="button"
        onClick={() => setShown(s => !s)}
        aria-label={shown ? 'Hide password' : 'Show password'}
        title={shown ? 'Hide password' : 'Show password'}
        className="absolute right-0 top-0 h-9 w-9 inline-flex items-center justify-center text-ink-4 hover:text-ink-2 transition-colors rounded-control"
      >
        {shown ? <EyeOff className="w-3.5 h-3.5" /> : <Eye className="w-3.5 h-3.5" />}
      </button>
    </div>
  );
}

export function Field({
  label,
  hint,
  error,
  children,
  className,
}: {
  label?: string;
  hint?: ReactNode;
  error?: string;
  children: ReactNode;
  className?: string;
}) {
  return (
    <div className={clsx('min-w-0', className)}>
      {label && <Label className="block mb-1.5">{label}</Label>}
      {children}
      {error ? (
        <p className="text-2xs text-deny mt-1.5">{error}</p>
      ) : (
        hint && <p className="text-2xs text-ink-4 mt-1.5">{hint}</p>
      )}
    </div>
  );
}

export function Toggle({
  checked,
  onChange,
  label,
  disabled,
}: {
  checked: boolean;
  onChange: (next: boolean) => void;
  label: string;
  disabled?: boolean;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      disabled={disabled}
      onClick={() => onChange(!checked)}
      className={clsx(
        'relative inline-flex h-[18px] w-8 shrink-0 items-center rounded-full border p-[2px]',
        'transition-colors duration-150 ease-[var(--ease-out-quint)]',
        'disabled:opacity-40 disabled:pointer-events-none',
        checked ? 'bg-beam-wash border-beam-edge' : 'bg-inset border-line'
      )}
    >
      <span
        className="h-3 w-3 rounded-full transition-transform duration-150 ease-[var(--ease-out-quint)]"
        style={{
          background: checked ? toneColor('ok') : 'var(--text-4)',
          transform: checked ? 'translateX(14px)' : 'none',
        }}
      />
    </button>
  );
}

/** A small exclusive choice — a range picker, a mode switch. */
export function Segmented<T extends string>({
  value,
  options,
  onChange,
  label,
}: {
  value: T;
  options: readonly { value: T; label: string }[];
  onChange: (next: T) => void;
  label: string;
}) {
  return (
    <div
      role="group"
      aria-label={label}
      className="inline-flex items-center gap-0.5 p-0.5 bg-inset border border-line rounded-control"
    >
      {options.map(o => (
        <button
          key={o.value}
          type="button"
          aria-pressed={value === o.value}
          onClick={() => onChange(o.value)}
          className={clsx(
            'h-[22px] px-2 rounded-[4px] text-micro font-semibold tracking-[0.02em]',
            'transition-[background-color,color] duration-150',
            value === o.value
              ? 'bg-panel text-ink shadow-[var(--shadow-card)]'
              : 'text-ink-4 hover:text-ink-2'
          )}
        >
          {o.label}
        </button>
      ))}
    </div>
  );
}

/* ── Overlays ──────────────────────────────────────────────────────────────
   Built on native <dialog>.

   `showModal()` gives the focus trap, Escape, the top layer, `inert` on the
   background and a real `::backdrop` — all of which a `fixed inset-0` div has
   to reimplement, and usually does not. The only thing it does not do is lock
   the page scroll, so that is handled here.
   ───────────────────────────────────────────────────────────────────────── */

export function Modal({
  open,
  onClose,
  title,
  description,
  children,
  footer,
  width = 'max-w-lg',
}: {
  open: boolean;
  onClose: () => void;
  title: string;
  description?: string;
  children: ReactNode;
  footer?: ReactNode;
  width?: string;
}) {
  const ref = useRef<HTMLDialogElement>(null);
  const titleId = useId();
  const descId = useId();

  useEffect(() => {
    const dialog = ref.current;
    if (!dialog) return;
    if (open && !dialog.open) dialog.showModal();
    else if (!open && dialog.open) dialog.close();
  }, [open]);

  useEffect(() => {
    if (!open) return;
    const previous = document.body.style.overflow;
    document.body.style.overflow = 'hidden';
    return () => {
      document.body.style.overflow = previous;
    };
  }, [open]);

  if (!open) return null;

  return (
    <dialog
      ref={ref}
      aria-labelledby={titleId}
      aria-describedby={description ? descId : undefined}
      // Escape fires `cancel`; let it through to the same handler as the X so
      // there is exactly one close path.
      onCancel={e => {
        e.preventDefault();
        onClose();
      }}
      onClose={onClose}
      // A click that lands on the <dialog> itself is a click on the backdrop —
      // the content is the inner div.
      onClick={e => {
        if (e.target === ref.current) onClose();
      }}
      className={clsx(
        'w-[calc(100vw-2rem)] bg-transparent p-0 m-auto text-ink',
        'motion-safe:animate-pop',
        width
      )}
    >
      <div className="bg-high border border-line rounded-panel shadow-[var(--shadow-overlay)] overflow-hidden">
        <div className="flex items-start justify-between gap-4 px-5 pt-4 pb-3">
          <div className="min-w-0">
            <h2 id={titleId} className="text-md font-semibold text-ink">
              {title}
            </h2>
            {description && (
              <p id={descId} className="text-2xs text-ink-3 mt-1">
                {description}
              </p>
            )}
          </div>
          <IconButton icon={X} label="Close" onClick={onClose} className="-mr-1.5 -mt-1" />
        </div>
        <div className="px-5 pb-5 max-h-[min(70vh,640px)] overflow-y-auto">{children}</div>
        {footer && (
          <div className="flex items-center justify-end gap-2 px-5 py-3.5 bg-raised border-t border-line">
            {footer}
          </div>
        )}
      </div>
    </dialog>
  );
}

export function ConfirmModal({
  open,
  onClose,
  onConfirm,
  title,
  description,
  confirmLabel = 'Confirm',
  tone = 'deny',
  loading,
  children,
}: {
  open: boolean;
  onClose: () => void;
  onConfirm: () => void;
  title: string;
  description?: string;
  confirmLabel?: string;
  tone?: 'deny' | 'primary';
  loading?: boolean;
  children?: ReactNode;
}) {
  return (
    <Modal
      open={open}
      onClose={onClose}
      title={title}
      description={description}
      width="max-w-md"
      footer={
        <>
          <Button onClick={onClose} variant="ghost">
            Cancel
          </Button>
          <Button
            onClick={onConfirm}
            loading={loading}
            variant={tone === 'deny' ? 'danger' : 'primary'}
          >
            {confirmLabel}
          </Button>
        </>
      }
    >
      {children}
    </Modal>
  );
}

/* ── States ────────────────────────────────────────────────────────────────
   Loading, empty and error are not optional. Missing states are the fastest
   tell of an unfinished interface.
   ───────────────────────────────────────────────────────────────────────── */

export function EmptyState({
  icon: Icon,
  title,
  message,
  action,
  className,
}: {
  icon: React.ComponentType<{ className?: string }>;
  title: string;
  message?: string;
  action?: ReactNode;
  className?: string;
}) {
  return (
    <div className={clsx('flex flex-col items-center text-center py-12 px-6', className)}>
      <Icon className="w-6 h-6 text-ink-4 mb-3" />
      <p className="text-sm font-medium text-ink">{title}</p>
      {message && <p className="text-xs text-ink-3 mt-1.5 max-w-[46ch]">{message}</p>}
      {action && <div className="mt-4">{action}</div>}
    </div>
  );
}

export function Banner({
  tone = 'deny',
  children,
  onDismiss,
  action,
  className,
}: {
  tone?: Tone;
  children: ReactNode;
  onDismiss?: () => void;
  action?: ReactNode;
  className?: string;
}) {
  return (
    <div
      role={tone === 'deny' ? 'alert' : 'status'}
      className={clsx(
        'flex items-center gap-3 px-3.5 py-2.5 rounded-row border text-xs',
        TONE_CHIP[tone],
        className
      )}
    >
      <div className="flex-1 min-w-0">{children}</div>
      {action}
      {onDismiss && (
        <button
          type="button"
          onClick={onDismiss}
          aria-label="Dismiss"
          className="shrink-0 opacity-60 hover:opacity-100 transition-opacity"
        >
          <X className="w-3.5 h-3.5" />
        </button>
      )}
    </div>
  );
}

export function Spinner({ className }: { className?: string }) {
  return <Loader2 className={clsx('w-4 h-4 animate-spin text-ink-4', className)} />;
}

export function Loading({ label = 'Loading...' }: { label?: string }) {
  return (
    <div className="flex items-center justify-center gap-2.5 py-12 text-xs text-ink-3">
      <Spinner />
      {label}
    </div>
  );
}

/** A placeholder with the shape of the thing that is coming, not a grey box. */
export function Skeleton({ className }: { className?: string }) {
  return (
    <div
      className={clsx(
        'bg-neutral-wash rounded-control motion-safe:animate-pulse',
        className
      )}
    />
  );
}

/* ── Tables ────────────────────────────────────────────────────────────── */

/**
 * Column priority.
 *
 * A dense table cannot reflow — eight columns at 375px is a 925px scroller, and
 * a nested horizontal scrollbar is not a mobile layout. So columns are *ranked*
 * instead: the ones that answer the page's question stay, the supporting ones
 * appear as the viewport earns them, and everything is still reachable by
 * expanding the row. `hide` names the breakpoint at which a column *appears*.
 */
export type Hide = 'sm' | 'md' | 'lg' | 'xl';

const HIDE: Record<Hide, string> = {
  sm: 'hidden sm:table-cell',
  md: 'hidden md:table-cell',
  lg: 'hidden lg:table-cell',
  xl: 'hidden xl:table-cell',
};

export function Table({ children, className }: { children: ReactNode; className?: string }) {
  return (
    <div
      className={clsx(
        'bg-panel border border-line rounded-card shadow-[var(--shadow-card)] overflow-x-auto',
        className
      )}
    >
      <table className="w-full border-collapse">{children}</table>
    </div>
  );
}

export function Th<K extends string>({
  children,
  sortKey,
  sort,
  onSort,
  align = 'left',
  hide,
  className,
}: {
  children: ReactNode;
  sortKey?: K;
  sort?: { key: K; dir: 'asc' | 'desc' };
  onSort?: (key: K) => void;
  align?: 'left' | 'right';
  hide?: Hide;
  className?: string;
}) {
  const sortable = !!sortKey && !!onSort;
  const active = sortable && sort?.key === sortKey;
  const Icon = !active ? ChevronsUpDown : sort!.dir === 'asc' ? ChevronUp : ChevronDown;

  return (
    <th
      scope="col"
      aria-sort={active ? (sort!.dir === 'asc' ? 'ascending' : 'descending') : undefined}
      className={clsx(
        'text-micro font-semibold uppercase tracking-[0.14em] text-ink-3',
        'px-2.5 sm:px-3.5 h-9 border-b border-line whitespace-nowrap',
        align === 'right' ? 'text-right' : 'text-left',
        hide && HIDE[hide],
        className
      )}
    >
      {sortable ? (
        <button
          type="button"
          onClick={() => onSort!(sortKey!)}
          className={clsx(
            'inline-flex items-center gap-1.5 transition-colors duration-150 hover:text-ink',
            active && 'text-ink'
          )}
        >
          {children}
          <Icon className={clsx('w-3 h-3', active ? 'text-beam' : 'opacity-40')} />
        </button>
      ) : (
        children
      )}
    </th>
  );
}

export function Td({
  children,
  align = 'left',
  hide,
  className,
  ...rest
}: React.TdHTMLAttributes<HTMLTableCellElement> & { align?: 'left' | 'right'; hide?: Hide }) {
  return (
    <td
      {...rest}
      className={clsx(
        'px-2.5 sm:px-3.5 py-2.5 text-xs text-ink-2 border-b border-line-soft align-middle',
        align === 'right' ? 'text-right' : 'text-left',
        hide && HIDE[hide],
        className
      )}
    >
      {children}
    </td>
  );
}

/** Full-width message inside a table body — loading, empty, or error. */
export function TableMessage({ colSpan, children }: { colSpan: number; children: ReactNode }) {
  return (
    <tr>
      <td colSpan={colSpan} className="border-b-0">
        {children}
      </td>
    </tr>
  );
}
