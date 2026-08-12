import Database from "better-sqlite3";
import { mkdirSync } from "node:fs";
import path from "node:path";
import { DB_PATH } from "./lib/paths.ts";

/**
 * Schema — exactly per docs/architecture.md §2, plus the indexes needed by
 * the API's query patterns (§4: point lookups, admin hierarchy walks,
 * election lookups). Every table declared here is populated by some step in
 * `src/build.ts`'s PIPELINE — see foundry/README.md's pipeline table for
 * which step owns which table.
 *
 * Schema creation is idempotent (CREATE TABLE/INDEX IF NOT EXISTS) and runs
 * on every DB open, not only in the `emit` step. That is what lets any step
 * run standalone via `--only <step>` against a partially-built database.
 */
const SCHEMA = `
CREATE TABLE IF NOT EXISTS meta (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS admin_units (
  pcode TEXT PRIMARY KEY,
  level INTEGER NOT NULL,
  name_en TEXT NOT NULL,
  name_si TEXT,
  name_ta TEXT,
  parent_pcode TEXT REFERENCES admin_units(pcode),
  area_km2 REAL,
  centroid_lat REAL,
  centroid_lon REAL
);
CREATE INDEX IF NOT EXISTS idx_admin_units_parent ON admin_units(parent_pcode);
CREATE INDEX IF NOT EXISTS idx_admin_units_level ON admin_units(level);

-- Simplified boundary geometry for instant map highlights (NOT for tiles —
-- tiles come from PMTiles). Bare GeoJSON geometry (no Feature wrapper),
-- Douglas-Peucker simplified to ~0.0005deg tolerance, 6 dp coordinates.
CREATE TABLE IF NOT EXISTS admin_geometry (
  pcode TEXT PRIMARY KEY REFERENCES admin_units(pcode),
  geojson TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS admin_population (
  pcode TEXT NOT NULL REFERENCES admin_units(pcode),
  year INTEGER NOT NULL,
  sex TEXT NOT NULL,
  age_bucket TEXT NOT NULL,
  count INTEGER NOT NULL,
  PRIMARY KEY (pcode, year, sex, age_bucket)
);

CREATE TABLE IF NOT EXISTS admin_stats (
  pcode TEXT NOT NULL,
  year INTEGER NOT NULL,
  key TEXT NOT NULL,
  value REAL NOT NULL,
  PRIMARY KEY (pcode, year, key)
);

CREATE TABLE IF NOT EXISTS cells (
  cell_id INTEGER PRIMARY KEY,
  pop REAL NOT NULL
);

CREATE TABLE IF NOT EXISTS cell_lookup (
  cell_id INTEGER PRIMARY KEY,
  gnd_pcode TEXT NOT NULL,
  postal_code TEXT,
  nearest_place_id INTEGER,
  nearest_place_dist_m INTEGER
);
CREATE INDEX IF NOT EXISTS idx_cell_lookup_gnd ON cell_lookup(gnd_pcode);

CREATE TABLE IF NOT EXISTS places (
  id INTEGER PRIMARY KEY,
  source TEXT NOT NULL,
  source_id TEXT,
  kind TEXT NOT NULL,
  name_en TEXT NOT NULL,
  name_si TEXT,
  name_ta TEXT,
  lat REAL NOT NULL,
  lon REAL NOT NULL,
  population INTEGER,
  admin_pcode TEXT
);
CREATE INDEX IF NOT EXISTS idx_places_admin_pcode ON places(admin_pcode);
CREATE INDEX IF NOT EXISTS idx_places_kind ON places(kind);

CREATE VIRTUAL TABLE IF NOT EXISTS places_fts USING fts5(
  name_en, name_si, name_ta,
  content='places', content_rowid='id',
  tokenize='unicode61'
);

CREATE TABLE IF NOT EXISTS postal_codes (
  code TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  admin_pcode TEXT,
  lat REAL,
  lon REAL
);
CREATE INDEX IF NOT EXISTS idx_postal_codes_admin_pcode ON postal_codes(admin_pcode);

CREATE TABLE IF NOT EXISTS pois (
  id INTEGER PRIMARY KEY,
  category TEXT NOT NULL,
  kind TEXT,
  name_en TEXT,
  name_si TEXT,
  name_ta TEXT,
  lat REAL NOT NULL,
  lon REAL NOT NULL,
  admin_pcode TEXT,
  attrs TEXT
);
CREATE INDEX IF NOT EXISTS idx_pois_category ON pois(category);
CREATE INDEX IF NOT EXISTS idx_pois_admin_pcode ON pois(admin_pcode);

CREATE TABLE IF NOT EXISTS elections (
  id TEXT PRIMARY KEY,
  type TEXT,
  year INTEGER,
  date TEXT,
  label TEXT
);

CREATE TABLE IF NOT EXISTS election_parties (
  election_id TEXT,
  code TEXT,
  candidate TEXT,
  name TEXT,
  color TEXT,
  PRIMARY KEY (election_id, code)
);

CREATE TABLE IF NOT EXISTS election_entities (
  entity_id TEXT PRIMARY KEY,
  kind TEXT NOT NULL,
  name TEXT,
  ed_id TEXT
);
CREATE INDEX IF NOT EXISTS idx_election_entities_kind ON election_entities(kind);
CREATE INDEX IF NOT EXISTS idx_election_entities_ed_id ON election_entities(ed_id);

CREATE TABLE IF NOT EXISTS election_results (
  election_id TEXT NOT NULL,
  entity_id TEXT NOT NULL,
  electors INTEGER,
  polled INTEGER,
  valid INTEGER,
  rejected INTEGER,
  winner_party TEXT,
  winner_votes INTEGER,
  results TEXT NOT NULL,
  PRIMARY KEY (election_id, entity_id)
);
CREATE INDEX IF NOT EXISTS idx_election_results_entity ON election_results(entity_id);

CREATE TABLE IF NOT EXISTS datasets (
  id TEXT PRIMARY KEY,
  title TEXT,
  category TEXT,
  description TEXT,
  source_name TEXT,
  source_url TEXT,
  license TEXT,
  feature_count INTEGER,
  download_path TEXT
);
`;

let db: Database.Database | undefined;

/** Opens (creating if needed) the build database and ensures the schema exists. */
export function getDb(): Database.Database {
  if (db) return db;
  mkdirSync(path.dirname(DB_PATH), { recursive: true });
  db = new Database(DB_PATH);
  // WAL during the build for concurrent-friendly writes; emit finalizes to
  // journal_mode=OFF per docs/architecture.md §3.
  db.pragma("journal_mode = WAL");
  db.exec(SCHEMA);
  return db;
}

export function closeDb(): void {
  db?.close();
  db = undefined;
}
