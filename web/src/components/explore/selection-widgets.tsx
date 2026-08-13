import * as React from "react";

import { MONO_FONT, TEXT, TEXT3 } from "@/components/explore/glass";
import type { StatSegment } from "@/components/explore/selection-format";
import { usePrefersReducedMotion } from "@/hooks/use-media-query";

const COUNT_UP_DURATION_MS = 380;

/** cubic-bezier(0.22, 1, 0.36, 1) sampled as a plain ease-out-ish curve for the JS-driven count-up (a real cubic-bezier solve is overkill for a number tween — this cubic ease-out reads the same: fast start, gentle settle). */
function easeOutCubic(t: number): number {
  return 1 - (1 - t) ** 3;
}

/**
 * Ticks a number up from 0 to `value` over ~380ms on first mount (keyed by
 * the caller via `key=` when it should replay for a new selection).
 * Respects prefers-reduced-motion by rendering the final value immediately.
 */
export function CountUp({ value, format }: { value: number; format?: (n: number) => string }) {
  const reducedMotion = usePrefersReducedMotion();
  const [display, setDisplay] = React.useState(reducedMotion ? value : 0);

  React.useEffect(() => {
    if (reducedMotion) {
      setDisplay(value);
      return;
    }
    let raf = 0;
    const start = performance.now();
    const tick = (now: number) => {
      const t = Math.min(1, (now - start) / COUNT_UP_DURATION_MS);
      setDisplay(value * easeOutCubic(t));
      if (t < 1) raf = requestAnimationFrame(tick);
    };
    raf = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(raf);
  }, [value, reducedMotion]);

  const text = format ? format(display) : Math.round(display).toLocaleString("en-US");
  return <span style={{ fontFamily: MONO_FONT, fontVariantNumeric: "tabular-nums" }}>{text}</span>;
}

/** Loading placeholder block — a soft gradient sweep (index.css's .animate-skeleton-shimmer), reduced to a flat static fill under prefers-reduced-motion (handled in CSS, not here — no reducedMotion prop needed). Used for both the core full-card skeleton (selection-card.tsx) and each progressively-loading secondary section's skeleton (selection-sections.tsx). */
export function ShimmerBlock({ className }: { className: string }) {
  return <div className={`animate-skeleton-shimmer rounded ${className}`} />;
}

/** 11px uppercase micro-heading — "POPULATION", "AREA", etc. per the card anatomy spec. */
export function MicroHeading({ children }: { children: React.ReactNode }) {
  return (
    <div
      className="mb-1.5 text-[11px] font-semibold uppercase"
      style={{ color: TEXT3, letterSpacing: "0.08em", fontFamily: MONO_FONT }}
    >
      {children}
    </div>
  );
}

/** Compact horizontal stacked bar (top-4 + other) with a wrapping legend below — used for ethnicity/religion shares. */
export function StatBar({ title, segments }: { title: string; segments: StatSegment[] }) {
  if (segments.length === 0) return null;
  return (
    <div>
      <div className="mb-1 text-[11.5px] font-medium" style={{ color: TEXT }}>
        {title}
      </div>
      <div className="flex h-2 w-full overflow-hidden rounded-full" style={{ background: "rgba(120,140,160,0.16)" }}>
        {segments.map((seg) => (
          <div
            key={seg.key}
            title={`${seg.label} · ${(seg.share * 100).toFixed(1)}%`}
            style={{ width: `${Math.max(seg.share * 100, 0.5)}%`, background: seg.color }}
          />
        ))}
      </div>
      <div className="mt-1.5 flex flex-wrap gap-x-3 gap-y-1">
        {segments.map((seg) => (
          <div key={seg.key} className="flex items-center gap-1.5 text-[10.5px]" style={{ color: TEXT3 }}>
            <span aria-hidden="true" className="size-2 shrink-0 rounded-full" style={{ background: seg.color }} />
            <span>{seg.label}</span>
            <span style={{ fontFamily: MONO_FONT }}>{(seg.share * 100).toFixed(0)}%</span>
          </div>
        ))}
      </div>
    </div>
  );
}
