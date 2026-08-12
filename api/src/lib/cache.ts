import type Database from "better-sqlite3";

/**
 * Lazy per-database prepared-statement cache. better-sqlite3 statements are
 * synchronous and cheap to reuse, but preparing them is not free — every
 * route calls `prepared(db, sql)` instead of `db.prepare(sql)` so a given
 * SQL string is compiled once per database instance.
 *
 * Keyed by db instance (WeakMap) rather than a single module-level map so
 * that tests, which open a fresh fixture database per test file, never
 * reuse a statement bound to a closed connection.
 */
const statementCaches = new WeakMap<Database.Database, Map<string, Database.Statement>>();

export function prepared<Row = unknown>(db: Database.Database, sql: string): Database.Statement<unknown[], Row> {
  let cache = statementCaches.get(db);
  if (!cache) {
    cache = new Map();
    statementCaches.set(db, cache);
  }
  let stmt = cache.get(sql);
  if (!stmt) {
    stmt = db.prepare(sql);
    cache.set(sql, stmt);
  }
  return stmt as Database.Statement<unknown[], Row>;
}
