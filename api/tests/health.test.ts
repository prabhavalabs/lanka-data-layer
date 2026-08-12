import { test } from "node:test";
import assert from "node:assert/strict";
import { buildApp } from "../src/app.ts";
import { buildFixtureDb, DATA_VERSION } from "./fixture.ts";
import { readJson } from "./helpers.ts";

const db = buildFixtureDb();
const app = buildApp(db);

test("GET /v1/health returns ok status", async () => {
  const res = await app.request("/v1/health");
  assert.equal(res.status, 200);
  const body = await readJson(res);
  assert.equal(body.success, true);
  assert.deepEqual(body.payload, { status: "ok" });
  assert.equal(body.meta.data_version, DATA_VERSION);
});

test("GET /v1/health sets no ETag / Cache-Control", async () => {
  const res = await app.request("/v1/health");
  assert.equal(res.headers.get("ETag"), null);
  assert.equal(res.headers.get("Cache-Control"), null);
});
