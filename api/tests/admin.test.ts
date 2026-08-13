import { test } from "node:test";
import assert from "node:assert/strict";
import { buildApp } from "../src/app.ts";
import { buildFixtureDb } from "./fixture.ts";
import { readJson } from "./helpers.ts";

const db = buildFixtureDb();
const app = buildApp(db);

test("GET /v1/admin/:pcode returns the unit, parent chain (root first) and children summary", async () => {
  const res = await app.request("/v1/admin/LK1101");
  assert.equal(res.status, 200);
  const body = await readJson(res);
  assert.equal(body.success, true);
  assert.equal(body.payload.unit.pcode, "LK1101");
  assert.equal(body.payload.unit.name, "Colombo DS Division");

  assert.deepEqual(
    body.payload.parent_chain.map((u: { pcode: string }) => u.pcode),
    ["LK", "LK1", "LK11"],
  );

  const childPcodes = body.payload.children.map((c: { pcode: string }) => c.pcode).sort();
  assert.deepEqual(childPcodes, ["LK110101", "LK110102"]);

  assert.equal(body.payload.population, undefined);
  assert.equal(body.payload.stats, undefined);
});

test("root unit LK has an empty parent chain", async () => {
  const res = await app.request("/v1/admin/LK");
  const body = await readJson(res);
  assert.deepEqual(body.payload.parent_chain, []);
});

test("unknown pcode returns 404 not_found", async () => {
  const res = await app.request("/v1/admin/LK999999");
  assert.equal(res.status, 404);
  const body = await readJson(res);
  assert.equal(body.success, false);
  assert.equal(body.payload, null);
});

test("?include=population returns the 17-bucket age x sex breakdown for the latest year", async () => {
  const res = await app.request("/v1/admin/LK1101?include=population");
  const body = await readJson(res);
  assert.equal(body.payload.population.year, 2023);
  assert.equal(Object.keys(body.payload.population.buckets).length, 17);
  assert.deepEqual(body.payload.population.buckets["0-4"], { f: 100, m: 100, t: 200 });
  assert.deepEqual(body.payload.population.buckets["80+"], { f: 1700, m: 1700, t: 3400 });
  assert.ok(body.payload.population.total.t > 0);
  assert.equal(body.payload.population.total.t, body.payload.population.total.f + body.payload.population.total.m);
});

test("?include=stats returns the latest-year stats map", async () => {
  const res = await app.request("/v1/admin/LK1101?include=stats");
  const body = await readJson(res);
  assert.equal(body.payload.stats.year, 2023);
  assert.equal(body.payload.stats.values["ethnicity.sinhala"], 65.5);
});

test("?include=population,stats returns both, and an unbuilt pcode gets nulls", async () => {
  // LK1102 (Kolonnawa DS Division) deliberately carries no admin_population/admin_stats
  // rows in the fixture at all — LK11 itself now has 2024 census rows (see fixture.ts's
  // demographics block), so it no longer demonstrates the "unbuilt pcode" case here.
  const res = await app.request("/v1/admin/LK1102?include=population,stats");
  const body = await readJson(res);
  assert.equal(body.payload.population, null);
  assert.equal(body.payload.stats, null);
});

test("?include with an invalid value is a 400 validation error", async () => {
  const res = await app.request("/v1/admin/LK1101?include=bogus");
  assert.equal(res.status, 400);
  const body = await readJson(res);
  assert.equal(body.success, false);
});

test("lang=si resolves Sinhala names where present, falling back to English otherwise", async () => {
  const res = await app.request("/v1/admin/LK1101?lang=si");
  const body = await readJson(res);
  assert.equal(body.payload.unit.name, "Colombo DS Division"); // no name_si on this row -> falls back to en

  const country = await app.request("/v1/admin/LK?lang=si");
  const countryBody = await readJson(country);
  assert.equal(countryBody.payload.unit.name, "ශ්‍රී ලංකාව");
});

test("GET /v1/admin/:pcode includes rolled-up postal codes for GN/DS divisions", async () => {
  const res = await app.request("/v1/admin/LK110101");
  const body = await readJson(res);
  assert.deepEqual(
    body.payload.postal_codes.map((p: { code: string }) => p.code),
    ["00100", "00300"],
  );
  assert.ok(body.payload.postal_codes[0].share > body.payload.postal_codes[1].share);
  assert.ok(body.meta.source.length >= 2, "postal attribution joins the meta sources");
});

test("GET /v1/admin/:pcode omits postal_codes where no rollup exists", async () => {
  const res = await app.request("/v1/admin/LK1");
  const body = await readJson(res);
  assert.equal(body.payload.postal_codes, undefined);
});
