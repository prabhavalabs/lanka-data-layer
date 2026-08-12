import { mkdir, readFile, writeFile } from "node:fs/promises";
import path from "node:path";
import type { StepContext } from "../step.ts";
import { artifactPath, rawPath } from "../lib/paths.ts";
import { roundGeometry, type Feature, type FeatureCollection } from "../lib/feature-collection.ts";

export const name = "layers";

async function readFC(relPath: string): Promise<FeatureCollection> {
  const text = await readFile(rawPath(relPath), "utf8");
  return JSON.parse(text) as FeatureCollection;
}

/** Copies only the given keys from `props` into a new object, defaulting missing/undefined values to null. */
function pick(props: Record<string, unknown>, keys: string[]): Record<string, unknown> {
  const out: Record<string, unknown> = {};
  for (const key of keys) out[key] = props[key] ?? null;
  return out;
}

interface LayerSpec {
  /** Path under data/raw/, produced by `seed`. */
  src: string;
  /** File name written under data/artifacts/layers/. */
  dest: string;
  /** Builds the trimmed properties object for one feature; return null to drop the feature. */
  properties: (props: Record<string, unknown>) => Record<string, unknown>;
}

const LAYERS: LayerSpec[] = [
  {
    src: "geo/roads.geojson",
    dest: "roads.geojson",
    properties: (p) => pick(p, ["id", "highway", "name", "ref"]),
  },
  {
    src: "geo/railways.geojson",
    dest: "railways.geojson",
    // Carries both LineString track segments (kind='rail') and Point
    // station/halt features in one layer — `kind` is the styling
    // discriminant on the web/tiles side. Station points are additionally
    // fed into `pois` by the `pois-extend` step for search purposes; that
    // is a separate artifact serving a separate use case, not a duplicate
    // of this one.
    properties: (p) => ({
      id: p.id ?? null,
      name: p.name ?? null,
      name_si: p.nameSi ?? null,
      name_ta: p.nameTa ?? null,
      kind: p.kind ?? null,
    }),
  },
  {
    src: "geo/waterways.geojson",
    dest: "waterways.geojson",
    properties: (p) => pick(p, ["id", "name", "kind"]),
  },
  {
    src: "geo/protected-areas.geojson",
    dest: "protected-areas.geojson",
    properties: (p) => pick(p, ["id", "name", "kind", "protectionType"]),
  },
  {
    src: "geo/country.geojson",
    dest: "country.geojson",
    // No trim list given for this one — pass source properties through
    // unchanged (id/name/level/iso in this build).
    properties: (p) => p,
  },
];

/**
 * layers: writes map-layer GeoJSON FeatureCollections to
 * data/artifacts/layers/ — one file per source in `LAYERS`, properties
 * trimmed to the columns each layer needs for styling, coordinates rounded
 * to 6 dp per docs/architecture.md §3. These feed the PMTiles tile build
 * (not run by this step) and the `downloads` step's GeoJSON.gz exports.
 */
export async function run({ log }: StepContext): Promise<void> {
  const outDir = artifactPath("layers");
  await mkdir(outDir, { recursive: true });

  for (const layer of LAYERS) {
    const fc = await readFC(layer.src);
    const out: FeatureCollection = {
      type: "FeatureCollection",
      features: fc.features.map(
        (f): Feature => ({
          type: "Feature",
          properties: layer.properties(f.properties),
          geometry: roundGeometry(f.geometry),
        }),
      ),
    };
    const destPath = path.join(outDir, layer.dest);
    await writeFile(destPath, JSON.stringify(out), "utf8");
    log(`layers: wrote ${layer.dest} (${out.features.length} features)`);
  }
}
