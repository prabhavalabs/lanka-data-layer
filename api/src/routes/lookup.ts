import type { Hono } from "hono";
import type Database from "better-sqlite3";
import { z } from "zod";
import { cellId, SEARCH } from "@geopub/shared";
import type { AdminLevel, Lang } from "@geopub/shared";
import { buildMeta, ok } from "../lib/envelope.ts";
import { prepared } from "../lib/cache.ts";
import { NotInCoverageError, ValidationError, formatZodIssues } from "../lib/errors.ts";
import { getAdminUnitRow, mapAdminUnit, resolveAdminLevels } from "../lib/admin.ts";
import { resolveName } from "../lib/lang.ts";
import { parseCoordinatePair } from "../lib/coords.ts";
import { reverseGeocodeAtCell } from "../lib/reverseGeocode.ts";
import { sanitizeForFts, scorePlaceCandidates } from "../lib/placeSearch.ts";
import type { ScoredPlace } from "../lib/placeSearch.ts";
import { findPostalByCode, findPostalByCodePrefix, searchPostalByName } from "../lib/postalSearch.ts";
import type { PostalRow } from "../lib/postalSearch.ts";

const SUGGEST_MAX = 10;

const LEVEL_NAMES: Record<AdminLevel, string> = {
  0: "Country",
  1: "Province",
  2: "District",
  3: "DS Division",
  4: "GN Division",
};

const QuerySchema = z.object({
  q: z.string().trim().min(1).max(200),
  lang: z.enum(["en", "si", "ta"]).optional().default("en"),
  suggest: z.string().optional(),
  limit: z.coerce.number().int().min(1).max(SEARCH.MAX_LIMIT).optional().default(SEARCH.DEFAULT_LIMIT),
});

type Classification =
  | { kind: "coordinate"; lat: number; lon: number }
  | { kind: "postal"; code: string }
  | { kind: "admin"; pcode: string }
  | { kind: "blended" };

/**
 * Classifier order per contract §4: coordinate pair → 5-digit postal code
 * → LK-prefixed p-code → free-text blend. Each branch is a cheap regex
 * test, so classification itself never touches the database.
 */
function classify(q: string): Classification {
  const coord = parseCoordinatePair(q);
  if (coord) return { kind: "coordinate", lat: coord.lat, lon: coord.lon };
  // 3-4 digits classify as postal too: codes are exactly 5 digits, so a
  // shorter all-numeric query can only sensibly be a code being typed —
  // the prefix branch handles it (e.g. "105" → 10500, 10502, …).
  if (/^\d{3,5}$/.test(q)) return { kind: "postal", code: q };
  if (/^LK\d+$/i.test(q)) return { kind: "admin", pcode: q.toUpperCase() };
  return { kind: "blended" };
}

function capitalize(s: string): string {
  return s.length === 0 ? s : s.charAt(0).toUpperCase() + s.slice(1);
}

interface FullRow {
  type: "coordinate" | "postal" | "admin" | "place";
  score: number;
  payload: unknown;
}

interface SuggestRow {
  type: "coordinate" | "postal" | "admin" | "place";
  id: number | string;
  label: string;
  sublabel: string;
  lat: number | null;
  lon: number | null;
  pcode?: string;
}

interface BranchOutcome<Row> {
  rows: Row[];
  datasetIds: Set<string>;
}

function postalToFullRow(row: PostalRow, score: number): FullRow {
  return { type: "postal", score, payload: row };
}

function postalToSuggestRow(row: PostalRow): SuggestRow {
  // Contract §4 suggest-mode shape: "postal: place name" is the sublabel —
  // `postal_codes.name` (e.g. "Nugegoda") IS that place name, so it goes in
  // the sublabel; the code itself (what a 5-digit query actually matched
  // on) is the label.
  return { type: "postal", id: row.code, label: row.code, sublabel: row.name, lat: row.lat, lon: row.lon };
}

/**
 * Best-effort "which admin area is this place in" for suggest-mode
 * sublabels (contract §4: "admin parent for places via cell_lookup of its
 * coords if cheap, else kind"). "Cheap" here means a handful of indexed
 * point lookups (cell_lookup by cell_id PK, then a short parent-pcode
 * chain) — no scans, no joins beyond what admin.ts already does for
 * /v1/reverse. Falls back to the place's `kind` (e.g. "town") when the
 * point isn't covered by cell_lookup at all.
 */
function placeToSuggestRow(db: Database.Database, place: ScoredPlace, lang: Lang): SuggestRow {
  let sublabel = capitalize(place.kind);
  let pcode: string | undefined;

  const id = cellId(place.lat, place.lon);
  if (id !== null) {
    const lookup = prepared<{ gnd_pcode: string }>(
      db,
      "SELECT gnd_pcode FROM cell_lookup WHERE cell_id = ?",
    ).get(id);
    if (lookup) {
      pcode = lookup.gnd_pcode;
      const levels = resolveAdminLevels(db, lookup.gnd_pcode, lang);
      sublabel = levels.district?.name ?? levels.ds_division?.name ?? levels.gnd?.name ?? capitalize(place.kind);
    }
  }

  return { type: "place", id: place.id, label: place.name, sublabel, lat: place.lat, lon: place.lon, pcode };
}

function runCoordinateFull(db: Database.Database, lat: number, lon: number, lang: Lang): BranchOutcome<FullRow> {
  const id = cellId(lat, lon);
  if (id === null) throw new NotInCoverageError();
  const { result, datasetIds } = reverseGeocodeAtCell(db, id, lat, lon, lang);
  return { rows: [{ type: "coordinate", score: 1, payload: result }], datasetIds };
}

function runCoordinateSuggest(db: Database.Database, lat: number, lon: number, lang: Lang): BranchOutcome<SuggestRow> {
  const id = cellId(lat, lon);
  if (id === null) throw new NotInCoverageError();
  const { result, datasetIds } = reverseGeocodeAtCell(db, id, lat, lon, lang);
  const label = result.gnd?.name ?? result.nearest_place?.name ?? `${lat.toFixed(4)}, ${lon.toFixed(4)}`;
  const sublabel =
    result.district?.name ?? result.province?.name ?? (result.postal_code ? `Postal ${result.postal_code}` : "Sri Lanka");
  const row: SuggestRow = {
    type: "coordinate",
    id: result.cell_id,
    label,
    sublabel,
    lat,
    lon,
    pcode: result.gnd?.pcode ?? undefined,
  };
  return { rows: [row], datasetIds };
}

function runPostalFull(db: Database.Database, code: string, limit: number): BranchOutcome<FullRow> {
  const rows: FullRow[] = [];
  const exact = findPostalByCode(db, code);
  if (exact) rows.push(postalToFullRow(exact, SEARCH.MATCH_EXACT));
  const remaining = limit - rows.length;
  if (remaining > 0) {
    for (const row of findPostalByCodePrefix(db, code, remaining)) {
      rows.push(postalToFullRow(row, SEARCH.MATCH_PREFIX));
    }
  }
  return { rows, datasetIds: new Set(["postal-codes"]) };
}

function runPostalSuggest(db: Database.Database, code: string, limit: number): BranchOutcome<SuggestRow> {
  const rows: SuggestRow[] = [];
  const exact = findPostalByCode(db, code);
  if (exact) rows.push(postalToSuggestRow(exact));
  const remaining = limit - rows.length;
  if (remaining > 0) {
    for (const row of findPostalByCodePrefix(db, code, remaining)) {
      rows.push(postalToSuggestRow(row));
    }
  }
  return { rows, datasetIds: new Set(["postal-codes"]) };
}

function runAdminFull(db: Database.Database, pcode: string, lang: Lang): BranchOutcome<FullRow> {
  const row = getAdminUnitRow(db, pcode);
  if (!row) return { rows: [], datasetIds: new Set() };
  return { rows: [{ type: "admin", score: 1, payload: mapAdminUnit(row, lang) }], datasetIds: new Set(["admin-units"]) };
}

function runAdminSuggest(db: Database.Database, pcode: string, lang: Lang): BranchOutcome<SuggestRow> {
  const row = getAdminUnitRow(db, pcode);
  if (!row) return { rows: [], datasetIds: new Set() };
  const unit = mapAdminUnit(row, lang);
  const levelName = LEVEL_NAMES[row.level];
  const parent = row.parent_pcode ? getAdminUnitRow(db, row.parent_pcode) : undefined;
  const sublabel = parent ? `${levelName} · ${resolveName(parent, lang)}` : levelName;
  const suggestRow: SuggestRow = {
    type: "admin",
    id: unit.pcode,
    label: unit.name,
    sublabel,
    lat: unit.centroid?.lat ?? null,
    lon: unit.centroid?.lon ?? null,
    pcode: unit.pcode,
  };
  return { rows: [suggestRow], datasetIds: new Set(["admin-units"]) };
}

function blendedDatasetIds(places: ScoredPlace[], postal: unknown[], touchedAdmin: boolean): Set<string> {
  const ids = new Set<string>();
  if (places.length > 0) ids.add("places");
  if (postal.length > 0) ids.add("postal-codes");
  if (touchedAdmin) ids.add("admin-units");
  return ids;
}

function runBlendedFull(db: Database.Database, q: string, lang: Lang, limit: number): BranchOutcome<FullRow> {
  const sanitized = sanitizeForFts(q);
  const places = sanitized ? scorePlaceCandidates(db, sanitized, q, lang, { limit }) : [];
  const postal = searchPostalByName(db, q, limit);

  const merged: FullRow[] = [
    ...places.map((p): FullRow => ({ type: "place", score: p.score, payload: p })),
    ...postal.map((p): FullRow => postalToFullRow(p, p.score)),
  ];
  merged.sort((a, b) => b.score - a.score);

  return { rows: merged.slice(0, limit), datasetIds: blendedDatasetIds(places, postal, false) };
}

function runBlendedSuggest(db: Database.Database, q: string, lang: Lang, limit: number): BranchOutcome<SuggestRow> {
  const sanitized = sanitizeForFts(q);
  const places = sanitized ? scorePlaceCandidates(db, sanitized, q, lang, { limit }) : [];
  const postal = searchPostalByName(db, q, limit);

  const scoredRows: { score: number; row: SuggestRow }[] = [
    ...places.map((p) => ({ score: p.score, row: placeToSuggestRow(db, p, lang) })),
    ...postal.map((p) => ({ score: p.score, row: postalToSuggestRow(p) })),
  ];
  scoredRows.sort((a, b) => b.score - a.score);

  return {
    rows: scoredRows.slice(0, limit).map((r) => r.row),
    // The place branch always attempts a cell_lookup lookup for its sublabel (see placeToSuggestRow),
    // so tag admin-units attribution whenever there were any place candidates at all.
    datasetIds: blendedDatasetIds(places, postal, places.length > 0),
  };
}

function runFull(db: Database.Database, classification: Classification, q: string, lang: Lang, limit: number): BranchOutcome<FullRow> {
  switch (classification.kind) {
    case "coordinate":
      return runCoordinateFull(db, classification.lat, classification.lon, lang);
    case "postal":
      return runPostalFull(db, classification.code, limit);
    case "admin":
      return runAdminFull(db, classification.pcode, lang);
    case "blended":
      return runBlendedFull(db, q, lang, limit);
  }
}

function runSuggest(db: Database.Database, classification: Classification, q: string, lang: Lang, limit: number): BranchOutcome<SuggestRow> {
  switch (classification.kind) {
    case "coordinate":
      return runCoordinateSuggest(db, classification.lat, classification.lon, lang);
    case "postal":
      return runPostalSuggest(db, classification.code, limit);
    case "admin":
      return runAdminSuggest(db, classification.pcode, lang);
    case "blended":
      return runBlendedSuggest(db, q, lang, limit);
  }
}

/**
 * GET /v1/lookup?q&lang&suggest&limit — the universal lookup (contract §4).
 * Classifies `q` (coordinate pair → postal code → p-code → free text) and
 * dispatches to the matching branch. `suggest=1` switches every branch to
 * the lightweight {type, id, label, sublabel, lat, lon, pcode?} shape
 * capped at 10 rows; full mode returns {type, score, payload} rows sized
 * by `limit` (same bounds as /v1/search).
 *
 * Only the coordinate branch can 404: it defers to the same
 * reverseGeocodeAtCell path /v1/reverse uses, which throws
 * not_in_coverage for out-of-bbox points. Every other branch treats "no
 * match" as a normal empty result list, not an error — postal/admin/blended
 * queries are searches, and an empty search result isn't a failure.
 */
export function mountLookupRoute(app: Hono, db: Database.Database): void {
  app.get("/v1/lookup", (c) => {
    const parsed = QuerySchema.safeParse(c.req.query());
    if (!parsed.success) throw new ValidationError(formatZodIssues(parsed.error.issues));
    const { q, lang, suggest, limit } = parsed.data;

    const isSuggest = suggest === "1";
    const effectiveLimit = isSuggest ? Math.min(limit, SUGGEST_MAX) : limit;
    const classification = classify(q);

    const outcome = isSuggest
      ? runSuggest(db, classification, q, lang, effectiveLimit)
      : runFull(db, classification, q, lang, effectiveLimit);

    const meta = buildMeta(db, [...outcome.datasetIds]);
    return c.json(ok(outcome.rows, meta));
  });
}
