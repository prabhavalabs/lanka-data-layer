import * as React from "react";
import { getChildren, getParentChain, searchByName, type AdminUnit } from "@lanka-data-layer/admin";

import { DemoInput, EmptyHint, ResultRow, Stat } from "@/pages/docs/components/packages/shared";

const LEVEL_LABEL: Record<number, string> = {
  0: "Country",
  1: "Province",
  2: "District",
  3: "DS Division",
  4: "GN Division",
};

const DEBOUNCE_MS = 200;
const DEFAULT_QUERY = "colombo";

function formatCentroid(unit: AdminUnit): string {
  if (!unit.centroid) return "—";
  return `${unit.centroid.lat.toFixed(4)}, ${unit.centroid.lon.toFixed(4)}`;
}

/**
 * Live `@lanka-data-layer/admin` demo: debounced searchByName() as you type,
 * click a result to see its detail — trilingual names, hierarchy chain via
 * getParentChain(), area, centroid. Every call runs synchronously against
 * the package's in-memory index; nothing here touches the network.
 */
export function AdminDemo() {
  const [query, setQuery] = React.useState(DEFAULT_QUERY);
  const [results, setResults] = React.useState<AdminUnit[]>(() => searchByName(DEFAULT_QUERY, { limit: 8 }));
  const [selected, setSelected] = React.useState<AdminUnit | null>(() => results[0] ?? null);

  React.useEffect(() => {
    const trimmed = query.trim();
    if (trimmed.length < 2) {
      setResults([]);
      return;
    }
    const timer = window.setTimeout(() => {
      setResults(searchByName(trimmed, { limit: 8 }));
    }, DEBOUNCE_MS);
    return () => window.clearTimeout(timer);
  }, [query]);

  const chain = selected ? getParentChain(selected.pcode) : [];

  return (
    <div className="grid grid-cols-1 gap-4 md:grid-cols-[220px_1fr]">
      <div className="flex flex-col gap-2">
        <DemoInput
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder="Search a name, e.g. Kandy"
          aria-label="Search admin units by name"
        />
        <div className="flex flex-col gap-px">
          {results.length === 0 && <EmptyHint>Type at least 2 characters — searches English, Sinhala and Tamil names.</EmptyHint>}
          {results.map((unit) => (
            <ResultRow
              key={unit.pcode}
              active={selected?.pcode === unit.pcode}
              onClick={() => setSelected(unit)}
              title={unit.name}
              meta={LEVEL_LABEL[unit.level]}
            />
          ))}
        </div>
      </div>

      <div className="min-w-0 rounded-lg bg-bg2 p-3.5">
        {!selected && <EmptyHint>Pick a result to see its detail.</EmptyHint>}
        {selected && (
          <div className="flex flex-col gap-3">
            <div>
              <div className="flex flex-wrap items-baseline gap-2">
                <span className="text-[15px] font-semibold text-ink">{selected.name}</span>
                <span className="font-mono text-[11px] text-ink3">{selected.pcode}</span>
              </div>
              <div className="mt-0.5 flex flex-wrap gap-x-3 font-mono text-[11.5px] text-ink2">
                {selected.nameSi && <span lang="si">{selected.nameSi}</span>}
                {selected.nameTa && <span lang="ta">{selected.nameTa}</span>}
                {!selected.nameSi && !selected.nameTa && <span className="text-ink3">No si/ta translation on record</span>}
              </div>
            </div>

            <div className="text-[11.5px] leading-[1.6] text-ink3">
              {chain.length > 0 ? (
                <>
                  {chain.map((c) => c.name).join(" › ")} › <span className="text-ink">{selected.name}</span>
                </>
              ) : (
                <span className="text-ink">Root of the hierarchy — no parents.</span>
              )}
            </div>

            <div className="grid grid-cols-2 gap-2 sm:grid-cols-4">
              <Stat label="Level" value={LEVEL_LABEL[selected.level]} />
              <Stat label="Area" value={selected.areaKm2 != null ? `${selected.areaKm2.toFixed(2)} km²` : "—"} />
              <Stat label="Centroid" value={formatCentroid(selected)} mono />
              <Stat label="Children" value={getChildren(selected.pcode).length.toLocaleString()} />
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
