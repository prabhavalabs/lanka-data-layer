#!/usr/bin/env node
import type { Step } from "./step.ts";
import { getDb, closeDb } from "./db.ts";

import * as seed from "./steps/seed.ts";
import * as fetchPostal from "./steps/fetch-postal.ts";
import * as admin from "./steps/admin.ts";
import * as population from "./steps/population.ts";
import * as elections from "./steps/elections.ts";
import * as places from "./steps/places.ts";
import * as postal from "./steps/postal.ts";
import * as pois from "./steps/pois.ts";
import * as datasets from "./steps/datasets.ts";
import * as emit from "./steps/emit.ts";

// Fixed pipeline order. `--only` filters this list but never reorders it,
// so dependencies (e.g. admin before population/elections) always hold.
const PIPELINE: Step[] = [seed, fetchPostal, admin, population, elections, places, postal, pois, datasets, emit];

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
