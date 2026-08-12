import * as React from "react";

import { Button } from "@/components/ui/button";
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
  | { status: "error"; message: string; elapsedMs: number }
  | { status: "json"; httpStatus: number; elapsedMs: number; value: unknown; headers: Record<string, string> }
  | { status: "binary"; httpStatus: number; elapsedMs: number; headers: Record<string, string> };

function StatusChip({ httpStatus }: { httpStatus: number }) {
  const tone =
    httpStatus === 0
      ? "bg-muted text-muted-foreground"
      : httpStatus < 300
        ? "bg-emerald-500/15 text-emerald-700 dark:text-emerald-400"
        : httpStatus < 500
          ? "bg-amber-500/15 text-amber-700 dark:text-amber-400"
          : "bg-destructive/15 text-destructive";
  return (
    <span className={cn("rounded-full px-2 py-0.5 text-xs font-semibold tabular-nums", tone)}>
      {httpStatus === 0 ? "network error" : httpStatus}
    </span>
  );
}

function ElapsedBadge({ elapsedMs }: { elapsedMs: number }) {
  return (
    <span className="rounded-full bg-muted px-2 py-0.5 text-xs font-medium text-muted-foreground tabular-nums">
      {elapsedMs < 1000 ? `${Math.round(elapsedMs)} ms` : `${(elapsedMs / 1000).toFixed(2)} s`}
    </span>
  );
}

function ParamInput({ param, value, onChange }: { param: DocsParam; value: string; onChange: (v: string) => void }) {
  const baseClass =
    "h-8 w-full rounded-md border border-input bg-background px-2 text-xs outline-none focus-visible:ring-2 focus-visible:ring-ring/50";

  if (param.kind === "enum" && param.enumValues) {
    return (
      <select className={cn(baseClass, "font-mono")} value={value} onChange={(e) => onChange(e.target.value)}>
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
      <label className="flex h-8 items-center gap-2 text-xs">
        <input type="checkbox" checked={value === "true"} onChange={(e) => onChange(e.target.checked ? "true" : "false")} />
        <span className="text-muted-foreground">{value === "true" ? "true (sends =1)" : "false (omitted)"}</span>
      </label>
    );
  }

  if (param.kind === "number" || param.kind === "integer") {
    return (
      <input
        type="number"
        step={param.kind === "integer" ? 1 : "any"}
        className={cn(baseClass, "font-mono")}
        value={value}
        onChange={(e) => onChange(e.target.value)}
      />
    );
  }

  return <input type="text" className={cn(baseClass, "font-mono")} value={value} onChange={(e) => onChange(e.target.value)} />;
}

function CopyButton({ text }: { text: string }) {
  const [copied, setCopied] = React.useState(false);
  return (
    <Button
      type="button"
      variant="outline"
      size="sm"
      onClick={() => {
        void navigator.clipboard.writeText(text).then(() => {
          setCopied(true);
          window.setTimeout(() => setCopied(false), 1500);
        });
      }}
    >
      {copied ? "Copied" : "Copy"}
    </Button>
  );
}

function headerRecord(headers: Headers): Record<string, string> {
  const out: Record<string, string> = {};
  headers.forEach((value, key) => {
    out[key] = value;
  });
  return out;
}

const NOTABLE_HEADERS = ["etag", "cache-control", "content-type", "content-length", "content-range"];

/**
 * Live playground for one endpoint: editable param inputs (prefilled with
 * the spec's examples), a Send button that fetches the real API through
 * Vite's dev proxy (web/vite.config.ts), a status chip + response-time
 * badge, a copyable curl line, and the parsed response rendered via
 * JsonViewer. Binary responses (/v1/tiles/:file) never attempt a JSON
 * parse — they show headers from a small ranged request instead.
 */
export function Playground({ endpoint }: { endpoint: DocsEndpoint }) {
  const [values, setValues] = React.useState<Record<string, string>>(() => defaultParamValues(endpoint));
  const [state, setState] = React.useState<SendState>({ status: "idle" });

  // Reset local state when the endpoint changes (navigating between docs pages reuses this component).
  React.useEffect(() => {
    setValues(defaultParamValues(endpoint));
    setState({ status: "idle" });
  }, [endpoint]);

  const requestPath = buildDocsUrl(endpoint, values);
  const curlLine =
    endpoint.responseKind === "binary"
      ? `curl -H "Range: bytes=0-15" "${curlBaseUrl()}${requestPath}"`
      : `curl "${curlBaseUrl()}${requestPath}"`;

  async function send() {
    setState({ status: "loading" });
    const start = performance.now();
    try {
      const res = await fetch(requestPath, {
        headers: endpoint.responseKind === "binary" ? { Range: "bytes=0-15" } : undefined,
      });
      const elapsedMs = performance.now() - start;
      const headers = headerRecord(res.headers);

      if (endpoint.responseKind === "binary") {
        // Drain the (tiny, ranged) body so the connection closes cleanly, but never render raw bytes.
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
      setState({ status: "json", httpStatus: res.status, elapsedMs, value, headers });
    } catch (err) {
      const elapsedMs = performance.now() - start;
      setState({ status: "error", message: err instanceof Error ? err.message : String(err), elapsedMs });
    }
  }

  return (
    <div className="flex flex-col gap-4 rounded-lg border border-border bg-card p-4">
      <div className="flex items-center justify-between gap-2">
        <h3 className="text-sm font-semibold">Playground</h3>
        {state.status === "json" || state.status === "binary" ? (
          <div className="flex items-center gap-2">
            <StatusChip httpStatus={state.httpStatus} />
            <ElapsedBadge elapsedMs={state.elapsedMs} />
          </div>
        ) : state.status === "error" ? (
          <StatusChip httpStatus={0} />
        ) : null}
      </div>

      {endpoint.params.length > 0 && (
        <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
          {endpoint.params.map((param) => (
            <div key={param.name} className="flex flex-col gap-1">
              <label className="flex items-center gap-1.5 text-xs font-medium">
                <span className="font-mono">{param.name}</span>
                <span className="text-[10px] font-normal text-muted-foreground">({param.location})</span>
                {param.required && <span className="text-destructive">*</span>}
              </label>
              <ParamInput param={param} value={values[param.name] ?? ""} onChange={(v) => setValues((prev) => ({ ...prev, [param.name]: v }))} />
            </div>
          ))}
        </div>
      )}

      <div className="flex flex-col gap-1.5">
        <div className="flex items-center justify-between">
          <span className="text-xs font-medium text-muted-foreground">Request</span>
        </div>
        <code className="overflow-x-auto whitespace-nowrap rounded-md border border-border bg-muted/30 px-2.5 py-1.5 font-mono text-xs">
          GET {requestPath}
        </code>
      </div>

      <div className="flex flex-col gap-1.5">
        <span className="text-xs font-medium text-muted-foreground">curl</span>
        <div className="flex items-center gap-2">
          <code className="flex-1 overflow-x-auto whitespace-nowrap rounded-md border border-border bg-muted/30 px-2.5 py-1.5 font-mono text-xs">
            {curlLine}
          </code>
          <CopyButton text={curlLine} />
        </div>
      </div>

      <div>
        <Button type="button" onClick={() => void send()} disabled={state.status === "loading"}>
          {state.status === "loading" ? "Sending…" : "Send"}
        </Button>
      </div>

      {state.status === "error" && (
        <div className="rounded-md border border-dashed border-destructive/40 p-3 text-sm text-destructive">
          {state.message} — is the API running? (<code className="font-mono text-xs">pnpm api run dev</code>)
        </div>
      )}

      {(state.status === "json" || state.status === "binary") && (
        <div className="flex flex-col gap-3">
          <div className="flex flex-wrap gap-x-4 gap-y-1 font-mono text-[11px] text-muted-foreground">
            {NOTABLE_HEADERS.filter((h) => state.headers[h] !== undefined).map((h) => (
              <span key={h}>
                <span className="text-foreground/70">{h}:</span> {state.headers[h]}
              </span>
            ))}
          </div>
          {state.status === "json" ? (
            <JsonViewer value={state.value} />
          ) : (
            <p className="rounded-md border border-border bg-muted/30 p-3 text-xs text-muted-foreground">
              Binary response ({state.headers["content-range"]?.split("/")[1] ?? "unknown"} bytes total) — body not decoded, headers
              shown above.
            </p>
          )}
        </div>
      )}
    </div>
  );
}
