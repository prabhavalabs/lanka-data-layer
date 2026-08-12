import type { Hono } from "hono";
import type Database from "better-sqlite3";
import { buildMeta, ok } from "../lib/envelope.ts";
import { prepared } from "../lib/cache.ts";

interface DatasetRow {
  id: string;
  title: string;
  category: string;
  description: string | null;
  source_name: string;
  source_url: string;
  license: string;
  feature_count: number | null;
  download_path: string | null;
}

/** GET /v1/datasets — the full dataset catalog. */
export function mountDatasetsRoute(app: Hono, db: Database.Database): void {
  app.get("/v1/datasets", (c) => {
    const stmt = prepared<DatasetRow>(
      db,
      `SELECT id, title, category, description, source_name, source_url, license, feature_count, download_path
       FROM datasets ORDER BY id`,
    );
    const rows = stmt.all();
    const datasetIds = rows.map((r) => r.id);
    const meta = buildMeta(db, datasetIds);
    return c.json(ok(rows, meta));
  });
}
