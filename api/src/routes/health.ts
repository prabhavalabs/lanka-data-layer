import type { Hono } from "hono";
import type Database from "better-sqlite3";
import { buildMeta, ok } from "../lib/envelope.ts";

/** GET /v1/health — liveness probe. No caching, no dataset attribution, excluded from any future auth. */
export function mountHealthRoute(app: Hono, db: Database.Database): void {
  app.get("/v1/health", (c) => {
    const meta = buildMeta(db, []);
    return c.json(ok({ status: "ok" }, meta));
  });
}
