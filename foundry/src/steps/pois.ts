import { readFile } from "node:fs/promises";
import type { StepContext } from "../step.ts";
import { rawPath } from "../lib/paths.ts";

export const name = "pois";

interface PoiFeature {
  properties: {
    name: string | null;
    nameSi: string | null;
    nameTa: string | null;
    kind: string | null;
    [extra: string]: unknown;
  };
  geometry: { type: "Point"; coordinates: [number, number] };
}
interface FeatureCollection {
  features: PoiFeature[];
}

const SOURCES: { file: string; category: string }[] = [
  { file: "geo/hospitals.geojson", category: "health" },
  { file: "geo/education.geojson", category: "education" },
  { file: "geo/airports.geojson", category: "transport" },
];

// Columns already carry these — everything else in `properties` goes into attrs JSON.
const MAPPED_PROPS = new Set(["name", "nameSi", "nameTa", "kind"]);

/**
 * pois: hospitals/clinics (health), schools/colleges/universities
 * (education), aerodromes (transport) — from OSM extracts. Extra
 * source-specific fields (operator, beds, iata, icao, military, …) are
 * preserved verbatim in `attrs` as JSON rather than growing the column set.
 */
export async function run({ db, log }: StepContext): Promise<void> {
  const collections = await Promise.all(
    SOURCES.map(async ({ file, category }) => ({
      category,
      data: JSON.parse(await readFile(rawPath(file), "utf8")) as FeatureCollection,
    })),
  );

  const insert = db.prepare(`
    INSERT INTO pois (category, kind, name_en, name_si, name_ta, lat, lon, admin_pcode, attrs)
    VALUES (@category, @kind, @name_en, @name_si, @name_ta, @lat, @lon, NULL, @attrs)
  `);

  let total = 0;
  const tx = db.transaction(() => {
    // pois is built entirely by this step (all categories) — clear it
    // first so re-runs don't accumulate duplicates.
    db.prepare(`DELETE FROM pois`).run();
    for (const { category, data } of collections) {
      for (const f of data.features) {
        const attrs: Record<string, unknown> = {};
        for (const [key, value] of Object.entries(f.properties)) {
          if (!MAPPED_PROPS.has(key)) attrs[key] = value;
        }
        insert.run({
          category,
          kind: f.properties.kind,
          name_en: f.properties.name,
          name_si: f.properties.nameSi,
          name_ta: f.properties.nameTa,
          lat: f.geometry.coordinates[1],
          lon: f.geometry.coordinates[0],
          attrs: JSON.stringify(attrs),
        });
        total++;
      }
    }
  });
  tx();

  log(`pois: upserted ${total} pois across ${SOURCES.length} categories`);
}
