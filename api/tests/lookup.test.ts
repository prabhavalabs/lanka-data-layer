import { test } from "node:test";
import assert from "node:assert/strict";
import { buildApp } from "../src/app.ts";
import { buildFixtureDb, FORT_CELL_ID, FORT_LAT, FORT_LON } from "./fixture.ts";
import { readJson } from "./helpers.ts";

const db = buildFixtureDb();
const app = buildApp(db);

// --- classifier: coordinate pair -------------------------------------------------

test("GET /v1/lookup?q=<lat,lon> classifies as coordinate and runs the reverse-geocode logic", async () => {
  const res = await app.request(`/v1/lookup?q=${FORT_LAT},${FORT_LON}`);
  assert.equal(res.status, 200);
  const body = await readJson(res);
  assert.equal(body.payload.length, 1);
  assert.equal(body.payload[0].type, "coordinate");
  assert.equal(body.payload[0].score, 1);
  assert.equal(body.payload[0].payload.cell_id, FORT_CELL_ID);
  assert.equal(body.payload[0].payload.gnd.pcode, "LK110101");
  assert.equal(body.payload[0].payload.postal_code, "00100");
});

test("GET /v1/lookup accepts space-separated coordinates with hemisphere letters and degree signs", async () => {
  const res = await app.request(`/v1/lookup?q=${encodeURIComponent(`${FORT_LAT}° N, ${FORT_LON}° E`)}`);
  const body = await readJson(res);
  assert.equal(body.payload[0].type, "coordinate");
  assert.equal(body.payload[0].payload.cell_id, FORT_CELL_ID);
});

test("GET /v1/lookup coordinate branch out of coverage is a 404 not_in_coverage, matching /v1/reverse", async () => {
  const res = await app.request("/v1/lookup?q=0,0");
  assert.equal(res.status, 404);
  const body = await readJson(res);
  assert.match(body.message, /not_in_coverage|coverage/i);
});

// --- classifier: postal code -------------------------------------------------------

test("GET /v1/lookup?q=<5 digits> classifies as postal, exact match first", async () => {
  const res = await app.request("/v1/lookup?q=00100");
  assert.equal(res.status, 200);
  const body = await readJson(res);
  assert.ok(body.payload.length >= 1);
  assert.equal(body.payload[0].type, "postal");
  assert.equal(body.payload[0].score, 1);
  assert.equal(body.payload[0].payload.code, "00100");
  assert.equal(body.payload[0].payload.name, "Colombo Fort");
});

test("GET /v1/lookup postal branch also returns prefix matches after the exact hit", async () => {
  const res = await app.request("/v1/lookup?q=10250");
  const body = await readJson(res);
  const codes = body.payload.map((r: { payload: { code: string } }) => r.payload.code);
  assert.deepEqual(codes, ["10250", "102501"]);
  assert.ok(body.payload[0].score > body.payload[1].score);
});

test("GET /v1/lookup postal branch with no exact or prefix match returns an empty list, not a 404", async () => {
  const res = await app.request("/v1/lookup?q=99999");
  assert.equal(res.status, 200);
  const body = await readJson(res);
  assert.deepEqual(body.payload, []);
});

// --- classifier: admin p-code -------------------------------------------------------

test("GET /v1/lookup?q=LK... classifies as admin", async () => {
  const res = await app.request("/v1/lookup?q=LK1101");
  assert.equal(res.status, 200);
  const body = await readJson(res);
  assert.equal(body.payload.length, 1);
  assert.equal(body.payload[0].type, "admin");
  assert.equal(body.payload[0].payload.pcode, "LK1101");
  assert.equal(body.payload[0].payload.name, "Colombo DS Division");
});

test("GET /v1/lookup admin p-code match is case-insensitive", async () => {
  const res = await app.request("/v1/lookup?q=lk1101");
  const body = await readJson(res);
  assert.equal(body.payload[0].payload.pcode, "LK1101");
});

test("GET /v1/lookup admin branch with an unknown pcode returns an empty list, not a 404", async () => {
  const res = await app.request("/v1/lookup?q=LK999999");
  assert.equal(res.status, 200);
  const body = await readJson(res);
  assert.deepEqual(body.payload, []);
});

// --- classifier: blended free text --------------------------------------------------

test("GET /v1/lookup free text blends places and postal name matches", async () => {
  const res = await app.request("/v1/lookup?q=colo");
  assert.equal(res.status, 200);
  const body = await readJson(res);
  const types = new Set(body.payload.map((r: { type: string }) => r.type));
  assert.ok(types.has("place"), "expected at least one place result");
  assert.ok(types.has("postal"), "expected at least one postal result (Colombo Fort postal code)");
});

test("GET /v1/lookup free text results are sorted by score descending", async () => {
  const res = await app.request("/v1/lookup?q=colo");
  const body = await readJson(res);
  const scores = body.payload.map((r: { score: number }) => r.score);
  const sorted = [...scores].sort((a: number, b: number) => b - a);
  assert.deepEqual(scores, sorted);
});

test("GET /v1/lookup free text finds Nugegoda place and Nugegoda postal code together", async () => {
  const res = await app.request("/v1/lookup?q=nuge");
  const body = await readJson(res);
  const place = body.payload.find((r: { type: string; payload: { name: string } }) => r.type === "place" && r.payload.name === "Nugegoda");
  const postal = body.payload.find((r: { type: string; payload: { code: string } }) => r.type === "postal" && r.payload.code === "10250");
  assert.ok(place, "expected the Nugegoda place");
  assert.ok(postal, "expected the Nugegoda postal code");
});

test("GET /v1/lookup free text with no matches returns an empty list", async () => {
  const res = await app.request("/v1/lookup?q=zzzznomatch");
  assert.equal(res.status, 200);
  const body = await readJson(res);
  assert.deepEqual(body.payload, []);
});

// --- suggest mode --------------------------------------------------------------------

test("GET /v1/lookup?suggest=1 returns lightweight rows capped at 10", async () => {
  const res = await app.request("/v1/lookup?q=colo&suggest=1");
  assert.equal(res.status, 200);
  const body = await readJson(res);
  assert.ok(body.payload.length <= 10);
  for (const row of body.payload) {
    assert.ok("type" in row && "id" in row && "label" in row && "sublabel" in row && "lat" in row && "lon" in row);
  }
});

test("suggest mode for a place resolves its admin parent as sublabel via cell_lookup", async () => {
  // "Colombo Fort" (place #3) sits exactly at FORT_LAT/FORT_LON, which cell_lookup
  // maps to LK110101 -> district LK11 "Colombo District". Plain "Colombo" (place #1)
  // is a few hundred meters off and lands in an uncovered cell, so this
  // deliberately checks the suburb, not the city.
  const res = await app.request(`/v1/lookup?q=${encodeURIComponent("colombo fort")}&suggest=1`);
  const body = await readJson(res);
  const fort = body.payload.find((r: { type: string; label: string }) => r.type === "place" && r.label === "Colombo Fort");
  assert.ok(fort, "expected the Colombo Fort place");
  assert.equal(fort.sublabel, "Colombo District");
  assert.equal(fort.pcode, "LK110101");
});

test("suggest mode for a place falls back to kind when cell_lookup has no row for its coords", async () => {
  // Nugegoda's coords are deliberately uncovered by cell_lookup (see fixture.ts).
  const res = await app.request("/v1/lookup?q=nugegoda&suggest=1");
  const body = await readJson(res);
  const nugegoda = body.payload.find((r: { type: string; label: string }) => r.type === "place" && r.label === "Nugegoda");
  assert.ok(nugegoda);
  assert.equal(nugegoda.sublabel, "Town");
  assert.equal(nugegoda.pcode, undefined);
});

test("suggest mode for postal uses the code as label and the place name as sublabel", async () => {
  const res = await app.request("/v1/lookup?q=00100&suggest=1");
  const body = await readJson(res);
  assert.equal(body.payload[0].type, "postal");
  assert.equal(body.payload[0].label, "00100");
  assert.equal(body.payload[0].sublabel, "Colombo Fort");
});

test("suggest mode for admin uses the unit name as label and level + parent as sublabel", async () => {
  const res = await app.request("/v1/lookup?q=LK1101&suggest=1");
  const body = await readJson(res);
  assert.equal(body.payload[0].type, "admin");
  assert.equal(body.payload[0].label, "Colombo DS Division");
  assert.equal(body.payload[0].sublabel, "DS Division · Colombo District");
  assert.equal(body.payload[0].pcode, "LK1101");
});

test("the root country pcode \"LK\" has no digit suffix, so it never matches the admin classifier (/^LK\\d+$/i) and falls through to blended text search instead", async () => {
  const res = await app.request("/v1/lookup?q=LK&suggest=1");
  assert.equal(res.status, 200);
  const body = await readJson(res);
  assert.ok(body.payload.every((r: { type: string }) => r.type !== "admin"));
});

test("suggest mode for coordinates returns a single row labeled by the resolved GN division", async () => {
  const res = await app.request(`/v1/lookup?q=${FORT_LAT},${FORT_LON}&suggest=1`);
  const body = await readJson(res);
  assert.equal(body.payload.length, 1);
  assert.equal(body.payload[0].type, "coordinate");
  assert.equal(body.payload[0].label, "Fort");
  assert.equal(body.payload[0].sublabel, "Colombo District");
  assert.equal(body.payload[0].pcode, "LK110101");
});

// --- general validation ----------------------------------------------------------------

test("GET /v1/lookup requires a non-empty q", async () => {
  const res = await app.request("/v1/lookup?q=");
  assert.equal(res.status, 400);
});

test("GET /v1/lookup carries the standard envelope", async () => {
  const res = await app.request("/v1/lookup?q=colombo");
  const body = await readJson(res);
  assert.ok("success" in body && "message" in body && "payload" in body && "meta" in body);
  assert.ok("data_version" in body.meta);
});
