import type Database from "better-sqlite3";
import { SEARCH } from "@lanka-data-layer/shared";
import { prepared } from "./cache.ts";

export interface PostalRow {
  code: string;
  name: string;
  lat: number | null;
  lon: number | null;
}

export interface ScoredPostal extends PostalRow {
  score: number;
}

/** Below MATCH_PREFIX (shared/src/search.ts) so a plain substring hit never outranks a prefix/exact one. */
const MATCH_CONTAINS = 0.4;

/** Exact code lookup — `postal_codes.code` is the primary key, so this is a single indexed point read. */
export function findPostalByCode(db: Database.Database, code: string): PostalRow | undefined {
  return prepared<PostalRow>(db, "SELECT code, name, lat, lon FROM postal_codes WHERE code = ?").get(code);
}

/**
 * Codes sharing a prefix with `prefix`, excluding an exact match on
 * `prefix` itself (the caller already has that, if it exists). Every code
 * in the current artifact is exactly 5 digits (contract §2), so this only
 * matters once codes of varying lengths exist — kept here so /v1/lookup's
 * "postal exact then prefix" branch (contract §4) degrades gracefully
 * either way. `code` has no secondary index beyond the primary key, but
 * `LIKE 'prefix%'` can still use the PK's b-tree ordering.
 */
export function findPostalByCodePrefix(db: Database.Database, prefix: string, limit: number): PostalRow[] {
  return prepared<PostalRow>(
    db,
    "SELECT code, name, lat, lon FROM postal_codes WHERE code LIKE ?||'%' AND code != ? ORDER BY code LIMIT ?",
  ).all(prefix, prefix, limit);
}

/**
 * Name-based postal search for /v1/lookup's blended branch (contract §4,
 * "postal_codes name prefix/LIKE matches"). `postal_codes.name` has no
 * index (foundry owns table DDL, out of scope here) — at 1,833 rows this
 * table scan stays sub-millisecond, well inside the endpoint's latency
 * budget, but it's the one branch of /v1/lookup that isn't index-backed.
 */
export function searchPostalByName(db: Database.Database, q: string, limit: number): ScoredPostal[] {
  const qLower = q.trim().toLowerCase();
  if (!qLower) return [];
  const rows = prepared<PostalRow>(
    db,
    "SELECT code, name, lat, lon FROM postal_codes WHERE name LIKE ?||'%' OR name LIKE '%'||?||'%' LIMIT ?",
  ).all(qLower, qLower, Math.min(200, limit * 5));

  const scored = rows.map((row) => {
    const nameLower = row.name.toLowerCase();
    let score: number;
    if (nameLower === qLower) score = SEARCH.MATCH_EXACT;
    else if (nameLower.startsWith(qLower)) score = SEARCH.MATCH_PREFIX;
    else score = MATCH_CONTAINS;
    return { ...row, score };
  });

  scored.sort((a, b) => b.score - a.score);
  return scored.slice(0, limit);
}
