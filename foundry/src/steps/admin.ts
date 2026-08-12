import { readFile } from "node:fs/promises";
import type { StepContext } from "../step.ts";
import { rawPath } from "../lib/paths.ts";
import { bboxCenter } from "../lib/geo.ts";

export const name = "admin";

interface GeoFeature {
  id?: string;
  properties: Record<string, unknown>;
  geometry: { type: string; coordinates: unknown };
}
interface FeatureCollection {
  features: GeoFeature[];
}

interface PopUnit {
  pcode: string;
  nameSi?: string;
  nameTa?: string;
}

async function readJson<T>(relPath: string): Promise<T> {
  const text = await readFile(rawPath(relPath), "utf8");
  return JSON.parse(text) as T;
}

interface AdminRow {
  pcode: string;
  level: 0 | 1 | 2 | 3;
  name_en: string;
  name_si: string | null;
  name_ta: string | null;
  parent_pcode: string | null;
  area_km2: number | null;
  centroid_lat: number | null;
  centroid_lon: number | null;
}

/**
 * Builds admin_units (levels 0-3; level 4 GN divisions are out of scope —
 * see docs/architecture.md §2 note on cell_lookup). Levels 1-2 carry
 * official OCHA p-codes straight from the geoBoundaries GeoJSON. Level 3
 * (DS divisions) has no official p-code source yet, so each unit gets an
 * interim `GB:<geoBoundaries id>` pcode; the `meta` table records this so
 * consumers can detect and later re-key these rows without guessing.
 */
export async function run({ db, log }: StepContext): Promise<void> {
  const country = await readJson<FeatureCollection>("geo/country.geojson");
  const provinces = await readJson<FeatureCollection>("geo/provinces.geojson");
  const districts = await readJson<FeatureCollection>("geo/districts.geojson");
  const dsDivisions = await readJson<FeatureCollection>("geo/ds-divisions.geojson");
  const population = await readJson<{
    provinces: Record<string, PopUnit>;
    districts: Record<string, PopUnit>;
  }>("data/population-2023.json");

  // pcode -> {nameSi, nameTa}, sourced from population-2023.json (which is
  // itself keyed by name, not pcode — flatten to a pcode-keyed lookup).
  const siTaByPcode = new Map<string, { si?: string; ta?: string }>();
  for (const unit of [...Object.values(population.provinces), ...Object.values(population.districts)]) {
    siTaByPcode.set(unit.pcode, { si: unit.nameSi, ta: unit.nameTa });
  }

  const provinceIdToPcode = new Map<string, string>();
  for (const f of provinces.features) {
    provinceIdToPcode.set(String(f.id), String(f.properties.pcode));
  }
  const districtIdToPcode = new Map<string, string>();
  for (const f of districts.features) {
    districtIdToPcode.set(String(f.id), String(f.properties.pcode));
  }

  const rows: AdminRow[] = [];

  // Level 0: country.
  const countryFeature = country.features[0];
  if (!countryFeature) throw new Error("admin: country.geojson has no features");
  const totalArea = provinces.features.reduce((sum, f) => sum + (Number(f.properties.areaKm2) || 0), 0);
  const countryCentroid = bboxCenter(countryFeature.geometry as never);
  rows.push({
    pcode: "LK",
    level: 0,
    name_en: String(countryFeature.properties.name),
    name_si: null,
    name_ta: null,
    parent_pcode: null,
    area_km2: totalArea || null,
    centroid_lat: countryCentroid.lat,
    centroid_lon: countryCentroid.lon,
  });

  // Level 1: provinces.
  for (const f of provinces.features) {
    const pcode = String(f.properties.pcode);
    const c = bboxCenter(f.geometry as never);
    const names = siTaByPcode.get(pcode);
    rows.push({
      pcode,
      level: 1,
      name_en: String(f.properties.name),
      name_si: names?.si ?? null,
      name_ta: names?.ta ?? null,
      parent_pcode: "LK",
      area_km2: typeof f.properties.areaKm2 === "number" ? f.properties.areaKm2 : null,
      centroid_lat: c.lat,
      centroid_lon: c.lon,
    });
  }

  // Level 2: districts. parent_pcode via parentId -> province id -> province pcode.
  for (const f of districts.features) {
    const pcode = String(f.properties.pcode);
    const parentId = String(f.properties.parentId);
    const parentPcode = provinceIdToPcode.get(parentId);
    if (!parentPcode) throw new Error(`admin: district ${pcode} has unresolved parentId ${parentId}`);
    const c = bboxCenter(f.geometry as never);
    const names = siTaByPcode.get(pcode);
    rows.push({
      pcode,
      level: 2,
      name_en: String(f.properties.name),
      name_si: names?.si ?? null,
      name_ta: names?.ta ?? null,
      parent_pcode: parentPcode,
      area_km2: typeof f.properties.areaKm2 === "number" ? f.properties.areaKm2 : null,
      centroid_lat: c.lat,
      centroid_lon: c.lon,
    });
  }

  // Level 3: DS divisions. No official pcode — interim GB:<geoBoundaries id>.
  // parent_pcode via parentId -> district id -> district pcode.
  for (const f of dsDivisions.features) {
    const geoBoundariesId = String(f.id);
    const pcode = `GB:${geoBoundariesId}`;
    const parentId = String(f.properties.parentId);
    const parentPcode = districtIdToPcode.get(parentId);
    if (!parentPcode) throw new Error(`admin: DS division ${pcode} has unresolved parentId ${parentId}`);
    const c = bboxCenter(f.geometry as never);
    rows.push({
      pcode,
      level: 3,
      name_en: String(f.properties.name),
      name_si: null,
      name_ta: null,
      parent_pcode: parentPcode,
      area_km2: typeof f.properties.areaKm2 === "number" ? f.properties.areaKm2 : null,
      centroid_lat: c.lat,
      centroid_lon: c.lon,
    });
  }

  const insert = db.prepare(`
    INSERT INTO admin_units (pcode, level, name_en, name_si, name_ta, parent_pcode, area_km2, centroid_lat, centroid_lon)
    VALUES (@pcode, @level, @name_en, @name_si, @name_ta, @parent_pcode, @area_km2, @centroid_lat, @centroid_lon)
    ON CONFLICT(pcode) DO UPDATE SET
      level=excluded.level, name_en=excluded.name_en, name_si=excluded.name_si, name_ta=excluded.name_ta,
      parent_pcode=excluded.parent_pcode, area_km2=excluded.area_km2,
      centroid_lat=excluded.centroid_lat, centroid_lon=excluded.centroid_lon
  `);
  const insertMeta = db.prepare(`INSERT INTO meta (key, value) VALUES (?, ?) ON CONFLICT(key) DO UPDATE SET value=excluded.value`);

  const tx = db.transaction(() => {
    for (const row of rows) insert.run(row);
    insertMeta.run(
      "adm3_pcode_interim",
      "true — DS-division (level 3) pcodes are GB:<geoBoundaries id>, not an official OCHA p-code",
    );
  });
  tx();

  log(
    `admin: upserted ${rows.length} admin_units (1 country, ${provinces.features.length} provinces, ` +
      `${districts.features.length} districts, ${dsDivisions.features.length} DS divisions)`,
  );
}
