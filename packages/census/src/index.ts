import raw from "../data/census.json" with { type: "json" };

/**
 * @lanka-data-layer/census — offline Sri Lankan census indicators.
 *
 * Covers population, age structure, ethnicity, and religion from the 2024
 * census (country, all 25 districts, all 339 DS divisions — 365 p-coded
 * units total), plus a 2012-census comparison for the 25 districts. The
 * full dataset is bundled at build time — there is no network access, no
 * filesystem read at runtime, and nothing to configure.
 */

/** The census year every indicator in this package (other than `change2012`,
 * which compares against the 2012 census) is drawn from. */
export const CENSUS_YEAR = 2024;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

export type AgeBucket = "0-14" | "15-59" | "60-64" | "65+";

/** Raw population counts for one p-coded unit. */
export interface Population {
  total: number;
  male: number;
  female: number;
  ages: Record<AgeBucket, number>;
}

/** One ethnicity or religion group's share of a unit's 2024 population. */
export interface StatEntry {
  key: string;
  count: number;
  /** Fraction of the group's own total (e.g. all ethnicity counts), 4 decimal places. */
  share: number;
}

/** One age bucket's share of a unit's total 2024 population. */
export interface AgeBucketShare {
  bucket: AgeBucket;
  count: number;
  share: number;
}

/** One ethnicity or religion group's share in 2012 vs. 2024, keys present in both years only. */
export interface ChangeEntry {
  key: string;
  share2012: number;
  share2024: number;
  delta: number;
}

/** Full derived demographic profile for one p-coded unit. */
export interface Demographics {
  pcode: string;
  censusYear: number;
  population: {
    total: number;
    male: number;
    female: number;
    /** Males per 100 females, 1 decimal place. */
    sexRatio: number;
    /** Fraction of total population, 4 decimal places. */
    femaleShare: number;
  };
  age: {
    buckets: AgeBucketShare[];
    /** (0-14 + 65+) / (15-59 + 60-64) * 100, 1 decimal place. */
    dependencyRatio: number;
    workingAgeShare: number;
    childShare: number;
    elderlyShare: number;
  };
  ethnicity: StatEntry[];
  religion: StatEntry[];
  /** A 2012-vs-2024 comparison, or null when the unit predates or falls
   * outside 2012 census coverage (only the 25 districts carry 2012 rows). */
  change2012: { ethnicity: ChangeEntry[]; religion: ChangeEntry[] } | null;
}

export interface SourceAttribution {
  name: string;
  url: string;
  license: string;
}

// ---------------------------------------------------------------------------
// Raw data shape (as emitted by the foundry)
// ---------------------------------------------------------------------------

interface RawPopulation {
  t: number;
  m: number;
  f: number;
  ages: Record<AgeBucket, number>;
}

/** ethnicity/religion groups: member counts plus a `total` key that is the
 * group's own denominator, not a member of the group. */
type RawStatsGroup = Record<string, number>;

interface RawStats {
  ethnicity: RawStatsGroup;
  religion: RawStatsGroup;
}

interface RawData {
  data_version: string;
  generated: string;
  sources: SourceAttribution[];
  population_2024: Record<string, RawPopulation>;
  stats_2024: Record<string, RawStats>;
  stats_2012: Record<string, RawStats>;
}

const data = raw as unknown as RawData;

/** The dataset's build version (`YYYYMMDD.N`), matching the foundry's
 * `manifest.json`. Bump-tracks the underlying artifact — compare it if
 * you need to detect a stale cached copy of this package. */
export const DATA_VERSION: string = data.data_version;

/** Source attribution for every indicator in this package. */
export const SOURCES: SourceAttribution[] = data.sources;

// ---------------------------------------------------------------------------
// Rounding
// ---------------------------------------------------------------------------

/** Rounds to `decimals` places, guarding the classic binary-float overshoot
 * (plain `Math.round(1.005 * 100) / 100` undershoots to 1). */
function round(value: number, decimals: number): number {
  const factor = 10 ** decimals;
  return Math.round((value + Number.EPSILON) * factor) / factor;
}
const round1 = (value: number) => round(value, 1);
const round4 = (value: number) => round(value, 4);
const share = (count: number, total: number) => (total > 0 ? round4(count / total) : 0);

const AGE_BUCKET_ORDER: AgeBucket[] = ["0-14", "15-59", "60-64", "65+"];

// ---------------------------------------------------------------------------
// coveredPcodes — lazy, memoized on first use
// ---------------------------------------------------------------------------

let pcodesCache: string[] | null = null;

/** Every p-code with 2024 census data — country, 25 districts, 339 DS
 * divisions (365 total). GN divisions are outside census coverage. */
export function coveredPcodes(): string[] {
  if (!pcodesCache) {
    pcodesCache = Object.keys(data.population_2024);
  }
  return pcodesCache;
}

// ---------------------------------------------------------------------------
// population / ethnicity / religion
// ---------------------------------------------------------------------------

/** Raw 2024 population counts for `pcode`, or null if the unit has no census data. */
export function population(pcode: string): Population | null {
  const p = data.population_2024[pcode];
  if (!p) return null;
  return { total: p.t, male: p.m, female: p.f, ages: { ...p.ages } };
}

interface StatsGroup {
  /** The group's `.total` value; 0 when absent (share() then falls back to 0 rather than NaN). */
  total: number;
  /** Member keys, `.total` excluded. */
  items: Map<string, number>;
}

function toGroup(raw: RawStatsGroup | undefined): StatsGroup {
  const items = new Map<string, number>();
  let total = 0;
  if (raw) {
    for (const [key, value] of Object.entries(raw)) {
      if (key === "total") total = value;
      else items.set(key, value);
    }
  }
  return { total, items };
}

/** Group members sorted desc by count, each `share` a fraction of the group's own total. */
function toEntries(group: StatsGroup): StatEntry[] {
  return [...group.items.entries()]
    .map(([key, count]) => ({ key, count, share: share(count, group.total) }))
    .sort((a, b) => b.count - a.count);
}

/** 2024 ethnicity breakdown for `pcode`, desc by count, or null if uncovered. */
export function ethnicity(pcode: string): StatEntry[] | null {
  const stats = data.stats_2024[pcode];
  if (!stats) return null;
  return toEntries(toGroup(stats.ethnicity));
}

/** 2024 religion breakdown for `pcode`, desc by count, or null if uncovered. */
export function religion(pcode: string): StatEntry[] | null {
  const stats = data.stats_2024[pcode];
  if (!stats) return null;
  return toEntries(toGroup(stats.religion));
}

// ---------------------------------------------------------------------------
// change2012
// ---------------------------------------------------------------------------

/** Keys present in both census years only (2012 ethnicity lacks
 * sriLankaChetty/bharatha/veddha — those are 2024-only groups, excluded
 * here). Sorted desc by the 2024 count, matching ethnicity()/religion() order. */
function toChangeEntries(group2012: StatsGroup, group2024: StatsGroup): ChangeEntry[] {
  const entries: (ChangeEntry & { count2024: number })[] = [];
  for (const [key, count2012] of group2012.items) {
    const count2024 = group2024.items.get(key);
    if (count2024 === undefined) continue;
    const share2012 = share(count2012, group2012.total);
    const share2024 = share(count2024, group2024.total);
    entries.push({ key, share2012, share2024, delta: round4(share2024 - share2012), count2024 });
  }
  return entries.sort((a, b) => b.count2024 - a.count2024).map(({ count2024: _count2024, ...rest }) => rest);
}

// ---------------------------------------------------------------------------
// demographics
// ---------------------------------------------------------------------------

/**
 * Full derived demographic profile for `pcode`, matching the hosted API's
 * `/v1/demographics` semantics. Null when the unit has no 2024 census data
 * (only country + district + DS division p-codes are covered — GN
 * divisions return null). `change2012` is null unless `pcode` is one of
 * the 25 districts the 2012 census covered.
 */
export function demographics(pcode: string): Demographics | null {
  const pop = population(pcode);
  if (!pop) return null;

  const child = pop.ages["0-14"];
  const workingCore = pop.ages["15-59"];
  const workingElder = pop.ages["60-64"];
  const elderly = pop.ages["65+"];
  const workingTotal = workingCore + workingElder;
  const dependents = child + elderly;

  const ageBuckets: AgeBucketShare[] = AGE_BUCKET_ORDER.map((bucket) => {
    const count = pop.ages[bucket];
    return { bucket, count, share: share(count, pop.total) };
  });

  const stats2024 = data.stats_2024[pcode];
  const ethnicity2024 = toGroup(stats2024?.ethnicity);
  const religion2024 = toGroup(stats2024?.religion);

  let change2012: Demographics["change2012"] = null;
  const stats2012 = data.stats_2012[pcode];
  if (stats2012) {
    change2012 = {
      ethnicity: toChangeEntries(toGroup(stats2012.ethnicity), ethnicity2024),
      religion: toChangeEntries(toGroup(stats2012.religion), religion2024),
    };
  }

  return {
    pcode,
    censusYear: CENSUS_YEAR,
    population: {
      total: pop.total,
      male: pop.male,
      female: pop.female,
      sexRatio: round1(pop.female > 0 ? (pop.male / pop.female) * 100 : 0),
      femaleShare: share(pop.female, pop.total),
    },
    age: {
      buckets: ageBuckets,
      dependencyRatio: round1(workingTotal > 0 ? (dependents / workingTotal) * 100 : 0),
      workingAgeShare: share(workingTotal, pop.total),
      childShare: share(child, pop.total),
      elderlyShare: share(elderly, pop.total),
    },
    ethnicity: toEntries(ethnicity2024),
    religion: toEntries(religion2024),
    change2012,
  };
}
