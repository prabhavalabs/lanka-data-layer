import type { DocsParam } from "@/lib/docs-spec";

function ParamKindLabel({ param }: { param: DocsParam }) {
  if (param.kind === "enum" && param.enumValues) {
    return <>{param.enumValues.join(" | ")}</>;
  }
  return <>{param.kind}</>;
}

/**
 * Static reference table for one endpoint's params, matching the design's
 * four-column layout (Name/Type/Default/Constraints — "required" is a small
 * inline badge next to the name rather than its own column, and "location"
 * — path vs query — is called out in the caller's paramNote sentence
 * instead of a fifth column, per the design). Separate from the Try panel's
 * editable inputs (try-panel.tsx).
 */
export function ParamTable({ params }: { params: DocsParam[] }) {
  if (params.length === 0) {
    return <p className="text-[13px] text-ink2">This endpoint takes no parameters.</p>;
  }

  return (
    <div className="overflow-hidden overflow-x-auto rounded-[10px] border border-border">
      <div className="grid min-w-[520px] grid-cols-[1.1fr_0.7fr_0.7fr_1.6fr] gap-0 border-b border-border bg-bg2 px-3.5 py-2 text-[10.5px] font-semibold uppercase tracking-[0.07em] text-ink3">
        <span>Name</span>
        <span>Type</span>
        <span>Default</span>
        <span>Constraints</span>
      </div>
      {params.map((param) => (
        <div
          key={param.name}
          className="grid min-w-[520px] grid-cols-[1.1fr_0.7fr_0.7fr_1.6fr] items-baseline border-b border-border px-3.5 py-2.5 text-[12.5px] last:border-b-0 hover:bg-bg2"
        >
          <span className="flex items-baseline gap-1.5">
            <span className="font-mono font-semibold text-ink">{param.name}</span>
            {param.required && <span className="text-[9.5px] font-semibold text-brand">required</span>}
          </span>
          <span className="font-mono text-[12px] text-ink2">
            <ParamKindLabel param={param} />
          </span>
          <span className="font-mono text-[12px] text-ink3">{param.defaultValue ?? "—"}</span>
          <span className="text-ink2">{param.constraints ?? param.description}</span>
        </div>
      ))}
    </div>
  );
}
