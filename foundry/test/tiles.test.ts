import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtemp, rm, utimes, writeFile } from "node:fs/promises";
import path from "node:path";
import os from "node:os";
import {
  needsRebuild,
  placesFeatureCollection,
  postalFeatureCollection,
  withMinzoom,
  type PlaceRow,
  type PostalRow,
} from "../src/lib/tiles.ts";

async function withTmpDir<T>(fn: (dir: string) => Promise<T>): Promise<T> {
  const dir = await mkdtemp(path.join(os.tmpdir(), "foundry-tiles-test-"));
  try {
    return await fn(dir);
  } finally {
    await rm(dir, { recursive: true, force: true });
  }
}

// --- needsRebuild ---

test("needsRebuild: true when the output file doesn't exist yet", async () => {
  await withTmpDir(async (dir) => {
    const input = path.join(dir, "in.geojson");
    await writeFile(input, "{}");
    const output = path.join(dir, "out.pmtiles"); // never written
    assert.equal(await needsRebuild(output, [input]), true);
  });
});

test("needsRebuild: false when the output is newer than every input", async () => {
  await withTmpDir(async (dir) => {
    const input = path.join(dir, "in.geojson");
    const output = path.join(dir, "out.pmtiles");
    await writeFile(input, "{}");
    await writeFile(output, "tiles");
    const past = new Date(Date.now() - 60_000);
    const future = new Date(Date.now() + 60_000);
    await utimes(input, past, past);
    await utimes(output, future, future);
    assert.equal(await needsRebuild(output, [input]), false);
  });
});

test("needsRebuild: true when an input is newer than the output", async () => {
  await withTmpDir(async (dir) => {
    const input = path.join(dir, "in.geojson");
    const output = path.join(dir, "out.pmtiles");
    await writeFile(output, "tiles");
    await writeFile(input, "{}");
    const past = new Date(Date.now() - 60_000);
    const future = new Date(Date.now() + 60_000);
    await utimes(output, past, past);
    await utimes(input, future, future);
    assert.equal(await needsRebuild(output, [input]), true);
  });
});

test("needsRebuild: true when any one of several inputs is newer, even if others are older", async () => {
  await withTmpDir(async (dir) => {
    const staleInput = path.join(dir, "stale.geojson");
    const freshInput = path.join(dir, "fresh.geojson");
    const output = path.join(dir, "out.pmtiles");
    await writeFile(output, "tiles");
    await writeFile(staleInput, "{}");
    await writeFile(freshInput, "{}");
    const past = new Date(Date.now() - 60_000);
    const now = new Date(Date.now());
    const future = new Date(Date.now() + 60_000);
    await utimes(output, now, now);
    await utimes(staleInput, past, past);
    await utimes(freshInput, future, future);
    assert.equal(await needsRebuild(output, [staleInput, freshInput]), true);
  });
});

test("needsRebuild: force=true always rebuilds, even with a fresh output", async () => {
  await withTmpDir(async (dir) => {
    const input = path.join(dir, "in.geojson");
    const output = path.join(dir, "out.pmtiles");
    await writeFile(input, "{}");
    await writeFile(output, "tiles");
    const past = new Date(Date.now() - 60_000);
    const future = new Date(Date.now() + 60_000);
    await utimes(input, past, past);
    await utimes(output, future, future);
    assert.equal(await needsRebuild(output, [input], true), true);
  });
});

test("needsRebuild: throws if a declared input doesn't exist (a real config error, not staleness)", async () => {
  await withTmpDir(async (dir) => {
    const output = path.join(dir, "out.pmtiles");
    await writeFile(output, "tiles");
    const missingInput = path.join(dir, "does-not-exist.geojson");
    await assert.rejects(() => needsRebuild(output, [missingInput]));
  });
});

// --- withMinzoom ---

test("withMinzoom: stamps a tippecanoe.minzoom extension on every feature", () => {
  const features = [
    { type: "Feature" as const, properties: { pcode: "LK1" }, geometry: { type: "Point" as const, coordinates: [1, 2] } },
    { type: "Feature" as const, properties: { pcode: "LK2" }, geometry: { type: "Point" as const, coordinates: [3, 4] } },
  ];
  const stamped = withMinzoom(features, 6);
  assert.equal(stamped.length, 2);
  for (const f of stamped) assert.deepEqual(f.tippecanoe, { minzoom: 6 });
});

test("withMinzoom: preserves feature properties/geometry unchanged", () => {
  const features = [
    { type: "Feature" as const, properties: { pcode: "LK1", name_en: "Western" }, geometry: { type: "Point" as const, coordinates: [1, 2] } },
  ];
  const [stamped] = withMinzoom(features, 8);
  assert.deepEqual(stamped!.properties, { pcode: "LK1", name_en: "Western" });
  assert.deepEqual(stamped!.geometry, { type: "Point", coordinates: [1, 2] });
});

test("withMinzoom: merges into (doesn't replace) an existing tippecanoe object on the feature", () => {
  const features = [
    {
      type: "Feature" as const,
      properties: {},
      geometry: { type: "Point" as const, coordinates: [1, 2] },
      tippecanoe: { layer: "custom" },
    },
  ];
  const [stamped] = withMinzoom(features, 10);
  assert.deepEqual(stamped!.tippecanoe, { layer: "custom", minzoom: 10 });
});

test("withMinzoom: does not mutate the input array or its feature objects", () => {
  const original = { type: "Feature" as const, properties: {}, geometry: { type: "Point" as const, coordinates: [1, 2] } };
  const features = [original];
  withMinzoom(features, 5);
  assert.equal("tippecanoe" in original, false);
});

// --- placesFeatureCollection ---

test("placesFeatureCollection: maps place rows to Point features with rounded coordinates", () => {
  const rows: PlaceRow[] = [
    { id: 1, name_en: "Colombo", name_si: "කොළඹ", name_ta: "கொழும்பு", kind: "city", lat: 6.9319444444, lon: 79.8478333333, population: 753000 },
  ];
  const fc = placesFeatureCollection(rows);
  assert.equal(fc.type, "FeatureCollection");
  assert.equal(fc.features.length, 1);
  const [f] = fc.features;
  assert.deepEqual(f!.geometry, { type: "Point", coordinates: [79.847833, 6.931944] });
  assert.deepEqual(f!.properties, {
    id: 1,
    name_en: "Colombo",
    name_si: "කොළඹ",
    name_ta: "கொழும்பு",
    kind: "city",
    population: 753000,
  });
});

test("placesFeatureCollection: passes through null name_si/name_ta/population as-is", () => {
  const rows: PlaceRow[] = [
    { id: 2, name_en: "Somewhere", name_si: null, name_ta: null, kind: "village", lat: 7, lon: 80, population: null },
  ];
  const [f] = placesFeatureCollection(rows).features;
  assert.equal(f!.properties.name_si, null);
  assert.equal(f!.properties.name_ta, null);
  assert.equal(f!.properties.population, null);
});

test("placesFeatureCollection: empty input yields an empty FeatureCollection", () => {
  const fc = placesFeatureCollection([]);
  assert.deepEqual(fc, { type: "FeatureCollection", features: [] });
});

// --- postalFeatureCollection ---

test("postalFeatureCollection: maps postal rows to Point features with rounded coordinates", () => {
  const rows: PostalRow[] = [{ code: "00100", name: "Colombo", lat: 6.9271234567, lon: 79.8612345678 }];
  const fc = postalFeatureCollection(rows);
  assert.equal(fc.features.length, 1);
  const [f] = fc.features;
  assert.deepEqual(f!.geometry, { type: "Point", coordinates: [79.861235, 6.927123] });
  assert.deepEqual(f!.properties, { code: "00100", name: "Colombo" });
});

test("postalFeatureCollection: drops rows with null lat or lon", () => {
  const rows: PostalRow[] = [
    { code: "00100", name: "Colombo", lat: 6.9, lon: 79.8 },
    { code: "20000", name: "No coordinates", lat: null, lon: null },
    { code: "30000", name: "Missing lon only", lat: 7.1, lon: null },
  ];
  const fc = postalFeatureCollection(rows);
  assert.equal(fc.features.length, 1);
  assert.equal(fc.features[0]!.properties.code, "00100");
});

test("postalFeatureCollection: empty input yields an empty FeatureCollection", () => {
  const fc = postalFeatureCollection([]);
  assert.deepEqual(fc, { type: "FeatureCollection", features: [] });
});
