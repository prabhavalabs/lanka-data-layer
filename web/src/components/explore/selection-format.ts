import type { AdminLevel } from "@lanka-data-layer/shared";

/** Level name shown in the type chip / breadcrumb tooltips. */
export const ADMIN_LEVEL_LABELS: Record<AdminLevel, string> = {
  0: "Country",
  1: "Province",
  2: "District",
  3: "DS Division",
  4: "GN Division",
};

/** What a unit's *children* are called, for the "N Districts" summary line — null at level 4 (GN divisions have none). */
export const ADMIN_CHILD_LABELS: Record<AdminLevel, string | null> = {
  0: "Provinces",
  1: "Districts",
  2: "DS Divisions",
  3: "GN Divisions",
  4: null,
};

/**
 * Drops the redundant level word from an admin unit's name for breadcrumb
 * display — "Colombo District" -> "Colombo", "Western Province" ->
 * "Western". DS/GN division names never carry a suffix, so this is a no-op
 * for them.
 */
export function shortAdminLabel(name: string): string {
  return name.replace(/\s+(District|Province)$/, "");
}

/**
 * `admin_population.year` tells the two source generations apart, per the
 * task brief: GN/DS divisions carry a 2020 WorldPop-modeled total (sex
 * breakdown not attempted at that resolution); districts/provinces carry a
 * 2023 HDX projection with a real f/m split. Keyed on the year the API
 * actually returned rather than hardcoded per-level, so this stays correct
 * if a future data release changes which levels have which vintage.
 */
export function populationSourceLabel(year: number): string {
  return year === 2020 ? `WorldPop ${year} (modeled)` : `HDX projection ${year}`;
}

/** camelCase stat key segment -> "Title Case" — "sriLankanTamil" -> "Sri Lankan Tamil", "otherChristian" -> "Other Christian". */
export function humanizeStatKey(key: string): string {
  const withSpaces = key.replace(/([a-z0-9])([A-Z])/g, "$1 $2");
  return withSpaces.charAt(0).toUpperCase() + withSpaces.slice(1);
}

export function formatNumber(n: number): string {
  return Math.round(n).toLocaleString("en-US");
}

export function formatArea(km2: number): string {
  return `${km2 < 10 ? km2.toFixed(2) : km2.toFixed(1)} km²`;
}

export function formatDensity(perKm2: number): string {
  return `${formatNumber(perKm2)} / km²`;
}

export function formatDistance(meters: number): string {
  return meters < 1000 ? `${Math.round(meters)} m` : `${(meters / 1000).toFixed(1)} km`;
}

export function formatCoord(lat: number, lon: number): string {
  return `${lat.toFixed(4)}, ${lon.toFixed(4)}`;
}

/** A small, theme-agnostic categorical palette for the demographics stacked bars — reuses the JSON-syntax tokens already in index.css (blue/green/amber) plus the brand rose, so no new arbitrary colors enter the design system. "Other" gets a neutral gray. */
export const STAT_BAR_COLORS = [
  "var(--brand, #D8446B)",
  "var(--json-key, #8AB4F8)",
  "var(--json-str, #6BD9A5)",
  "var(--json-num, #F0B15C)",
];
export const STAT_BAR_OTHER_COLOR = "var(--ink3, #66737F)";

export interface StatSegment {
  key: string;
  label: string;
  value: number;
  share: number;
  color: string;
}

/**
 * Turns a flat `{"ethnicity.sinhala": 1732530, "ethnicity.total": 2324349,
 * ...}` stats block into the top-4-plus-other segments a stacked bar wants,
 * for one `prefix` ("ethnicity" or "religion") at a time. Shares are
 * against the prefix's own `.total` key when present, else the sum of its
 * parts — admin_stats is real census data so these are expected to already
 * sum close to the total, but computing shares off a fallback sum keeps the
 * bar meaningful even if a future dataset omits the total row.
 */
export function buildStatSegments(values: Record<string, number>, prefix: string): StatSegment[] {
  const entries = Object.entries(values)
    .filter(([key]) => key.startsWith(`${prefix}.`) && key !== `${prefix}.total`)
    .map(([key, value]) => ({ key: key.slice(prefix.length + 1), value }))
    .filter((e) => e.value > 0)
    .sort((a, b) => b.value - a.value);

  const total = values[`${prefix}.total`] ?? entries.reduce((sum, e) => sum + e.value, 0);
  if (total <= 0 || entries.length === 0) return [];

  // The dataset carries its own catch-all bucket (e.g. "ethnicity.other")
  // alongside the named categories, which can easily out-rank a smaller
  // named category by raw count and land in the top-4 on its own — then
  // the *next* pass's top-4-plus-other overflow bucket would also be
  // labeled "Other", showing two identically-labeled segments in the same
  // bar. Demoting the dataset's own "other" entry to sort last (but still
  // keeping the rest value-sorted) means it only ever contributes to this
  // function's single "Other" overflow bucket instead of also claiming a
  // top-4 slot of its own.
  const ranked = [...entries.filter((e) => e.key !== "other"), ...entries.filter((e) => e.key === "other")];

  const top = ranked.slice(0, 4);
  const rest = ranked.slice(4);
  const restValue = rest.reduce((sum, e) => sum + e.value, 0);

  const segments: StatSegment[] = top.map((e, i) => ({
    key: e.key,
    label: humanizeStatKey(e.key),
    value: e.value,
    share: e.value / total,
    color: STAT_BAR_COLORS[i % STAT_BAR_COLORS.length],
  }));
  if (restValue > 0) {
    segments.push({ key: "other", label: "Other", value: restValue, share: restValue / total, color: STAT_BAR_OTHER_COLOR });
  }
  return segments;
}
