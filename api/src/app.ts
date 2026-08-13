import { Hono } from "hono";
import type Database from "better-sqlite3";
import { createHash } from "node:crypto";
import { ZodError } from "zod";
import { buildMeta, fail, getDataVersion } from "./lib/envelope.ts";
import { HttpError, formatZodIssues } from "./lib/errors.ts";
import { mountHealthRoute } from "./routes/health.ts";
import { mountDatasetsRoute } from "./routes/datasets.ts";
import { mountAdminRoute } from "./routes/admin.ts";
import { mountAdminGeometryRoute } from "./routes/geometry.ts";
import { mountReverseRoute } from "./routes/reverse.ts";
import { mountSearchRoute } from "./routes/search.ts";
import { mountLookupRoute } from "./routes/lookup.ts";
import { mountPopulationRoute } from "./routes/population.ts";
import { mountElectionsRoute } from "./routes/elections.ts";
import { mountPostalRoute } from "./routes/postal.ts";
import { mountTilesRoute } from "./routes/tiles.ts";

/** Paths that never get the caching layer applied — just the health check for now (contract §4, task spec). */
const UNCACHED_PATHS = new Set(["/v1/health"]);

/** Canonical `<data_version>-<path+query hash>` ETag (contract §4). Query params are sorted so param order never changes the tag. */
function computeEtag(dataVersion: string, path: string, searchParams: URLSearchParams): string {
  const sorted = [...searchParams.entries()].sort(([a], [b]) => a.localeCompare(b));
  const canonical = path + "?" + new URLSearchParams(sorted).toString();
  const hash = createHash("sha1").update(canonical).digest("hex").slice(0, 16);
  return `"${dataVersion}-${hash}"`;
}

/**
 * Builds the Hono app against an already-open database. Factory pattern so
 * tests can inject a fixture database via app.request() instead of the
 * process's real LANKA_DB (docs/architecture.md §5 — api never writes,
 * only ever reads whatever db it's handed).
 */
export function buildApp(db: Database.Database): Hono {
  const app = new Hono();

  // ETag / Cache-Control per contract §4, applied to every /v1 GET except /health.
  // Computed up front — the tag only depends on data_version + path/query, never
  // on response content — so a matching If-None-Match short-circuits before any
  // route handler (and its DB queries) run at all.
  app.use("/v1/*", async (c, next) => {
    const path = new URL(c.req.url).pathname;
    if (UNCACHED_PATHS.has(path)) {
      await next();
      return;
    }
    const dataVersion = getDataVersion(db);
    const etag = computeEtag(dataVersion, path, new URL(c.req.url).searchParams);
    // Short freshness + long stale-while-revalidate: responses serve
    // instantly from cache, but browsers revalidate (cheap ETag 304) within
    // minutes — so data corrections reach users promptly. A long max-age
    // here previously pinned stale data client-side for a full day after a
    // release, with no revalidation at all.
    c.header("Cache-Control", "public, max-age=300, stale-while-revalidate=86400");
    c.header("ETag", etag);
    if (c.req.method === "GET" && c.req.header("If-None-Match") === etag) {
      return c.body(null, 304);
    }
    await next();
  });

  mountHealthRoute(app, db);
  mountDatasetsRoute(app, db);
  mountAdminRoute(app, db);
  mountAdminGeometryRoute(app, db);
  mountReverseRoute(app, db);
  mountSearchRoute(app, db);
  mountLookupRoute(app, db);
  mountPopulationRoute(app, db);
  mountElectionsRoute(app, db);
  mountPostalRoute(app, db);
  mountTilesRoute(app, db);

  app.notFound((c) => {
    const meta = buildMeta(db, []);
    return c.json(fail("not found", meta), 404);
  });

  app.onError((err, c) => {
    const meta = buildMeta(db, []);
    if (err instanceof HttpError) {
      return c.json(fail(err.message, meta), err.status as 400 | 404 | 500);
    }
    if (err instanceof ZodError) {
      return c.json(fail(formatZodIssues(err.issues), meta), 400);
    }
    // Internals masked per contract §4 — log server-side, never leak details in the response.
    console.error(err);
    return c.json(fail("internal server error", meta), 500);
  });

  return app;
}
