import Database from "better-sqlite3";
import { existsSync } from "node:fs";

/**
 * Opens the Lanka Data Layer SQLite artifact read-only. The API never writes to this
 * file (docs/architecture.md §2, §5) — foundry owns the build, api only
 * reads it. `fileMustExist` means better-sqlite3 refuses to silently create
 * an empty database if the artifact hasn't been built yet; we turn that
 * into a clear startup error instead.
 */
export function openDb(dbPath: string): Database.Database {
  if (!existsSync(dbPath)) {
    throw new Error(
      `Lanka Data Layer SQLite artifact not found at "${dbPath}". ` +
        `Build it with "pnpm foundry run build" or point LANKA_DB at an existing lanka.sqlite file.`,
    );
  }
  const db = new Database(dbPath, { readonly: true, fileMustExist: true });
  db.pragma("query_only = true");
  return db;
}
