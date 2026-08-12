#!/usr/bin/env node
import type { Step } from "./step.ts";
import { getDb, closeDb } from "./db.ts";

import * as seed from "./steps/seed.ts";
import * as fetchPostal from "./steps/fetch-postal.ts";
import * as fetchWorldpop from "./steps/fetch-worldpop.ts";
import * as fetchGnd from "./steps/fetch-gnd.ts";
import * as admin from "./steps/admin.ts";
import * as population from "./steps/population.ts";
import * as elections from "./steps/elections.ts";
import * as places from "./steps/places.ts";
import * as postal from "./steps/postal.ts";
import * as pois from "./steps/pois.ts";
import * as layers from "./steps/layers.ts";
import * as poisExtend from "./steps/pois-extend.ts";
import * as cells from "./steps/cells.ts";
import * as adminGeometry from "./steps/admin-geometry.ts";
import * as cellLookup from "./steps/cell-lookup.ts";
import * as downloads from "./steps/downloads.ts";
import * as datasets from "./steps/datasets.ts";
import * as emit from "./steps/emit.ts";

// Fixed pipeline order. `--only` filters this list but never reorders it, so
// dependencies always hold:
//   - fetch-worldpop/fetch-gnd sit right after fetch-postal (network
//     fetchers, independent of everything else) and before `admin`, which
//     now reads fetch-gnd's COD-AB ADM3/ADM4 files for levels 3-4.
//   - layers/pois-extend/downloads (sit after pois: pois-extend adds to the
//     table pois just built; downloads reads layers' output) and before
//     datasets (datasets stamps download_path by checking which files
//     downloads produced — see datasets.ts).
//   - cells needs fetch-worldpop's raster; admin-geometry needs admin_units
//     (admin) + the COD-AB files (fetch-gnd); cell-lookup needs cells,
//     admin_units level 4 (admin), and places/postal_codes — all three run
//     before downloads/datasets so those steps' table counts and CSV/gz
//     exports reflect the populated cells/cell_lookup tables.
const PIPELINE: Step[] = [
  seed,
  fetchPostal,
  fetchWorldpop,
  fetchGnd,
  admin,
  population,
  elections,
  places,
  postal,
  pois,
  layers,
  poisExtend,
  cells,
  adminGeometry,
  cellLookup,
  downloads,
  datasets,
  emit,
];

function parseOnly(argv: string[]): Set<string> | null {
  const flag = argv.find((a) => a === "--only" || a.startsWith("--only="));
  if (!flag) return null;
  const value = flag.includes("=") ? flag.split("=")[1] : argv[argv.indexOf(flag) + 1];
  if (!value) throw new Error("--only requires a comma-separated list of step names");
  return new Set(value.split(",").map((s) => s.trim()).filter(Boolean));
}

async function main(): Promise<void> {
  const argv = process.argv.slice(2);
  const only = parseOnly(argv);

  if (only) {
    const known = new Set(PIPELINE.map((s) => s.name));
    for (const requested of only) {
      if (!known.has(requested)) {
        throw new Error(`Unknown step "${requested}". Known steps: ${[...known].join(", ")}`);
      }
    }
  }

  const steps = only ? PIPELINE.filter((s) => only.has(s.name)) : PIPELINE;
  if (steps.length === 0) {
    console.error("No steps selected.");
    process.exitCode = 1;
    return;
  }

  const db = getDb();
  const startedAt = Date.now();
  console.log(`geopub foundry: running ${steps.map((s) => s.name).join(" -> ")}`);

  for (const step of steps) {
    const t0 = Date.now();
    console.log(`\n[${step.name}] starting`);
    try {
      await step.run({ db, log: (msg: string) => console.log(`[${step.name}] ${msg}`) });
    } catch (err) {
      console.error(`[${step.name}] FAILED:`, err);
      closeDb();
      process.exitCode = 1;
      return;
    }
    console.log(`[${step.name}] done in ${Date.now() - t0}ms`);
  }

  closeDb();
  console.log(`\ngeopub foundry: build complete in ${Date.now() - startedAt}ms`);
}

main().catch((err) => {
  console.error("geopub foundry: unhandled error", err);
  process.exitCode = 1;
});
