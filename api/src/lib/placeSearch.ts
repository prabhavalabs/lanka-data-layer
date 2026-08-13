import type Database from "better-sqlite3";
import { SEARCH } from "@lanka-data-layer/shared";
import type { Lang } from "@lanka-data-layer/shared";
import { prepared } from "./cache.ts";
import { resolveName } from "./lang.ts";

export interface PlaceCandidate {
  id: number;
  kind: string;
  name_en: string;
  name_si: string | null;
  name_ta: string | null;
  lat: number;
  lon: number;
  population: number | null;
  admin_pcode: string | null;
  rank: number;
}

export interface ScoredPlace {
  id: number;
  name: string;
  kind: string;
  lat: number;
  lon: number;
  population: number | null;
  admin_pcode: string | null;
  score: number;
}

/** Strips everything but letters (incl. Sinhala/Tamil), digits and spaces so the term is safe to hand to FTS5 MATCH. */
export function sanitizeForFts(q: string): string {
  return q.replace(/[^\p{L}\p{N}\s]/gu, " ").replace(/\s+/g, " ").trim();
}

export interface ScorePlacesOptions {
  limit: number;
  minPopulation?: number;
}

/**
 * FTS5 prefix search over places_fts, re-ranked by
 * score = matchQuality + population boost (shared/src/search.ts constants,
 * contract §2 "Search notes"). matchQuality: exact name match = MATCH_EXACT,
 * name starts with q = MATCH_PREFIX, everything else keeps SQLite's bm25
 * order but is scaled below MATCH_PREFIX so exact/prefix hits always win
 * ties.
 *
 * Extracted from routes/search.ts so /v1/lookup's blended branch (contract
 * §4) can reuse the exact same ranking instead of re-implementing it.
 * Callers own query validation — `sanitized` must already be non-empty (see
 * `sanitizeForFts`); this function never throws on empty input, it just
 * returns no candidates, since /v1/lookup wants graceful degradation rather
 * than a 400 when free text happens to sanitize down to nothing.
 */
export function scorePlaceCandidates(
  db: Database.Database,
  sanitized: string,
  rawQuery: string,
  lang: Lang,
  { limit, minPopulation }: ScorePlacesOptions,
): ScoredPlace[] {
  if (!sanitized) return [];
  const matchQuery = `${sanitized}*`;

  const candidateLimit = Math.min(200, limit * 5);
  const candidates = prepared<PlaceCandidate>(
    db,
    `SELECT p.id, p.kind, p.name_en, p.name_si, p.name_ta, p.lat, p.lon, p.population, p.admin_pcode,
            bm25(places_fts) as rank
     FROM places_fts
     JOIN places p ON p.id = places_fts.rowid
     WHERE places_fts MATCH ?
     ORDER BY rank
     LIMIT ?`,
  ).all(matchQuery, candidateLimit);

  const qLower = rawQuery.trim().toLowerCase();
  const names = (row: PlaceCandidate) =>
    [row.name_en, row.name_si, row.name_ta].filter((n): n is string => !!n).map((n) => n.toLowerCase());

  const scored = candidates.map((row, i) => {
    const rowNames = names(row);
    let matchQuality: number;
    if (rowNames.includes(qLower)) {
      matchQuality = SEARCH.MATCH_EXACT;
    } else if (rowNames.some((n) => n.startsWith(qLower))) {
      matchQuality = SEARCH.MATCH_PREFIX;
    } else {
      // bm25 order, scaled strictly below MATCH_PREFIX so it never outranks a real prefix/exact hit.
      matchQuality = SEARCH.MATCH_PREFIX * (1 - i / candidates.length) * 0.99;
    }
    const population = row.population ?? 0;
    const popBoost = Math.min(SEARCH.POP_BOOST_CAP, Math.log10(1 + population) * SEARCH.POP_BOOST_WEIGHT);
    return { row, score: matchQuality + popBoost };
  });

  const filtered = scored.filter(({ row }) => {
    if (minPopulation === undefined) return true;
    return (row.population ?? 0) >= minPopulation;
  });

  filtered.sort((a, b) => b.score - a.score);

  return filtered.slice(0, limit).map(({ row, score }) => ({
    id: row.id,
    name: resolveName(row, lang),
    kind: row.kind,
    lat: row.lat,
    lon: row.lon,
    population: row.population,
    admin_pcode: row.admin_pcode,
    score,
  }));
}
