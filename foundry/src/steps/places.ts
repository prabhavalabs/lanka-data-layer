import { readFile } from "node:fs/promises";
import type { StepContext } from "../step.ts";
import { rawPath } from "../lib/paths.ts";

export const name = "places";

interface CityFeature {
  properties: {
    id: number;
    name: string;
    nameSi: string | null;
    nameTa: string | null;
    place: string;
    population: number | null;
  };
  geometry: { type: "Point"; coordinates: [number, number] };
}
interface FeatureCollection {
  features: CityFeature[];
}

/**
 * places: OSM-sourced named places (cities/towns/suburbs) from cities.geojson.
 * GeoNames place names (with postal-derived localities) are a planned
 * second source into this same table — not wired up yet, so `postal` only
 * fills postal_codes for now; see foundry/README.md.
 *
 * admin_pcode is left null: assigning each place to its containing admin
 * unit needs point-in-polygon (or the cell_lookup grid, itself pending
 * GN-division boundaries), out of scope for this build.
 */
export async function run({ db, log }: StepContext): Promise<void> {
  const cities = JSON.parse(await readFile(rawPath("geo/cities.geojson"), "utf8")) as FeatureCollection;

  const insert = db.prepare(`
    INSERT INTO places (source, source_id, kind, name_en, name_si, name_ta, lat, lon, population, admin_pcode)
    VALUES ('osm', @source_id, @kind, @name_en, @name_si, @name_ta, @lat, @lon, @population, NULL)
  `);

  const tx = db.transaction(() => {
    db.prepare(`DELETE FROM places WHERE source = 'osm'`).run();
    for (const f of cities.features) {
      insert.run({
        source_id: String(f.properties.id),
        kind: f.properties.place,
        name_en: f.properties.name,
        name_si: f.properties.nameSi,
        name_ta: f.properties.nameTa,
        lat: f.geometry.coordinates[1],
        lon: f.geometry.coordinates[0],
        population: f.properties.population,
      });
    }
    // places_fts is an external-content FTS5 index over places; rebuild
    // syncs it after the bulk delete+insert above.
    db.prepare(`INSERT INTO places_fts(places_fts) VALUES('rebuild')`).run();
  });
  tx();

  log(`places: upserted ${cities.features.length} places (source=osm) and rebuilt places_fts`);
}
