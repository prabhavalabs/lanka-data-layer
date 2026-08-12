import type { Hono } from "hono";
import type Database from "better-sqlite3";
import { z } from "zod";
import { SEARCH } from "@geopub/shared";
import { buildMeta, ok } from "../lib/envelope.ts";
import { ValidationError, formatZodIssues } from "../lib/errors.ts";
import { sanitizeForFts, scorePlaceCandidates } from "../lib/placeSearch.ts";

const QuerySchema = z.object({
  q: z.string().min(SEARCH.MIN_QUERY_LEN).max(SEARCH.MAX_QUERY_LEN),
  lang: z.enum(["en", "si", "ta"]).optional().default("en"),
  limit: z.coerce.number().int().min(1).max(SEARCH.MAX_LIMIT).optional().default(SEARCH.DEFAULT_LIMIT),
  min_population: z.coerce.number().min(0).optional(),
});

/**
 * GET /v1/search?q&lang&limit&min_population — see lib/placeSearch.ts for
 * the ranking behavior; this route just validates and shapes the query.
 */
export function mountSearchRoute(app: Hono, db: Database.Database): void {
  app.get("/v1/search", (c) => {
    const parsed = QuerySchema.safeParse(c.req.query());
    if (!parsed.success) throw new ValidationError(formatZodIssues(parsed.error.issues));
    const { q, lang, limit, min_population } = parsed.data;

    const sanitized = sanitizeForFts(q);
    if (!sanitized) throw new ValidationError("q must contain at least one searchable character");

    const results = scorePlaceCandidates(db, sanitized, q, lang, { limit, minPopulation: min_population });

    const meta = buildMeta(db, ["places"]);
    return c.json(ok(results, meta));
  });
}
