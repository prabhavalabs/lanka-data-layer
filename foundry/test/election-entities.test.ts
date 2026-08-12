import { test } from "node:test";
import assert from "node:assert/strict";
import { classifyEntityKind, edIdOf } from "../src/lib/election-entities.ts";

test("classifies national entity", () => {
  assert.equal(classifyEntityKind("LK"), "NATIONAL");
});

test("classifies electoral districts", () => {
  assert.equal(classifyEntityKind("EC-01"), "ED");
  assert.equal(classifyEntityKind("EC-22"), "ED");
});

test("classifies postal entities before polling divisions (P is also a letter suffix)", () => {
  assert.equal(classifyEntityKind("EC-01P"), "POSTAL");
  assert.equal(classifyEntityKind("EC-22P"), "POSTAL");
});

test("classifies polling divisions", () => {
  assert.equal(classifyEntityKind("EC-01A"), "PD");
  assert.equal(classifyEntityKind("EC-21A"), "PD");
  assert.equal(classifyEntityKind("EC-01O"), "PD");
});

test("throws on unrecognized entity id shapes", () => {
  assert.throws(() => classifyEntityKind("XX-01"));
  assert.throws(() => classifyEntityKind(""));
  assert.throws(() => classifyEntityKind("EC-01AA"));
});

test("edIdOf derives the parent ED for PD and POSTAL, self for ED, null for NATIONAL", () => {
  assert.equal(edIdOf("LK", "NATIONAL"), null);
  assert.equal(edIdOf("EC-04", "ED"), "EC-04");
  assert.equal(edIdOf("EC-04B", "PD"), "EC-04");
  assert.equal(edIdOf("EC-04P", "POSTAL"), "EC-04");
});
