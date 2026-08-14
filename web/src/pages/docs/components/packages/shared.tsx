import * as React from "react";

import { cn } from "@/lib/utils";

/** Copy-to-clipboard with a transient "copied ✓" label, identical timing/behavior to docs-endpoint-page.tsx's useCopyLabel — duplicated locally rather than shared so this whole file tree stays self-contained inside the /docs/packages lazy chunk. */
export function useCopyLabel(): [string, (text: string) => void] {
  const [label, setLabel] = React.useState("copy");
  const timer = React.useRef<number | undefined>(undefined);
  const copy = React.useCallback((text: string) => {
    void navigator.clipboard.writeText(text).catch(() => {});
    setLabel("copied ✓");
    window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => setLabel("copy"), 1400);
  }, []);
  return [label, copy];
}

/** A live npm version badge (shields.io) linking to the package's npmjs.com page — same markup pattern as the root README's package table. */
export function NpmBadge({ pkg }: { pkg: string }) {
  return (
    <a
      href={`https://www.npmjs.com/package/${pkg}`}
      target="_blank"
      rel="noreferrer"
      className="inline-flex items-center rounded-[4px] align-middle"
      title={`${pkg} on npm`}
    >
      <img src={`https://img.shields.io/npm/v/${encodeURIComponent(pkg)}`} alt={`npm version for ${pkg}`} className="h-[18px]" />
    </a>
  );
}

/** One-line `$ npm install …` block with the same copy-button treatment as the rest of the docs' code panels. */
export function InstallBlock({ pkg }: { pkg: string }) {
  const cmd = `npm install ${pkg}`;
  const [copyLabel, copy] = useCopyLabel();
  return (
    <div className="flex items-center gap-2 overflow-hidden rounded-xl border border-border bg-code-bg px-3.5 py-[9px]">
      <div className="overflow-x-auto whitespace-pre font-mono text-[12.5px] leading-[1.65] text-code-ink">
        <span className="text-json-punc">$</span> {cmd}
      </div>
      <button
        type="button"
        onClick={() => copy(cmd)}
        className="ml-auto shrink-0 rounded-md border border-white/10 px-2.5 py-[3px] font-mono text-[10.5px] text-code-ink2 transition-colors hover:text-code-ink"
      >
        {copyLabel}
      </button>
    </div>
  );
}

/** Multi-line code snippet panel, matching docs-endpoint-page.tsx's "Response" block chrome (header row + copy button + dark pre). */
export function SnippetBlock({ code, label }: { code: string; label?: string }) {
  const [copyLabel, copy] = useCopyLabel();
  return (
    <div className="overflow-hidden rounded-xl border border-border bg-code-bg">
      <div className="flex items-center gap-2 border-b border-white/10 px-3.5 py-2">
        <span className="font-mono text-[11.5px] text-code-ink2">{label ?? "TypeScript"}</span>
        <button
          type="button"
          onClick={() => copy(code)}
          className="ml-auto rounded-md border border-white/10 px-2.5 py-[3px] font-mono text-[10.5px] text-code-ink2 transition-colors hover:text-code-ink"
        >
          {copyLabel}
        </button>
      </div>
      <pre className="overflow-x-auto p-3.5 font-mono text-[12px] leading-relaxed text-code-ink">{code}</pre>
    </div>
  );
}

/** Small rounded pill for a runtime fact ("DATA_VERSION 20260812.7", "14,417 units"), matching the design's source/credit pill style used on home-page.tsx and docs-endpoint-page.tsx. */
export function FactPill({ children }: { children: React.ReactNode }) {
  return (
    <span className="inline-flex items-center gap-1.5 rounded-full border border-border bg-bg2 px-2.5 py-[3.5px] font-mono text-[11px] text-ink2">
      {children}
    </span>
  );
}

/** The small pulsing dot used to label each interactive demo as genuinely live. */
export function LiveDot() {
  return (
    <span className="relative inline-flex size-[7px]" aria-hidden="true">
      <span className="absolute inline-flex h-full w-full animate-ping rounded-full bg-positive opacity-75" />
      <span className="relative inline-flex size-[7px] rounded-full bg-positive" />
    </span>
  );
}

export function EmptyHint({ children }: { children: React.ReactNode }) {
  return (
    <div className="flex min-h-[80px] items-center rounded-lg border border-dashed border-line2 px-3.5 py-3 text-[12.5px] leading-[1.5] text-ink3">
      {children}
    </div>
  );
}

/** A compact label/value stat, used across every demo's detail panel (admin unit, election result, census profile). */
export function Stat({ label, value, mono }: { label: string; value: React.ReactNode; mono?: boolean }) {
  return (
    <div className="flex flex-col gap-0.5 rounded-md bg-bg px-2.5 py-2">
      <span className="text-[10px] uppercase tracking-[0.06em] text-ink3">{label}</span>
      <span className={cn("text-[12.5px] text-ink", mono && "font-mono text-[11.5px]")}>{value}</span>
    </div>
  );
}

/** A labeled horizontal share bar — reused for election party vote shares and census age/ethnicity/religion breakdowns. `tone` swaps the fill color for the highlighted row (an election winner). */
export function ShareBar({
  label,
  sublabel,
  share,
  tone = "default",
}: {
  label: string;
  sublabel?: string;
  share: number;
  tone?: "default" | "brand";
}) {
  const pct = Math.max(0, Math.min(1, share)) * 100;
  return (
    <div className="flex items-center gap-2.5">
      <span className="w-[92px] shrink-0 truncate text-[11.5px] text-ink2" title={label}>
        {label}
      </span>
      <div className="h-[7px] flex-1 overflow-hidden rounded-full bg-bg3">
        <div
          className={cn("h-full rounded-full", tone === "brand" ? "bg-brand" : "bg-ink3/70")}
          style={{ width: `${pct}%` }}
        />
      </div>
      <span className="w-[52px] shrink-0 text-right font-mono text-[11px] tabular-nums text-ink3">{pct.toFixed(1)}%</span>
      {sublabel && <span className="w-[64px] shrink-0 truncate text-right font-mono text-[10px] text-ink3">{sublabel}</span>}
    </div>
  );
}

/** Text input matching try-panel.tsx's ParamInput styling, reused across every demo's search box. */
export function DemoInput(props: React.InputHTMLAttributes<HTMLInputElement>) {
  return (
    <input
      {...props}
      spellCheck={false}
      className={cn(
        "h-9 w-full rounded-[7px] border border-border bg-bg px-2.5 font-mono text-[12.5px] text-ink outline-none placeholder:text-ink3 focus-visible:border-brand",
        props.className
      )}
    />
  );
}

export function DemoButton(props: React.ButtonHTMLAttributes<HTMLButtonElement>) {
  return (
    <button
      {...props}
      type="button"
      className={cn(
        "h-9 shrink-0 rounded-[7px] border border-line2 px-3 text-[12px] font-medium text-ink transition-colors hover:border-brand disabled:opacity-60",
        props.className
      )}
    />
  );
}

/** Result-list row shared by the admin and census search demos: a name + right-aligned meta chip, selectable. */
export function ResultRow({
  active,
  onClick,
  title,
  meta,
}: {
  active?: boolean;
  onClick: () => void;
  title: React.ReactNode;
  meta?: React.ReactNode;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={cn(
        "flex w-full items-center gap-2 rounded-md px-2.5 py-[7px] text-left text-[12.5px] transition-colors",
        active ? "bg-brand-soft text-ink" : "text-ink2 hover:bg-bg3"
      )}
    >
      <span className="truncate">{title}</span>
      {meta && <span className="ml-auto shrink-0 font-mono text-[10px] text-ink3">{meta}</span>}
    </button>
  );
}
