import * as React from "react";

import { FactPill, InstallBlock, LiveDot, NpmBadge, SnippetBlock } from "@/pages/docs/components/packages/shared";

export function PackageSection({
  id,
  pkg,
  pitch,
  facts,
  snippet,
  demo,
}: {
  /** Anchor id for the sidebar/jump-nav to target. */
  id: string;
  /** Full npm package name, e.g. "@lanka-data-layer/admin". */
  pkg: string;
  pitch: string;
  /** Runtime facts pulled from the imported package itself — DATA_VERSION, entry counts. */
  facts: string[];
  snippet: string;
  demo: React.ReactNode;
}) {
  return (
    <section id={id} className="scroll-mt-6 border-t border-border pt-10 first:border-t-0 first:pt-0">
      <div className="mb-1.5 flex flex-wrap items-center gap-2.5">
        <h2 className="font-mono text-[17px] font-bold tracking-[-0.01em] text-ink">{pkg}</h2>
        <NpmBadge pkg={pkg} />
      </div>
      <p className="mb-3 max-w-[64ch] text-[14px] leading-[1.6] text-ink2">{pitch}</p>
      <div className="mb-6 flex flex-wrap gap-2">
        {facts.map((f) => (
          <FactPill key={f}>{f}</FactPill>
        ))}
      </div>

      <div className="mb-1.5 text-[10.5px] font-semibold uppercase tracking-[0.08em] text-ink3">Install</div>
      <InstallBlock pkg={pkg} />

      <div className="mb-1.5 mt-5 text-[10.5px] font-semibold uppercase tracking-[0.08em] text-ink3">Usage</div>
      <SnippetBlock code={snippet} />

      <div className="mb-2.5 mt-5 flex items-center gap-2">
        <LiveDot />
        <span className="text-[10.5px] font-semibold uppercase tracking-[0.08em] text-ink3">
          Live — running in your browser from the same package
        </span>
      </div>
      <div className="rounded-xl border border-border bg-bg2 p-4">{demo}</div>
    </section>
  );
}
