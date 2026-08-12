import { copyFile, mkdir } from "node:fs/promises";
import path from "node:path";
import type { StepContext } from "../step.ts";
import { exists } from "../lib/fetch.ts";
import { rawPath } from "../lib/paths.ts";

/**
 * Seeds foundry/data/raw/ from a local clone of the predecessor project
 * (prabhavalabs/ceylon-hub), which already carries the OSM/HDX/census/EC
 * extracts this build needs. This is a placeholder for real fetchers:
 * every entry below names one source file and where it lands under
 * data/raw/. To replace ceylon-hub with a live fetch later, swap the
 * `copy` implementation for a `download*` one and keep the same `dest`
 * path — every downstream step only ever reads from data/raw/, never from
 * SOURCE_ROOT.
 *
 * Idempotent: a file already present in data/raw/ is left untouched, so
 * re-running `seed` after a partial build (or after editing raw data by
 * hand for testing) does not clobber it.
 */

// Required: path to a local ceylon-hub checkout. There's no sensible
// hardcoded default (that clone lives outside this repo and outside any
// fixed location on disk), so this step fails fast with a clear message
// rather than silently pointing at a path that happens to exist on one
// machine.
const SOURCE_ROOT = process.env.FOUNDRY_SEED_SOURCE;

interface SeedEntry {
  /** Path relative to SOURCE_ROOT. */
  src: string;
  /** Path relative to data/raw/. */
  dest: string;
}

const GEO_FILES = [
  "provinces.geojson",
  "districts.geojson",
  "ds-divisions.geojson",
  "country.geojson",
  "cities.geojson",
  "hospitals.geojson",
  "education.geojson",
  "airports.geojson",
  "electoral-divisions.geojson",
  "polling-divisions.geojson",
  // Map-layer sources for the `layers` step (docs/architecture.md §3).
  "roads.geojson",
  "railways.geojson",
  "waterways.geojson",
  "protected-areas.geojson",
];

const DATA_FILES = [
  "population-2023.json",
  "ethnicity-religion-2012.json",
  "election-pres-2024.json",
  "election-pres-2019.json",
  "election-pres-2015.json",
  "election-parl-2024.json",
];

const ENTRIES: SeedEntry[] = [
  ...GEO_FILES.map((f) => ({ src: `public/geo/${f}`, dest: `geo/${f}` })),
  ...DATA_FILES.map((f) => ({ src: `scripts/data/${f}`, dest: `data/${f}` })),
];

export const name = "seed";

export async function run({ log }: StepContext): Promise<void> {
  const missing: SeedEntry[] = [];
  for (const entry of ENTRIES) {
    if (!(await exists(rawPath(entry.dest)))) missing.push(entry);
  }

  if (missing.length === 0) {
    log(`seed: copied 0 file(s), skipped ${ENTRIES.length} already present`);
    return;
  }

  if (!SOURCE_ROOT) {
    throw new Error(
      `seed: ${missing.length} raw file(s) missing from data/raw/ and FOUNDRY_SEED_SOURCE is not set. ` +
        `Set it to a local checkout of the source project, e.g. FOUNDRY_SEED_SOURCE=/path/to/ceylon-hub pnpm run build`,
    );
  }

  for (const entry of missing) {
    const destPath = rawPath(entry.dest);
    await mkdir(path.dirname(destPath), { recursive: true });
    const srcPath = path.join(SOURCE_ROOT, entry.src);
    if (!(await exists(srcPath))) {
      throw new Error(`seed: source file missing: ${srcPath}`);
    }
    await copyFile(srcPath, destPath);
  }
  log(`seed: copied ${missing.length} file(s), skipped ${ENTRIES.length - missing.length} already present`);
}
