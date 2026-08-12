import type { Hono } from "hono";
import type Database from "better-sqlite3";
import { z } from "zod";
import type { Lang, Sex } from "@geopub/shared";
import { buildMeta, ok } from "../lib/envelope.ts";
import { prepared } from "../lib/cache.ts";
import { NotFoundError, ValidationError, formatZodIssues } from "../lib/errors.ts";
import { getAdminUnitRow, getChildren, getParentChain, mapAdminUnit } from "../lib/admin.ts";

const QuerySchema = z.object({
  lang: z.enum(["en", "si", "ta"]).optional().default("en"),
  include: z.string().optional(),
});

const INCLUDE_VALUES = new Set(["population", "stats"]);

interface PopulationRow {
  sex: Sex;
  age_bucket: string;
  count: number;
}

interface StatsRow {
  key: string;
  value: number;
}

type SexBreakdown = Record<Sex, number>;

function emptyBreakdown(): SexBreakdown {
  return { f: 0, m: 0, t: 0 };
}

function loadPopulation(db: Database.Database, pcode: string) {
  const yearRow = prepared<{ year: number | null }>(
    db,
    "SELECT MAX(year) as year FROM admin_population WHERE pcode = ?",
  ).get(pcode);
  if (!yearRow?.year) return null;

  const rows = prepared<PopulationRow>(
    db,
    "SELECT sex, age_bucket, count FROM admin_population WHERE pcode = ? AND year = ?",
  ).all(pcode, yearRow.year);

  const buckets: Record<string, SexBreakdown> = {};
  let total: SexBreakdown | null = null;
  for (const row of rows) {
    if (row.age_bucket === "total") {
      total = total ?? emptyBreakdown();
      total[row.sex] = row.count;
    } else {
      const bucket = buckets[row.age_bucket] ?? emptyBreakdown();
      bucket[row.sex] = row.count;
      buckets[row.age_bucket] = bucket;
    }
  }
  return { year: yearRow.year, buckets, total };
}

function loadStats(db: Database.Database, pcode: string) {
  const yearRow = prepared<{ year: number | null }>(
    db,
    "SELECT MAX(year) as year FROM admin_stats WHERE pcode = ?",
  ).get(pcode);
  if (!yearRow?.year) return null;

  const rows = prepared<StatsRow>(db, "SELECT key, value FROM admin_stats WHERE pcode = ? AND year = ?").all(
    pcode,
    yearRow.year,
  );
  const values: Record<string, number> = {};
  for (const row of rows) values[row.key] = row.value;
  return { year: yearRow.year, values };
}

/**
 * GET /v1/admin/:pcode — a unit, its ancestor chain, and children summary.
 * ?include=population,stats adds the heavier per-unit breakdowns.
 */
export function mountAdminRoute(app: Hono, db: Database.Database): void {
  app.get("/v1/admin/:pcode", (c) => {
    const parsed = QuerySchema.safeParse(c.req.query());
    if (!parsed.success) throw new ValidationError(formatZodIssues(parsed.error.issues));
    const lang: Lang = parsed.data.lang;

    const include = parsed.data.include
      ? parsed.data.include
          .split(",")
          .map((s) => s.trim())
          .filter(Boolean)
      : [];
    for (const value of include) {
      if (!INCLUDE_VALUES.has(value)) {
        throw new ValidationError(`include: unknown value "${value}" (expected population, stats)`);
      }
    }

    const pcode = c.req.param("pcode");
    const row = getAdminUnitRow(db, pcode);
    if (!row) throw new NotFoundError(`admin unit "${pcode}" not found`);

    const unit = mapAdminUnit(row, lang);
    const parentChain = getParentChain(db, pcode).map((r) => mapAdminUnit(r, lang));
    const children = getChildren(db, pcode, lang);

    const payload: Record<string, unknown> = { unit, parent_chain: parentChain, children };
    if (include.includes("population")) payload.population = loadPopulation(db, pcode);
    if (include.includes("stats")) payload.stats = loadStats(db, pcode);

    const meta = buildMeta(db, ["admin-units"]);
    return c.json(ok(payload, meta));
  });
}
