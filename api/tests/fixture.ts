import Database from "better-sqlite3";
import { cellId, GRID } from "@geopub/shared";

/**
 * Builds an in-memory SQLite database implementing the full contract schema
 * (docs/architecture.md §2) with a small, hand-picked dataset covering
 * every route this package serves. Colombo Fort (6.9344, 79.8428) is the
 * fixture's spatial anchor — its cell id is pinned by shared/src/grid.test.ts
 * so any grid drift would break both suites together.
 */
export const DATA_VERSION = "20260101.1";
export const FORT_LAT = 6.9344;
export const FORT_LON = 79.8428;
export const FORT_CELL_ID = cellId(FORT_LAT, FORT_LON)!;

export function buildFixtureDb(): Database.Database {
  const db = new Database(":memory:");
  db.pragma("journal_mode = MEMORY");

  db.exec(`
    CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);

    CREATE TABLE admin_units (
      pcode TEXT PRIMARY KEY, level INTEGER NOT NULL,
      name_en TEXT NOT NULL, name_si TEXT, name_ta TEXT,
      parent_pcode TEXT REFERENCES admin_units(pcode),
      area_km2 REAL, centroid_lat REAL, centroid_lon REAL
    );

    CREATE TABLE admin_population (
      pcode TEXT NOT NULL REFERENCES admin_units(pcode),
      year INTEGER NOT NULL, sex TEXT NOT NULL, age_bucket TEXT NOT NULL,
      count INTEGER NOT NULL,
      PRIMARY KEY (pcode, year, sex, age_bucket)
    );

    CREATE TABLE admin_stats (
      pcode TEXT NOT NULL, year INTEGER NOT NULL, key TEXT NOT NULL,
      value REAL NOT NULL,
      PRIMARY KEY (pcode, year, key)
    );

    CREATE TABLE cells (cell_id INTEGER PRIMARY KEY, pop REAL NOT NULL);

    CREATE TABLE cell_lookup (
      cell_id INTEGER PRIMARY KEY,
      gnd_pcode TEXT NOT NULL,
      postal_code TEXT,
      nearest_place_id INTEGER,
      nearest_place_dist_m INTEGER
    );

    CREATE TABLE places (
      id INTEGER PRIMARY KEY, source TEXT NOT NULL, source_id TEXT,
      kind TEXT NOT NULL,
      name_en TEXT NOT NULL, name_si TEXT, name_ta TEXT,
      lat REAL NOT NULL, lon REAL NOT NULL,
      population INTEGER, admin_pcode TEXT
    );

    CREATE VIRTUAL TABLE places_fts USING fts5(
      name_en, name_si, name_ta,
      content='places', content_rowid='id', tokenize='unicode61'
    );

    CREATE TABLE postal_codes (
      code TEXT PRIMARY KEY, name TEXT NOT NULL,
      admin_pcode TEXT, lat REAL, lon REAL
    );

    CREATE TABLE pois (
      id INTEGER PRIMARY KEY, category TEXT NOT NULL, kind TEXT,
      name_en TEXT, name_si TEXT, name_ta TEXT,
      lat REAL NOT NULL, lon REAL NOT NULL,
      admin_pcode TEXT, attrs TEXT
    );

    CREATE TABLE elections (id TEXT PRIMARY KEY, type TEXT, year INTEGER, date TEXT, label TEXT);
    CREATE TABLE election_parties (
      election_id TEXT, code TEXT, candidate TEXT, name TEXT, color TEXT,
      PRIMARY KEY (election_id, code)
    );
    CREATE TABLE election_entities (entity_id TEXT PRIMARY KEY, kind TEXT NOT NULL, name TEXT, ed_id TEXT);
    CREATE TABLE election_results (
      election_id TEXT NOT NULL, entity_id TEXT NOT NULL,
      electors INTEGER, polled INTEGER, valid INTEGER, rejected INTEGER,
      winner_party TEXT, winner_votes INTEGER,
      results TEXT NOT NULL,
      PRIMARY KEY (election_id, entity_id)
    );

    CREATE TABLE datasets (
      id TEXT PRIMARY KEY, title TEXT, category TEXT, description TEXT,
      source_name TEXT, source_url TEXT, license TEXT,
      feature_count INTEGER, download_path TEXT
    );
  `);

  // --- meta ---------------------------------------------------------------
  const insertMeta = db.prepare("INSERT INTO meta (key, value) VALUES (?, ?)");
  insertMeta.run("data_version", DATA_VERSION);
  insertMeta.run("built_at", "2026-01-01T00:00:00Z");

  // --- datasets -------------------------------------------------------------
  const insertDataset = db.prepare(`
    INSERT INTO datasets (id, title, category, description, source_name, source_url, license, feature_count, download_path)
    VALUES (@id, @title, @category, @description, @source_name, @source_url, @license, @feature_count, @download_path)
  `);
  for (const d of [
    {
      id: "admin_boundaries",
      title: "Administrative boundaries",
      category: "admin",
      description: "ADM0-ADM4 boundaries and population",
      source_name: "geoBoundaries / HDX COD-AB",
      source_url: "https://www.geoboundaries.org",
      license: "CC BY 3.0 IGO",
      feature_count: 10,
      download_path: "downloads/admin_boundaries.geojson.gz",
    },
    {
      id: "gridded_population",
      title: "Gridded population",
      category: "population",
      description: "WorldPop 100m resampled onto the canonical grid",
      source_name: "WorldPop",
      source_url: "https://www.worldpop.org",
      license: "CC BY 4.0",
      feature_count: 25,
      download_path: "downloads/gridded_population.csv.gz",
    },
    {
      id: "places",
      title: "Named places",
      category: "places",
      description: "GeoNames + OSM cities, deduplicated",
      source_name: "GeoNames",
      source_url: "https://www.geonames.org",
      license: "CC BY 4.0",
      feature_count: 2,
      download_path: "downloads/places.geojson.gz",
    },
    {
      id: "postal_codes",
      title: "Postal codes",
      category: "places",
      description: "Sri Lanka postal code directory",
      source_name: "GeoNames postal dump",
      source_url: "https://www.geonames.org/postal-codes/",
      license: "CC BY 4.0",
      feature_count: 1,
      download_path: "downloads/postal_codes.csv.gz",
    },
    {
      id: "elections",
      title: "Election results",
      category: "elections",
      description: "Election Commission of Sri Lanka results",
      source_name: "Election Commission of Sri Lanka",
      source_url: "https://elections.gov.lk",
      license: "open",
      feature_count: 1,
      download_path: "downloads/elections.json.gz",
    },
  ]) {
    insertDataset.run(d);
  }

  // --- admin_units ----------------------------------------------------------
  // LK (country) -> LK1 (Western province) -> LK11 (Colombo district)
  //   -> LK1101 (Colombo DS division) -> LK110101 / LK110102 (GN divisions)
  //   -> LK1102 (Kolonnawa DS division) -> LK110201 (GN division)
  // LK (country) -> LK2 (Southern province) -> LK21 (Galle district)
  const insertAdmin = db.prepare(`
    INSERT INTO admin_units (pcode, level, name_en, name_si, name_ta, parent_pcode, area_km2, centroid_lat, centroid_lon)
    VALUES (@pcode, @level, @name_en, @name_si, @name_ta, @parent_pcode, @area_km2, @centroid_lat, @centroid_lon)
  `);
  for (const u of [
    { pcode: "LK", level: 0, name_en: "Sri Lanka", name_si: "ශ්‍රී ලංකාව", name_ta: "இலங்கை", parent_pcode: null, area_km2: 65610, centroid_lat: 7.87, centroid_lon: 80.77 },
    { pcode: "LK1", level: 1, name_en: "Western Province", name_si: "බස්නාහිර පළාත", name_ta: "மேல் மாகாணம்", parent_pcode: "LK", area_km2: 3709, centroid_lat: 6.97, centroid_lon: 80.03 },
    { pcode: "LK11", level: 2, name_en: "Colombo District", name_si: "කොළඹ දිස්ත්‍රික්කය", name_ta: "கொழும்பு மாவட்டம்", parent_pcode: "LK1", area_km2: 699, centroid_lat: 6.93, centroid_lon: 79.9 },
    { pcode: "LK1101", level: 3, name_en: "Colombo DS Division", name_si: null, name_ta: null, parent_pcode: "LK11", area_km2: 37.3, centroid_lat: 6.9319, centroid_lon: 79.8478 },
    { pcode: "LK1102", level: 3, name_en: "Kolonnawa DS Division", name_si: null, name_ta: null, parent_pcode: "LK11", area_km2: 8.5, centroid_lat: 6.9317, centroid_lon: 79.8886 },
    { pcode: "LK110101", level: 4, name_en: "Fort", name_si: "කොටුව", name_ta: "கோட்டை", parent_pcode: "LK1101", area_km2: 1.2, centroid_lat: FORT_LAT, centroid_lon: FORT_LON },
    { pcode: "LK110102", level: 4, name_en: "Kompannavidiya", name_si: null, name_ta: null, parent_pcode: "LK1101", area_km2: 1.5, centroid_lat: 6.921, centroid_lon: 79.8565 },
    { pcode: "LK110201", level: 4, name_en: "Kolonnawa", name_si: null, name_ta: null, parent_pcode: "LK1102", area_km2: 2.1, centroid_lat: 6.932, centroid_lon: 79.8886 },
    { pcode: "LK2", level: 1, name_en: "Southern Province", name_si: null, name_ta: null, parent_pcode: "LK", area_km2: 5559, centroid_lat: 6.13, centroid_lon: 80.5 },
    { pcode: "LK21", level: 2, name_en: "Galle District", name_si: null, name_ta: null, parent_pcode: "LK2", area_km2: 1652, centroid_lat: 6.05, centroid_lon: 80.22 },
  ]) {
    insertAdmin.run(u);
  }

  // --- admin_population (LK1101, year 2023: 17 age buckets + total, x3 sexes) --
  const AGE_BUCKETS = [
    "0-4", "5-9", "10-14", "15-19", "20-24", "25-29", "30-34", "35-39", "40-44",
    "45-49", "50-54", "55-59", "60-64", "65-69", "70-74", "75-79", "80+",
  ];
  const insertPop = db.prepare(
    "INSERT INTO admin_population (pcode, year, sex, age_bucket, count) VALUES (?, ?, ?, ?, ?)",
  );
  // f and m get the same per-bucket count (symmetric, deterministic fixture data);
  // t (total) is f+m per bucket, so every row is easy to predict in assertions.
  AGE_BUCKETS.forEach((bucket, i) => {
    const perSex = (i + 1) * 100;
    insertPop.run("LK1101", 2023, "f", bucket, perSex);
    insertPop.run("LK1101", 2023, "m", bucket, perSex);
    insertPop.run("LK1101", 2023, "t", bucket, perSex * 2);
  });
  const perSexTotal = AGE_BUCKETS.reduce((sum, _, i) => sum + (i + 1) * 100, 0);
  insertPop.run("LK1101", 2023, "f", "total", perSexTotal);
  insertPop.run("LK1101", 2023, "m", "total", perSexTotal);
  insertPop.run("LK1101", 2023, "t", "total", perSexTotal * 2);

  // --- admin_stats (LK1101, year 2023) ---------------------------------------
  const insertStats = db.prepare("INSERT INTO admin_stats (pcode, year, key, value) VALUES (?, ?, ?, ?)");
  insertStats.run("LK1101", 2023, "ethnicity.sinhala", 65.5);
  insertStats.run("LK1101", 2023, "ethnicity.tamil", 15.2);
  insertStats.run("LK1101", 2023, "religion.buddhist", 70.1);

  // --- places + FTS -----------------------------------------------------------
  const insertPlace = db.prepare(`
    INSERT INTO places (id, source, source_id, kind, name_en, name_si, name_ta, lat, lon, population, admin_pcode)
    VALUES (@id, @source, @source_id, @kind, @name_en, @name_si, @name_ta, @lat, @lon, @population, @admin_pcode)
  `);
  insertPlace.run({
    id: 1, source: "geonames", source_id: "1248991", kind: "city",
    name_en: "Colombo", name_si: "කොළඹ", name_ta: "கொழும்பு",
    lat: 6.9319, lon: 79.8478, population: 752993, admin_pcode: "LK110101",
  });
  insertPlace.run({
    id: 2, source: "geonames", source_id: "1231732", kind: "town",
    name_en: "Nugegoda", name_si: "නුගේගොඩ", name_ta: null,
    lat: 6.8649, lon: 79.8997, population: 43312, admin_pcode: "LK1102",
  });
  // Shares the "Colombo" prefix with place #1 but is much smaller — lets tests
  // exercise multi-result ranking (score = matchQuality + population boost)
  // and min_population filtering without relying on an invalid 1-char query.
  insertPlace.run({
    id: 3, source: "osm", source_id: "way/123", kind: "suburb",
    name_en: "Colombo Fort", name_si: null, name_ta: null,
    lat: FORT_LAT, lon: FORT_LON, population: 15000, admin_pcode: "LK110101",
  });
  db.exec("INSERT INTO places_fts(places_fts) VALUES('rebuild')");

  // --- postal_codes ------------------------------------------------------------
  db.prepare("INSERT INTO postal_codes (code, name, admin_pcode, lat, lon) VALUES (?, ?, ?, ?, ?)").run(
    "00100",
    "Colombo Fort",
    "LK110101",
    FORT_LAT,
    FORT_LON,
  );

  // --- cells + cell_lookup around Colombo Fort ---------------------------------
  // A 5x5 block of cells centered on the Fort cell so radius sums have something
  // to add beyond the point cell itself.
  const insertCell = db.prepare("INSERT INTO cells (cell_id, pop) VALUES (?, ?)");
  const fortRow = Math.floor(FORT_CELL_ID / GRID.cols);
  const fortCol = FORT_CELL_ID % GRID.cols;
  for (let dr = -2; dr <= 2; dr++) {
    for (let dc = -2; dc <= 2; dc++) {
      const id = (fortRow + dr) * GRID.cols + (fortCol + dc);
      const pop = 50 - (Math.abs(dr) + Math.abs(dc)) * 5; // denser near the center
      insertCell.run(id, pop);
    }
  }

  db.prepare(
    "INSERT INTO cell_lookup (cell_id, gnd_pcode, postal_code, nearest_place_id, nearest_place_dist_m) VALUES (?, ?, ?, ?, ?)",
  ).run(FORT_CELL_ID, "LK110101", "00100", 1, 350);

  // --- elections ----------------------------------------------------------------
  db.prepare("INSERT INTO elections (id, type, year, date, label) VALUES (?, ?, ?, ?, ?)").run(
    "pres-2024",
    "presidential",
    2024,
    "2024-09-21",
    "Presidential Election 2024",
  );

  const insertParty = db.prepare(
    "INSERT INTO election_parties (election_id, code, candidate, name, color) VALUES (?, ?, ?, ?, ?)",
  );
  insertParty.run("pres-2024", "NPP", "A. K. Dissanayake", "National People's Power", "#B2001A");
  insertParty.run("pres-2024", "SJB", "Sajith Premadasa", "Samagi Jana Balawegaya", "#00A651");
  insertParty.run("pres-2024", "UNP", "Ranil Wickremesinghe", "United National Party", "#00A0DC");

  const insertEntity = db.prepare(
    "INSERT INTO election_entities (entity_id, kind, name, ed_id) VALUES (?, ?, ?, ?)",
  );
  insertEntity.run("EC-01", "ED", "Colombo", null);
  insertEntity.run("LK", "NATIONAL", "Sri Lanka", null);

  const insertResult = db.prepare(`
    INSERT INTO election_results (election_id, entity_id, electors, polled, valid, rejected, winner_party, winner_votes, results)
    VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
  `);
  insertResult.run(
    "pres-2024",
    "EC-01",
    1_600_000,
    1_200_000,
    1_180_000,
    20_000,
    "NPP",
    500_000,
    JSON.stringify({ NPP: 500_000, SJB: 400_000, UNP: 280_000 }),
  );
  insertResult.run(
    "pres-2024",
    "LK",
    17_000_000,
    12_000_000,
    11_800_000,
    200_000,
    "NPP",
    5_700_000,
    JSON.stringify({ NPP: 5_700_000, SJB: 4_200_000, UNP: 1_900_000 }),
  );

  return db;
}
