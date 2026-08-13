import { mkdir, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import Database from "better-sqlite3";
import type { StepContext } from "../step.ts";
import { DB_PATH } from "../lib/paths.ts";

export const name = "packages";

// foundry/src/steps/packages.ts -> repo root (one level above `foundry/`).
const REPO_ROOT = path.resolve(fileURLToPath(import.meta.url), "../../../..");

export function round(n: number | null | undefined, dp: number): number | null {
  if (n === null || n === undefined) return null;
  const f = 10 ** dp;
  return Math.round(n * f) / f;
}

/** Reads a required `meta` row — throws with an actionable message if the build hasn't reached `emit` yet. */
export function metaValue(db: Database.Database, key: string): string {
  const row = db.prepare(`SELECT value FROM meta WHERE key = ?`).get(key) as { value: string } | undefined;
  if (!row) throw new Error(`packages: meta.${key} is not set — run the full pipeline (through \`emit\`) first`);
  return row.value;
}

interface DatasetSource {
  name: string;
  url: string;
  license: string;
}

function datasetSource(db: Database.Database, id: string): DatasetSource {
  const row = db.prepare(`SELECT source_name, source_url, license FROM datasets WHERE id = ?`).get(id) as
    | { source_name: string; source_url: string; license: string }
    | undefined;
  if (!row) throw new Error(`packages: datasets row "${id}" not found — run \`datasets\` first`);
  return { name: row.source_name, url: row.source_url, license: row.license };
}

interface AdminUnitRow {
  pcode: string;
  level: number;
  name_en: string;
  name_si: string | null;
  name_ta: string | null;
  parent_pcode: string | null;
  area_km2: number | null;
  centroid_lat: number | null;
  centroid_lon: number | null;
}

export type AdminUnitTuple = [
  string,
  number,
  string,
  string | null,
  string | null,
  string | null,
  number | null,
  number | null,
  number | null,
];

export interface AdminPackage {
  data_version: string;
  generated: string;
  sources: DatasetSource[];
  units: AdminUnitTuple[];
}

/** packages/admin/data/admin.json — all admin_units rows, array-of-arrays, ordered by pcode. */
export function buildAdmin(db: Database.Database, dataVersion: string, generated: string): AdminPackage {
  const source = datasetSource(db, "admin-units");
  const rows = db
    .prepare(
      `SELECT pcode, level, name_en, name_si, name_ta, parent_pcode, area_km2, centroid_lat, centroid_lon
       FROM admin_units ORDER BY pcode`,
    )
    .all() as AdminUnitRow[];

  const units: AdminUnitTuple[] = rows.map((r) => [
    r.pcode,
    r.level,
    r.name_en,
    r.name_si,
    r.name_ta,
    r.parent_pcode,
    round(r.area_km2, 2),
    round(r.centroid_lat, 6),
    round(r.centroid_lon, 6),
  ]);

  return { data_version: dataVersion, generated, sources: [source], units };
}

interface PostalCodeRow {
  code: string;
  name: string;
  admin_pcode: string | null;
  lat: number | null;
  lon: number | null;
}

export type PostalCodeTuple = [string, string, string | null, number | null, number | null];
export type DivisionCodeTuple = [string, number];

export interface PostalPackage {
  data_version: string;
  generated: string;
  sources: DatasetSource[];
  codes: PostalCodeTuple[];
  division_codes: Record<string, DivisionCodeTuple[]>;
}

/**
 * packages/postal/data/postal.json — postal_codes ordered by code, plus
 * admin_postal rolled up per pcode (desc by share — the SQL ORDER BY does
 * the sort so the JS side just groups in the order rows arrive).
 */
export function buildPostal(db: Database.Database, dataVersion: string, generated: string): PostalPackage {
  const source = datasetSource(db, "postal-codes");
  const codeRows = db.prepare(`SELECT code, name, admin_pcode, lat, lon FROM postal_codes ORDER BY code`).all() as PostalCodeRow[];
  const codes: PostalCodeTuple[] = codeRows.map((r) => [r.code, r.name, r.admin_pcode, round(r.lat, 6), round(r.lon, 6)]);

  const divisionRows = db
    .prepare(`SELECT pcode, code, share FROM admin_postal ORDER BY pcode, share DESC, code`)
    .all() as { pcode: string; code: string; share: number }[];

  const division_codes: Record<string, DivisionCodeTuple[]> = {};
  for (const r of divisionRows) {
    (division_codes[r.pcode] ??= []).push([r.code, round(r.share, 4) as number]);
  }

  return { data_version: dataVersion, generated, sources: [source], codes, division_codes };
}

interface ElectionRow {
  id: string;
  type: string | null;
  year: number | null;
  date: string | null;
  label: string | null;
}

interface EntityRow {
  entity_id: string;
  kind: string;
  name: string | null;
  ed_id: string | null;
}

interface PartyRow {
  election_id: string;
  code: string;
  candidate: string | null;
  name: string | null;
  color: string | null;
}

interface ResultRow {
  election_id: string;
  entity_id: string;
  electors: number | null;
  polled: number | null;
  valid: number | null;
  rejected: number | null;
  winner_party: string | null;
  winner_votes: number | null;
  results: string;
}

// The `results` TEXT column (docs/architecture.md §2) is the full computed
// breakdown (winner/runnerUp/margin/turnoutPct/topParties/otherVotes) built
// by `elections.ts`. Everything except the per-party vote list is derivable
// from electors/polled/valid/rejected + winner_party/winner_votes (already
// dedicated election_results columns) + the party list itself, so only
// `topParties` gets re-emitted here, as compact [party, votes] pairs —
// already sorted by votes desc in the source JSON.
interface ParsedResults {
  topParties?: { party: string; votes: number }[];
}

export type EntityTuple = [string, string, string | null, string | null];
export type PartyTuple = [string, string, string | null, string | null, string | null];
export type PartyVoteTuple = [string, number];

export interface ElectionResultCompact {
  electors: number | null;
  polled: number | null;
  valid: number | null;
  rejected: number | null;
  winner_party: string | null;
  winner_votes: number | null;
  parties: PartyVoteTuple[];
}

export interface ElectoralPackage {
  data_version: string;
  generated: string;
  sources: DatasetSource[];
  elections: ElectionRow[];
  entities: EntityTuple[];
  parties: PartyTuple[];
  results: Record<string, Record<string, ElectionResultCompact>>;
}

/** packages/electoral/data/electoral.json */
export function buildElectoral(db: Database.Database, dataVersion: string, generated: string): ElectoralPackage {
  const source = datasetSource(db, "elections");

  const elections = db.prepare(`SELECT id, type, year, date, label FROM elections ORDER BY id`).all() as ElectionRow[];

  const entityRows = db.prepare(`SELECT entity_id, kind, name, ed_id FROM election_entities ORDER BY entity_id`).all() as EntityRow[];
  const entities: EntityTuple[] = entityRows.map((r) => [r.entity_id, r.kind, r.name, r.ed_id]);

  const partyRows = db
    .prepare(`SELECT election_id, code, candidate, name, color FROM election_parties ORDER BY election_id, code`)
    .all() as PartyRow[];
  const parties: PartyTuple[] = partyRows.map((r) => [r.election_id, r.code, r.candidate, r.name, r.color]);

  const resultRows = db
    .prepare(
      `SELECT election_id, entity_id, electors, polled, valid, rejected, winner_party, winner_votes, results
       FROM election_results ORDER BY election_id, entity_id`,
    )
    .all() as ResultRow[];

  const results: Record<string, Record<string, ElectionResultCompact>> = {};
  for (const r of resultRows) {
    const parsed = JSON.parse(r.results) as ParsedResults;
    const partyVotes: PartyVoteTuple[] = (parsed.topParties ?? []).map((p) => [p.party, p.votes]);
    const byElection = (results[r.election_id] ??= {});
    byElection[r.entity_id] = {
      electors: r.electors,
      polled: r.polled,
      valid: r.valid,
      rejected: r.rejected,
      winner_party: r.winner_party,
      winner_votes: r.winner_votes,
      parties: partyVotes,
    };
  }

  return { data_version: dataVersion, generated, sources: [source], elections, entities, parties, results };
}

interface PopulationRow {
  pcode: string;
  sex: string;
  age_bucket: string;
  count: number;
}

export interface PopulationEntry {
  t: number;
  m: number;
  f: number;
  ages: Record<string, number>;
}

interface StatsRow {
  pcode: string;
  key: string;
  value: number;
}

export interface StatsEntry {
  ethnicity: Record<string, number>;
  religion: Record<string, number>;
}

export interface CensusPackage {
  data_version: string;
  generated: string;
  sources: DatasetSource[];
  population_2024: Record<string, PopulationEntry>;
  stats_2024: Record<string, StatsEntry>;
  stats_2012: Record<string, StatsEntry>;
}

function buildStats(db: Database.Database, year: number): Record<string, StatsEntry> {
  const rows = db.prepare(`SELECT pcode, key, value FROM admin_stats WHERE year = ? ORDER BY pcode, key`).all(year) as StatsRow[];
  const out: Record<string, StatsEntry> = {};
  for (const r of rows) {
    const dot = r.key.indexOf(".");
    if (dot === -1) continue;
    const group = r.key.slice(0, dot);
    const label = r.key.slice(dot + 1);
    if (group !== "ethnicity" && group !== "religion") continue;
    const entry = (out[r.pcode] ??= { ethnicity: {}, religion: {} });
    entry[group][label] = r.value;
  }
  return out;
}

/** packages/census/data/census.json */
export function buildCensus(db: Database.Database, dataVersion: string, generated: string): CensusPackage {
  const source = datasetSource(db, "census-2024");

  const popRows = db
    .prepare(`SELECT pcode, sex, age_bucket, count FROM admin_population WHERE year = 2024 ORDER BY pcode, sex, age_bucket`)
    .all() as PopulationRow[];

  const population_2024: Record<string, PopulationEntry> = {};
  for (const r of popRows) {
    const entry = (population_2024[r.pcode] ??= { t: 0, m: 0, f: 0, ages: {} });
    if (r.age_bucket === "total") {
      if (r.sex === "t") entry.t = r.count;
      else if (r.sex === "m") entry.m = r.count;
      else if (r.sex === "f") entry.f = r.count;
    } else if (r.sex === "t") {
      entry.ages[r.age_bucket] = r.count;
    }
  }

  const stats_2024 = buildStats(db, 2024);
  const stats_2012 = buildStats(db, 2012);

  return { data_version: dataVersion, generated, sources: [source], population_2024, stats_2024, stats_2012 };
}

interface SanityInput {
  adminUnitCount: number;
  postalCodeCount: number;
  lkPopulation2024: number | undefined;
  divisionPcodes: string[];
  knownAdminPcodes: Set<string>;
}

/**
 * The four build*() functions above only shape rows already read from the
 * DB — they don't second-guess row counts. This is the single place that
 * enforces the "did the upstream pipeline actually run" invariants the
 * emitted packages depend on, so it's separately testable against both
 * healthy and deliberately-broken fixtures without needing 14k+ row tables.
 */
export function assertSanity(input: SanityInput): void {
  if (input.adminUnitCount <= 14000) {
    throw new Error(`packages: admin_units has ${input.adminUnitCount} rows, expected > 14000`);
  }
  if (input.postalCodeCount <= 1800) {
    throw new Error(`packages: postal_codes has ${input.postalCodeCount} rows, expected > 1800`);
  }
  if (!input.lkPopulation2024 || input.lkPopulation2024 <= 20_000_000) {
    throw new Error(`packages: census population_2024["LK"].t is ${input.lkPopulation2024 ?? "missing"}, expected > 20,000,000`);
  }
  for (const pcode of input.divisionPcodes) {
    if (!input.knownAdminPcodes.has(pcode)) {
      throw new Error(`packages: admin_postal has division_codes for pcode "${pcode}", which is not in admin_units`);
    }
  }
}

async function writePackageJson(pkg: string, data: unknown): Promise<{ file: string; bytes: number }> {
  const dir = path.join(REPO_ROOT, "packages", pkg, "data");
  await mkdir(dir, { recursive: true });
  const file = path.join(dir, `${pkg}.json`);
  const json = JSON.stringify(data);
  await writeFile(file, json, "utf8");
  return { file, bytes: Buffer.byteLength(json, "utf8") };
}

/**
 * packages: emits four compact, single-line JSON data files for standalone
 * npm packages (packages/{admin,postal,electoral,census}/data/*.json) from
 * the finished lanka.sqlite. Runs last, after `emit`, because it needs
 * meta.data_version — only stamped there — and `generated` is meta.built_at
 * (not `new Date()`), so re-running this step alone against an unchanged
 * artifact produces byte-identical output.
 *
 * Deliberately opens its own read-only connection to DB_PATH rather than
 * using `ctx.db`: by the time this step runs in a full pipeline, `emit` has
 * already closed the shared connection (its last act before VACUUM +
 * journal_mode=OFF finalizes the artifact); reusing `ctx.db` would either
 * hit that closed connection, or — for a bare `--only packages` run — pick
 * up the read-write connection `build.ts` opens unconditionally before
 * filtering steps (which forces journal_mode back to WAL, the opposite of
 * what emit just finalized). A dedicated read-only handle sidesteps both
 * and matches this step's actual job: read the finished artifact, don't
 * touch it.
 */
export async function run({ log }: StepContext): Promise<void> {
  const db = new Database(DB_PATH, { readonly: true, fileMustExist: true });
  try {
    const dataVersion = metaValue(db, "data_version");
    const generated = metaValue(db, "built_at");

    const admin = buildAdmin(db, dataVersion, generated);
    const postal = buildPostal(db, dataVersion, generated);
    const electoral = buildElectoral(db, dataVersion, generated);
    const census = buildCensus(db, dataVersion, generated);

    assertSanity({
      adminUnitCount: admin.units.length,
      postalCodeCount: postal.codes.length,
      lkPopulation2024: census.population_2024["LK"]?.t,
      divisionPcodes: Object.keys(postal.division_codes),
      knownAdminPcodes: new Set(admin.units.map((u) => u[0])),
    });

    const written = await Promise.all([
      writePackageJson("admin", admin),
      writePackageJson("postal", postal),
      writePackageJson("electoral", electoral),
      writePackageJson("census", census),
    ]);

    for (const w of written) {
      log(`packages: wrote ${path.relative(REPO_ROOT, w.file)} (${w.bytes} bytes)`);
    }
  } finally {
    db.close();
  }
}
