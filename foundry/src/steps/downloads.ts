import { mkdir, readFile, writeFile } from "node:fs/promises";
import path from "node:path";
import type Database from "better-sqlite3";
import type { StepContext } from "../step.ts";
import { artifactPath } from "../lib/paths.ts";
import { exists } from "../lib/fetch.ts";
import { toCsv, type CsvValue } from "../lib/csv.ts";
import { gzipBuffer } from "../lib/gzip.ts";

export const name = "downloads";

// Layer files written by `layers` (data/artifacts/layers/<file>) that get a
// gzip'd copy under downloads/. Kept as a plain list rather than a glob so
// this step's output is deterministic and doesn't silently pick up
// unrelated files someone drops in data/artifacts/layers/ later.
const LAYER_FILES = ["roads.geojson", "railways.geojson", "waterways.geojson", "protected-areas.geojson", "country.geojson"];

interface TableCsvSpec {
  /** File name written under data/artifacts/downloads/. */
  file: string;
  /** Table whose row count gates this export — skipped gracefully if 0. */
  countTable: string;
  headers: string[];
  query: string;
}

const TABLE_CSVS: TableCsvSpec[] = [
  {
    file: "admin-units.csv",
    countTable: "admin_units",
    headers: ["pcode", "level", "name_en", "name_si", "name_ta", "parent_pcode", "area_km2", "centroid_lat", "centroid_lon"],
    query: `SELECT pcode, level, name_en, name_si, name_ta, parent_pcode, area_km2, centroid_lat, centroid_lon
            FROM admin_units ORDER BY pcode`,
  },
  {
    file: "population-2023.csv",
    countTable: "admin_population",
    headers: ["pcode", "name_en", "name_si", "name_ta", "year", "sex", "age_bucket", "count"],
    query: `SELECT ap.pcode, au.name_en, au.name_si, au.name_ta, ap.year, ap.sex, ap.age_bucket, ap.count
            FROM admin_population ap LEFT JOIN admin_units au ON au.pcode = ap.pcode
            ORDER BY ap.pcode, ap.year, ap.sex, ap.age_bucket`,
  },
  {
    file: "elections.csv",
    countTable: "election_results",
    headers: ["election_id", "entity_id", "entity_name", "kind", "electors", "polled", "valid", "rejected", "winner_party", "winner_votes"],
    query: `SELECT er.election_id, er.entity_id, ee.name AS entity_name, ee.kind,
                   er.electors, er.polled, er.valid, er.rejected, er.winner_party, er.winner_votes
            FROM election_results er LEFT JOIN election_entities ee ON ee.entity_id = er.entity_id
            ORDER BY er.election_id, er.entity_id`,
  },
  {
    file: "places.csv",
    countTable: "places",
    headers: ["id", "source", "source_id", "kind", "name_en", "name_si", "name_ta", "lat", "lon", "population", "admin_pcode"],
    query: `SELECT id, source, source_id, kind, name_en, name_si, name_ta, lat, lon, population, admin_pcode
            FROM places ORDER BY id`,
  },
  {
    file: "postal-codes.csv",
    countTable: "postal_codes",
    headers: ["code", "name", "admin_pcode", "lat", "lon"],
    query: `SELECT code, name, admin_pcode, lat, lon FROM postal_codes ORDER BY code`,
  },
  {
    file: "pois.csv",
    countTable: "pois",
    headers: ["id", "category", "kind", "name_en", "name_si", "name_ta", "lat", "lon", "admin_pcode", "attrs"],
    query: `SELECT id, category, kind, name_en, name_si, name_ta, lat, lon, admin_pcode, attrs FROM pois ORDER BY id`,
  },
  // Optional: owned by a concurrently-developed part of the pipeline
  // (worldpop/cells + gnd/cell_lookup). Both tables exist per the schema
  // contract but stay empty until that work lands, so these are skipped
  // gracefully rather than failing the build — re-running `downloads` (or
  // the full build) once they're populated picks them up automatically.
  {
    file: "cells.csv",
    countTable: "cells",
    headers: ["cell_id", "pop"],
    query: `SELECT cell_id, pop FROM cells ORDER BY cell_id`,
  },
  {
    file: "cell-lookup.csv",
    countTable: "cell_lookup",
    headers: ["cell_id", "gnd_pcode", "postal_code", "nearest_place_id", "nearest_place_dist_m"],
    query: `SELECT cell_id, gnd_pcode, postal_code, nearest_place_id, nearest_place_dist_m FROM cell_lookup ORDER BY cell_id`,
  },
];

function rowCount(db: Database.Database, table: string): number {
  const row = db.prepare(`SELECT COUNT(*) AS n FROM ${table}`).get() as { n: number };
  return row.n;
}

/**
 * downloads: writes public bulk-download artifacts to
 * data/artifacts/downloads/ — CSV for every tabular dataset, GeoJSON.gz for
 * the `layers` step's map layers. Does not touch `datasets` rows itself;
 * `datasets` (which runs immediately after this step) stamps
 * download_path by checking which of these files exist on disk, so the two
 * steps only need to agree on the file-naming convention, not call into
 * each other.
 */
export async function run({ db, log }: StepContext): Promise<void> {
  const outDir = artifactPath("downloads");
  await mkdir(outDir, { recursive: true });

  let written = 0;
  let skipped = 0;

  for (const spec of TABLE_CSVS) {
    const n = rowCount(db, spec.countTable);
    if (n === 0) {
      log(`downloads: skipping ${spec.file} (${spec.countTable} is empty)`);
      skipped++;
      continue;
    }
    const rows = db.prepare(spec.query).all() as Record<string, CsvValue>[];
    const csv = toCsv(
      spec.headers,
      rows.map((r) => spec.headers.map((h) => r[h])),
    );
    await writeFile(path.join(outDir, spec.file), csv, "utf8");
    log(`downloads: wrote ${spec.file} (${rows.length} rows)`);
    written++;
  }

  for (const layerFile of LAYER_FILES) {
    const srcPath = artifactPath("layers", layerFile);
    if (!(await exists(srcPath))) {
      log(`downloads: skipping ${layerFile}.gz (layers/${layerFile} not found — run the layers step first)`);
      skipped++;
      continue;
    }
    const geojson = await readFile(srcPath);
    const gz = gzipBuffer(geojson);
    await writeFile(path.join(outDir, `${layerFile}.gz`), gz);
    log(`downloads: wrote ${layerFile}.gz (${geojson.length} -> ${gz.length} bytes)`);
    written++;
  }

  log(`downloads: wrote ${written} file(s), skipped ${skipped}`);
}
