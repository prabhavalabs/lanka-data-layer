import type { Hono } from "hono";
import type Database from "better-sqlite3";
import { z } from "zod";
import { cellId } from "@geopub/shared";
import { buildMeta, ok } from "../lib/envelope.ts";
import { NotInCoverageError, ValidationError, formatZodIssues } from "../lib/errors.ts";
import { reverseGeocodeAtCell } from "../lib/reverseGeocode.ts";

const QuerySchema = z.object({
  lat: z.coerce.number().finite(),
  lon: z.coerce.number().finite(),
  lang: z.enum(["en", "si", "ta"]).optional().default("en"),
});

/**
 * GET /v1/reverse?lat&lon — cellId via the shared grid, then cell_lookup +
 * admin parent chain (lib/reverseGeocode.ts has the full behavior notes,
 * including the nearest-place fallback for cells missing from cell_lookup).
 */
export function mountReverseRoute(app: Hono, db: Database.Database): void {
  app.get("/v1/reverse", (c) => {
    const parsed = QuerySchema.safeParse(c.req.query());
    if (!parsed.success) throw new ValidationError(formatZodIssues(parsed.error.issues));
    const { lat, lon, lang } = parsed.data;

    const id = cellId(lat, lon);
    if (id === null) throw new NotInCoverageError();

    const { result, datasetIds } = reverseGeocodeAtCell(db, id, lat, lon, lang);

    const meta = buildMeta(db, [...datasetIds]);
    return c.json(ok(result, meta));
  });
}
