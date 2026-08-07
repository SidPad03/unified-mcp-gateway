import clsx from 'clsx';

/**
 * The MCP Gateway "Aperture" mark.
 *
 * Three traces converge on a single point and pass through a diamond aperture,
 * leaving as one beam: many MCP servers, one guarded endpoint.
 *
 * This is a transcription of `brand/mcp-gateway-mark.svg` — the same five paths
 * the agent's `BrandMark.swift` draws and the app icon is cut from. **The
 * product has one logo.** If this drifts from the SVG the dashboard and the app
 * stop looking like the same thing, so change all three together.
 */
export function BrandMark({
  size = 24,
  strokeWidth = 2.2,
  className,
}: {
  size?: number;
  /** On the 24 grid. 2.2 is the standard cut; go heavier below ~20px. */
  strokeWidth?: number;
  className?: string;
}) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={strokeWidth}
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
      aria-hidden="true"
    >
      <path d="M2 6.5H5L9 12" />
      <path d="M2 12H9" />
      <path d="M2 17.5H5L9 12" />
      <path d="M9 12L13 8L17 12L13 16Z" />
      <path d="M17 12H22" />
    </svg>
  );
}

/** Mark + wordmark. The only place the two appear together is app identity. */
export function BrandLockup({
  size = 26,
  className,
  subtitle,
}: {
  size?: number;
  className?: string;
  subtitle?: string;
}) {
  return (
    <div className={clsx('flex items-center gap-2.5 min-w-0', className)}>
      <BrandMark size={size} strokeWidth={size < 20 ? 2.5 : 2.2} className="text-beam shrink-0" />
      <div className="min-w-0">
        <div className="text-md font-semibold tracking-[-0.025em] leading-none text-ink">
          MCP Gateway
        </div>
        {subtitle && (
          <div className="text-micro font-mono text-ink-4 truncate mt-[3px] leading-none">
            {subtitle}
          </div>
        )}
      </div>
    </div>
  );
}
