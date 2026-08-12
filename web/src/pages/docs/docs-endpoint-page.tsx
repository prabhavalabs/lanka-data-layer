import * as React from "react";
import { Link, useParams } from "react-router-dom";

import { getDocsEndpoint } from "@/lib/docs-spec";
import { ParamTable } from "@/pages/docs/components/param-table";
import { TryPanel } from "@/pages/docs/components/try-panel";

const RESPONSE_KIND_LABEL = {
  envelope: "{success, message, payload, meta} envelope",
  geojson: "bare GeoJSON Feature (not the usual envelope)",
  binary: "raw bytes (not JSON)",
} as const;

function useCopyLabel(): [string, (text: string) => void] {
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

/**
 * The endpoint page's three-column-in-spirit layout from the design: center
 * = title/summary/copyable pill/params/response/notes/errors/sources, right
 * = the sticky Try panel with its own scroll (try-panel.tsx). Collapses to
 * stacked below ~1100px per the design's responsive rule.
 */
export function DocsEndpointPage() {
  const { slug } = useParams<{ slug: string }>();
  const endpoint = slug ? getDocsEndpoint(slug) : undefined;
  const [pillLabel, copyPill] = useCopyLabel();
  const [responseLabel, copyResponse] = useCopyLabel();

  if (!endpoint) {
    return (
      <div className="mx-auto flex max-w-3xl flex-col gap-3 p-6">
        <h1 className="text-lg font-semibold">Endpoint not found</h1>
        <p className="text-sm text-ink2">
          There&apos;s no docs page for &quot;{slug}&quot;.{" "}
          <Link to="/" className="font-medium text-brand hover:underline">
            Back to the overview
          </Link>
          .
        </p>
      </div>
    );
  }

  return (
    <div className="flex h-full min-w-0 flex-col overflow-y-auto min-[1100px]:flex-row min-[1100px]:overflow-hidden">
      <div className="min-w-0 flex-1 min-[1100px]:overflow-y-auto">
        <div className="mx-auto max-w-[720px] animate-fade-up px-6 py-10 pb-14 sm:px-11 sm:py-12">
          <div className="mb-2.5 font-mono text-[11px] text-ink3">
            {endpoint.group} · {endpoint.method}
          </div>
          <h1 className="mb-2.5 text-[27px] font-bold tracking-[-0.02em]">{endpoint.summary}</h1>
          <p className="mb-5 text-[14.5px] leading-[1.6] text-ink2">{endpoint.description}</p>

          <button
            type="button"
            onClick={() => copyPill(`${endpoint.method} ${endpoint.pathTemplate}`)}
            className="inline-flex max-w-full items-center gap-2.5 rounded-lg border border-border bg-bg3 px-3 py-2 transition-colors hover:border-line2"
          >
            <span className="shrink-0 rounded-[5px] bg-positive-soft px-[7px] py-[2.5px] font-mono text-[10.5px] font-semibold text-positive">
              {endpoint.method}
            </span>
            <span className="truncate font-mono text-[13px] text-ink">{endpoint.pathTemplate}</span>
            <span className="shrink-0 font-mono text-[10px] text-ink3">{pillLabel}</span>
          </button>

          <h2 className="mb-1 mt-9 text-[15px] font-semibold tracking-[-0.01em]">Parameters</h2>
          <div className="mb-3 text-[12.5px] text-ink3">
            {endpoint.params.length === 0
              ? "This endpoint takes no parameters."
              : endpoint.params.some((p) => p.location === "path")
                ? `${endpoint.params
                    .filter((p) => p.location === "path")
                    .map((p) => `{${p.name}}`)
                    .join(", ")} — path parameter${endpoint.params.filter((p) => p.location === "path").length > 1 ? "s" : ""}; the rest are query string.`
                : "Query string parameters."}
          </div>
          <ParamTable params={endpoint.params} />

          <h2 className="mb-3 mt-9 text-[15px] font-semibold tracking-[-0.01em]">Response</h2>
          <div className="mb-2 text-[12.5px] text-ink3">
            Body: <span className="font-mono">{RESPONSE_KIND_LABEL[endpoint.responseKind]}</span>
          </div>
          <div className="overflow-hidden rounded-xl border border-border bg-code-bg">
            <div className="flex items-center gap-2 border-b border-white/10 px-3.5 py-2">
              <span className="font-mono text-[11.5px] text-code-ink2">Captured from a real request</span>
              <button
                type="button"
                onClick={() => copyResponse(endpoint.exampleResponse)}
                className="ml-auto rounded-md border border-white/10 px-2.5 py-[3px] font-mono text-[10.5px] text-code-ink2 transition-colors hover:text-code-ink"
              >
                {responseLabel}
              </button>
            </div>
            <pre className="overflow-x-auto p-3.5 font-mono text-[12px] leading-relaxed text-code-ink">{endpoint.exampleResponse}</pre>
          </div>

          {endpoint.notes.length > 0 && (
            <>
              <h2 className="mb-3 mt-9 text-[15px] font-semibold tracking-[-0.01em]">Good to know</h2>
              <ul className="flex flex-col gap-2">
                {endpoint.notes.map((note, i) => (
                  <li key={i} className="flex gap-2.5 text-[13px] leading-[1.55]">
                    <span className="select-none text-ink3">–</span>
                    <span className="text-ink2">{note}</span>
                  </li>
                ))}
              </ul>
            </>
          )}

          {endpoint.errors.length > 0 && (
            <>
              <h2 className="mb-3 mt-9 text-[15px] font-semibold tracking-[-0.01em]">Errors</h2>
              <div className="flex flex-col gap-2">
                {endpoint.errors.map((err, i) => (
                  <div key={i} className="flex flex-wrap items-baseline gap-2.5 text-[12.5px]">
                    <span className="shrink-0 rounded-[5px] border border-border bg-bg3 px-[7px] py-[2px] font-mono font-semibold text-warn">
                      {err.status}
                    </span>
                    <span className="shrink-0 font-mono text-[12px] text-ink">{err.code}</span>
                    <span className="text-ink2">{err.description}</span>
                  </div>
                ))}
              </div>
            </>
          )}

          {endpoint.sources.length > 0 && (
            <div className="mt-12 flex flex-wrap items-center gap-2 border-t border-border pt-4.5">
              <span className="mr-1 text-[11.5px] text-ink3">Sources</span>
              {endpoint.sources.map((s) => (
                <span key={s.name} className="inline-flex items-center gap-1.5 rounded-full border border-border bg-bg2 px-2.5 py-[3.5px] text-[11.5px] text-ink2">
                  {s.name}
                  <span className="font-mono text-[9.5px] text-ink3">{s.license}</span>
                </span>
              ))}
            </div>
          )}
        </div>
      </div>

      <div className="w-full shrink-0 border-t border-border bg-bg2 min-[1100px]:w-[400px] min-[1100px]:overflow-y-auto min-[1100px]:border-l min-[1100px]:border-t-0">
        <TryPanel endpoint={endpoint} />
      </div>
    </div>
  );
}
