import { test } from "node:test";
import assert from "node:assert/strict";
import { buildApp } from "../src/app.ts";
import { buildFixtureDb } from "./fixture.ts";
import { readJson } from "./helpers.ts";

const db = buildFixtureDb();
const app = buildApp(db);

test("GET /v1/search?q=colombo finds Colombo by exact match", async () => {
  const res = await app.request("/v1/search?q=colombo");
  assert.equal(res.status, 200);
  const body = await readJson(res);
  assert.ok(body.payload.length >= 1);
  assert.equal(body.payload[0].name, "Colombo");
  assert.equal(body.payload[0].kind, "city");
});

test("GET /v1/search?q=colo prefix-matches Colombo", async () => {
  const res = await app.request("/v1/search?q=colo");
  const body = await readJson(res);
  assert.ok(body.payload.some((p: { name: string }) => p.name === "Colombo"));
});

test("GET /v1/search?q=nu finds Nugegoda by prefix", async () => {
  const res = await app.request("/v1/search?q=nu");
  const body = await readJson(res);
  assert.ok(body.payload.some((p: { name: string }) => p.name === "Nugegoda"));
});

test("q shorter than 2 chars is a 400", async () => {
  const res = await app.request("/v1/search?q=a");
  assert.equal(res.status, 400);
});

test("limit above 50 is a 400", async () => {
  const res = await app.request("/v1/search?q=colombo&limit=51");
  assert.equal(res.status, 400);
});

test("min_population filters out smaller places", async () => {
  // "colo" prefix-matches both Colombo (pop 752,993) and Colombo Fort (pop 15,000).
  const res = await app.request("/v1/search?q=colo&min_population=100000");
  const body = await readJson(res);
  assert.ok(body.payload.every((p: { population: number }) => p.population >= 100000));
  assert.ok(body.payload.some((p: { name: string }) => p.name === "Colombo"));
  assert.ok(!body.payload.some((p: { name: string }) => p.name === "Colombo Fort"));
});

test("lang=si resolves the Sinhala name", async () => {
  const res = await app.request("/v1/search?q=colombo&lang=si");
  const body = await readJson(res);
  assert.equal(body.payload[0].name, "කොළඹ");
});

test("results are sorted by score descending, population boost breaking prefix-match ties", async () => {
  // Both places prefix-match "colo" (MATCH_PREFIX tie) — the much larger population
  // of Colombo city should push it ahead of the smaller Colombo Fort suburb.
  const res = await app.request("/v1/search?q=colo");
  const body = await readJson(res);
  assert.ok(body.payload.length >= 2);
  const scores = body.payload.map((p: { score: number }) => p.score);
  const sorted = [...scores].sort((a: number, b: number) => b - a);
  assert.deepEqual(scores, sorted);
  assert.equal(body.payload[0].name, "Colombo");
});
