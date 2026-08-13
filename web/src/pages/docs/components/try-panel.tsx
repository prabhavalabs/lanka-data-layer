import * as React from "react";

import { MiniMap } from "@/components/minimap/mini-map";
import { buildMiniMapScene, ENDPOINT_MINIMAP_VIEW } from "@/components/minimap/scene";
import { cn } from "@/lib/utils";
import { buildDocsUrl, defaultParamValues, type DocsEndpoint, type DocsParam } from "@/lib/docs-spec";
import { JsonViewer } from "@/pages/docs/components/json-viewer";

/** http://localhost:8600 in dev (matches api/README.md's default PORT); the deployed origin otherwise — see vite.config.ts's /v1 proxy, which only exists in dev. */
function curlBaseUrl(): string {
  return import.meta.env.DEV ? "http://localhost:8600" : window.location.origin;
}

type SendState =
  | { status: "idle" }
  | { status: "loading" }
  | { status: "offline"; elapsedMs: number }
  | { status: "json"; httpStatus: number; elapsedMs: number; value: unknown; headers: Record<string, string>; bytes: number }
  | { status: "binary"; httpStatus: number; elapsedMs: number; headers: Record<string, string> };

function headerRecord(headers: Headers): Record<string, string> {
  const out: Record<string, string> = {};
  headers.forEach((value, key) => {
    out[key] = value;
  });
  return out;
}

const NOTABLE_HEADERS = ["etag", "cache-control", "content-type", "content-length", "content-range"];

function formatBytes(n: number): string {
  return n < 1024 ? `${n} B` : `${(n / 1024).toFixed(1)} kB`;
}

function StatusChip({ httpStatus }: { httpStatus: number }) {
  const tone =
    httpStatus < 300
      ? "bg-positive-soft text-ok"
      : httpStatus < 500
        ? "border border-border bg-bg3 text-warn"
        : "bg-brand-soft text-brand2";
  return <span className={cn("rounded-full px-2 py-[2.5px] font-mono text-[10.5px] font-semibold", tone)}>{httpStatus} {httpStatus < 300 ? "OK" : ""}</span>;
}

function ElapsedBadge({ elapsedMs }: { elapsedMs: number }) {
  return (
    <span className="rounded-[5px] border border-border px-[7px] py-[2px] font-mono text-[10.5px] tabular-nums text-ink2">
      {elapsedMs < 1000 ? `${Math.round(elapsedMs)} ms` : `${(elapsedMs / 1000).toFixed(2)} s`}
    </span>
  );
}

function useCopy(): [string, (text: string) => void] {
  const [copied, setCopied] = React.useState("");
  const timer = React.useRef<number | undefined>(undefined);
  const copy = React.useCallback((text: string) => {
    void navigator.clipboard.writeText(text).catch(() => {});
    setCopied(text);
    window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => setCopied(""), 1400);
  }, []);
  return [copied, copy];
}

function ParamInput({ param, value, onChange }: { param: DocsParam; value: string; onChange: (v: string) => void }) {
  const baseClass = "w-full rounded-[7px] border border-border bg-bg px-2.5 py-[7px] font-mono text-[12.5px] text-ink outline-none focus-visible:border-brand";

  if (param.kind === "enum" && param.enumValues) {
    return (
      <select className={baseClass} value={value} onChange={(e) => onChange(e.target.value)}>
        {!param.required && <option value="">(unset)</option>}
        {param.enumValues.map((option) => (
          <option key={option} value={option}>
            {option}
          </option>
        ))}
      </select>
    );
  }

  if (param.kind === "boolean") {
    return (
      <label className="flex h-8 items-center gap-2 text-[12.5px]">
        <input type="checkbox" checked={value === "true"} onChange={(e) => onChange(e.target.checked ? "true" : "false")} />
        <span className="text-ink2">{value === "true" ? "true (sends =1)" : "false (omitted)"}</span>
      </label>
    );
  }

  return (
    <input
      type={param.kind === "number" || param.kind === "integer" ? "number" : "text"}
      step={param.kind === "integer" ? 1 : "any"}
      spellCheck={false}
      className={baseClass}
      value={value}
      onChange={(e) => onChange(e.target.value)}
      placeholder={param.example}
    />
  );
}

/**
 * The endpoint page's right-hand "Try it" panel — editable param inputs
 * prefilled from docs-spec.ts's examples, a Send button that fetches the
 * real API through Vite's dev proxy, a status chip + latency badge, the
 * design-colored JsonViewer, a copyable curl line, and the MiniMap driven
 * off the actual response (falling back to a scene built from the request's
 * own values before the first Send, for endpoints where that's possible —
 * see scene.ts). Supersedes the old playground.tsx.
 */
export function TryPanel({ endpoint }: { endpoint: DocsEndpoint }) {
  const [values, setValues] = React.useState<Record<string, string>>(() => defaultParamValues(endpoint));
  const [state, setState] = React.useState<SendState>({ status: "idle" });
  const [jsonCopied, copyJson] = useCopy();
  const [curlCopied, copyCurl] = useCopy();

  React.useEffect(() => {
    setValues(defaultParamValues(endpoint));
    setState({ status: "idle" });
  }, [endpoint]);

  const requestPath = buildDocsUrl(endpoint, values);
  const curlLine = endpoint.responseKind === "binary" ? `curl -H "Range: bytes=0-15" "${curlBaseUrl()}${requestPath}"` : `curl "${curlBaseUrl()}${requestPath}"`;

  async function send() {
    setState({ status: "loading" });
    const start = performance.now();
    try {
      const res = await fetch(requestPath, { headers: endpoint.responseKind === "binary" ? { Range: "bytes=0-15" } : undefined });
      const elapsedMs = performance.now() - start;
      const headers = headerRecord(res.headers);

      if (endpoint.responseKind === "binary") {
        await res.arrayBuffer();
        setState({ status: "binary", httpStatus: res.status, elapsedMs, headers });
        return;
      }

      const text = await res.text();
      let value: unknown;
      try {
        value = text.length > 0 ? JSON.parse(text) : null;
      } catch {
        value = text;
      }
      setState({ status: "json", httpStatus: res.status, elapsedMs, value, headers, bytes: text.length });
    } catch {
      const elapsedMs = performance.now() - start;
      setState({ status: "offline", elapsedMs });
    }
  }

  const minimapView = ENDPOINT_MINIMAP_VIEW[endpoint.slug];
  const responsePayload =
    state.status === "json" ? (endpoint.responseKind === "envelope" && isEnvelope(state.value) ? state.value.payload : state.value) : null;
  const scene = minimapView ? buildMiniMapScene(endpoint.slug, responsePayload, values) : null;

  return (
    <div className="flex h-full flex-col">
      <div className="px-[18px] pt-4">
        <div className="mb-3.5 flex items-center gap-2">
          <span className="text-[13px] font-semibold">Try it</span>
          <span className="ml-auto font-mono text-[10px] text-ink3">no auth · CORS open</span>
        </div>

        {endpoint.params.length > 0 && (
          <div className="flex flex-col gap-[9px]">
            {endpoint.params.map((param) => (
              <label key={param.name} className="flex flex-col gap-1">
                <span className="flex items-baseline gap-1.5">
                  <span className="font-mono text-[11px] text-ink2">{param.name}</span>
                  {param.required && <span className="text-[9px] font-semibold text-brand">required</span>}
                  <span className="ml-auto font-mono text-[9.5px] text-ink3">{param.location}</span>
                </span>
                <ParamInput param={param} value={values[param.name] ?? ""} onChange={(v) => setValues((prev) => ({ ...prev, [param.name]: v }))} />
              </label>
            ))}
          </div>
        )}

        <div className="mt-3 break-all rounded-lg border border-border bg-bg px-2.5 py-2 font-mono text-[11px] leading-[1.55] text-ink2">
          <span className="font-semibold text-positive">GET</span> {requestPath}
        </div>

        <button
          type="button"
          onClick={() => void send()}
          disabled={state.status === "loading"}
          className="mt-3 flex w-full items-center justify-center gap-2 rounded-lg bg-brand py-2.5 text-[13.5px] font-semibold text-white transition-colors hover:bg-brand2 disabled:opacity-70"
        >
          {state.status === "loading" && (
            <span className="size-[13px] animate-spin rounded-full border-2 border-white/35 border-t-white" aria-hidden="true" />
          )}
          {state.status === "loading" ? "Sending…" : "Send request"}
        </button>
      </div>

      <div className="flex flex-col gap-3.5 px-[18px] py-4">
        {state.status === "offline" && (
          <div className="rounded-[10px] border border-dashed border-line2 px-[18px] py-[22px] text-center">
            <div className="mb-2 font-mono text-[11px] text-warn">ERR_CONNECTION_REFUSED</div>
            <div className="mb-1.5 text-[13.5px] font-semibold">Can&rsquo;t reach the API</div>
            <div className="mb-3.5 text-[12.5px] leading-[1.55] text-ink2">
              The public instance may be down, or you&rsquo;re offline. Lanka Data Layer is self-hostable — one container serves the full island.
            </div>
            <div className="mb-3 rounded-[7px] bg-code-bg px-2.5 py-2 font-mono text-[11.5px] text-json-str">docker run -p 8080:8080 lanka-data-layer/api</div>
            <button
              type="button"
              onClick={() => void send()}
              className="rounded-[7px] border border-line2 px-3.5 py-1.5 text-[12px] font-medium text-ink transition-colors hover:border-brand"
            >
              Retry
            </button>
          </div>
        )}

        {(state.status === "json" || state.status === "binary") && (
          <div className="animate-fade-up">
            <div className="mb-2 flex items-center gap-2">
              <StatusChip httpStatus={state.httpStatus} />
              <ElapsedBadge elapsedMs={state.elapsedMs} />
              {state.status === "json" && <span className="font-mono text-[10.5px] text-ink3">{formatBytes(state.bytes)}</span>}
              {state.status === "json" && (
                <button
                  type="button"
                  onClick={() => copyJson(JSON.stringify(state.value, null, 2))}
                  className="ml-auto rounded-[5px] border border-border px-2 py-[2.5px] font-mono text-[10px] text-ink3 transition-colors hover:border-line2 hover:text-ink"
                >
                  {jsonCopied ? "copied ✓" : "copy JSON"}
                </button>
              )}
            </div>

            {state.status === "json" && <JsonViewer value={state.value} />}
            {state.status === "binary" && (
              <div className="flex flex-col gap-2">
                <div className="flex flex-wrap gap-x-4 gap-y-1 font-mono text-[11px] text-ink2">
                  {NOTABLE_HEADERS.filter((h) => state.headers[h] !== undefined).map((h) => (
                    <span key={h}>
                      <span className="text-ink3">{h}:</span> {state.headers[h]}
                    </span>
                  ))}
                </div>
                <p className="rounded-md border border-border bg-bg2 p-3 text-[12px] text-ink2">
                  Binary response ({state.headers["content-range"]?.split("/")[1] ?? "unknown"} bytes total) — body not decoded, headers shown above.
                </p>
              </div>
            )}
          </div>
        )}

        <div>
          <div className="mb-1.5 flex items-center">
            <span className="text-[10.5px] font-semibold uppercase tracking-[0.08em] text-ink3">curl</span>
            <button type="button" onClick={() => copyCurl(curlLine)} className="ml-auto font-mono text-[10px] text-ink3 hover:text-ink">
              {curlCopied ? "copied ✓" : "copy"}
            </button>
          </div>
          <div className="break-all rounded-lg bg-code-bg px-3 py-[9px] font-mono text-[11px] leading-[1.6] text-code-ink2">
            curl &quot;<span className="text-json-str">{requestPath}</span>&quot;
          </div>
        </div>

        {minimapView && (
          <div>
            <div className="mb-1.5 flex items-center">
              <span className="text-[10.5px] font-semibold uppercase tracking-[0.08em] text-ink3">On the map</span>
              <span className="ml-auto font-mono text-[10px] text-ink3">{minimapView}</span>
            </div>
            <div className="overflow-hidden rounded-[10px] border border-border bg-bg">
              <MiniMap scene={scene} className="h-[225px]" emptyHint="Send a request to see it on the map" />
            </div>
          </div>
        )}
      </div>
    </div>
  );
}

function isEnvelope(v: unknown): v is { payload: unknown } {
  return typeof v === "object" && v !== null && "payload" in v;
}
