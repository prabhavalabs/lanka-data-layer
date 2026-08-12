import { mkdir, readFile, writeFile } from "node:fs/promises";
import path from "node:path";
import AdmZip from "adm-zip";
import type { StepContext } from "../step.ts";
import { downloadIfMissing, exists } from "../lib/fetch.ts";
import { rawPath } from "../lib/paths.ts";

export const name = "fetch-gnd";

/**
 * OCHA COD-AB Sri Lanka administrative boundaries (HDX dataset `cod-ab-lka`,
 * https://data.humdata.org/dataset/cod-ab-lka). The dataset's `package_show`
 * API lists four resource formats (geodatabase, shapefile, GeoJSON, xlsx);
 * we use the GeoJSON zip directly — HDX does carry a ready-made GeoJSON
 * bundle for this dataset, so no shapefile-parser dependency is needed. The
 * zip contains one file per admin level (0-4) plus admin lines/points; we
 * only extract ADM3 (339 DS divisions) and ADM4 (~14k GN divisions) — `admin`
 * still sources levels 0-2 from geoBoundaries, which already carry the same
 * p-codes as COD-AB's adm1/adm2 (verified: every districts.geojson `pcode`
 * matches an adm2_pcode value in the COD-AB ADM3 file).
 */
const ZIP_URL =
  "https://data.humdata.org/dataset/0bedcaf3-88cd-4591-b9d5-5d3220e26abf/resource/ac173fa4-dd42-4be4-aaed-a2e445525865/download/lka_admin_boundaries.geojson.zip";

const ENTRIES = ["lka_admin3.geojson", "lka_admin4.geojson"];

export async function run({ log }: StepContext): Promise<void> {
  const missing = (
    await Promise.all(ENTRIES.map(async (name) => ((await exists(rawPath("cod-ab", name))) ? null : name)))
  ).filter((n): n is string => n !== null);

  if (missing.length === 0) {
    log("fetch-gnd: ADM3/ADM4 GeoJSON already extracted");
    return;
  }

  const zipPath = rawPath("cod-ab", "lka_admin_boundaries.geojson.zip");
  await mkdir(path.dirname(zipPath), { recursive: true });
  const downloaded = await downloadIfMissing(ZIP_URL, zipPath);
  log(downloaded ? `fetch-gnd: downloaded ${ZIP_URL}` : "fetch-gnd: zip already cached");

  const zip = new AdmZip(await readFile(zipPath));
  for (const entryName of missing) {
    const entry = zip.getEntry(entryName);
    if (!entry) {
      throw new Error(`fetch-gnd: ${entryName} not found inside lka_admin_boundaries.geojson.zip`);
    }
    const data = entry.getData();
    await writeFile(rawPath("cod-ab", entryName), data);
    log(`fetch-gnd: extracted ${entryName} (${data.length} bytes)`);
  }
}
