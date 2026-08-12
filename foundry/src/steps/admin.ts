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
  level: 0 | 1 | 2 | 3 | 4;
  name_en: string;
  name_si: string | null;
  name_ta: string | null;
  parent_pcode: string | null;
  area_km2: number | null;
  centroid_lat: number | null;
  centroid_lon: number | null;
}

/** COD-AB ADM3/ADM4 feature properties (see foundry/data/raw/cod-ab/lka_admin{3,4}.geojson). */
interface CodAbProperties {
  adm3_name: string;
  adm3_name1: string | null; // si
  adm3_name2: string | null; // ta
  adm3_pcode: string;
  adm2_pcode: string;
  adm4_name?: string;
  adm4_name1?: string | null;
  adm4_name2?: string | null;
  adm4_pcode?: string;
  adm3_pcode_ref?: string;
  area_sqkm: number;
  center_lat: number;
  center_lon: number;
}
interface CodAbFeature {
  properties: CodAbProperties;
}
interface CodAbFeatureCollection {
  features: CodAbFeature[];
}

/**
 * Builds admin_units (levels 0-4).
 *
 * Levels 0-2 (country/provinces/districts) come from geoBoundaries, as
 * before — their p-codes (LK, LK1..LK9, LK11..LK92) already agree with
 * OCHA COD-AB's adm1_pcode/adm2_pcode (verified against the ADM3 file: every
 * districts.geojson `pcode` appears as some feature's `adm2_pcode`), so
 * there's no reason to re-source them.
 *
 * Levels 3-4 (DS divisions, GN divisions) come from OCHA COD-AB
 * (`fetch-gnd`, data/raw/cod-ab/lka_admin{3,4}.geojson) — the official
 * p-code source geoBoundaries never had for these levels. This replaces the
 * old `GB:<geoBoundaries id>` interim scheme entirely (see meta.pcode_scheme
 * below); COD-AB also ships area_sqkm/center_lat/center_lon per feature
 * (official Survey Department figures), which is more accurate than this
 * pipeline's own bbox-center/area approximation, so levels 3-4 use those
 * directly instead of computing from geometry.
 */
export async function run({ db, log }: StepContext): Promise<void> {
  const country = await readJson<FeatureCollection>("geo/country.geojson");
  const provinces = await readJson<FeatureCollection>("geo/provinces.geojson");
  const districts = await readJson<FeatureCollection>("geo/districts.geojson");
  const dsDivisions = await readJson<CodAbFeatureCollection>("cod-ab/lka_admin3.geojson");
  const gnDivisions = await readJson<CodAbFeatureCollection>("cod-ab/lka_admin4.geojson");
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

  // Level 3: DS divisions, official COD-AB p-codes (e.g. LK1103). parent_pcode
  // is COD-AB's own adm2_pcode — no id-lookup needed, and it already matches
  // this build's level-2 pcodes (see the module doc comment).
  const districtPcodes = new Set(districts.features.map((f) => String(f.properties.pcode)));
  for (const f of dsDivisions.features) {
    const p = f.properties;
    if (!districtPcodes.has(p.adm2_pcode)) {
      throw new Error(`admin: DS division ${p.adm3_pcode} has unresolved parent district ${p.adm2_pcode}`);
    }
    rows.push({
      pcode: p.adm3_pcode,
      level: 3,
      name_en: p.adm3_name,
      name_si: p.adm3_name1,
      name_ta: p.adm3_name2,
      parent_pcode: p.adm2_pcode,
      area_km2: p.area_sqkm,
      centroid_lat: p.center_lat,
      centroid_lon: p.center_lon,
    });
  }

  // Level 4: GN divisions, official COD-AB p-codes (e.g. LK1103005).
  // parent_pcode is COD-AB's own adm3_pcode — always one of the level-3 rows
  // just pushed above (both files come from the same COD-AB release).
  const dsPcodes = new Set(dsDivisions.features.map((f) => f.properties.adm3_pcode));
  for (const f of gnDivisions.features) {
    const p = f.properties;
    const pcode = p.adm4_pcode;
    const parentPcode = p.adm3_pcode;
    if (!pcode) throw new Error("admin: GN division feature missing adm4_pcode");
    if (!dsPcodes.has(parentPcode)) {
      throw new Error(`admin: GN division ${pcode} has unresolved parent DS division ${parentPcode}`);
    }
    rows.push({
      pcode,
      level: 4,
      name_en: p.adm4_name ?? "",
      name_si: p.adm4_name1 ?? null,
      name_ta: p.adm4_name2 ?? null,
      parent_pcode: parentPcode,
      area_km2: p.area_sqkm,
      centroid_lat: p.center_lat,
      centroid_lon: p.center_lon,
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

  // admin_geometry.pcode has a FOREIGN KEY REFERENCES admin_units(pcode), and
  // this connection enforces foreign keys — so on a re-run (once
  // admin-geometry has already populated rows for these exact pcodes),
  // deleting a level 3/4 admin_units row would be rejected immediately even
  // though the very next statement reinserts the same pcode. defer_foreign_keys
  // postpones the check to COMMIT, where it passes because the final row set
  // is unchanged (or correctly fails if upstream COD-AB data actually dropped
  // a pcode that admin_geometry still references — a real problem, not a
  // false positive). It auto-resets to OFF at the end of the transaction, so
  // it doesn't leak into other steps sharing this connection.
  db.pragma("defer_foreign_keys = ON");
  const tx = db.transaction(() => {
    // Levels 3-4 are fully replaced every run (official COD-AB p-codes now,
    // not upserted against a stable key that predates this rework) — clear
    // both before reinserting so a build that ran against the old
    // GB:-prefixed interim scheme doesn't leave orphaned rows behind.
    // Level 4 first: it has no children, but keeping delete order the
    // mirror of insert order (3 before 4) is one less thing to reason about.
    db.prepare(`DELETE FROM admin_units WHERE level = 4`).run();
    db.prepare(`DELETE FROM admin_units WHERE level = 3`).run();
    for (const row of rows) insert.run(row);

    insertMeta.run("pcode_scheme", "cod-ab-v1");
    db.prepare(`DELETE FROM meta WHERE key = 'adm3_pcode_interim'`).run();
  });
  tx();

  const dsCount = dsDivisions.features.length;
  const gnCount = gnDivisions.features.length;
  log(
    `admin: upserted ${rows.length} admin_units (1 country, ${provinces.features.length} provinces, ` +
      `${districts.features.length} districts, ${dsCount} DS divisions, ${gnCount} GN divisions) — pcode_scheme=cod-ab-v1`,
  );
}
