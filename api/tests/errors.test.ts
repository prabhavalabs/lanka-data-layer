import { test } from "node:test";
import assert from "node:assert/strict";
import { buildApp } from "../src/app.ts";
import { buildFixtureDb, DATA_VERSION } from "./fixture.ts";
import { readJson } from "./helpers.ts";

const db = buildFixtureDb();
const app = buildApp(db);

test("unknown route returns the standard envelope with success:false and a 404", async () => {
  const res = await app.request("/v1/this-route-does-not-exist");
  assert.equal(res.status, 404);
  const body = await readJson(res);
  assert.equal(body.success, false);
  assert.equal(body.payload, null);
  assert.equal(body.meta.data_version, DATA_VERSION);
});

test("every response uses the {success, message, payload, meta} envelope", async () => {
  const res = await app.request("/v1/datasets");
  const body = await readJson(res);
  assert.ok("success" in body && "message" in body && "payload" in body && "meta" in body);
  assert.ok("data_version" in body.meta && "source" in body.meta);
});
