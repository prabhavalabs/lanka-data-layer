import { test } from "node:test";
import assert from "node:assert/strict";
import { buildApp } from "../src/app.ts";
import { buildFixtureDb } from "./fixture.ts";
import { readJson } from "./helpers.ts";

const db = buildFixtureDb();
const app = buildApp(db);

test("GET /v1/demographics/:pcode — district happy path, every derived number hand-verified", async () => {
  const res = await app.request("/v1/demographics/LK11");
  assert.equal(res.status, 200);
  const body = await readJson(res);
  assert.equal(body.success, true);

  assert.equal(body.payload.unit.pcode, "LK11");
  assert.equal(body.payload.unit.name, "Colombo District");
  assert.equal(body.payload.census_year, 2024);

  // population: 1000 = 480 male + 520 female
  assert.deepEqual(body.payload.population, {
    total: 1000,
    male: 480,
    female: 520,
    sex_ratio: 92.3, // 480/520*100
    female_share: 0.52, // 520/1000
  });

  // age: 200/600/50/150 (0-14 / 15-59 / 60-64 / 65+), sums to 1000
  assert.deepEqual(body.payload.age.buckets, [
    { bucket: "0-14", count: 200, share: 0.2 },
    { bucket: "15-59", count: 600, share: 0.6 },
    { bucket: "60-64", count: 50, share: 0.05 },
    { bucket: "65+", count: 150, share: 0.15 },
  ]);
  assert.equal(body.payload.age.dependency_ratio, 53.8); // (200+150)/(600+50)*100
  assert.equal(body.payload.age.working_age_share, 0.65); // (600+50)/1000
  assert.equal(body.payload.age.child_share, 0.2);
  assert.equal(body.payload.age.elderly_share, 0.15);

  // ethnicity: desc by count, share = count / ethnicity.total (1000)
  assert.deepEqual(body.payload.ethnicity, [
    { key: "sinhala", count: 676, share: 0.676 },
    { key: "sriLankanTamil", count: 190, share: 0.19 },
    { key: "moor", count: 71, share: 0.071 },
    { key: "indianTamil", count: 36, share: 0.036 },
    { key: "burgher", count: 12, share: 0.012 },
    { key: "malay", count: 6, share: 0.006 },
    { key: "sriLankaChetty", count: 4, share: 0.004 },
    { key: "bharatha", count: 3, share: 0.003 },
    { key: "veddha", count: 2, share: 0.002 },
    { key: "other", count: 0, share: 0 },
  ]);

  // religion: desc by count, share = count / religion.total (1000)
  assert.deepEqual(body.payload.religion, [
    { key: "buddhist", count: 619, share: 0.619 },
    { key: "hindu", count: 170, share: 0.17 },
    { key: "muslim", count: 140, share: 0.14 },
    { key: "romanCatholic", count: 46, share: 0.046 },
    { key: "otherChristian", count: 20, share: 0.02 },
    { key: "other", count: 5, share: 0.005 },
  ]);

  // change_2012: only the 7 ethnicity keys present in both years (2012 lacks
  // sriLankaChetty/bharatha/veddha); all 6 religion keys are present in both.
  assert.deepEqual(body.payload.change_2012.ethnicity, [
    { key: "sinhala", share_2012: 0.65, share_2024: 0.676, delta: 0.026 },
    { key: "sriLankanTamil", share_2012: 0.2, share_2024: 0.19, delta: -0.01 },
    { key: "moor", share_2012: 0.09, share_2024: 0.071, delta: -0.019 },
    { key: "indianTamil", share_2012: 0.04, share_2024: 0.036, delta: -0.004 },
    { key: "burgher", share_2012: 0.01, share_2024: 0.012, delta: 0.002 },
    { key: "malay", share_2012: 0.005, share_2024: 0.006, delta: 0.001 },
    { key: "other", share_2012: 0.005, share_2024: 0, delta: -0.005 },
  ]);
  assert.deepEqual(body.payload.change_2012.religion, [
    { key: "buddhist", share_2012: 0.6, share_2024: 0.619, delta: 0.019 },
    { key: "hindu", share_2012: 0.18, share_2024: 0.17, delta: -0.01 },
    { key: "muslim", share_2012: 0.15, share_2024: 0.14, delta: -0.01 },
    { key: "romanCatholic", share_2012: 0.05, share_2024: 0.046, delta: -0.004 },
    { key: "otherChristian", share_2012: 0.015, share_2024: 0.02, delta: 0.005 },
    { key: "other", share_2012: 0.005, share_2024: 0.005, delta: 0 },
  ]);

  // parent_share: LK11's literal admin parent is LK1 (a province), which has
  // zero 2024 rows, so the endpoint walks past it to the country LK instead.
  assert.deepEqual(body.payload.parent_share, {
    parent_pcode: "LK",
    parent_name: "Sri Lanka",
    share: 0.05, // 1000/20000
  });

  // meta: only the two datasets this response actually drew from.
  assert.equal(body.meta.source.length, 2);
  const sourceNames = body.meta.source.map((s: { name: string }) => s.name);
  assert.ok(sourceNames.every((n: string) => n === "Department of Census and Statistics, Sri Lanka"));
});

test("GET /v1/demographics/:pcode — country: no 2012 rows and no parent, so both go null", async () => {
  const res = await app.request("/v1/demographics/LK");
  assert.equal(res.status, 200);
  const body = await readJson(res);

  assert.equal(body.payload.unit.pcode, "LK");
  assert.deepEqual(body.payload.population, {
    total: 20000,
    male: 9800,
    female: 10200,
    sex_ratio: 96.1,
    female_share: 0.51,
  });
  assert.deepEqual(body.payload.age.buckets, [
    { bucket: "0-14", count: 3600, share: 0.18 },
    { bucket: "15-59", count: 12500, share: 0.625 },
    { bucket: "60-64", count: 1200, share: 0.06 },
    { bucket: "65+", count: 2700, share: 0.135 },
  ]);
  assert.equal(body.payload.age.dependency_ratio, 46);
  assert.equal(body.payload.age.working_age_share, 0.685);
  assert.equal(body.payload.age.child_share, 0.18);
  assert.equal(body.payload.age.elderly_share, 0.135);

  assert.equal(body.payload.change_2012, null); // LK has no 2012 admin_stats rows at all
  assert.equal(body.payload.parent_share, null); // level 0 — no parent to compare against

  // change_2012 is null, so census-2012 must not appear even though the
  // fixture's datasets table does carry that id.
  assert.equal(body.meta.source.length, 1);
  assert.equal(body.meta.source[0].name, "Department of Census and Statistics, Sri Lanka");
});

test("GET /v1/demographics/:pcode — a GN division exists but carries no 2024 census rows: 404", async () => {
  const res = await app.request("/v1/demographics/LK110101");
  assert.equal(res.status, 404);
  const body = await readJson(res);
  assert.equal(body.success, false);
  assert.equal(body.payload, null);
  assert.match(body.message, /no census data for "LK110101"/);
});

test("GET /v1/demographics/:pcode — unknown pcode: 404", async () => {
  const res = await app.request("/v1/demographics/LK999999");
  assert.equal(res.status, 404);
  const body = await readJson(res);
  assert.equal(body.success, false);
  assert.equal(body.payload, null);
  assert.match(body.message, /not found/);
});

test("GET /v1/demographics/:pcode?lang=si resolves the unit and parent names in Sinhala", async () => {
  const res = await app.request("/v1/demographics/LK11?lang=si");
  const body = await readJson(res);
  assert.equal(body.payload.unit.name, "කොළඹ දිස්ත්‍රික්කය");
  assert.equal(body.payload.parent_share.parent_name, "ශ්‍රී ලංකාව");
});

test("GET /v1/demographics/:pcode?lang=xx is a 400 validation error", async () => {
  const res = await app.request("/v1/demographics/LK11?lang=xx");
  assert.equal(res.status, 400);
  const body = await readJson(res);
  assert.equal(body.success, false);
});
