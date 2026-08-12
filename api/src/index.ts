import { serve } from "@hono/node-server";
import type Database from "better-sqlite3";
import { resolve } from "node:path";
import { buildApp } from "./app.ts";
import { openDb } from "./db.ts";

const PORT = Number.parseInt(process.env.PORT ?? "8600", 10);
const DB_PATH = resolve(process.env.GEOPUB_DB ?? "../foundry/data/artifacts/geopub.sqlite");
// Always 0.0.0.0: the deployment model (docs/architecture.md §6) restricts exposure by
// publishing the container's port to 127.0.0.1 on the host, not by binding inside it.
const HOSTNAME = "0.0.0.0";

let db: Database.Database;
try {
  db = openDb(DB_PATH);
} catch (err) {
  console.error(`[geopub-api] failed to start: ${err instanceof Error ? err.message : String(err)}`);
  process.exit(1);
}

const app = buildApp(db);

const server = serve({ fetch: app.fetch, port: PORT, hostname: HOSTNAME }, (info) => {
  console.log(`[geopub-api] listening on ${HOSTNAME}:${info.port} (db: ${DB_PATH})`);
});

function shutdown(signal: string) {
  console.log(`[geopub-api] received ${signal}, shutting down`);
  server.close(() => {
    db.close();
    process.exit(0);
  });
}

process.on("SIGINT", () => shutdown("SIGINT"));
process.on("SIGTERM", () => shutdown("SIGTERM"));
