import { test } from "node:test";
import assert from "node:assert/strict";
import { buildApp } from "../src/app.ts";
import { buildFixtureDb, FORT_LAT, FORT_LON } from "./fixture.ts";
import { readJson } from "./helpers.ts";

const db = buildFixtureDb();
const app = buildApp(db);

test("GET /v1/postal/:code returns the record with its admin chain resolved via cell_lookup", async () => {
  // "00300" sits at the Kompannavidiya GN division centroid, which cell_lookup covers.
  const res = await app.request("/v1/postal/00300");
  assert.equal(res.status, 200);
  const body = await readJson(res);
  assert.equal(body.payload.code, "00300");
  assert.equal(body.payload.name, "Kompannavidiya");
  assert.equal(body.payload.gnd.pcode, "LK110102");
  assert.equal(body.payload.ds_division.pcode, "LK1101");
  assert.equal(body.payload.district.pcode, "LK11");
  assert.equal(body.payload.province.pcode, "LK1");
});

test("GET /v1/postal/:code returns an all-null admin chain when its point isn't covered by cell_lookup", async () => {
  // "10250" sits at Nugegoda's coords, which cell_lookup deliberately doesn't cover (fixture.ts).
  const res = await app.request("/v1/postal/10250");
  assert.equal(res.status, 200);
  const body = await readJson(res);
  assert.equal(body.payload.code, "10250");
  assert.equal(body.payload.gnd, null);
  assert.equal(body.payload.ds_division, null);
  assert.equal(body.payload.district, null);
  assert.equal(body.payload.province, null);
});

test("GET /v1/postal/:code 404s for an unknown code", async () => {
  const res = await app.request("/v1/postal/00000");
  assert.equal(res.status, 404);
  const body = await readJson(res);
  assert.equal(body.success, false);
});

test("GET /v1/postal/:code resolves the admin chain names in the requested lang", async () => {
  // LK110102 (Kompannavidiya) has no name_si in the fixture, so it falls back to English.
  const res = await app.request("/v1/postal/00300?lang=si");
  const body = await readJson(res);
  assert.equal(body.payload.gnd.name, "Kompannavidiya");
  // LK11 (Colombo District) does have a name_si.
  assert.equal(body.payload.district.name, "කොළඹ දිස්ත්‍රික්කය");
});

test("GET /v1/postal?lat&lon returns the cell_lookup postal_code and its name for that point", async () => {
  const res = await app.request(`/v1/postal?lat=${FORT_LAT}&lon=${FORT_LON}`);
  assert.equal(res.status, 200);
  const body = await readJson(res);
  assert.equal(body.payload.postal_code, "00100");
  assert.equal(body.payload.name, "Colombo Fort");
});

test("GET /v1/postal?lat&lon returns null postal_code for a covered cell with no postal code (Kolonnawa)", async () => {
  const res = await app.request("/v1/postal?lat=6.932&lon=79.8886");
  assert.equal(res.status, 200);
  const body = await readJson(res);
  assert.equal(body.payload.postal_code, null);
  assert.equal(body.payload.name, null);
});

test("GET /v1/postal?lat&lon out of coverage is a 404 not_in_coverage", async () => {
  const res = await app.request("/v1/postal?lat=0&lon=0");
  assert.equal(res.status, 404);
});

test("GET /v1/postal?lat&lon requires numeric lat/lon", async () => {
  const res = await app.request("/v1/postal?lat=notanumber&lon=79.8");
  assert.equal(res.status, 400);
});

test("GET /v1/postal carries the standard envelope", async () => {
  const res = await app.request(`/v1/postal?lat=${FORT_LAT}&lon=${FORT_LON}`);
  const body = await readJson(res);
  assert.ok("success" in body && "message" in body && "payload" in body && "meta" in body);
});
