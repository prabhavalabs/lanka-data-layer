import { spawnSync } from "node:child_process";
import { mkdir, readFile, rm, stat, writeFile } from "node:fs/promises";
import path from "node:path";
import type { StepContext } from "../step.ts";
import { artifactPath, DB_PATH } from "../lib/paths.ts";
import {
  needsRebuild,
  placesFeatureCollection,
  postalFeatureCollection,
  withMinzoom,
  type PlaceRow,
  type PostalRow,
} from "../lib/tiles.ts";

export const name = "tiles";

const layersDir = () => artifactPath("layers");
const tilesDir = () => artifactPath("tiles");
const stagingDir = () => artifactPath("tiles", ".staging");

/** Runs `tippecanoe --version` and fails fast with an actionable message if the binary isn't on PATH. */
function checkTippecanoe(log: (msg: string) => void): void {
  const result = spawnSync("tippecanoe", ["--version"], { encoding: "utf8" });
  if (result.error) {
    if ((result.error as NodeJS.ErrnoException).code === "ENOENT") {
      throw new Error(
        "tiles: tippecanoe binary not found on PATH. This step shells out to the host tippecanoe " +
          "(v2.79.0) to build PMTiles vector tiles. Install it with: brew install tippecanoe",
      );
    }
    throw result.error;
  }
  // tippecanoe writes --version to stderr, not stdout.
  const version = (result.stdout || result.stderr || "").trim();
  log(`tiles: using ${version || "tippecanoe (version unknown)"}`);
}

interface PreparedLayer {
  /** -L layer name inside the output tileset. */
  layer: string;
  /** Absolute path to the GeoJSON tippecanoe reads for this layer (may be a staged, minzoom-stamped copy). */
  file: string;
}

interface TilesetSpec {
  /** File name written under data/artifacts/tiles/. */
  output: string;
  /** Files whose mtime gates the freshness check — the *real* upstream sources, not staged copies (see stageStamped). */
  freshnessInputs(): string[];
  minzoom: number;
  maxzoom: number;
  /** -y/--include allowlist applied to the whole tileset; omit to pass every source property through. */
  include?: string[];
  dropDensestAsNeeded?: boolean;
  /** Builds the tippecanoe -L layer list, staging any per-feature-stamped or generated GeoJSON it needs first. */
  prepare(ctx: StepContext): Promise<PreparedLayer[]>;
}

/** Reads a FeatureCollection under layers/, stamps every feature with tippecanoe.minzoom, writes it into the staging dir, and returns the staged path. */
async function stageStamped(srcFile: string, minzoom: number): Promise<string> {
  const raw = JSON.parse(await readFile(path.join(layersDir(), srcFile), "utf8")) as {
    type: string;
    features: { type: "Feature"; properties: Record<string, unknown>; geometry: unknown }[];
  };
  const stamped = { ...raw, features: withMinzoom(raw.features as never, minzoom) };
  const destPath = path.join(stagingDir(), srcFile);
  await writeFile(destPath, JSON.stringify(stamped), "utf8");
  return destPath;
}

function layersFile(name: string): string {
  return path.join(layersDir(), name);
}

// Tileset catalog (docs/architecture.md §3, `tiles/<layer>.pmtiles`). Zoom
// ranges and property allowlists per foundry's tiles spec. Multi-level
// tilesets (admin, electoral) use one tippecanoe invocation with several
// -L named layers; per-layer minzoom (tippecanoe has no such CLI option)
// comes from stamping each feature with the `tippecanoe.minzoom` GeoJSON
// extension before handing the file to tippecanoe (see stageStamped/
// withMinzoom) — the tileset-wide -Z/-z only sets the outer bound.
const TILESETS: TilesetSpec[] = [
  {
    output: "admin.pmtiles",
    minzoom: 0,
    maxzoom: 14,
    include: ["pcode", "name_en", "name_si", "name_ta"],
    freshnessInputs: () => [1, 2, 3, 4].map((n) => layersFile(`admin-adm${n}.geojson`)),
    async prepare() {
      // adm1 needs no per-feature stamp: its intended range (z0-14) already
      // matches the tileset-wide default, so it's read straight from layers/.
      const [adm2, adm3, adm4] = await Promise.all([
        stageStamped("admin-adm2.geojson", 6),
        stageStamped("admin-adm3.geojson", 8),
        stageStamped("admin-adm4.geojson", 10),
      ]);
      return [
        { layer: "adm1", file: layersFile("admin-adm1.geojson") },
        { layer: "adm2", file: adm2 },
        { layer: "adm3", file: adm3 },
        { layer: "adm4", file: adm4 },
      ];
    },
  },
  {
    output: "electoral.pmtiles",
    minzoom: 0,
    maxzoom: 14,
    include: ["id", "name"],
    freshnessInputs: () => [layersFile("electoral-divisions.geojson"), layersFile("polling-divisions.geojson")],
    async prepare() {
      // ed needs no stamp (z0-14 matches the tileset default); pd starts at z7.
      const pd = await stageStamped("polling-divisions.geojson", 7);
      return [
        { layer: "ed", file: layersFile("electoral-divisions.geojson") },
        { layer: "pd", file: pd },
      ];
    },
  },
  {
    output: "transport.pmtiles",
    minzoom: 6,
    maxzoom: 14,
    include: ["highway", "ref", "name", "kind"],
    dropDensestAsNeeded: true,
    freshnessInputs: () => [layersFile("roads.geojson"), layersFile("railways.geojson")],
    async prepare() {
      // Both layers start at z6 (the tileset-wide minzoom), so neither needs
      // a per-feature stamp.
      return [
        { layer: "roads", file: layersFile("roads.geojson") },
        { layer: "railways", file: layersFile("railways.geojson") },
      ];
    },
  },
  {
    output: "water.pmtiles",
    minzoom: 6,
    maxzoom: 14,
    dropDensestAsNeeded: true,
    freshnessInputs: () => [layersFile("waterways.geojson")],
    async prepare() {
      return [{ layer: "waterways", file: layersFile("waterways.geojson") }];
    },
  },
  {
    output: "protected.pmtiles",
    minzoom: 6,
    maxzoom: 14,
    freshnessInputs: () => [layersFile("protected-areas.geojson")],
    async prepare() {
      return [{ layer: "protected", file: layersFile("protected-areas.geojson") }];
    },
  },
  {
    output: "places.pmtiles",
    minzoom: 0,
    maxzoom: 14,
    // places/postal come straight from the DB, not a layers/*.geojson file
    // written by an earlier step — lanka.sqlite is their real upstream
    // input. This is coarser than per-table freshness (any table's write
    // touches the same file's mtime), so an unrelated table change can
    // trigger an unnecessary rebuild here, but never a stale skip.
    freshnessInputs: () => [DB_PATH],
    async prepare(ctx) {
      const { db } = ctx;
      await mkdir(layersDir(), { recursive: true });

      const placeRows = db
        .prepare(`SELECT id, name_en, name_si, name_ta, kind, lat, lon, population FROM places ORDER BY id`)
        .all() as PlaceRow[];
      const placesFc = placesFeatureCollection(placeRows);
      const placesPath = layersFile("places.geojson");
      await writeFile(placesPath, JSON.stringify(placesFc), "utf8");
      ctx.log(`tiles: wrote layers/places.geojson (${placesFc.features.length} features)`);

      const postalRows = db.prepare(`SELECT code, name, lat, lon FROM postal_codes ORDER BY code`).all() as PostalRow[];
      const postalFc = postalFeatureCollection(postalRows);
      const postalPath = layersFile("postal.geojson");
      await writeFile(postalPath, JSON.stringify(postalFc), "utf8");
      ctx.log(`tiles: wrote layers/postal.geojson (${postalFc.features.length} features)`);

      return [
        { layer: "places", file: placesPath },
        { layer: "postal", file: postalPath },
      ];
    },
  },
];

function buildArgs(spec: TilesetSpec, layers: PreparedLayer[], outputPath: string): string[] {
  const args = [
    "--force",
    "--no-progress-indicator", // -Q: suppresses progress spam but still reports warnings on stderr
    "--projection=EPSG:4326",
    `--minimum-zoom=${spec.minzoom}`,
    `--maximum-zoom=${spec.maxzoom}`,
    `--output=${outputPath}`,
  ];
  if (spec.dropDensestAsNeeded) args.push("--drop-densest-as-needed");
  if (spec.include) for (const prop of spec.include) args.push(`--include=${prop}`);
  for (const l of layers) args.push(`--named-layer=${JSON.stringify({ file: l.file, layer: l.layer })}`);
  return args;
}

/** Runs tippecanoe for one tileset, logging any warning lines from its (otherwise quieted) stderr. */
function runTippecanoe(spec: TilesetSpec, layers: PreparedLayer[], outputPath: string, log: (msg: string) => void): void {
  const args = buildArgs(spec, layers, outputPath);
  const result = spawnSync("tippecanoe", args, { encoding: "utf8" });
  if (result.error) throw result.error;
  if (result.status !== 0) {
    throw new Error(`tiles: tippecanoe failed for ${spec.output} (exit ${result.status}):\n${result.stderr}`);
  }
  const warnings = (result.stderr ?? "")
    .split("\n")
    .filter((line) => /warning/i.test(line) && line.trim().length > 0);
  for (const w of warnings) log(`tiles: [${spec.output}] ${w.trim()}`);
}

/**
 * tiles: builds PMTiles vector tilesets (docs/architecture.md §3,
 * `tiles/<layer>.pmtiles`) from the GeoJSON `layers`/`admin-geometry` wrote
 * plus a `places`/`postal` GeoJSON generated here from the DB, via the host
 * `tippecanoe` binary. Idempotent per tileset: skips any tileset whose
 * output is newer than all of its declared inputs, unless
 * `FOUNDRY_FORCE_TILES=1` is set.
 */
export async function run(ctx: StepContext): Promise<void> {
  const { log } = ctx;
  checkTippecanoe(log);

  const force = process.env.FOUNDRY_FORCE_TILES === "1";
  await mkdir(tilesDir(), { recursive: true });
  await mkdir(stagingDir(), { recursive: true });

  let built = 0;
  let skipped = 0;

  try {
    for (const spec of TILESETS) {
      const outputPath = path.join(tilesDir(), spec.output);

      if (!(await needsRebuild(outputPath, spec.freshnessInputs(), force))) {
        log(`tiles: skipping ${spec.output} (up to date)`);
        skipped++;
        continue;
      }

      const layers = await spec.prepare(ctx);
      const t0 = Date.now();
      runTippecanoe(spec, layers, outputPath, log);
      const { size } = await stat(outputPath);
      log(`tiles: wrote ${spec.output} (${size} bytes, ${layers.length} layer(s)) in ${Date.now() - t0}ms`);
      built++;
    }
  } finally {
    await rm(stagingDir(), { recursive: true, force: true });
  }

  log(`tiles: built ${built} tileset(s), skipped ${skipped}`);
}
