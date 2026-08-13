import { test } from "node:test";
import assert from "node:assert/strict";
import {
  getUnit,
  getChildren,
  getParentChain,
  unitsAtLevel,
  searchByName,
  DATA_VERSION,
  UNIT_COUNT,
  SOURCES,
} from "./index.ts";

test("LK is the country root at level 0", () => {
  const lk = getUnit("LK");
  assert.ok(lk);
  assert.equal(lk!.level, 0);
  assert.equal(lk!.name, "Sri Lanka");
  assert.equal(lk!.parentPcode, null);
});

test("LK11 is Colombo District at level 2 with parent chain [LK, LK1]", () => {
  const unit = getUnit("LK11");
  assert.ok(unit);
  assert.equal(unit!.level, 2);
  assert.equal(unit!.name, "Colombo District");
  assert.equal(unit!.parentPcode, "LK1");

  const chain = getParentChain("LK11");
  assert.deepEqual(
    chain.map((u) => u.pcode),
    ["LK", "LK1"],
  );
});

test("getUnit is case-insensitive on pcode", () => {
  const upper = getUnit("LK11");
  const lower = getUnit("lk11");
  const mixed = getUnit("Lk11");
  assert.ok(upper);
  assert.deepEqual(lower, upper);
  assert.deepEqual(mixed, upper);
});

test("getUnit returns null for an unknown pcode", () => {
  assert.equal(getUnit("LK999999"), null);
});

test("getParentChain excludes the unit itself and is root-first", () => {
  const chain = getParentChain("LK1103005"); // a GN division
  assert.deepEqual(
    chain.map((u) => u.pcode),
    ["LK", "LK1", "LK11", "LK1103"],
  );
});

test("getParentChain is empty for the country root and for unknown pcodes", () => {
  assert.deepEqual(getParentChain("LK"), []);
  assert.deepEqual(getParentChain("ZZZZZZ"), []);
});

test("getChildren returns direct children only", () => {
  const provinces = getChildren("LK");
  assert.equal(provinces.length, 9);

  const districts = getChildren("LK1");
  assert.deepEqual(
    districts.map((u) => u.pcode).sort(),
    ["LK11", "LK12", "LK13"],
  );

  assert.equal(getChildren("LK9999999").length, 0);
});

test("unitsAtLevel returns every unit at that level", () => {
  assert.equal(unitsAtLevel(0).length, 1);
  assert.equal(unitsAtLevel(1).length, 9);
  assert.ok(unitsAtLevel(4).length > 10000);
  assert.deepEqual(unitsAtLevel(99), []);
});

test("searchByName('colombo') finds LK11 and ranks the exact match first", () => {
  const results = searchByName("colombo");
  assert.ok(results.some((u) => u.pcode === "LK11"));
  assert.equal(results[0]!.pcode, "LK1103"); // exact name "Colombo" outranks "Colombo District"
});

test("searchByName is case-insensitive and respects the level filter and limit", () => {
  const lower = searchByName("colombo");
  const upper = searchByName("COLOMBO");
  assert.deepEqual(
    upper.map((u) => u.pcode),
    lower.map((u) => u.pcode),
  );

  const districtsOnly = searchByName("colombo", { level: 2 });
  assert.ok(districtsOnly.every((u) => u.level === 2));

  const capped = searchByName("a", { limit: 3 });
  assert.equal(capped.length, 3);
});

test("searchByName returns an empty array for a blank query", () => {
  assert.deepEqual(searchByName("   "), []);
});

test("exposes DATA_VERSION, UNIT_COUNT and SOURCES", () => {
  assert.equal(typeof DATA_VERSION, "string");
  assert.ok(DATA_VERSION.length > 0);
  assert.equal(UNIT_COUNT, 14417);
  assert.ok(SOURCES.length > 0);
  for (const s of SOURCES) {
    assert.equal(typeof s.name, "string");
    assert.equal(typeof s.url, "string");
    assert.equal(typeof s.license, "string");
  }
});
