import { readFile } from "node:fs/promises";
import type { StepContext } from "../step.ts";
import { rawPath } from "../lib/paths.ts";
import { readSheetMatrix } from "../lib/xlsx.ts";
import {
  CENSUS_YEAR,
  buildDsMatchIndex,
  matchDistrict,
  matchDs,
  parseCensusSheet,
  type Adm3Properties,
  type CensusRow,
  type DsMatchIndex,
} from "../lib/census.ts";
import { CENSUS_FILES } from "./fetch-census.ts";

export const name = "census";

// Column layouts of the three tables (values start at sheet column C).
// A5: total, male, female, <15, 15-59, 60-64, 65+
const A5_VALUE_COUNT = 7;
const A5_AGE_BUCKETS = ["0-14", "15-59", "60-64", "65+"] as const;
// A6/A7: total, then one column per group. Keys follow the 2012
// ethnicity-religion dataset's camelCase convention so the same group keeps
// the same `admin_stats` key across census years (the 2024 tables label the
// Moor/Muslim column "Sri Lanka Yonaka/Muslim" and the Islam column "Islam";
// they stay `moor`/`muslim` here for cross-year continuity).
const A6_GROUPS = [
  "sinhala",
  "sriLankanTamil",
  "indianTamil",
  "moor",
  "burgher",
  "malay",
  "sriLankaChetty",
  "bharatha",
  "veddha",
  "other",
] as const;
const A7_GROUPS = ["buddhist", "hindu", "muslim", "romanCatholic", "otherChristian", "other"] as const;

// The census carries a handful more DS-division rows than COD-AB has ADM3
// features (2024: exactly one, "Kalmunai North Sub" — a sub-office COD-AB
// doesn't gazette as its own division). Those rows are reported and skipped;
// more than this many means the name matching actually broke.
const MAX_UNMATCHED_DS = 5;

interface ResolvedRow {
  pcode: string;
  row: CensusRow;
}

interface ParsedTable {
  table: string;
  resolved: ResolvedRow[];
  unmatched: { district: string; name: string; total: number }[];
}

/**
 * Walks a parsed sheet in document order, resolving each row to a p-code:
 * country -> LK, districts by COD-AB adm2 name, DS divisions by name within
 * the current district. Also enforces the tables' internal additivity (DS
 * rows sum to their district row, district rows sum to the country row —
 * exact in the published tables), so a silent parse drift fails loudly.
 */
function resolveRows(table: string, rows: CensusRow[], index: DsMatchIndex): ParsedTable {
  const resolved: ResolvedRow[] = [];
  const unmatched: ParsedTable["unmatched"] = [];

  let districtPcode: string | null = null;
  let districtName = "";
  let districtValues: number[] | null = null;
  let dsSums: number[] | null = null;
  const districtTotals: number[][] = [];
  let countryValues: number[] | null = null;

  const checkDistrict = () => {
    if (!districtValues || !dsSums) return;
    if (districtValues.some((v, i) => v !== dsSums![i])) {
      throw new Error(`census: ${table} DS rows of ${districtName} do not sum to the district row`);
    }
  };

  for (const row of rows) {
    if (row.kind === "country") {
      countryValues = row.values;
      resolved.push({ pcode: "LK", row });
    } else if (row.kind === "district") {
      checkDistrict();
      districtPcode = matchDistrict(index, row.nameEn);
      if (!districtPcode) throw new Error(`census: ${table} has unknown district "${row.nameEn}"`);
      districtName = row.nameEn;
      districtValues = row.values;
      dsSums = row.values.map(() => 0);
      districtTotals.push(row.values);
      resolved.push({ pcode: districtPcode, row });
    } else {
      if (!districtPcode || !dsSums) throw new Error(`census: ${table} DS row "${row.nameEn}" before any district row`);
      row.values.forEach((v, i) => (dsSums![i] = dsSums![i]! + v));
      const pcode = matchDs(index, districtPcode, row.nameEn);
      if (!pcode) {
        unmatched.push({ district: districtPcode, name: row.nameEn, total: row.values[0]! });
        continue;
      }
      resolved.push({ pcode, row });
    }
  }
  checkDistrict();

  if (!countryValues) throw new Error(`census: ${table} has no country row`);
  for (let i = 0; i < countryValues.length; i++) {
    const sum = districtTotals.reduce((acc, values) => acc + values[i]!, 0);
    if (sum !== countryValues[i]) {
      throw new Error(`census: ${table} district rows do not sum to the country row (column ${i})`);
    }
  }

  return { table, resolved, unmatched };
}

async function parseTable(table: string, file: string, valueCount: number, index: DsMatchIndex): Promise<ParsedTable> {
  const matrix = await readSheetMatrix(rawPath("census-2024", file));
  return resolveRows(table, parseCensusSheet(matrix, valueCount), index);
}

/**
 * census: loads the 2024 Census of Population and Housing final-report
 * tables (fetched by `fetch-census`) into admin_population (A5: sex totals
 * plus coarse age groups, year 2024) and admin_stats (A6/A7: ethnicity.* /
 * religion.* counts, year 2024) for the country, all 25 districts, and every
 * DS division whose name matches a COD-AB ADM3 feature (see lib/census.ts
 * for the matching rules). The published tables have no sex-by-age
 * cross-tabulation, so age-bucket rows exist for sex 't' only, and no
 * GN-division resolution — GN divisions keep their WorldPop-modeled totals.
 */
export async function run({ db, log }: StepContext): Promise<void> {
  const adm3 = JSON.parse(await readFile(rawPath("cod-ab", "lka_admin3.geojson"), "utf8")) as {
    features: { properties: Adm3Properties }[];
  };
  const index = buildDsMatchIndex(adm3.features);

  const [a5File, a6File, a7File] = CENSUS_FILES.map((f) => f.file);
  const a5 = await parseTable("A5", a5File!, A5_VALUE_COUNT, index);
  const a6 = await parseTable("A6", a6File!, 1 + A6_GROUPS.length, index);
  const a7 = await parseTable("A7", a7File!, 1 + A7_GROUPS.length, index);

  // The three tables publish the same enumeration — every unit's grand total
  // must agree across them, or the sheets drifted apart upstream.
  const a5TotalByPcode = new Map(a5.resolved.map(({ pcode, row }) => [pcode, row.values[0]!]));
  for (const t of [a6, a7]) {
    for (const { pcode, row } of t.resolved) {
      if (a5TotalByPcode.get(pcode) !== row.values[0]) {
        throw new Error(`census: ${t.table} total for ${pcode} disagrees with A5`);
      }
    }
  }

  for (const t of [a5, a6, a7]) {
    if (t.unmatched.length > MAX_UNMATCHED_DS) {
      throw new Error(
        `census: ${t.table} has ${t.unmatched.length} unmatched DS divisions (max ${MAX_UNMATCHED_DS}): ` +
          t.unmatched.map((u) => `${u.name} (${u.district})`).join(", "),
      );
    }
    for (const u of t.unmatched) {
      log(`census: ${t.table} unmatched DS division "${u.name}" in ${u.district} (population ${u.total}) — skipped`);
    }
  }

  const insertPop = db.prepare(`
    INSERT INTO admin_population (pcode, year, sex, age_bucket, count)
    VALUES (@pcode, @year, @sex, @age_bucket, @count)
  `);
  const insertStat = db.prepare(`
    INSERT INTO admin_stats (pcode, year, key, value)
    VALUES (@pcode, @year, @key, @value)
  `);

  let popRows = 0;
  let statRows = 0;

  const tx = db.transaction(() => {
    // This step owns the census year outright — clear and rebuild, so a
    // re-run against refreshed source files never leaves stale rows.
    db.prepare(`DELETE FROM admin_population WHERE year = ?`).run(CENSUS_YEAR);
    db.prepare(`DELETE FROM admin_stats WHERE year = ?`).run(CENSUS_YEAR);

    for (const { pcode, row } of a5.resolved) {
      const [total, male, female, ...ages] = row.values;
      insertPop.run({ pcode, year: CENSUS_YEAR, sex: "t", age_bucket: "total", count: total });
      insertPop.run({ pcode, year: CENSUS_YEAR, sex: "m", age_bucket: "total", count: male });
      insertPop.run({ pcode, year: CENSUS_YEAR, sex: "f", age_bucket: "total", count: female });
      A5_AGE_BUCKETS.forEach((age_bucket, i) => {
        insertPop.run({ pcode, year: CENSUS_YEAR, sex: "t", age_bucket, count: ages[i] });
      });
      popRows += 3 + A5_AGE_BUCKETS.length;
    }

    const writeStats = (t: ParsedTable, prefix: string, groups: readonly string[]) => {
      for (const { pcode, row } of t.resolved) {
        insertStat.run({ pcode, year: CENSUS_YEAR, key: `${prefix}.total`, value: row.values[0] });
        groups.forEach((group, i) => {
          insertStat.run({ pcode, year: CENSUS_YEAR, key: `${prefix}.${group}`, value: row.values[1 + i] });
        });
        statRows += 1 + groups.length;
      }
    };
    writeStats(a6, "ethnicity", A6_GROUPS);
    writeStats(a7, "religion", A7_GROUPS);
  });
  tx();

  const dsCount = a5.resolved.filter(({ row }) => row.kind === "ds").length;
  log(
    `census: year ${CENSUS_YEAR} — ${popRows} admin_population rows, ${statRows} admin_stats rows ` +
      `(country + 25 districts + ${dsCount} DS divisions; ${a5.unmatched.length} census DS row(s) unmatched)`,
  );
}
