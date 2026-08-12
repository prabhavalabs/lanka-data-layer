import { readFile } from "node:fs/promises";
import type { StepContext } from "../step.ts";
import { rawPath } from "../lib/paths.ts";
import type { FeatureCollection } from "../lib/feature-collection.ts";

export const name = "pois-extend";

interface StationProps {
  id: string;
  name: string | null;
  nameSi: string | null;
  nameTa: string | null;
  kind: "station" | "halt";
  [extra: string]: unknown;
}

const MAPPED_PROPS = new Set(["id", "name", "nameSi", "nameTa", "kind"]);
const STATION_KINDS = new Set(["station", "halt"]);

/**
 * pois-extend: adds railway stations/halts (Point features in
 * geo/railways.geojson, `kind` in {station, halt} — the LineString track
 * segments in the same file are not pois, they feed the `layers` step
 * instead) into `pois` with category='transport'.
 *
 * `pois` has no natural key (its id is a plain autoincrement rowid, see
 * pois.ts), so this step stays idempotent the same way places.ts does:
 * delete the subset it owns (category='transport' AND kind IN
 * ('station','halt') — disjoint from the existing 'aerodrome' transport
 * rows the `pois` step inserts) and reinsert, rather than trying to upsert
 * against a key that doesn't exist. Running `pois` again afterwards is
 * still safe: it unconditionally clears the whole table before its own
 * insert, so a full `pois,pois-extend` re-run never accumulates duplicates
 * either way.
 */
export async function run({ db, log }: StepContext): Promise<void> {
  const text = await readFile(rawPath("geo/railways.geojson"), "utf8");
  const data = JSON.parse(text) as FeatureCollection<StationProps>;
  const stations = data.features.filter(
    (f) => f.geometry.type === "Point" && STATION_KINDS.has(f.properties.kind),
  );

  const insert = db.prepare(`
    INSERT INTO pois (category, kind, name_en, name_si, name_ta, lat, lon, admin_pcode, attrs)
    VALUES ('transport', @kind, @name_en, @name_si, @name_ta, @lat, @lon, NULL, @attrs)
  `);

  const tx = db.transaction(() => {
    db.prepare(`DELETE FROM pois WHERE category = 'transport' AND kind IN ('station', 'halt')`).run();
    for (const f of stations) {
      const attrs: Record<string, unknown> = {};
      for (const [key, value] of Object.entries(f.properties)) {
        if (!MAPPED_PROPS.has(key)) attrs[key] = value;
      }
      insert.run({
        kind: f.properties.kind,
        name_en: f.properties.name,
        name_si: f.properties.nameSi,
        name_ta: f.properties.nameTa,
        lat: f.geometry.coordinates[1],
        lon: f.geometry.coordinates[0],
        attrs: JSON.stringify(attrs),
      });
    }
  });
  tx();

  log(`pois-extend: upserted ${stations.length} railway station/halt pois (category=transport)`);
}
