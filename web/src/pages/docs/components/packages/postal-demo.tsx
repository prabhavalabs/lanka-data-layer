import * as React from "react";
import { getPostalCode, nearestPostalCode, searchOffices, type PostalCode } from "@lanka-data-layer/postal";

import { DemoButton, DemoInput, EmptyHint, ResultRow } from "@/pages/docs/components/packages/shared";

const DEBOUNCE_MS = 200;
const DEFAULT_QUERY = "Padukka";
const DEFAULT_LAT = "6.9271";
const DEFAULT_LON = "79.8612";

/**
 * Live `@lanka-data-layer/postal` demo, two parts:
 *  1. A combined lookup — a full 5-digit code resolves exactly via
 *     getPostalCode(), any text also runs searchOffices() over office/place
 *     names (the package has no partial-code search, same real constraint
 *     the hosted /v1/lookup endpoint documents for postal matching).
 *  2. "Nearest to" — two lat/lon inputs (prefilled to Colombo Fort) feeding
 *     nearestPostalCode(), a plain haversine scan, closest first.
 */
export function PostalDemo() {
  const [query, setQuery] = React.useState(DEFAULT_QUERY);
  const [results, setResults] = React.useState<PostalCode[]>(() => searchOffices(DEFAULT_QUERY, { limit: 6 }));
  const [exact, setExact] = React.useState<PostalCode | null>(null);

  React.useEffect(() => {
    const trimmed = query.trim();
    if (!trimmed) {
      setResults([]);
      setExact(null);
      return;
    }
    const timer = window.setTimeout(() => {
      setResults(searchOffices(trimmed, { limit: 6 }));
      setExact(/^\d{5}$/.test(trimmed) ? getPostalCode(trimmed) : null);
    }, DEBOUNCE_MS);
    return () => window.clearTimeout(timer);
  }, [query]);

  const [lat, setLat] = React.useState(DEFAULT_LAT);
  const [lon, setLon] = React.useState(DEFAULT_LON);
  const [nearest, setNearest] = React.useState<(PostalCode & { distanceKm: number })[]>(() =>
    nearestPostalCode(Number(DEFAULT_LAT), Number(DEFAULT_LON), { limit: 5 })
  );

  function findNearest() {
    const la = Number(lat);
    const lo = Number(lon);
    if (Number.isFinite(la) && Number.isFinite(lo)) setNearest(nearestPostalCode(la, lo, { limit: 5 }));
  }

  return (
    <div className="flex flex-col gap-6">
      <div className="flex flex-col gap-2">
        <div className="text-[10.5px] font-semibold uppercase tracking-[0.08em] text-ink3">Code or office / area name</div>
        <DemoInput
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder="10500 or Padukka"
          aria-label="Search postal codes or offices"
        />
        <div className="flex flex-col gap-px">
          {exact && <ResultRow active onClick={() => setQuery(exact.code)} title={`${exact.code} · ${exact.name}`} meta="exact match" />}
          {results
            .filter((r) => r.code !== exact?.code)
            .map((r) => (
              <ResultRow key={r.code} onClick={() => setQuery(r.name)} title={`${r.code} · ${r.name}`} meta={r.adminPcode ?? undefined} />
            ))}
          {!exact && results.length === 0 && <EmptyHint>No offices match &quot;{query.trim()}&quot;. Try a full 5-digit code or a place name.</EmptyHint>}
        </div>
      </div>

      <div className="flex flex-col gap-2 border-t border-border pt-4">
        <div className="text-[10.5px] font-semibold uppercase tracking-[0.08em] text-ink3">Nearest to a coordinate</div>
        <div className="flex flex-wrap items-center gap-2">
          <DemoInput value={lat} onChange={(e) => setLat(e.target.value)} placeholder="lat" aria-label="Latitude" className="w-[110px]" />
          <DemoInput value={lon} onChange={(e) => setLon(e.target.value)} placeholder="lon" aria-label="Longitude" className="w-[110px]" />
          <DemoButton onClick={findNearest}>Find nearest</DemoButton>
        </div>
        <div className="flex flex-col gap-px">
          {nearest.map((r, i) => (
            <div key={r.code} className="flex items-center gap-2.5 rounded-md px-2.5 py-[7px] text-[12.5px] text-ink2">
              <span className="w-4 shrink-0 text-right font-mono text-[10px] text-ink3">{i + 1}</span>
              <span className="truncate text-ink">
                {r.code} · {r.name}
              </span>
              <span className="ml-auto shrink-0 font-mono text-[11px] tabular-nums text-ink3">{r.distanceKm.toFixed(2)} km</span>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}
