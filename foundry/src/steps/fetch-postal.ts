import { readFile, writeFile } from "node:fs/promises";
import AdmZip from "adm-zip";
import type { StepContext } from "../step.ts";
import { downloadIfMissing, exists } from "../lib/fetch.ts";
import { parsePostalTsv } from "../lib/postal-parser.ts";
import { rawPath } from "../lib/paths.ts";

const ZIP_URL = "https://download.geonames.org/export/zip/LK.zip";

export const name = "fetch-postal";

/**
 * Downloads the GeoNames LK postal dump and extracts the TSV it contains
 * (LK.txt) into data/raw/, so the `postal` step never needs network access.
 * Both the zip and the extracted TSV are cached — skipped if already present.
 */
export async function run({ log }: StepContext): Promise<void> {
  const zipPath = rawPath("postal", "LK.zip");
  const tsvPath = rawPath("postal", "LK.txt");

  const downloaded = await downloadIfMissing(ZIP_URL, zipPath);
  log(downloaded ? `fetch-postal: downloaded ${ZIP_URL}` : "fetch-postal: LK.zip already cached");

  if (await exists(tsvPath)) {
    log("fetch-postal: LK.txt already extracted");
    return;
  }

  const zipBuf = await readFile(zipPath);
  const zip = new AdmZip(zipBuf);
  const entry = zip.getEntry("LK.txt");
  if (!entry) throw new Error("fetch-postal: LK.txt not found inside LK.zip");
  const tsvText = entry.getData().toString("utf8");

  // Sanity-check the shape before caching it, so a corrupt/changed upstream
  // format fails loudly here rather than silently in the `postal` step.
  const rows = parsePostalTsv(tsvText);
  if (rows.length === 0) throw new Error("fetch-postal: parsed zero rows from LK.txt");

  await writeFile(tsvPath, tsvText, "utf8");
  log(`fetch-postal: extracted LK.txt (${rows.length} rows)`);
}
