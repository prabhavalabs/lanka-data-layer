import { test } from "node:test";
import assert from "node:assert/strict";
import { buildApp } from "../src/app.ts";
import { buildFixtureDb, DATA_VERSION } from "./fixture.ts";
import { readJson } from "./helpers.ts";

const db = buildFixtureDb();
const app = buildApp(db);

test("GET /v1/admin/:pcode/geometry returns a bare GeoJSON Feature (no envelope)", async () => {
  const res = await app.request("/v1/admin/LK1101/geometry");
  assert.equal(res.status, 200);
  const body = await readJson(res);
  assert.equal(body.type, "Feature");
  assert.equal(body.properties.pcode, "LK1101");
  assert.equal(body.properties.name, "Colombo DS Division");
  assert.equal(body.properties.level, 3);
  assert.equal(body.properties.parent_pcode, "LK11");
  assert.equal(body.geometry.type, "Polygon");
  assert.ok(Array.isArray(body.geometry.coordinates));
  // Not wrapped in {success, message, payload, meta} like every other route.
  assert.equal("success" in body, false);
  assert.equal("payload" in body, false);
});

test("GET /v1/admin/:pcode/geometry resolves the name in the requested lang", async () => {
  const res = await app.request("/v1/admin/LK110101/geometry?lang=si");
  const body = await readJson(res);
  assert.equal(body.properties.name, "කොටුව");
});

test("GET /v1/admin/:pcode/geometry 404s when the pcode exists but has no geometry row", async () => {
  // LK21 (Galle District) is a real admin_units row with no matching admin_geometry row.
  const res = await app.request("/v1/admin/LK21/geometry");
  assert.equal(res.status, 404);
  const body = await readJson(res);
  assert.equal(body.success, false);
});

test("GET /v1/admin/:pcode/geometry 404s for a wholly unknown pcode", async () => {
  const res = await app.request("/v1/admin/LK999999/geometry");
  assert.equal(res.status, 404);
});

test("GET /v1/admin/:pcode/geometry sets a strong, immutable Cache-Control", async () => {
  const res = await app.request("/v1/admin/LK1101/geometry");
  assert.equal(res.headers.get("Cache-Control"), "public, max-age=31536000, immutable");
});

test("GET /v1/admin/:pcode/geometry still sets a data_version-based ETag", async () => {
  const res = await app.request("/v1/admin/LK1101/geometry");
  const etag = res.headers.get("ETag");
  assert.ok(etag);
  assert.ok(etag!.startsWith(`"${DATA_VERSION}-`));
});
