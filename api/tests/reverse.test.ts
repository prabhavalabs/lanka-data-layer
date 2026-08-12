import { test } from "node:test";
import assert from "node:assert/strict";
import { buildApp } from "../src/app.ts";
import { buildFixtureDb, FORT_CELL_ID, FORT_LAT, FORT_LON } from "./fixture.ts";
import { readJson } from "./helpers.ts";

const db = buildFixtureDb();
const app = buildApp(db);

test("GET /v1/reverse at a covered cell resolves the full admin chain, postal code and nearest place", async () => {
  const res = await app.request(`/v1/reverse?lat=${FORT_LAT}&lon=${FORT_LON}`);
  assert.equal(res.status, 200);
  const body = await readJson(res);
  assert.equal(body.payload.cell_id, FORT_CELL_ID);
  assert.equal(body.payload.gnd.pcode, "LK110101");
  assert.equal(body.payload.ds_division.pcode, "LK1101");
  assert.equal(body.payload.district.pcode, "LK11");
  assert.equal(body.payload.province.pcode, "LK1");
  assert.equal(body.payload.postal_code, "00100");
  assert.equal(body.payload.nearest_place.id, 1);
  assert.equal(body.payload.nearest_place.distance_m, 350);
});

test("GET /v1/reverse outside the Sri Lanka bbox returns 404 not_in_coverage", async () => {
  const res = await app.request("/v1/reverse?lat=0&lon=0");
  assert.equal(res.status, 404);
  const body = await readJson(res);
  assert.equal(body.success, false);
  assert.match(body.message, /not_in_coverage|coverage/i);
});

test("GET /v1/reverse falls back to nearest place when cell_lookup has no row for the cell", async () => {
  // Nugegoda coordinates: in-bbox, but not the seeded Fort cell, so cell_lookup misses.
  const res = await app.request("/v1/reverse?lat=6.8649&lon=79.8997");
  assert.equal(res.status, 200);
  const body = await readJson(res);
  assert.equal(body.payload.gnd, null);
  assert.equal(body.payload.ds_division, null);
  assert.equal(body.payload.district, null);
  assert.equal(body.payload.province, null);
  assert.equal(body.payload.postal_code, null);
  assert.ok(body.payload.nearest_place, "expected a nearest_place fallback within 25km");
  assert.equal(body.payload.nearest_place.id, 2); // Nugegoda itself, distance ~0
  assert.ok(body.payload.nearest_place.distance_m < 1000);
});

test("GET /v1/reverse requires numeric lat/lon", async () => {
  const res = await app.request("/v1/reverse?lat=notanumber&lon=79.8");
  assert.equal(res.status, 400);
});

test("GET /v1/reverse resolves nearest_place name in the requested lang", async () => {
  const res = await app.request(`/v1/reverse?lat=${FORT_LAT}&lon=${FORT_LON}&lang=si`);
  const body = await readJson(res);
  assert.equal(body.payload.nearest_place.name, "කොළඹ");
});
