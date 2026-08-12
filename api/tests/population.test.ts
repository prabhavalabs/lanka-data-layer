import { test } from "node:test";
import assert from "node:assert/strict";
import { buildApp } from "../src/app.ts";
import { buildFixtureDb, FORT_LAT, FORT_LON } from "./fixture.ts";
import { readJson } from "./helpers.ts";

const db = buildFixtureDb();
const app = buildApp(db);

test("GET /v1/population point lookup at a seeded cell", async () => {
  const res = await app.request(`/v1/population?lat=${FORT_LAT}&lon=${FORT_LON}`);
  assert.equal(res.status, 200);
  const body = await readJson(res);
  assert.equal(body.payload.populated, true);
  assert.equal(body.payload.point.in_coverage, true);
  assert.equal(body.payload.point.population, 50);
  assert.equal(body.payload.radius, null);
});

test("GET /v1/population point lookup at an in-coverage but un-seeded cell returns 0 + in_coverage:false", async () => {
  // Well within the SL bbox but nowhere near the seeded 5x5 block around the Fort.
  const res = await app.request("/v1/population?lat=8.5&lon=81.0");
  const body = await readJson(res);
  assert.equal(body.payload.populated, true); // cells table has data in general
  assert.equal(body.payload.point.in_coverage, false); // just not for *this* cell
  assert.equal(body.payload.point.population, 0);
});

test("GET /v1/population?radius sums neighboring cells", async () => {
  const res = await app.request(`/v1/population?lat=${FORT_LAT}&lon=${FORT_LON}&radius=0.5`);
  const body = await readJson(res);
  assert.equal(body.payload.radius.radius_km, 0.5);
  assert.ok(body.payload.radius.cell_count > 1, "expected more than the single point cell");
  assert.ok(body.payload.radius.population > body.payload.point.population);
});

test("radius above 10km is a 400", async () => {
  const res = await app.request(`/v1/population?lat=${FORT_LAT}&lon=${FORT_LON}&radius=11`);
  assert.equal(res.status, 400);
});

test("out-of-coverage coordinates are a 404 not_in_coverage", async () => {
  const res = await app.request("/v1/population?lat=50&lon=50");
  assert.equal(res.status, 404);
});

test("an empty cells table degrades gracefully to populated:false", async () => {
  const emptyDb = buildFixtureDb();
  emptyDb.exec("DELETE FROM cells");
  const emptyApp = buildApp(emptyDb);
  const res = await emptyApp.request(`/v1/population?lat=${FORT_LAT}&lon=${FORT_LON}`);
  assert.equal(res.status, 200);
  const body = await readJson(res);
  assert.equal(body.payload.populated, false);
  assert.equal(body.payload.point.population, 0);
});
