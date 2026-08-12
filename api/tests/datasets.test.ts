import { test } from "node:test";
import assert from "node:assert/strict";
import { buildApp } from "../src/app.ts";
import { buildFixtureDb } from "./fixture.ts";
import { readJson } from "./helpers.ts";

const db = buildFixtureDb();
const app = buildApp(db);

test("GET /v1/datasets lists the catalog with source attribution", async () => {
  const res = await app.request("/v1/datasets");
  assert.equal(res.status, 200);
  const body = await readJson(res);
  assert.equal(body.success, true);
  assert.equal(body.payload.length, 5);
  const ids = body.payload.map((d: { id: string }) => d.id).sort();
  assert.deepEqual(ids, ["admin-units", "cells", "elections", "places", "postal-codes"]);
  assert.equal(body.meta.source.length, 5);
  assert.ok(body.meta.source.every((s: { name: string; url: string; license: string }) => s.name && s.url && s.license));
});
