import { test } from "node:test";
import assert from "node:assert/strict";
import { buildApp } from "../src/app.ts";
import { buildFixtureDb } from "./fixture.ts";
import { readJson } from "./helpers.ts";

const db = buildFixtureDb();
const app = buildApp(db);

test("GET /v1/elections lists the catalog", async () => {
  const res = await app.request("/v1/elections");
  assert.equal(res.status, 200);
  const body = await readJson(res);
  assert.equal(body.payload.length, 1);
  assert.equal(body.payload[0].id, "pres-2024");
  assert.equal(body.payload[0].year, 2024);
});

test("GET /v1/elections/:id/results/:entity returns parsed results with party metadata joined", async () => {
  const res = await app.request("/v1/elections/pres-2024/results/EC-01");
  assert.equal(res.status, 200);
  const body = await readJson(res);
  assert.equal(body.payload.election.id, "pres-2024");
  assert.equal(body.payload.entity.id, "EC-01");
  assert.equal(body.payload.entity.kind, "ED");
  assert.equal(body.payload.winner_party, "NPP");
  assert.equal(body.payload.parties.length, 3);
  assert.equal(body.payload.parties[0].code, "NPP"); // highest votes first
  assert.equal(body.payload.parties[0].candidate, "A. K. Dissanayake");
  assert.equal(body.payload.parties[0].votes, 500_000);
});

test("national entity results are also served", async () => {
  const res = await app.request("/v1/elections/pres-2024/results/LK");
  const body = await readJson(res);
  assert.equal(body.payload.entity.kind, "NATIONAL");
  assert.equal(body.payload.winner_votes, 5_700_000);
});

test("unknown election id is a 404", async () => {
  const res = await app.request("/v1/elections/does-not-exist/results/EC-01");
  assert.equal(res.status, 404);
});

test("unknown entity id is a 404", async () => {
  const res = await app.request("/v1/elections/pres-2024/results/DOES-NOT-EXIST");
  assert.equal(res.status, 404);
});
