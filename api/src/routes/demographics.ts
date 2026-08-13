import type { Hono } from "hono";
import type Database from "better-sqlite3";
import { z } from "zod";
import type { Lang } from "@lanka-data-layer/shared";
import { buildMeta, ok } from "../lib/envelope.ts";
import { prepared } from "../lib/cache.ts";
import { NotFoundError, ValidationError, formatZodIssues } from "../lib/errors.ts";
import { getAdminUnitRow, getParentChain, mapAdminUnit } from "../lib/admin.ts";
import { resolveName } from "../lib/lang.ts";

const CENSUS_YEAR = 2024;
const PRIOR_CENSUS_YEAR = 2012;
const CENSUS_DATASET_ID = "census-2024";
const PRIOR_CENSUS_DATASET_ID = "census-2012";

/** The 2024 census's coarse age groups (contract §2 — sex 't' only, no sex-by-age
 * cross-tab in the published tables), in the fixed display order the payload wants. */
const AGE_BUCKET_ORDER = ["0-14", "15-59", "60-64", "65+"] as const;

const QuerySchema = z.object({
  lang: z.enum(["en", "si", "ta"]).optional().default("en"),
});

/** Rounds to `decimals` places, guarding the classic binary-float overshoot
 * (e.g. plain `Math.round(1.005 * 100) / 100` undershoots to 1). */
function round(value: number, decimals: number): number {
  const factor = 10 ** decimals;
  return Math.round((value + Number.EPSILON) * factor) / factor;
}
const round4 = (v: number) => round(v, 4);
const round1 = (v: number) => round(v, 1);
const share = (count: number, total: number) => (total > 0 ? round4(count / total) : 0);

interface PopulationRow {
  sex: "t" | "m" | "f";
  age_bucket: string;
  count: number;
}

interface CensusPopulation {
  total: number;
  male: number;
  female: number;
  /** The four coarse 2024 age buckets (sex 't' only), keyed by bucket label. */
  buckets: Record<string, number>;
}

/**
 * Loads the 2024 population rows for `pcode`. Returns null when the unit carries
 * none at all — provinces and GN divisions have zero 2024 admin_population rows
 * (coverage is country + district + DS division only), which is this endpoint's
 * "no census data" case.
 */
function loadCensusPopulation(db: Database.Database, pcode: string): CensusPopulation | null {
  const rows = prepared<PopulationRow>(
    db,
    "SELECT sex, age_bucket, count FROM admin_population WHERE pcode = ? AND year = ?",
  ).all(pcode, CENSUS_YEAR);
  if (rows.length === 0) return null;

  let total: number | null = null;
  let male = 0;
  let female = 0;
  const buckets: Record<string, number> = {};
  for (const row of rows) {
    if (row.sex === "t" && row.age_bucket === "total") total = row.count;
    else if (row.sex === "m" && row.age_bucket === "total") male = row.count;
    else if (row.sex === "f" && row.age_bucket === "total") female = row.count;
    else if (row.sex === "t") buckets[row.age_bucket] = row.count;
  }
  if (total === null) return null;
  return { total, male, female, buckets };
}

interface StatsGroup {
  /** The group's `.total` row value; 0 when absent (share() then falls back to 0 rather than NaN). */
  total: number;
  /** Member keys with their group prefix stripped ("ethnicity.sinhala" -> "sinhala"). */
  items: Map<string, number>;
}

/**
 * Splits a pcode/year's admin_stats rows into ethnicity/religion groups. The
 * `.total` row becomes the group's denominator, not a member of `items` — the
 * payload's ethnicity/religion arrays never carry a synthetic "total" entry.
 */
function loadStatsGroups(
  db: Database.Database,
  pcode: string,
  year: number,
): { ethnicity: StatsGroup; religion: StatsGroup } {
  const rows = prepared<{ key: string; value: number }>(
    db,
    "SELECT key, value FROM admin_stats WHERE pcode = ? AND year = ?",
  ).all(pcode, year);

  const ethnicity: StatsGroup = { total: 0, items: new Map() };
  const religion: StatsGroup = { total: 0, items: new Map() };
  for (const row of rows) {
    const dot = row.key.indexOf(".");
    if (dot === -1) continue;
    const group = row.key.slice(0, dot);
    const key = row.key.slice(dot + 1);
    const target = group === "ethnicity" ? ethnicity : group === "religion" ? religion : null;
    if (!target) continue;
    if (key === "total") target.total = row.value;
    else target.items.set(key, row.value);
  }
  return { ethnicity, religion };
}

interface StatEntry {
  key: string;
  count: number;
  share: number;
}

/** Group members sorted desc by count, each `share` a fraction of the group's `.total`. */
function toStatEntries(group: StatsGroup): StatEntry[] {
  return [...group.items.entries()]
    .map(([key, count]) => ({ key, count, share: share(count, group.total) }))
    .sort((a, b) => b.count - a.count);
}

interface ChangeEntry {
  key: string;
  share_2012: number;
  share_2024: number;
  delta: number;
}

/**
 * Keys present in both census years only (2012 ethnicity lacks sriLankaChetty/
 * bharatha/veddha — those are 2024-only groups and are excluded here, per spec).
 * Sorted desc by the 2024 count to match the top-level ethnicity/religion order.
 */
function toChangeEntries(group2012: StatsGroup, group2024: StatsGroup): ChangeEntry[] {
  const entries: (ChangeEntry & { count2024: number })[] = [];
  for (const [key, count2012] of group2012.items) {
    const count2024 = group2024.items.get(key);
    if (count2024 === undefined) continue;
    const share2012 = share(count2012, group2012.total);
    const share2024 = share(count2024, group2024.total);
    entries.push({ key, share_2012: share2012, share_2024: share2024, delta: round4(share2024 - share2012), count2024 });
  }
  return entries.sort((a, b) => b.count2024 - a.count2024).map(({ count2024: _count2024, ...rest }) => rest);
}

/** True if `pcode` has any admin_stats rows for `year` — only the 25 districts carry 2012 rows. */
function hasStatsYear(db: Database.Database, pcode: string, year: number): boolean {
  const row = prepared<{ present: number }>(
    db,
    "SELECT EXISTS(SELECT 1 FROM admin_stats WHERE pcode = ? AND year = ?) as present",
  ).get(pcode, year);
  return row?.present === 1;
}

/** Whether `datasets` actually carries a row for `id` — attribution must never claim a
 * dataset id that doesn't exist in this build (the real artifact has no 2012 entry yet). */
function hasDataset(db: Database.Database, id: string): boolean {
  const row = prepared<{ present: number }>(
    db,
    "SELECT EXISTS(SELECT 1 FROM datasets WHERE id = ?) as present",
  ).get(id);
  return row?.present === 1;
}

/** The 2024 total population for `pcode`, or null if it has none. */
function totalPopulationOf(db: Database.Database, pcode: string): number | null {
  const row = prepared<{ count: number }>(
    db,
    "SELECT count FROM admin_population WHERE pcode = ? AND year = ? AND sex = 't' AND age_bucket = 'total'",
  ).get(pcode, CENSUS_YEAR);
  return row?.count ?? null;
}

/**
 * Finds the nearest ancestor that actually carries 2024 census data to compute
 * parent_share against, rather than trusting admin_units.parent_pcode literally.
 * Provinces (level 1) have zero 2024 admin_population rows in the real artifact —
 * coverage is country + district + DS division only — so a district's literal
 * parent (its province) never has a total to divide by. Walking the chain keeps
 * parent_share meaningful for the endpoint's most common caller (district
 * lookups) instead of it being null for every district.
 */
function findParentShare(
  db: Database.Database,
  pcode: string,
  unitTotal: number,
  lang: Lang,
): { parent_pcode: string; parent_name: string; share: number } | null {
  // root-first from getParentChain; reversed so the nearest ancestor (the
  // immediate parent) is tried first, walking outward toward the country.
  const nearestFirst = [...getParentChain(db, pcode)].reverse();
  for (const ancestor of nearestFirst) {
    const parentTotal = totalPopulationOf(db, ancestor.pcode);
    if (parentTotal !== null && parentTotal > 0) {
      return { parent_pcode: ancestor.pcode, parent_name: resolveName(ancestor, lang), share: round4(unitTotal / parentTotal) };
    }
  }
  return null;
}

/**
 * GET /v1/demographics/:pcode — derived 2024-census indicators (population,
 * age structure, ethnicity, religion) plus a 2012-vs-2024 ethnicity/religion
 * comparison for the districts the 2012 census covered. 404s when the pcode
 * is unknown, or when the unit exists but carries no 2024 census rows (GN
 * divisions, and provinces — see findParentShare's comment above).
 */
export function mountDemographicsRoute(app: Hono, db: Database.Database): void {
  app.get("/v1/demographics/:pcode", (c) => {
    const parsed = QuerySchema.safeParse(c.req.query());
    if (!parsed.success) throw new ValidationError(formatZodIssues(parsed.error.issues));
    const lang: Lang = parsed.data.lang;

    const pcode = c.req.param("pcode");
    const row = getAdminUnitRow(db, pcode);
    if (!row) throw new NotFoundError(`admin unit "${pcode}" not found`);

    const population = loadCensusPopulation(db, pcode);
    if (!population) throw new NotFoundError(`no census data for "${pcode}"`);

    const unit = mapAdminUnit(row, lang);

    const child = population.buckets["0-14"] ?? 0;
    const working = population.buckets["15-59"] ?? 0;
    const elderlyWorking = population.buckets["60-64"] ?? 0;
    const elderly = population.buckets["65+"] ?? 0;
    const workingTotal = working + elderlyWorking;
    const dependents = child + elderly;

    const ageBuckets = AGE_BUCKET_ORDER.map((bucket) => {
      const count = population.buckets[bucket] ?? 0;
      return { bucket, count, share: share(count, population.total) };
    });

    const stats2024 = loadStatsGroups(db, pcode, CENSUS_YEAR);
    const ethnicity = toStatEntries(stats2024.ethnicity);
    const religion = toStatEntries(stats2024.religion);

    let change2012: { ethnicity: ChangeEntry[]; religion: ChangeEntry[] } | null = null;
    if (hasStatsYear(db, pcode, PRIOR_CENSUS_YEAR)) {
      const stats2012 = loadStatsGroups(db, pcode, PRIOR_CENSUS_YEAR);
      change2012 = {
        ethnicity: toChangeEntries(stats2012.ethnicity, stats2024.ethnicity),
        religion: toChangeEntries(stats2012.religion, stats2024.religion),
      };
    }

    const parentShare = row.level === 0 ? null : findParentShare(db, pcode, population.total, lang);

    const payload = {
      unit,
      census_year: CENSUS_YEAR,
      population: {
        total: population.total,
        male: population.male,
        female: population.female,
        sex_ratio: round1(population.female > 0 ? (population.male / population.female) * 100 : 0),
        female_share: share(population.female, population.total),
      },
      age: {
        buckets: ageBuckets,
        dependency_ratio: round1(workingTotal > 0 ? (dependents / workingTotal) * 100 : 0),
        working_age_share: share(workingTotal, population.total),
        child_share: share(child, population.total),
        elderly_share: share(elderly, population.total),
      },
      ethnicity,
      religion,
      change_2012: change2012,
      parent_share: parentShare,
    };

    const datasetIds = [CENSUS_DATASET_ID];
    if (change2012 && hasDataset(db, PRIOR_CENSUS_DATASET_ID)) datasetIds.push(PRIOR_CENSUS_DATASET_ID);
    const meta = buildMeta(db, datasetIds);
    return c.json(ok(payload, meta));
  });
}
