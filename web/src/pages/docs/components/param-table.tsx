import type { DocsParam } from "@/lib/docs-spec";

function ParamKindLabel({ param }: { param: DocsParam }) {
  if (param.kind === "enum" && param.enumValues) {
    return <>{param.enumValues.join(" | ")}</>;
  }
  return <>{param.kind}</>;
}

/** Static reference table for one endpoint's params — separate from the playground's editable inputs, which live in playground.tsx. */
export function ParamTable({ params }: { params: DocsParam[] }) {
  if (params.length === 0) {
    return <p className="text-sm text-muted-foreground">This endpoint takes no parameters.</p>;
  }

  return (
    <div className="overflow-x-auto rounded-md border border-border">
      <table className="w-full min-w-[640px] border-collapse text-sm">
        <thead>
          <tr className="border-b border-border bg-muted/40 text-left text-xs uppercase tracking-wide text-muted-foreground">
            <th className="px-3 py-2 font-medium">Name</th>
            <th className="px-3 py-2 font-medium">In</th>
            <th className="px-3 py-2 font-medium">Type</th>
            <th className="px-3 py-2 font-medium">Required</th>
            <th className="px-3 py-2 font-medium">Default</th>
            <th className="px-3 py-2 font-medium">Constraints</th>
            <th className="px-3 py-2 font-medium">Description</th>
          </tr>
        </thead>
        <tbody>
          {params.map((param) => (
            <tr key={param.name} className="border-b border-border last:border-b-0">
              <td className="whitespace-nowrap px-3 py-2 font-mono text-xs font-medium">{param.name}</td>
              <td className="whitespace-nowrap px-3 py-2 text-xs text-muted-foreground">{param.location}</td>
              <td className="whitespace-nowrap px-3 py-2 font-mono text-xs text-muted-foreground">
                <ParamKindLabel param={param} />
              </td>
              <td className="px-3 py-2 text-xs">
                {param.required ? (
                  <span className="rounded-full bg-accent px-2 py-0.5 font-medium text-accent-foreground">required</span>
                ) : (
                  <span className="text-muted-foreground">optional</span>
                )}
              </td>
              <td className="whitespace-nowrap px-3 py-2 font-mono text-xs text-muted-foreground">{param.defaultValue ?? "—"}</td>
              <td className="px-3 py-2 text-xs text-muted-foreground">{param.constraints ?? "—"}</td>
              <td className="px-3 py-2 text-xs">{param.description}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
