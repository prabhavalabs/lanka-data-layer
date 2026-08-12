/**
 * Pure helpers for the `tiles` step (PMTiles vector-tile build via the host
 * `tippecanoe` binary — docs/architecture.md §3, `tiles/<layer>.pmtiles`).
 * Deliberately dependency-free like `lib/feature-collection.ts`, which this
 * borrows coordinate rounding from. Nothing here shells out to tippecanoe —
 * that lives in `steps/tiles.ts` so this file stays unit-testable without a
 * tippecanoe binary on the test machine.
 */

import { stat } from "node:fs/promises";
import { roundCoord, type Feature, type FeatureCollection } from "./feature-collection.ts";

/**
 * Returns true if `outputPath` should be (re)built: missing, `force`, or any
 * of `inputPaths` is newer than it. A missing input is a real configuration
 * error (the caller declared a dependency that doesn't exist) and throws
 * rather than being treated as "not stale".
 */
export async function needsRebuild(outputPath: string, inputPaths: string[], force = false): Promise<boolean> {
  if (force) return true;

  let outMtimeMs: number;
  try {
    outMtimeMs = (await stat(outputPath)).mtimeMs;
  } catch {
    return true; // output doesn't exist yet
  }

  for (const inputPath of inputPaths) {
    const inMtimeMs = (await stat(inputPath)).mtimeMs;
    if (inMtimeMs > outMtimeMs) return true;
  }
  return false;
}

interface TippecanoeExtension {
  minzoom?: number;
  maxzoom?: number;
  layer?: string;
}

/** A GeoJSON Feature carrying tippecanoe's per-feature `tippecanoe` extension object (sibling of `properties`, not inside it). */
export type StampedFeature<P = Record<string, unknown>> = Feature<P> & { tippecanoe?: TippecanoeExtension };

/**
 * Stamps every feature in `features` with a `tippecanoe.minzoom` extension
 * value (tippecanoe's GeoJSON extension for per-feature zoom gating — see
 * `man tippecanoe`, "GeoJSON extension"). This is how one tippecanoe
 * invocation can merge several admin levels (or ED/PD) into a single
 * tileset while each still only appears from its own minzoom up — a plain
 * `-L` named layer has no per-source zoom option, only this per-feature one.
 * Any existing `tippecanoe` object on a feature is preserved and extended,
 * not replaced. Does not mutate the input.
 */
export function withMinzoom<P>(features: StampedFeature<P>[], minzoom: number): StampedFeature<P>[] {
  return features.map((f) => ({ ...f, tippecanoe: { ...(f.tippecanoe ?? {}), minzoom } }));
}

export interface PlaceRow {
  id: number;
  name_en: string;
  name_si: string | null;
  name_ta: string | null;
  kind: string;
  lat: number;
  lon: number;
  population: number | null;
}

export interface PlaceProperties {
  id: number;
  name_en: string;
  name_si: string | null;
  name_ta: string | null;
  kind: string;
  population: number | null;
}

/**
 * Builds the `places` point layer GeoJSON (docs/architecture.md §3,
 * `places.pmtiles`) from `places` table rows — id/name_en/name_si/name_ta/
 * kind/population properties, coordinates rounded to 6dp per §3.
 */
export function placesFeatureCollection(rows: PlaceRow[]): FeatureCollection<PlaceProperties> {
  return {
    type: "FeatureCollection",
    features: rows.map(
      (r): Feature<PlaceProperties> => ({
        type: "Feature",
        properties: {
          id: r.id,
          name_en: r.name_en,
          name_si: r.name_si,
          name_ta: r.name_ta,
          kind: r.kind,
          population: r.population,
        },
        geometry: { type: "Point", coordinates: [roundCoord(r.lon), roundCoord(r.lat)] },
      }),
    ),
  };
}

export interface PostalRow {
  code: string;
  name: string;
  lat: number | null;
  lon: number | null;
}

export interface PostalProperties {
  code: string;
  name: string;
}

/**
 * Builds the `postal` point layer GeoJSON (docs/architecture.md §3,
 * `places.pmtiles`) from `postal_codes` table rows — code/name properties,
 * coordinates rounded to 6dp per §3. `lat`/`lon` are nullable in the schema;
 * rows without coordinates can't be placed on a map and are dropped.
 */
export function postalFeatureCollection(rows: PostalRow[]): FeatureCollection<PostalProperties> {
  const withCoords = rows.filter(
    (r): r is PostalRow & { lat: number; lon: number } => r.lat !== null && r.lon !== null,
  );
  return {
    type: "FeatureCollection",
    features: withCoords.map(
      (r): Feature<PostalProperties> => ({
        type: "Feature",
        properties: { code: r.code, name: r.name },
        geometry: { type: "Point", coordinates: [roundCoord(r.lon), roundCoord(r.lat)] },
      }),
    ),
  };
}
