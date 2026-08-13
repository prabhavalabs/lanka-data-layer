import { test } from "node:test";
import assert from "node:assert/strict";
import Database from "better-sqlite3";
import { assertSanity, buildAdmin, buildCensus, buildElectoral, buildPostal, round } from "../src/steps/packages.ts";

test("round: rounds to the given decimal places", () => {
  assert.equal(round(24.5432, 2), 24.54);
  assert.equal(round(6.9469871, 6), 6.946987);
  assert.equal(round(1, 4), 1);
});

test("round: passes null/undefined through unchanged", () => {
  assert.equal(round(null, 2), null);
  assert.equal(round(undefined, 2), null);
});

/**
 * Builds an in-memory DB with just the tables/columns the four build*()
 * functions query (a deliberate subset of src/db.ts's full schema), seeded
 * with a small fixture: one country, one province, one district (admin),
 * three postal codes with an overlapping admin_postal rollup (postal), one
 * election with two entities and two parties (electoral), and one 2024 +
 * one 2012 census pcode (census).
 */
function fixtureDb(): Database.Database {
  const db = new Database(":memory:");
  db.exec(`
    CREATE TABLE datasets (id TEXT PRIMARY KEY, source_name TEXT, source_url TEXT, license TEXT);

    CREATE TABLE admin_units (
      pcode TEXT PRIMARY KEY, level INTEGER, name_en TEXT, name_si TEXT, name_ta TEXT,
      parent_pcode TEXT, area_km2 REAL, centroid_lat REAL, centroid_lon REAL
    );

    CREATE TABLE postal_codes (code TEXT PRIMARY KEY, name TEXT, admin_pcode TEXT, lat REAL, lon REAL);
    CREATE TABLE admin_postal (pcode TEXT, code TEXT, share REAL);

    CREATE TABLE elections (id TEXT PRIMARY KEY, type TEXT, year INTEGER, date TEXT, label TEXT);
    CREATE TABLE election_entities (entity_id TEXT PRIMARY KEY, kind TEXT, name TEXT, ed_id TEXT);
    CREATE TABLE election_parties (election_id TEXT, code TEXT, candidate TEXT, name TEXT, color TEXT);
    CREATE TABLE election_results (
      election_id TEXT, entity_id TEXT, electors INTEGER, polled INTEGER, valid INTEGER, rejected INTEGER,
      winner_party TEXT, winner_votes INTEGER, results TEXT
    );

    CREATE TABLE admin_population (pcode TEXT, year INTEGER, sex TEXT, age_bucket TEXT, count INTEGER);
    CREATE TABLE admin_stats (pcode TEXT, year INTEGER, key TEXT, value REAL);
  `);

  db.exec(`
    INSERT INTO datasets (id, source_name, source_url, license) VALUES
      ('admin-units', 'geoBoundaries + OCHA COD-AB Sri Lanka', 'https://www.geoboundaries.org', 'CC BY 3.0 IGO / CC BY-IGO'),
      ('postal-codes', 'GeoNames', 'https://download.geonames.org/export/zip/LK.zip', 'CC BY 4.0'),
      ('elections', 'Election Commission of Sri Lanka', 'https://github.com/nuuuwan/lk_elections', 'open'),
      ('census-2024', 'Department of Census and Statistics, Sri Lanka', 'http://www.statistics.gov.lk', 'Not stated');
  `);

  const insertUnit = db.prepare(
    `INSERT INTO admin_units (pcode, level, name_en, name_si, name_ta, parent_pcode, area_km2, centroid_lat, centroid_lon)
     VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)`,
  );
  insertUnit.run("LK", 0, "Sri Lanka", null, null, null, 66358.7, 7.8774, 80.7649);
  insertUnit.run("LK1", 1, "Western Province", "SiName", "TaName", "LK", 3709.0, 6.9, 80.0);
  insertUnit.run("LK11", 2, "Colombo", null, null, "LK1", 24.5432, 6.9469871, 79.8352229);

  const insertPostal = db.prepare(`INSERT INTO postal_codes (code, name, admin_pcode, lat, lon) VALUES (?, ?, ?, ?, ?)`);
  insertPostal.run("00100", "Fort", "LK11", 6.9440531, 79.8355331);
  insertPostal.run("00200", "Bambalapitiya", "LK11", 6.89, 79.85);
  insertPostal.run("00300", "Kollupitiya", null, null, null);

  const insertDivisionCode = db.prepare(`INSERT INTO admin_postal (pcode, code, share) VALUES (?, ?, ?)`);
  insertDivisionCode.run("LK11", "00200", 0.30001);
  insertDivisionCode.run("LK11", "00100", 0.70009);

  db.exec(`INSERT INTO elections (id, type, year, date, label) VALUES ('pres-2024', 'presidential', 2024, '2024-09-21', '2024 Presidential')`);
  db.exec(`
    INSERT INTO election_entities (entity_id, kind, name, ed_id) VALUES
      ('LK', 'NATIONAL', 'Sri Lanka', NULL),
      ('EC-01', 'ED', 'Colombo', 'EC-01');
  `);
  db.exec(`
    INSERT INTO election_parties (election_id, code, candidate, name, color) VALUES
      ('pres-2024', 'NPP', 'Anura Kumara Dissanayake', 'National People''s Power', '#dc2626'),
      ('pres-2024', 'SJB', 'Sajith Premadasa', 'Samagi Jana Balawegaya', '#16a34a');
  `);
  const insertResult = db.prepare(
    `INSERT INTO election_results (election_id, entity_id, electors, polled, valid, rejected, winner_party, winner_votes, results)
     VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)`,
  );
  insertResult.run(
    "pres-2024",
    "EC-01",
    1765351,
    1366186,
    1334390,
    31796,
    "NPP",
    629963,
    JSON.stringify({
      valid: 1334390,
      rejected: 31796,
      polled: 1366186,
      electors: 1765351,
      turnoutPct: 77.4,
      winner: { party: "NPP", votes: 629963, pct: 47.21 },
      runnerUp: { party: "SJB", votes: 342108, pct: 25.64 },
      margin: 21.57,
      topParties: [
        { party: "NPP", votes: 629963, pct: 47.21 },
        { party: "SJB", votes: 342108, pct: 25.64 },
      ],
      otherVotes: 362319,
    }),
  );

  const insertPop = db.prepare(`INSERT INTO admin_population (pcode, year, sex, age_bucket, count) VALUES (?, ?, ?, ?, ?)`);
  insertPop.run("LK", 2024, "t", "total", 21781800);
  insertPop.run("LK", 2024, "m", "total", 10512344);
  insertPop.run("LK", 2024, "f", "total", 11269456);
  insertPop.run("LK", 2024, "t", "0-14", 4506839);
  insertPop.run("LK", 2024, "t", "15-59", 13353837);
  insertPop.run("LK", 2024, "t", "60-64", 1183310);
  insertPop.run("LK", 2024, "t", "65+", 2737814);

  const insertStat = db.prepare(`INSERT INTO admin_stats (pcode, year, key, value) VALUES (?, ?, ?, ?)`);
  insertStat.run("LK", 2024, "ethnicity.sinhala", 16144037);
  insertStat.run("LK", 2024, "ethnicity.total", 21781800);
  insertStat.run("LK", 2024, "religion.buddhist", 15199093);
  insertStat.run("LK", 2024, "religion.total", 21781800);
  insertStat.run("LK11", 2012, "ethnicity.sinhala", 1732530);
  insertStat.run("LK11", 2012, "ethnicity.total", 2324349);
  insertStat.run("LK11", 2012, "religion.buddhist", 1585000);
  insertStat.run("LK11", 2012, "religion.total", 2324349);

  return db;
}

const DATA_VERSION = "20260813.1";
const GENERATED = "2026-08-13T11:53:02.717Z";

test("buildAdmin: emits array-of-array units ordered by pcode with rounded area/coords", () => {
  const db = fixtureDb();
  const pkg = buildAdmin(db, DATA_VERSION, GENERATED);
  db.close();

  assert.equal(pkg.data_version, DATA_VERSION);
  assert.equal(pkg.generated, GENERATED);
  assert.deepEqual(pkg.sources, [
    { name: "geoBoundaries + OCHA COD-AB Sri Lanka", url: "https://www.geoboundaries.org", license: "CC BY 3.0 IGO / CC BY-IGO" },
  ]);
  // Insertion order was LK, LK1, LK11 — already pcode-sorted, but assert the
  // shape/values explicitly rather than relying on insertion order alone.
  assert.deepEqual(
    pkg.units.map((u) => u[0]),
    ["LK", "LK1", "LK11"],
  );
  const lk11 = pkg.units.find((u) => u[0] === "LK11")!;
  assert.deepEqual(lk11, ["LK11", 2, "Colombo", null, null, "LK1", 24.54, 6.946987, 79.835223]);
  const lk = pkg.units.find((u) => u[0] === "LK")!;
  assert.equal(lk[5], null); // parent_pcode
});

test("buildPostal: codes ordered by code; division_codes grouped and sorted desc by share (4dp)", () => {
  const db = fixtureDb();
  const pkg = buildPostal(db, DATA_VERSION, GENERATED);
  db.close();

  assert.deepEqual(
    pkg.codes.map((c) => c[0]),
    ["00100", "00200", "00300"],
  );
  // Row with null lat/lon passes nulls through rather than dropping the row.
  const kollupitiya = pkg.codes.find((c) => c[0] === "00300")!;
  assert.deepEqual(kollupitiya, ["00300", "Kollupitiya", null, null, null]);

  assert.deepEqual(pkg.division_codes["LK11"], [
    ["00100", 0.7001],
    ["00200", 0.3],
  ]);
});

test("buildElectoral: entities/parties as compact tuples; results keep only per-party votes, desc", () => {
  const db = fixtureDb();
  const pkg = buildElectoral(db, DATA_VERSION, GENERATED);
  db.close();

  assert.deepEqual(pkg.elections, [{ id: "pres-2024", type: "presidential", year: 2024, date: "2024-09-21", label: "2024 Presidential" }]);
  assert.deepEqual(pkg.entities, [
    ["EC-01", "ED", "Colombo", "EC-01"],
    ["LK", "NATIONAL", "Sri Lanka", null],
  ]);
  assert.deepEqual(pkg.parties, [
    ["pres-2024", "NPP", "Anura Kumara Dissanayake", "National People's Power", "#dc2626"],
    ["pres-2024", "SJB", "Sajith Premadasa", "Samagi Jana Balawegaya", "#16a34a"],
  ]);

  const result = pkg.results["pres-2024"]!["EC-01"]!;
  assert.deepEqual(result, {
    electors: 1765351,
    polled: 1366186,
    valid: 1334390,
    rejected: 31796,
    winner_party: "NPP",
    winner_votes: 629963,
    parties: [
      ["NPP", 629963],
      ["SJB", 342108],
    ],
  });
  // Derivable fields (pct, winner, runnerUp, margin, turnoutPct, otherVotes) are dropped.
  assert.equal((result as unknown as Record<string, unknown>).turnoutPct, undefined);
});

test("buildCensus: population_2024 groups sex totals + t-only age buckets per pcode", () => {
  const db = fixtureDb();
  const pkg = buildCensus(db, DATA_VERSION, GENERATED);
  db.close();

  assert.deepEqual(pkg.population_2024["LK"], {
    t: 21781800,
    m: 10512344,
    f: 11269456,
    ages: { "0-14": 4506839, "15-59": 13353837, "60-64": 1183310, "65+": 2737814 },
  });
});

test("buildCensus: stats_2024/stats_2012 strip the ethnicity./religion. key prefix", () => {
  const db = fixtureDb();
  const pkg = buildCensus(db, DATA_VERSION, GENERATED);
  db.close();

  assert.deepEqual(pkg.stats_2024["LK"], {
    ethnicity: { sinhala: 16144037, total: 21781800 },
    religion: { buddhist: 15199093, total: 21781800 },
  });
  assert.deepEqual(pkg.stats_2012["LK11"], {
    ethnicity: { sinhala: 1732530, total: 2324349 },
    religion: { buddhist: 1585000, total: 2324349 },
  });
  // 2012 has no rows for LK (country-level) in this fixture, same as the
  // real dataset (2012 covers districts only) — no entry should appear.
  assert.equal(pkg.stats_2012["LK"], undefined);
});

test("assertSanity: passes when every threshold is met and division pcodes are known", () => {
  assert.doesNotThrow(() =>
    assertSanity({
      adminUnitCount: 14417,
      postalCodeCount: 1833,
      lkPopulation2024: 21781800,
      divisionPcodes: ["LK11", "LK1103005"],
      knownAdminPcodes: new Set(["LK11", "LK1103005"]),
    }),
  );
});

test("assertSanity: throws when admin_units count is too low", () => {
  assert.throws(
    () =>
      assertSanity({
        adminUnitCount: 100,
        postalCodeCount: 1833,
        lkPopulation2024: 21781800,
        divisionPcodes: [],
        knownAdminPcodes: new Set(),
      }),
    /admin_units has 100 rows/,
  );
});

test("assertSanity: throws when postal_codes count is too low", () => {
  assert.throws(
    () =>
      assertSanity({
        adminUnitCount: 14417,
        postalCodeCount: 5,
        lkPopulation2024: 21781800,
        divisionPcodes: [],
        knownAdminPcodes: new Set(),
      }),
    /postal_codes has 5 rows/,
  );
});

test("assertSanity: throws when LK population_2024 total is missing or too low", () => {
  assert.throws(
    () =>
      assertSanity({
        adminUnitCount: 14417,
        postalCodeCount: 1833,
        lkPopulation2024: undefined,
        divisionPcodes: [],
        knownAdminPcodes: new Set(),
      }),
    /population_2024\["LK"\]\.t is missing/,
  );
  assert.throws(
    () =>
      assertSanity({
        adminUnitCount: 14417,
        postalCodeCount: 1833,
        lkPopulation2024: 1000,
        divisionPcodes: [],
        knownAdminPcodes: new Set(),
      }),
    /population_2024\["LK"\]\.t is 1000/,
  );
});

test("assertSanity: throws when a division_codes pcode is not in admin_units", () => {
  assert.throws(
    () =>
      assertSanity({
        adminUnitCount: 14417,
        postalCodeCount: 1833,
        lkPopulation2024: 21781800,
        divisionPcodes: ["LK9999999"],
        knownAdminPcodes: new Set(["LK11"]),
      }),
    /pcode "LK9999999", which is not in admin_units/,
  );
});
