import * as React from "react";

import { cn } from "@/lib/utils";

const ARRAY_CHUNK = 20;
/** Objects rarely get large in this API (population buckets, stats values top out around 17 keys) — arrays (children, cells) are the ones that blow up, so only arrays get chunked truncation. */
const COLLAPSE_ARRAY_THRESHOLD = 8;

function isPlainObject(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function Bracket({ children }: { children: React.ReactNode }) {
  return <span className="text-muted-foreground">{children}</span>;
}

function PrimitiveValue({ value }: { value: unknown }) {
  if (value === null) return <span className="text-muted-foreground">null</span>;
  if (typeof value === "string") return <span className="text-emerald-600 dark:text-emerald-400">&quot;{value}&quot;</span>;
  if (typeof value === "number") return <span className="text-sky-600 dark:text-sky-400">{value}</span>;
  if (typeof value === "boolean") return <span className="text-amber-600 dark:text-amber-400">{String(value)}</span>;
  return <span>{String(value)}</span>;
}

/** One key/value row (or bare array item) — objects/arrays recurse into JsonNode, indented. */
function JsonEntry({ label, value }: { label: string | null; value: unknown }) {
  const isContainer = isPlainObject(value) || Array.isArray(value);
  return (
    <div className="flex gap-1.5">
      {label !== null && <span className="shrink-0 text-foreground/80">&quot;{label}&quot;:</span>}
      {isContainer ? <JsonNode value={value} /> : <PrimitiveValue value={value} />}
    </div>
  );
}

/** Renders an object or array node, recursively. Collapsible via the bracket; long arrays chunk-load via "… N more". */
function JsonNode({ value }: { value: unknown }) {
  const [collapsed, setCollapsed] = React.useState(false);
  const [visibleCount, setVisibleCount] = React.useState(ARRAY_CHUNK);

  if (Array.isArray(value)) {
    if (value.length === 0) return <Bracket>[]</Bracket>;
    if (collapsed) {
      return (
        <button
          type="button"
          onClick={() => setCollapsed(false)}
          className="rounded text-muted-foreground hover:text-foreground hover:underline"
        >
          [ … {value.length} item{value.length === 1 ? "" : "s"} ]
        </button>
      );
    }
    const shown = value.slice(0, visibleCount);
    const remaining = value.length - shown.length;
    return (
      <div className="flex flex-col">
        <div className="flex items-baseline gap-1">
          <button type="button" onClick={() => setCollapsed(true)} className="text-muted-foreground hover:text-foreground" aria-label="Collapse array">
            [
          </button>
          {value.length > COLLAPSE_ARRAY_THRESHOLD && (
            <span className="text-[10px] text-muted-foreground">{value.length} items</span>
          )}
        </div>
        <div className="flex flex-col gap-0.5 border-l border-border/60 pl-3">
          {shown.map((item, i) => (
            <JsonEntry key={i} label={null} value={item} />
          ))}
        </div>
        {remaining > 0 && (
          <button
            type="button"
            onClick={() => setVisibleCount((n) => n + ARRAY_CHUNK)}
            className="w-fit rounded pl-3 text-muted-foreground hover:text-foreground hover:underline"
          >
            … {remaining} more
          </button>
        )}
        <Bracket>]</Bracket>
      </div>
    );
  }

  if (isPlainObject(value)) {
    const keys = Object.keys(value);
    if (keys.length === 0) return <Bracket>{"{}"}</Bracket>;
    if (collapsed) {
      return (
        <button
          type="button"
          onClick={() => setCollapsed(false)}
          className="rounded text-muted-foreground hover:text-foreground hover:underline"
        >
          {"{ … "}
          {keys.length} key{keys.length === 1 ? "" : "s"} {" }"}
        </button>
      );
    }
    return (
      <div className="flex flex-col">
        <button type="button" onClick={() => setCollapsed(true)} className="w-fit text-muted-foreground hover:text-foreground" aria-label="Collapse object">
          {"{"}
        </button>
        <div className="flex flex-col gap-0.5 border-l border-border/60 pl-3">
          {keys.map((key) => (
            <JsonEntry key={key} label={key} value={value[key]} />
          ))}
        </div>
        <Bracket>{"}"}</Bracket>
      </div>
    );
  }

  return <PrimitiveValue value={value} />;
}

/** Pretty-prints a parsed JSON value with collapsible objects/arrays and chunked array truncation ("… N more"). Not a <pre>/JSON.stringify dump — long arrays (population/grid cells, admin children) would otherwise flood the DOM. */
export function JsonViewer({ value, className }: { value: unknown; className?: string }) {
  return (
    <div className={cn("overflow-x-auto rounded-md border border-border bg-muted/30 p-3 font-mono text-xs leading-relaxed", className)}>
      <JsonEntry label={null} value={value} />
    </div>
  );
}
