import type Database from "better-sqlite3";
import type { Envelope, ResponseMeta, SourceAttribution } from "@geopub/shared";
import { prepared } from "./cache.ts";

interface DatasetRow {
  source_name: string;
  source_url: string;
  license: string;
}

/** Reads `meta.value` for key 'data_version'. Falls back to "unknown" if the row is absent. */
export function getDataVersion(db: Database.Database): string {
  const stmt = prepared<{ value: string }>(db, "SELECT value FROM meta WHERE key = 'data_version'");
  const row = stmt.get();
  return row?.value ?? "unknown";
}

/**
 * Looks up source attribution rows from `datasets` for the given dataset
 * ids. Unknown ids are silently skipped — attribution is best-effort and
 * must never turn into a 500 for a route that otherwise succeeded.
 */
export function sourceFor(db: Database.Database, datasetIds: readonly string[]): SourceAttribution[] {
  if (datasetIds.length === 0) return [];
  const stmt = prepared<DatasetRow>(db, "SELECT source_name, source_url, license FROM datasets WHERE id = ?");
  const out: SourceAttribution[] = [];
  for (const id of datasetIds) {
    const row = stmt.get(id);
    if (row) out.push({ name: row.source_name, url: row.source_url, license: row.license });
  }
  return out;
}

/** Builds the `meta` block every envelope carries: data_version + source attribution for datasets touched. */
export function buildMeta(db: Database.Database, datasetIds: readonly string[] = []): ResponseMeta {
  return {
    data_version: getDataVersion(db),
    source: sourceFor(db, datasetIds),
  };
}

export function ok<T>(payload: T, meta: ResponseMeta, message = "OK"): Envelope<T> {
  return { success: true, message, payload, meta };
}

export function fail(message: string, meta: ResponseMeta): Envelope<null> {
  return { success: false, message, payload: null, meta };
}
