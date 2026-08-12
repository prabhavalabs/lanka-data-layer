import { mkdir, readFile, writeFile } from "node:fs/promises";
import path from "node:path";
import type { StepContext } from "../step.ts";
import { artifactPath, rawPath } from "../lib/paths.ts";
import { roundCoordinates, roundGeometry, simplifyPolygonalGeometry } from "../lib/geojson.ts";
import type { PolygonalGeometry } from "../lib/pip.ts";

export const name = "admin-geometry";

const SIMPLIFY_TOLERANCE_DEG = 0.0005;
const COORD_DP = 6;

interface GeoFeature {
  properties: Record<string, unknown>;
  geometry: PolygonalGeometry;
}
interface FeatureCollection {
  features: GeoFeature[];
}
interface CodAbFeature {
  properties: {
    // ADM3 features: adm3_pcode is this feature's own pcode, adm2_pcode its
    // parent. ADM4 features: adm4_pcode is this feature's own pcode,
    // adm3_pcode its parent (COD-AB reuses the same "adm3_pcode" property
    // name to mean "this feature's DS division" on every level below it).
    adm3_pcode?: string;
    adm3_name?: string;
    adm2_pcode?: string;
    adm4_pcode?: string;
    adm4_name?: string;
  };
  geometry: PolygonalGeometry;
}
interface CodAbFeatureCollection {
  features: CodAbFeature[];
}

interface UnitGeometry {
  pcode: string;
  level: 1 | 2 | 3 | 4;
  name_en: string;
  parent_pcode: string | null;
  geometry: PolygonalGeometry;
}

async function readJson<T>(relPath: string): Promise<T> {
  return JSON.parse(await readFile(rawPath(relPath), "utf8")) as T;
}

/**
 * admin_geometry (docs/architecture.md §2): simplified boundary geometry for
 * levels 1-4, plus the full-detail layer GeoJSON `admin-adm{1..4}.geojson`
 * files those simplifications are derived from (data/artifacts/layers/, for
 * the later PMTiles tile build — same directory the `layers` step writes
 * roads/railways/etc. into, different filenames). Also copies
 * electoral-divisions/polling-divisions into the same directory at 6 dp —
 * they're already at their final (non-admin) resolution, just relocated
 * from data/raw/ to the artifact layers directory.
 *
 * Level 0 (country) is intentionally excluded — docs/architecture.md's
 * admin_geometry note and this build's row-count expectations both scope it
 * to levels 1-4.
 */
export async function run({ db, log }: StepContext): Promise<void> {
  const provinces = await readJson<FeatureCollection>("geo/provinces.geojson");
  const districts = await readJson<FeatureCollection>("geo/districts.geojson");
  const dsDivisions = await readJson<CodAbFeatureCollection>("cod-ab/lka_admin3.geojson");
  const gnDivisions = await readJson<CodAbFeatureCollection>("cod-ab/lka_admin4.geojson");

  // geoBoundaries features carry the same id both at the top level (feature.id)
  // and duplicated into properties.id — using properties.id here avoids an
  // extra cast, since GeoFeature's properties bag is already typed loosely.
  const provinceIdToPcode = new Map<string, string>();
  for (const f of provinces.features) {
    provinceIdToPcode.set(String(f.properties.id), String(f.properties.pcode));
  }

  const units: UnitGeometry[] = [];

  for (const f of provinces.features) {
    units.push({
      pcode: String(f.properties.pcode),
      level: 1,
      name_en: String(f.properties.name),
      parent_pcode: "LK",
      geometry: f.geometry,
    });
  }

  for (const f of districts.features) {
    const parentId = String(f.properties.parentId);
    const parentPcode = provinceIdToPcode.get(parentId) ?? null;
    units.push({
      pcode: String(f.properties.pcode),
      level: 2,
      name_en: String(f.properties.name),
      parent_pcode: parentPcode,
      geometry: f.geometry,
    });
  }

  for (const f of dsDivisions.features) {
    const p = f.properties;
    if (!p.adm3_pcode || !p.adm2_pcode || !p.adm3_name) continue;
    units.push({
      pcode: p.adm3_pcode,
      level: 3,
      name_en: p.adm3_name,
      parent_pcode: p.adm2_pcode,
      geometry: f.geometry,
    });
  }

  for (const f of gnDivisions.features) {
    const p = f.properties;
    if (!p.adm4_pcode || !p.adm3_pcode) continue;
    units.push({
      pcode: p.adm4_pcode,
      level: 4,
      name_en: p.adm4_name ?? "",
      parent_pcode: p.adm3_pcode,
      geometry: f.geometry,
    });
  }

  // --- admin_geometry: simplified + rounded, bare GeoJSON geometry string ---
  const insertGeom = db.prepare(`
    INSERT INTO admin_geometry (pcode, geojson) VALUES (@pcode, @geojson)
    ON CONFLICT(pcode) DO UPDATE SET geojson = excluded.geojson
  `);
  const tx = db.transaction(() => {
    for (const u of units) {
      const simplified = simplifyPolygonalGeometry(u.geometry, SIMPLIFY_TOLERANCE_DEG);
      const rounded = roundGeometry(simplified, COORD_DP);
      insertGeom.run({ pcode: u.pcode, geojson: JSON.stringify(rounded) });
    }
  });
  tx();
  log(`admin-geometry: upserted ${units.length} admin_geometry rows (levels 1-4, simplified ~${SIMPLIFY_TOLERANCE_DEG}deg)`);

  // --- full-detail per-level FeatureCollections for the tile build ---
  const layersDir = artifactPath("layers");
  await mkdir(layersDir, { recursive: true });

  for (const level of [1, 2, 3, 4] as const) {
    const levelUnits = units.filter((u) => u.level === level);
    const fc = {
      type: "FeatureCollection" as const,
      features: levelUnits.map((u) => ({
        type: "Feature" as const,
        properties: { pcode: u.pcode, name_en: u.name_en, level: u.level, parent_pcode: u.parent_pcode },
        geometry: roundGeometry(u.geometry, COORD_DP),
      })),
    };
    const destFile = `admin-adm${level}.geojson`;
    await writeFile(path.join(layersDir, destFile), JSON.stringify(fc), "utf8");
    log(`admin-geometry: wrote ${destFile} (${fc.features.length} features, full detail)`);
  }

  // --- electoral/polling division GeoJSON, copied + rounded into layers/ ---
  for (const src of ["electoral-divisions.geojson", "polling-divisions.geojson"]) {
    const data = JSON.parse(await readFile(rawPath("geo", src), "utf8")) as {
      type: string;
      features: { type: string; properties: Record<string, unknown>; geometry: { type: string; coordinates: unknown } }[];
    };
    const rounded = {
      ...data,
      features: data.features.map((f) => ({
        ...f,
        geometry: { ...f.geometry, coordinates: roundCoordinates(f.geometry.coordinates, COORD_DP) },
      })),
    };
    await writeFile(path.join(layersDir, src), JSON.stringify(rounded), "utf8");
    log(`admin-geometry: copied ${src} into layers/ (${rounded.features.length} features, 6dp)`);
  }
}
