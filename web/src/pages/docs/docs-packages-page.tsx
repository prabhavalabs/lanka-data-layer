import { DATA_VERSION as ADMIN_DATA_VERSION, SOURCES as ADMIN_SOURCES, UNIT_COUNT } from "@lanka-data-layer/admin";
import { DATA_VERSION as CENSUS_DATA_VERSION, SOURCES as CENSUS_SOURCES, CENSUS_YEAR, coveredPcodes } from "@lanka-data-layer/census";
import { DATA_VERSION as ELECTORAL_DATA_VERSION, SOURCES as ELECTORAL_SOURCES, listElections, listEntities } from "@lanka-data-layer/electoral";
import { CODE_COUNT, DATA_VERSION as POSTAL_DATA_VERSION, SOURCES as POSTAL_SOURCES } from "@lanka-data-layer/postal";

import { AdminDemo } from "@/pages/docs/components/packages/admin-demo";
import { CensusDemo } from "@/pages/docs/components/packages/census-demo";
import { ElectoralDemo } from "@/pages/docs/components/packages/electoral-demo";
import { PackageSection } from "@/pages/docs/components/packages/package-section";
import { PostalDemo } from "@/pages/docs/components/packages/postal-demo";

const NPM_ORG_URL = "https://www.npmjs.com/org/lanka-data-layer";

const SECTIONS = [
  { id: "admin", pkg: "@lanka-data-layer/admin", label: "admin" },
  { id: "postal", pkg: "@lanka-data-layer/postal", label: "postal" },
  { id: "electoral", pkg: "@lanka-data-layer/electoral", label: "electoral" },
  { id: "census", pkg: "@lanka-data-layer/census", label: "census" },
] as const;

const ADMIN_SNIPPET = `import { getUnit, getParentChain, searchByName } from "@lanka-data-layer/admin";

searchByName("colombo", { level: 3, limit: 5 });
// DS divisions named "Colombo" — ranked exact > prefix > substring,
// matched across English, Sinhala and Tamil names

getUnit("LK1103");
// { pcode: "LK1103", level: 3, name: "Colombo", nameSi: "කොළඹ",
//   nameTa: "கொழும்பு", parentPcode: "LK11", areaKm2: 24.54,
//   centroid: { lat: 6.947, lon: 79.865 } }

getParentChain("LK1103");
// [Sri Lanka, Western Province, Colombo District] — root first`;

const POSTAL_SNIPPET = `import { getPostalCode, searchOffices, nearestPostalCode } from "@lanka-data-layer/postal";

getPostalCode("10500");
// { code: "10500", name: "Padukka", adminPcode: "LK11",
//   location: { lat: 6.8408, lon: 80.0897 } }

searchOffices("Padukka", { limit: 5 });

nearestPostalCode(6.9271, 79.8612, { limit: 5 });
// closest first, haversine kilometers rounded to 2dp`;

const ELECTORAL_SNIPPET = `import { listElections, getResult } from "@lanka-data-layer/electoral";

listElections();
// [{ id: "parl-2024", type: "parliamentary", year: 2024, ... }, ...]

getResult("parl-2024", "LK");
// { electors, polled, valid, turnout: 0.7024,
//   winner: { party: "NPP", votes: 6863186, share: 0.6221 },
//   parties: [{ party: "NPP", votes: 6863186, share: 0.6221 }, ...] }`;

const CENSUS_SNIPPET = `import { demographics } from "@lanka-data-layer/census";

demographics("LK11"); // Colombo District
// {
//   population: { total: 2375415, male: 1154799, female: 1220616, sexRatio: 94.6 },
//   age: { buckets: [...], dependencyRatio: 43 },
//   ethnicity: [{ key: "sinhala", count: 1807945, share: 0.7611 }, ...],
//   religion: [...],
//   change2012: { ethnicity: [...], religion: [...] },
// }`;

function sourceNames(sources: { name: string }[]): string {
  return sources.map((s) => s.name).join(", ");
}

/**
 * /docs/packages — showcases the four published npm libraries
 * (@lanka-data-layer/{admin,postal,electoral,census}), each with its own
 * install command, a short usage snippet, and a fully client-side demo that
 * imports and runs the real package in the visitor's browser. Route-level
 * lazy (see App.tsx) so the ~2.4MB of inlined data these four packages
 * carry never lands in the main bundle — everything imported from this
 * module, directly or transitively, belongs to this one lazy chunk.
 */
export default function DocsPackagesPage() {
  const elections = listElections();
  const entities = listEntities();
  const censusPcodes = coveredPcodes();

  return (
    <div className="h-full min-w-0 overflow-y-auto">
      <div className="mx-auto max-w-[820px] animate-fade-up px-6 py-10 pb-16 sm:px-11 sm:py-12">
        <div className="mb-2.5 font-mono text-[11px] text-ink3">Libraries</div>
        <h1 className="mb-2.5 text-[27px] font-bold tracking-[-0.02em]">The same data, as npm packages</h1>
        <p className="mb-5 max-w-[68ch] text-[14.5px] leading-[1.6] text-ink2">
          Four small, zero-dependency npm packages carry the same admin, postal, electoral and census data this API serves —
          inlined at build time, no network calls, no API keys. They run anywhere: Node 18+, the browser, or an edge runtime.
          Browse every release on the{" "}
          <a href={NPM_ORG_URL} target="_blank" rel="noreferrer" className="text-brand hover:text-brand2 hover:underline">
            npm org page ↗
          </a>
          .
        </p>

        <nav className="mb-10 flex flex-wrap gap-2" aria-label="Jump to a package">
          {SECTIONS.map((s) => (
            <a
              key={s.id}
              href={`#${s.id}`}
              className="rounded-full border border-border bg-bg2 px-3 py-1 font-mono text-[11.5px] text-ink2 transition-colors hover:border-brand hover:text-ink"
            >
              {s.label}
            </a>
          ))}
        </nav>

        <div className="flex flex-col">
          <PackageSection
            id="admin"
            pkg="@lanka-data-layer/admin"
            pitch="Sri Lanka's administrative hierarchy — province, district, DS division, GN division — trilingual names, p-code keyed."
            facts={[`data ${ADMIN_DATA_VERSION}`, `${UNIT_COUNT.toLocaleString()} units`, sourceNames(ADMIN_SOURCES)]}
            snippet={ADMIN_SNIPPET}
            demo={<AdminDemo />}
          />

          <PackageSection
            id="postal"
            pkg="@lanka-data-layer/postal"
            pitch="Every Sri Lankan postal code, cross-referenced to admin p-codes — exact lookup, name search, and nearest-code by coordinate."
            facts={[`data ${POSTAL_DATA_VERSION}`, `${CODE_COUNT.toLocaleString()} codes`, sourceNames(POSTAL_SOURCES)]}
            snippet={POSTAL_SNIPPET}
            demo={<PostalDemo />}
          />

          <PackageSection
            id="electoral"
            pkg="@lanka-data-layer/electoral"
            pitch="2015-2024 presidential and 2024 parliamentary results, by electoral district, polling division and postal vote — turnout, winners, party shares."
            facts={[
              `data ${ELECTORAL_DATA_VERSION}`,
              `${elections.length} elections`,
              `${entities.length.toLocaleString()} entities`,
              sourceNames(ELECTORAL_SOURCES),
            ]}
            snippet={ELECTORAL_SNIPPET}
            demo={<ElectoralDemo />}
          />

          <PackageSection
            id="census"
            pkg="@lanka-data-layer/census"
            pitch="2024 census demographics — population, age structure, ethnicity, religion — for the country, every district and DS division, with a 2012 comparison."
            facts={[
              `data ${CENSUS_DATA_VERSION}`,
              `census year ${CENSUS_YEAR}`,
              `${censusPcodes.length.toLocaleString()} units`,
              sourceNames(CENSUS_SOURCES),
            ]}
            snippet={CENSUS_SNIPPET}
            demo={<CensusDemo />}
          />
        </div>
      </div>
    </div>
  );
}
