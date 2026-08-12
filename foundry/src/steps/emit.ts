import { readdir, readFile, stat, writeFile } from "node:fs/promises";
import path from "node:path";
import type { StepContext } from "../step.ts";
import { closeDb } from "../db.ts";
import { artifactPath, DB_PATH, MANIFEST_PATH, rawPath } from "../lib/paths.ts";
import { sha256File } from "../lib/hash.ts";

export const name = "emit";

interface SourceEntry {
  id: string;
  license: string;
  url: string;
  /** Raw files that back this source — fetched_at is the newest of these mtimes. */
  files: string[];
}

// One entry per upstream source touched by this build. License/url text
// mirrors the top-level README.md data table.
const SOURCES: SourceEntry[] = [
  {
    id: "admin-boundaries",
    license: "CC BY 3.0 IGO",
    url: "https://www.geoboundaries.org",
    // Levels 0-2 only — level 3 (DS divisions) moved to COD-AB below
    // (admin.ts no longer reads geo/ds-divisions.geojson, which seed still
    // fetches but nothing consumes: see foundry/README.md's design notes).
    files: ["geo/country.geojson", "geo/provinces.geojson", "geo/districts.geojson"],
  },
  {
    id: "admin-boundaries-cod-ab",
    license: "CC BY-IGO",
    url: "https://data.humdata.org/dataset/cod-ab-lka",
    files: ["cod-ab/lka_admin3.geojson", "cod-ab/lka_admin4.geojson"],
  },
  {
    id: "population-grid",
    license: "CC BY 4.0",
    url: "https://www.worldpop.org",
    files: ["worldpop/lka_ppp_2020_1km_Aggregated_UNadj.tif"],
  },
  {
    id: "population-2023",
    license: "CC BY-IGO",
    url: "https://data.humdata.org/dataset/cod-ps-lka",
    files: ["data/population-2023.json"],
  },
  {
    id: "census-ethnicity-religion-2012",
    license: "Open data",
    url: "http://www.statistics.gov.lk/pophousat/cph2011/index.php?fileName=Ethnicity&gp=Activities&tpl=3",
    files: ["data/ethnicity-religion-2012.json"],
  },
  {
    id: "osm-places",
    license: "ODbL",
    url: "https://www.openstreetmap.org",
    files: ["geo/cities.geojson"],
  },
  {
    id: "osm-pois",
    license: "ODbL",
    url: "https://www.openstreetmap.org",
    files: ["geo/hospitals.geojson", "geo/education.geojson", "geo/airports.geojson"],
  },
  {
    id: "osm-layers",
    license: "ODbL",
    url: "https://www.openstreetmap.org",
    files: ["geo/roads.geojson", "geo/railways.geojson", "geo/waterways.geojson", "geo/protected-areas.geojson"],
  },
  {
    id: "electoral-boundaries",
    license: "open",
    url: "https://github.com/nuuuwan/sl-topojson",
    files: ["geo/electoral-divisions.geojson", "geo/polling-divisions.geojson"],
  },
  {
    id: "election-results",
    license: "open",
    url: "https://github.com/nuuuwan/lk_elections",
    files: ["data/election-pres-2024.json", "data/election-pres-2019.json", "data/election-pres-2015.json", "data/election-parl-2024.json"],
  },
  {
    id: "postal-codes",
    license: "CC BY 4.0",
    url: "https://download.geonames.org/export/zip/LK.zip",
    files: ["postal/LK.txt"],
  },
];

interface ArtifactEntry {
  path: string;
  bytes: number;
  sha256: string;
}

/**
 * Discovers the `.pmtiles` files the `tiles` step wrote (data/artifacts/
 * tiles/*.pmtiles) so they're hashed into the manifest alongside
 * geopub.sqlite. Returns [] rather than failing if `tiles/` doesn't exist —
 * an `--only` run that skipped `tiles` still produces a valid manifest, just
 * without tile entries.
 */
async function tileArtifacts(): Promise<ArtifactEntry[]> {
  const tilesDir = artifactPath("tiles");
  let files: string[];
  try {
    files = (await readdir(tilesDir)).filter((f) => f.endsWith(".pmtiles")).sort();
  } catch {
    return [];
  }
  return Promise.all(
    files.map(async (f): Promise<ArtifactEntry> => {
      const p = path.join(tilesDir, f);
      const [{ size }, sha256] = await Promise.all([stat(p), sha256File(p)]);
      return { path: `tiles/${f}`, bytes: size, sha256 };
    }),
  );
}

async function newestMtime(relFiles: string[]): Promise<Date> {
  const stats = await Promise.all(
    relFiles.map(async (f) => {
      try {
        return await stat(rawPath(f));
      } catch {
        return null;
      }
    }),
  );
  const present = stats.filter((s): s is NonNullable<typeof s> => s !== null);
  if (present.length === 0) throw new Error(`emit: none of the raw files for a source exist: ${relFiles.join(", ")}`);
  return present.reduce((latest, s) => (s.mtime > latest ? s.mtime : latest), present[0]!.mtime);
}

/** YYYYMMDD.N — N increments if a manifest for the same day already exists. */
async function nextDataVersion(): Promise<string> {
  const today = new Date();
  const ymd = `${today.getFullYear()}${String(today.getMonth() + 1).padStart(2, "0")}${String(today.getDate()).padStart(2, "0")}`;
  let n = 1;
  try {
    const prev = JSON.parse(await readFile(MANIFEST_PATH, "utf8")) as { data_version?: string };
    const [prevYmd, prevN] = (prev.data_version ?? "").split(".");
    if (prevYmd === ymd && prevN) n = Number(prevN) + 1;
  } catch {
    // No prior manifest, or unreadable — start at .1.
  }
  return `${ymd}.${n}`;
}

/**
 * Finalizes the build: stamps meta.data_version/built_at, switches to
 * journal_mode=OFF and VACUUMs (docs/architecture.md §3), then writes
 * manifest.json with per-source attribution and the artifact's sha256.
 */
export async function run({ db, log }: StepContext): Promise<void> {
  const dataVersion = await nextDataVersion();
  const builtAt = new Date().toISOString();

  const sourceEntries = await Promise.all(
    SOURCES.map(async (s) => ({
      id: s.id,
      fetched_at: (await newestMtime(s.files)).toISOString(),
      license: s.license,
      url: s.url,
    })),
  );

  const insertMeta = db.prepare(`INSERT INTO meta (key, value) VALUES (?, ?) ON CONFLICT(key) DO UPDATE SET value=excluded.value`);
  const tx = db.transaction(() => {
    insertMeta.run("data_version", dataVersion);
    insertMeta.run("built_at", builtAt);
    for (const s of sourceEntries) insertMeta.run(`source.${s.id}.fetched_at`, s.fetched_at);
  });
  tx();

  // Finalize per docs/architecture.md §3: read-only, fully vacuumed artifact.
  // better-sqlite3 runs with SQLITE_DBCONFIG_DEFENSIVE on by default, which
  // silently refuses to switch into OFF journal mode (the pragma no-ops and
  // reports the unchanged mode back). unsafeMode(true) lifts that guard for
  // this connection. It's safe here specifically because this is the very
  // last write this connection makes before VACUUM + close — there's no
  // further transaction for a missing rollback journal to protect. Note
  // OFF (like MEMORY) is a per-connection setting, not something SQLite
  // persists into the file header, so a fresh connection (e.g. the API
  // opening this artifact later) will see the default journal mode again —
  // that's expected and fine for a connection that only ever reads.
  db.unsafeMode(true);
  db.pragma("journal_mode = OFF");
  db.exec("VACUUM");
  closeDb();

  const dbStat = await stat(DB_PATH);
  const dbSha256 = await sha256File(DB_PATH);
  const tiles = await tileArtifacts();

  const manifest = {
    data_version: dataVersion,
    built_at: builtAt,
    sources: sourceEntries,
    artifacts: [{ path: "geopub.sqlite", bytes: dbStat.size, sha256: dbSha256 }, ...tiles],
  };
  await writeFile(MANIFEST_PATH, JSON.stringify(manifest, null, 2) + "\n", "utf8");

  log(
    `emit: data_version=${dataVersion}, geopub.sqlite=${dbStat.size} bytes, ${tiles.length} tile artifact(s), manifest.json written`,
  );
}
