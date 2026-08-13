import { test } from "node:test";
import assert from "node:assert/strict";
import { CENSUS_YEAR, DATA_VERSION, SOURCES, coveredPcodes, population, ethnicity, religion, demographics } from "./index.ts";

test("CENSUS_YEAR, DATA_VERSION, SOURCES are populated", () => {
  assert.equal(CENSUS_YEAR, 2024);
  assert.match(DATA_VERSION, /^\d{8}\.\d+$/);
  assert.ok(SOURCES.length > 0);
  assert.ok(SOURCES.every((s) => s.name && s.url));
});

test("coveredPcodes has 365 entries: country + 25 districts + 339 DS divisions", () => {
  const pcodes = coveredPcodes();
  assert.equal(pcodes.length, 365);
  assert.ok(pcodes.includes("LK"));
  assert.ok(pcodes.includes("LK11"));
});

test("population LK matches the published 2024 census total", () => {
  const lk = population("LK");
  assert.ok(lk);
  assert.equal(lk?.total, 21781800);
  assert.equal(lk?.male, 10512344);
  assert.equal(lk?.female, 11269456);
  assert.equal(lk?.male! + lk?.female!, lk?.total);
  const ageSum = Object.values(lk!.ages).reduce((s, v) => s + v, 0);
  assert.equal(ageSum, lk?.total);
});

test("population returns null for an unknown pcode", () => {
  assert.equal(population("ZZ999999"), null);
});

test("ethnicity / religion: desc by count, shares sum to ~1, null when uncovered", () => {
  const e = ethnicity("LK11");
  assert.ok(e);
  for (let i = 1; i < e!.length; i++) {
    assert.ok(e![i - 1]!.count >= e![i]!.count);
  }
  const eShareSum = e!.reduce((s, x) => s + x.share, 0);
  assert.ok(Math.abs(eShareSum - 1) < 0.01);

  const r = religion("LK11");
  assert.ok(r);
  const rShareSum = r!.reduce((s, x) => s + x.share, 0);
  assert.ok(Math.abs(rShareSum - 1) < 0.01);

  assert.equal(ethnicity("no-such-pcode"), null);
  assert.equal(religion("no-such-pcode"), null);
});

test("demographics LK11: sexRatio 94.6 and dependencyRatio 43.0 — hand-verified against the hosted API", () => {
  const d = demographics("LK11");
  assert.ok(d);
  assert.equal(d?.pcode, "LK11");
  assert.equal(d?.censusYear, 2024);
  assert.equal(d?.population.sexRatio, 94.6);
  assert.equal(d?.age.dependencyRatio, 43);
});

test("demographics: age buckets and population identity", () => {
  const d = demographics("LK11")!;
  assert.equal(d.age.buckets.length, 4);
  assert.deepEqual(
    d.age.buckets.map((b) => b.bucket),
    ["0-14", "15-59", "60-64", "65+"],
  );
  const bucketSum = d.age.buckets.reduce((s, b) => s + b.count, 0);
  assert.equal(bucketSum, d.population.total);

  const shareSum = d.age.buckets.reduce((s, b) => s + b.share, 0);
  assert.ok(Math.abs(shareSum - 1) < 0.01);

  assert.ok(d.population.femaleShare > 0 && d.population.femaleShare < 1);
  assert.ok(d.age.workingAgeShare > 0 && d.age.workingAgeShare < 1);
  assert.ok(d.age.childShare > 0 && d.age.childShare < 1);
  assert.ok(d.age.elderlyShare > 0 && d.age.elderlyShare < 1);
});

test("demographics change2012: non-null for a district (LK11), null for a DS division", () => {
  const district = demographics("LK11")!;
  assert.ok(district.change2012);
  assert.ok(district.change2012!.ethnicity.length > 0);
  assert.ok(district.change2012!.religion.length > 0);
  // sorted desc by 2024 count — the top entry should match the top ethnicity()/religion() key
  assert.equal(district.change2012!.ethnicity[0]!.key, district.ethnicity[0]!.key);
  assert.equal(district.change2012!.religion[0]!.key, district.religion[0]!.key);

  // every key present in change2012 must also be present in both the 2012 and 2024 arrays
  for (const entry of district.change2012!.ethnicity) {
    assert.ok(typeof entry.share2012 === "number");
    assert.ok(typeof entry.share2024 === "number");
    assert.ok(Math.abs(entry.delta - (entry.share2024 - entry.share2012)) < 1e-9);
  }

  const dsDivision = coveredPcodes().find((p) => p.length === 6);
  assert.ok(dsDivision);
  const dsDemo = demographics(dsDivision!)!;
  assert.ok(dsDemo);
  assert.equal(dsDemo.change2012, null);
});

test("demographics returns null for an unknown pcode", () => {
  assert.equal(demographics("no-such-pcode"), null);
});
