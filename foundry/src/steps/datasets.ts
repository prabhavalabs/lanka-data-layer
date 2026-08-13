import { readFile } from "node:fs/promises";
import type Database from "better-sqlite3";
import type { StepContext } from "../step.ts";
import { artifactPath } from "../lib/paths.ts";
import { exists } from "../lib/fetch.ts";

export const name = "datasets";

type FeatureCountSource =
  | { kind: "table"; table: string }
  | { kind: "layer-file"; file: string };

interface DatasetSpec {
  id: string;
  title: string;
  category: string;
  description: string;
  source_name: string;
  source_url: string;
  license: string;
  count: FeatureCountSource;
  /** Path under data/artifacts/downloads/, checked for existence before being stamped into download_path. */
  downloadFile: string;
}

// The bulk-download catalog (docs/architecture.md §2 `datasets` table).
// Tabular datasets count rows straight from their table; layer datasets
// (no DB table of their own — they're GeoJSON files from the `layers`
// step) count features in the written artifact instead.
const DATASETS: DatasetSpec[] = [
  {
    id: "admin-units",
    title: "Administrative units",
    category: "admin",
    description:
      "Sri Lanka's administrative hierarchy (country, provinces, districts, DS divisions, GN divisions), p-code keyed. " +
      "Levels 0-2 from geoBoundaries; levels 3-4 (DS/GN divisions) from OCHA COD-AB Sri Lanka " +
      "(https://data.humdata.org/dataset/cod-ab-lka), official p-codes throughout.",
    source_name: "geoBoundaries + OCHA COD-AB Sri Lanka",
    source_url: "https://www.geoboundaries.org",
    license: "CC BY 3.0 IGO / CC BY-IGO",
    count: { kind: "table", table: "admin_units" },
    downloadFile: "admin-units.csv",
  },
  {
    id: "population-2023",
    title: "Population by administrative unit (2023)",
    category: "population",
    description: "2023 population projections by sex and age bucket, per administrative unit.",
    source_name: "HDX cod-ps-lka",
    source_url: "https://data.humdata.org/dataset/cod-ps-lka",
    license: "CC BY-IGO",
    count: { kind: "table", table: "admin_population" },
    downloadFile: "population-2023.csv",
  },
  {
    id: "elections",
    title: "Election results",
    category: "elections",
    description: "Presidential and parliamentary election results by electoral/polling division and postal vote.",
    source_name: "Election Commission of Sri Lanka (via nuuuwan/lk_elections)",
    source_url: "https://github.com/nuuuwan/lk_elections",
    license: "open",
    count: { kind: "table", table: "election_results" },
    downloadFile: "elections.csv",
  },
  {
    id: "places",
    title: "Named places",
    category: "places",
    description: "Cities, towns, suburbs and villages for search and map labeling.",
    source_name: "OpenStreetMap",
    source_url: "https://www.openstreetmap.org",
    license: "ODbL",
    count: { kind: "table", table: "places" },
    downloadFile: "places.csv",
  },
  {
    id: "postal-codes",
    title: "Postal codes",
    category: "postal",
    description: "Postal codes and their associated localities.",
    source_name: "GeoNames",
    source_url: "https://download.geonames.org/export/zip/LK.zip",
    license: "CC BY 4.0",
    count: { kind: "table", table: "postal_codes" },
    downloadFile: "postal-codes.csv",
  },
  {
    id: "pois",
    title: "Points of interest",
    category: "pois",
    description: "Hospitals, clinics, schools, universities, airports and railway stations.",
    source_name: "OpenStreetMap",
    source_url: "https://www.openstreetmap.org",
    license: "ODbL",
    count: { kind: "table", table: "pois" },
    downloadFile: "pois.csv",
  },
  {
    id: "roads",
    title: "Roads",
    category: "layers",
    description: "Road network (trunk roads down to local streets) for map display.",
    source_name: "OpenStreetMap",
    source_url: "https://www.openstreetmap.org",
    license: "ODbL",
    count: { kind: "layer-file", file: "roads.geojson" },
    downloadFile: "roads.geojson.gz",
  },
  {
    id: "railways",
    title: "Railways",
    category: "layers",
    description: "Rail lines and stations/halts for map display.",
    source_name: "OpenStreetMap",
    source_url: "https://www.openstreetmap.org",
    license: "ODbL",
    count: { kind: "layer-file", file: "railways.geojson" },
    downloadFile: "railways.geojson.gz",
  },
  {
    id: "waterways",
    title: "Waterways",
    category: "layers",
    description: "Rivers and water bodies for map display.",
    source_name: "OpenStreetMap",
    source_url: "https://www.openstreetmap.org",
    license: "ODbL",
    count: { kind: "layer-file", file: "waterways.geojson" },
    downloadFile: "waterways.geojson.gz",
  },
  {
    id: "protected-areas",
    title: "Protected areas",
    category: "layers",
    description: "National parks, nature reserves and other protected areas for map display.",
    source_name: "OpenStreetMap",
    source_url: "https://www.openstreetmap.org",
    license: "ODbL",
    count: { kind: "layer-file", file: "protected-areas.geojson" },
    downloadFile: "protected-areas.geojson.gz",
  },
  {
    id: "country",
    title: "Country boundary",
    category: "layers",
    description: "Sri Lanka's national boundary for map display.",
    source_name: "geoBoundaries",
    source_url: "https://www.geoboundaries.org",
    license: "CC BY 3.0 IGO",
    count: { kind: "layer-file", file: "country.geojson" },
    downloadFile: "country.geojson.gz",
  },
  // Population grid (WorldPop) and the reverse-geocode lookup it feeds
  // (cell-lookup, keyed against OCHA COD-AB GN divisions) — both tables are
  // still declared unconditionally skippable below (feature_count 0 ->
  // dropped from the catalog) since any `--only` run that skips `cells`/
  // `cell-lookup` leaves them empty.
  {
    id: "cells",
    title: "Gridded population",
    category: "population",
    description: "WorldPop 1km 2025 UN-adjusted population estimate, distributed across the canonical fine grid.",
    source_name: "WorldPop",
    source_url: "https://www.worldpop.org",
    license: "CC BY 4.0",
    count: { kind: "table", table: "cells" },
    downloadFile: "cells.csv",
  },
  {
    id: "cell-lookup",
    title: "Reverse-geocode grid lookup",
    category: "admin",
    description: "Per-cell GN division, postal code, and nearest place — the reverse-geocode index.",
    source_name: "OCHA COD-AB Sri Lanka (GN division boundaries)",
    source_url: "https://data.humdata.org/dataset/cod-ab-lka",
    license: "CC BY-IGO",
    count: { kind: "table", table: "cell_lookup" },
    downloadFile: "cell-lookup.csv",
  },
];

function rowCount(db: Database.Database, table: string): number {
  const row = db.prepare(`SELECT COUNT(*) AS n FROM ${table}`).get() as { n: number };
  return row.n;
}

async function layerFileFeatureCount(file: string): Promise<number | null> {
  const p = artifactPath("layers", file);
  if (!(await exists(p))) return null;
  const data = JSON.parse(await readFile(p, "utf8")) as { features?: unknown[] };
  return data.features?.length ?? 0;
}

/**
 * datasets: builds the bulk-download catalog (docs/architecture.md §2). Runs
 * after `downloads` so it can stamp `download_path` by checking which files
 * that step actually produced (rather than the two steps calling into each
 * other) — a dataset whose source table/layer is empty is skipped entirely
 * rather than cataloged with feature_count=0 and no file.
 */
export async function run({ db, log }: StepContext): Promise<void> {
  const insert = db.prepare(`
    INSERT INTO datasets (id, title, category, description, source_name, source_url, license, feature_count, download_path)
    VALUES (@id, @title, @category, @description, @source_name, @source_url, @license, @feature_count, @download_path)
  `);

  // better-sqlite3 transactions must run a synchronous callback, so the
  // async work (layer-file reads, download-file existence checks — both
  // cheap local fs calls) happens first into a plain array; only the
  // actual DB writes below run inside transaction().
  interface Resolved {
    spec: DatasetSpec;
    featureCount: number | null;
    downloadPath: string | null;
  }
  const resolved: Resolved[] = [];
  for (const spec of DATASETS) {
    const featureCount =
      spec.count.kind === "table" ? rowCount(db, spec.count.table) : await layerFileFeatureCount(spec.count.file);
    const downloadRelPath = `downloads/${spec.downloadFile}`;
    const downloadPath = featureCount ? ((await exists(artifactPath(downloadRelPath))) ? downloadRelPath : null) : null;
    resolved.push({ spec, featureCount, downloadPath });
  }

  let cataloged = 0;
  let skipped = 0;

  // This step owns the whole `datasets` table (like `pois` owns `pois`):
  // clear it and rebuild from `DATASETS` every run, rather than upserting
  // by id, so a dataset dropped from the list (or a stray row from
  // unrelated manual testing against this artifact) doesn't linger forever.
  const tx = db.transaction(() => {
    db.prepare(`DELETE FROM datasets`).run();
    for (const { spec, featureCount, downloadPath } of resolved) {
      if (!featureCount) {
        skipped++;
        continue;
      }
      insert.run({
        id: spec.id,
        title: spec.title,
        category: spec.category,
        description: spec.description,
        source_name: spec.source_name,
        source_url: spec.source_url,
        license: spec.license,
        feature_count: featureCount,
        download_path: downloadPath,
      });
      cataloged++;
    }
  });
  tx();

  log(`datasets: cataloged ${cataloged} dataset(s), skipped ${skipped} (empty source)`);
}
