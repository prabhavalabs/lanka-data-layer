import { test } from "node:test";
import assert from "node:assert/strict";
import {
  getPostalCode,
  searchOffices,
  codesForDivision,
  divisionsForCode,
  nearestPostalCode,
  DATA_VERSION,
  CODE_COUNT,
  SOURCES,
} from "./index.ts";

test("a known code round-trips to its office and admin division", () => {
  const office = getPostalCode("61162");
  assert.ok(office);
  assert.equal(office!.name, "Pothuwatawana");
  assert.equal(office!.adminPcode, "LK62");
  assert.ok(office!.location);
  assert.equal(office!.location!.lat, 7.358483);
  assert.equal(office!.location!.lon, 79.922411);
});

test("getPostalCode returns null for an unknown code", () => {
  assert.equal(getPostalCode("00000"), null);
  assert.equal(getPostalCode("not-a-code"), null);
});

test("codesForDivision returns shares sorted descending", () => {
  const codes = codesForDivision("LK1103");
  assert.ok(codes.length > 1);
  for (let i = 1; i < codes.length; i++) {
    assert.ok(codes[i - 1]!.share >= codes[i]!.share);
  }
  assert.equal(codes[0]!.code, "01500"); // highest share for this division
});

test("codesForDivision is case-insensitive and empty for a division with no mapped codes", () => {
  const upper = codesForDivision("LK1103");
  const lower = codesForDivision("lk1103");
  assert.deepEqual(lower, upper);

  // LK62 is a district-level pcode; division_codes only covers DS/GN levels.
  assert.deepEqual(codesForDivision("LK62"), []);
  assert.deepEqual(codesForDivision("ZZZZZZ"), []);
});

test("divisionsForCode is the reverse index of codesForDivision", () => {
  const divisions = divisionsForCode("01500");
  assert.ok(divisions.length > 0);
  assert.ok(divisions.some((d) => d.pcode === "LK1103"));

  // Sorted descending by share.
  for (let i = 1; i < divisions.length; i++) {
    assert.ok(divisions[i - 1]!.share >= divisions[i]!.share);
  }

  // Consistency: every division listed must actually carry this code, at
  // the same share, in codesForDivision.
  for (const { pcode, share } of divisions) {
    const forward = codesForDivision(pcode).find((c) => c.code === "01500");
    assert.ok(forward, `expected ${pcode} to list 01500`);
    assert.equal(forward!.share, share);
  }
});

test("divisionsForCode is empty for a code with no division mapping", () => {
  assert.deepEqual(divisionsForCode("00000"), []);
});

test("searchOffices ranks an exact name match first", () => {
  const results = searchOffices("galle");
  assert.ok(results.length > 1);
  assert.equal(results[0]!.name, "Galle");
  assert.ok(results.some((r) => r.name === "Gallewa" || r.name === "Gallella"));
});

test("searchOffices returns an empty array for a blank query", () => {
  assert.deepEqual(searchOffices(""), []);
});

test("searchOffices respects the limit option", () => {
  const capped = searchOffices("a", { limit: 3 });
  assert.equal(capped.length, 3);
});

test("nearestPostalCode(Colombo Fort area) returns a nearby Colombo code by default", () => {
  const [nearest] = nearestPostalCode(6.9271, 79.8612);
  assert.ok(nearest);
  assert.equal(nearest!.adminPcode, "LK11");
  assert.ok(nearest!.distanceKm < 5, `expected a small distance, got ${nearest!.distanceKm}km`);
});

test("nearestPostalCode respects limit and returns results sorted by distance", () => {
  const results = nearestPostalCode(6.9271, 79.8612, { limit: 5 });
  assert.equal(results.length, 5);
  for (let i = 1; i < results.length; i++) {
    assert.ok(results[i - 1]!.distanceKm <= results[i]!.distanceKm);
  }
  // distanceKm is rounded to 2dp
  for (const r of results) {
    assert.equal(r.distanceKm, Math.round(r.distanceKm * 100) / 100);
  }
});

test("nearestPostalCode only returns codes with a known location", () => {
  // None of the bundled rows currently have a null location, but the
  // implementation must still skip them defensively if that ever changes.
  const results = nearestPostalCode(6.9271, 79.8612, { limit: CODE_COUNT });
  assert.ok(results.every((r) => r.location !== null));
});

test("exposes DATA_VERSION, CODE_COUNT and SOURCES", () => {
  assert.equal(typeof DATA_VERSION, "string");
  assert.ok(DATA_VERSION.length > 0);
  assert.equal(CODE_COUNT, 1833);
  assert.ok(SOURCES.length > 0);
  for (const s of SOURCES) {
    assert.equal(typeof s.name, "string");
    assert.equal(typeof s.url, "string");
    assert.equal(typeof s.license, "string");
  }
});
