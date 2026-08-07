import { ReactNode } from 'react';
import { BrandMark } from '@/components/BrandMark';
import { ThemeToggle } from '@/components/Layout';

/**
 * The frame every pre-session screen sits in.
 *
 * There is exactly one thing to do on these screens, so there is exactly one
 * thing that carries weight: the mark, lit, above a single card. No split
 * layout, no product pitch — someone here is trying to get in, not be sold to.
 */
export default function AuthShell({
  title,
  description,
  children,
  footer,
}: {
  title: string;
  description?: ReactNode;
  children: ReactNode;
  footer?: ReactNode;
}) {
  return (
    <div className="min-h-screen bg-void flex flex-col items-center justify-center p-6">
      <div className="fixed top-4 right-4">
        <ThemeToggle />
      </div>

      <div className="w-full max-w-[352px]">
        <div className="flex flex-col items-center text-center mb-7">
          <div className="w-12 h-12 rounded-panel bg-beam-wash border border-beam-edge grid place-items-center text-beam mb-4">
            <BrandMark size={26} />
          </div>
          <h1 className="text-lg font-semibold text-ink">{title}</h1>
          {description && <p className="text-xs text-ink-3 mt-1.5 max-w-[42ch]">{description}</p>}
        </div>

        <div className="bg-panel border border-line rounded-card shadow-[var(--shadow-card)] p-5">
          {children}
        </div>

        {footer && <div className="mt-4 text-center">{footer}</div>}
      </div>
    </div>
  );
}
