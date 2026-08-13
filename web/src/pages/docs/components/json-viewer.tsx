import * as React from "react";

import { cn } from "@/lib/utils";

const ARRAY_CHUNK = 20;
/** Objects rarely get large in this API (population buckets, stats values top out around 17 keys) — arrays (children, cells) are the ones that blow up, so only arrays get chunked truncation. */
const COLLAPSE_ARRAY_THRESHOLD = 8;

function isPlainObject(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function Punc({ children }: { children: React.ReactNode }) {
  return <span className="text-json-punc">{children}</span>;
}

function PrimitiveValue({ value }: { value: unknown }) {
  if (value === null) return <span className="text-json-bool">null</span>;
  if (typeof value === "string") return <span className="text-json-str">&quot;{value}&quot;</span>;
  if (typeof value === "number") return <span className="tabular-nums text-json-num">{value.toLocaleString("en-US", { maximumFractionDigits: 6, useGrouping: false })}</span>;
  if (typeof value === "boolean") return <span className="text-json-bool">{String(value)}</span>;
  return <span>{String(value)}</span>;
}

/** One key/value row (or bare array item) — objects/arrays recurse into JsonNode, indented. */
function JsonEntry({ label, value, path }: { label: string | null; value: unknown; path: string }) {
  const isContainer = isPlainObject(value) || Array.isArray(value);
  return (
    <div className="flex gap-1">
      {label !== null && (
        <span className="shrink-0">
          <span className="text-json-key">&quot;{label}&quot;</span>
          <Punc>: </Punc>
        </span>
      )}
      {isContainer ? <JsonNode value={value} path={path} /> : <PrimitiveValue value={value} />}
    </div>
  );
}

/** Renders an object or array node, recursively. Collapsible via the caret; long arrays chunk-load via "… N more". */
function JsonNode({ value, path }: { value: unknown; path: string }) {
  const [collapsed, setCollapsed] = React.useState(false);
  const [visibleCount, setVisibleCount] = React.useState(ARRAY_CHUNK);

  if (Array.isArray(value)) {
    if (value.length === 0) return <Punc>[]</Punc>;
    if (collapsed) {
      return (
        <button type="button" onClick={() => setCollapsed(false)} className="inline-flex items-center gap-1 text-json-punc hover:text-code-ink">
          <span className="inline-block w-3 select-none">▸</span>
          <Punc>[</Punc>
          <span className="italic text-code-ink3">
            {value.length} item{value.length === 1 ? "" : "s"}
          </span>
          <Punc>]</Punc>
        </button>
      );
    }
    const shown = value.slice(0, visibleCount);
    const remaining = value.length - shown.length;
    return (
      <div className="flex flex-col">
        <div className="flex items-baseline gap-1">
          <button type="button" onClick={() => setCollapsed(true)} className="text-json-punc hover:text-code-ink" aria-label="Collapse array">
            <span className="inline-block w-3 select-none">▾</span>
            <Punc>[</Punc>
          </button>
          {value.length > COLLAPSE_ARRAY_THRESHOLD && <span className="text-[10px] text-code-ink3">{value.length} items</span>}
        </div>
        <div className="flex flex-col gap-0.5 border-l border-border/70 pl-4">
          {shown.map((item, i) => (
            <JsonEntry key={i} label={null} value={item} path={`${path}.${i}`} />
          ))}
        </div>
        {remaining > 0 && (
          <button type="button" onClick={() => setVisibleCount((n) => n + ARRAY_CHUNK)} className="w-fit pl-4 text-json-punc hover:text-code-ink hover:underline">
            … {remaining} more
          </button>
        )}
        <Punc>]</Punc>
      </div>
    );
  }

  if (isPlainObject(value)) {
    const keys = Object.keys(value);
    if (keys.length === 0) return <Punc>{"{}"}</Punc>;
    if (collapsed) {
      return (
        <button type="button" onClick={() => setCollapsed(false)} className="inline-flex items-center gap-1 text-json-punc hover:text-code-ink">
          <span className="inline-block w-3 select-none">▸</span>
          <Punc>{"{"}</Punc>
          <span className="italic text-code-ink3">
            {keys.length} key{keys.length === 1 ? "" : "s"}
          </span>
          <Punc>{"}"}</Punc>
        </button>
      );
    }
    return (
      <div className="flex flex-col">
        <button type="button" onClick={() => setCollapsed(true)} className="w-fit text-json-punc hover:text-code-ink" aria-label="Collapse object">
          <span className="inline-block w-3 select-none">▾</span>
          <Punc>{"{"}</Punc>
        </button>
        <div className="flex flex-col gap-0.5 border-l border-border/70 pl-4">
          {keys.map((key) => (
            <JsonEntry key={key} label={key} value={value[key]} path={`${path}.${key}`} />
          ))}
        </div>
        <Punc>{"}"}</Punc>
      </div>
    );
  }

  return <PrimitiveValue value={value} />;
}

/**
 * Pretty-prints a parsed JSON value with the design's exact syntax colors
 * (key #8AB4F8, string #6BD9A5, number #F0B15C, bool/null #E5537A, punctuation
 * #5B6875 — see index.css's --json-* tokens, identical in both themes since
 * this panel's background stays --code-bg either way), collapsible
 * objects/arrays, and chunked array truncation ("… N more"). Not a
 * <pre>/JSON.stringify dump — long arrays (population/grid cells, admin
 * children) would otherwise flood the DOM.
 */
export function JsonViewer({ value, className }: { value: unknown; className?: string }) {
  return (
    <div className={cn("overflow-x-auto rounded-md bg-code-bg p-3 font-mono text-[12px] leading-relaxed text-code-ink", className)}>
      <JsonEntry label={null} value={value} path="root" />
    </div>
  );
}
