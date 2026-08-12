import type { Hono } from "hono";
import type Database from "better-sqlite3";
import { z } from "zod";
import { NotFoundError, ValidationError, formatZodIssues } from "../lib/errors.ts";
import { getAdminGeometryFeature } from "../lib/geometry.ts";

const QuerySchema = z.object({
  lang: z.enum(["en", "si", "ta"]).optional().default("en"),
});

/**
 * GET /v1/admin/:pcode/geometry — admin_geometry as a GeoJSON Feature
 * (contract §4 "Geometry for highlights": "{type Feature, properties
 * {pcode, name…}, geometry parsed}"). Unlike every other /v1/* route this
 * deliberately does NOT use the {success, message, payload, meta} envelope
 * — the whole point is that map libraries (MapLibre `source.setData`,
 * Leaflet `L.geoJSON`, ...) can hand the response straight to their GeoJSON
 * loader without unwrapping it first. data_version still travels, via the
 * same ETag every /v1/* GET carries (app.ts's caching middleware); there's
 * no per-request dataset attribution payload here the way there is for
 * envelope routes, matching how /v1/tiles/*.pmtiles also serves raw bytes
 * rather than an envelope.
 *
 * Cache-Control is strengthened beyond the app-wide default: a pcode's
 * geometry never changes within a data_version (contract §3 — artifacts
 * are rebuilt as a whole, versioned as a whole), so this is safe to mark
 * `immutable` rather than the usual max-age=86400 + revalidate.
 */
export function mountAdminGeometryRoute(app: Hono, db: Database.Database): void {
  app.get("/v1/admin/:pcode/geometry", (c) => {
    const parsed = QuerySchema.safeParse(c.req.query());
    if (!parsed.success) throw new ValidationError(formatZodIssues(parsed.error.issues));
    const { lang } = parsed.data;

    const pcode = c.req.param("pcode");
    const feature = getAdminGeometryFeature(db, pcode, lang);
    if (!feature) throw new NotFoundError(`geometry for admin unit "${pcode}" not found`);

    c.header("Cache-Control", "public, max-age=31536000, immutable");
    return c.json(feature);
  });
}
