import * as React from "react";
import { searchByName, type AdminUnit } from "@lanka-data-layer/admin";
import { demographics } from "@lanka-data-layer/census";

import { DemoInput, EmptyHint, ResultRow, ShareBar, Stat } from "@/pages/docs/components/packages/shared";

const LEVEL_LABEL: Record<number, string> = { 0: "Country", 1: "Province", 2: "District", 3: "DS Division" };
const DEBOUNCE_MS = 200;
const DEFAULT_QUERY = "Colombo District";
/** Census coverage stops at DS divisions — level 4 (GN division) is excluded from search results entirely, matching the package's own null-for-uncovered-pcode behavior. */
const MAX_CENSUS_LEVEL = 3;

/**
 * Live `@lanka-data-layer/census` demo. The census package only exposes
 * pcode-keyed lookups, so name search is borrowed from
 * `@lanka-data-layer/admin`'s searchByName() — restricted here to levels
 * 0-3 (country/province/district/DS division) — then the resolved pcode
 * feeds demographics() for the full derived profile.
 */
export function CensusDemo() {
  const [query, setQuery] = React.useState(DEFAULT_QUERY);
  const [results, setResults] = React.useState<AdminUnit[]>(() =>
    searchByName(DEFAULT_QUERY, { limit: 20 }).filter((u) => u.level <= MAX_CENSUS_LEVEL).slice(0, 8)
  );
  const [selected, setSelected] = React.useState<AdminUnit | null>(() => results[0] ?? null);

  React.useEffect(() => {
    const trimmed = query.trim();
    if (trimmed.length < 2) {
      setResults([]);
      return;
    }
    const timer = window.setTimeout(() => {
      setResults(searchByName(trimmed, { limit: 20 }).filter((u) => u.level <= MAX_CENSUS_LEVEL).slice(0, 8));
    }, DEBOUNCE_MS);
    return () => window.clearTimeout(timer);
  }, [query]);

  const profile = selected ? demographics(selected.pcode) : null;

  return (
    <div className="grid grid-cols-1 gap-4 md:grid-cols-[220px_1fr]">
      <div className="flex flex-col gap-2">
        <DemoInput
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder="Country, province, district…"
          aria-label="Search census-covered units by name"
        />
        <div className="flex flex-col gap-px">
          {results.length === 0 && <EmptyHint>Search a country, province, district or DS division name.</EmptyHint>}
          {results.map((unit) => (
            <ResultRow
              key={unit.pcode}
              active={selected?.pcode === unit.pcode}
              onClick={() => setSelected(unit)}
              title={unit.name}
              meta={LEVEL_LABEL[unit.level] ?? unit.level}
            />
          ))}
        </div>
      </div>

      <div className="min-w-0 rounded-lg bg-bg2 p-3.5">
        {!selected && <EmptyHint>Pick a result to see its demographic profile.</EmptyHint>}
        {selected && !profile && (
          <EmptyHint>
            {selected.name} has no 2024 census row — provinces and GN divisions aren&rsquo;t census-covered (country, districts and DS
            divisions only).
          </EmptyHint>
        )}
        {selected && profile && (
          <div className="flex flex-col gap-3.5">
            <div className="flex flex-wrap items-baseline gap-2">
              <span className="text-[15px] font-semibold text-ink">{selected.name}</span>
              <span className="font-mono text-[11px] text-ink3">
                {profile.pcode} · {profile.censusYear}
              </span>
            </div>

            <div className="grid grid-cols-2 gap-2 sm:grid-cols-4">
              <Stat label="Total" value={profile.population.total.toLocaleString()} />
              <Stat label="Male" value={profile.population.male.toLocaleString()} />
              <Stat label="Female" value={profile.population.female.toLocaleString()} />
              <Stat label="Sex ratio" value={`${profile.population.sexRatio} m/100f`} />
            </div>

            <div>
              <div className="mb-1.5 text-[10.5px] font-semibold uppercase tracking-[0.08em] text-ink3">
                Age structure · dependency ratio {profile.age.dependencyRatio}
              </div>
              <div className="flex flex-col gap-1.5">
                {profile.age.buckets.map((b) => (
                  <ShareBar key={b.bucket} label={b.bucket} share={b.share} sublabel={b.count.toLocaleString()} />
                ))}
              </div>
            </div>

            <div>
              <div className="mb-1.5 text-[10.5px] font-semibold uppercase tracking-[0.08em] text-ink3">Top ethnicity</div>
              <div className="flex flex-col gap-1.5">
                {profile.ethnicity.slice(0, 3).map((e) => (
                  <ShareBar key={e.key} label={e.key} share={e.share} sublabel={e.count.toLocaleString()} />
                ))}
              </div>
            </div>

            <div>
              <div className="mb-1.5 text-[10.5px] font-semibold uppercase tracking-[0.08em] text-ink3">Top religion</div>
              <div className="flex flex-col gap-1.5">
                {profile.religion.slice(0, 3).map((r) => (
                  <ShareBar key={r.key} label={r.key} share={r.share} sublabel={r.count.toLocaleString()} />
                ))}
              </div>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
