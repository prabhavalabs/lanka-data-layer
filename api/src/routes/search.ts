import type { Hono } from "hono";
import type Database from "better-sqlite3";
import { z } from "zod";
import { SEARCH } from "@geopub/shared";
import { buildMeta, ok } from "../lib/envelope.ts";
import { prepared } from "../lib/cache.ts";
import { ValidationError, formatZodIssues } from "../lib/errors.ts";
import { resolveName } from "../lib/lang.ts";

const QuerySchema = z.object({
  q: z.string().min(SEARCH.MIN_QUERY_LEN).max(SEARCH.MAX_QUERY_LEN),
  lang: z.enum(["en", "si", "ta"]).optional().default("en"),
  limit: z.coerce.number().int().min(1).max(SEARCH.MAX_LIMIT).optional().default(SEARCH.DEFAULT_LIMIT),
  min_population: z.coerce.number().min(0).optional(),
});

interface PlaceCandidate {
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

/** Strips everything but letters (incl. Sinhala/Tamil), digits and spaces so the term is safe to hand to FTS5 MATCH. */
function sanitizeForFts(q: string): string {
  return q.replace(/[^\p{L}\p{N}\s]/gu, " ").replace(/\s+/g, " ").trim();
}

/**
 * GET /v1/search?q&lang&limit&min_population — FTS5 prefix search over
 * places_fts, re-ranked by score = matchQuality + population boost
 * (shared/src/search.ts constants, contract §2 "Search notes").
 * matchQuality: exact name match = MATCH_EXACT, name starts with q =
 * MATCH_PREFIX, everything else keeps SQLite's bm25 order but is scaled
 * below MATCH_PREFIX so exact/prefix hits always win ties.
 */
export function mountSearchRoute(app: Hono, db: Database.Database): void {
  app.get("/v1/search", (c) => {
    const parsed = QuerySchema.safeParse(c.req.query());
    if (!parsed.success) throw new ValidationError(formatZodIssues(parsed.error.issues));
    const { q, lang, limit, min_population } = parsed.data;

    const sanitized = sanitizeForFts(q);
    if (!sanitized) throw new ValidationError("q must contain at least one searchable character");
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

    const qLower = q.trim().toLowerCase();
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
      if (min_population === undefined) return true;
      return (row.population ?? 0) >= min_population;
    });

    filtered.sort((a, b) => b.score - a.score);

    const results = filtered.slice(0, limit).map(({ row, score }) => ({
      id: row.id,
      name: resolveName(row, lang),
      kind: row.kind,
      lat: row.lat,
      lon: row.lon,
      population: row.population,
      admin_pcode: row.admin_pcode,
      score,
    }));

    const meta = buildMeta(db, ["places"]);
    return c.json(ok(results, meta));
  });
}
