import { test } from "node:test";
import assert from "node:assert/strict";
import { buildApp } from "../src/app.ts";
import { buildFixtureDb, DATA_VERSION } from "./fixture.ts";

const db = buildFixtureDb();
const app = buildApp(db);

test("GET /v1/datasets sets ETag and Cache-Control per contract §4", async () => {
  const res = await app.request("/v1/datasets");
  assert.equal(res.status, 200);
  assert.equal(res.headers.get("Cache-Control"), "public, max-age=300, stale-while-revalidate=86400");
  const etag = res.headers.get("ETag");
  assert.ok(etag, "expected an ETag header");
  assert.ok(etag!.startsWith(`"${DATA_VERSION}-`), `expected etag to start with data_version, got ${etag}`);
});

test("If-None-Match with the current ETag returns 304 with an empty body", async () => {
  const first = await app.request("/v1/datasets");
  const etag = first.headers.get("ETag")!;

  const second = await app.request("/v1/datasets", { headers: { "If-None-Match": etag } });
  assert.equal(second.status, 304);
  const text = await second.text();
  assert.equal(text, "");
});

test("a stale If-None-Match still returns 200 with fresh content", async () => {
  const res = await app.request("/v1/datasets", { headers: { "If-None-Match": '"stale-etag"' } });
  assert.equal(res.status, 200);
});

test("ETag is stable across query-param order for the same logical request", async () => {
  const a = await app.request("/v1/search?q=colombo&limit=5");
  const b = await app.request("/v1/search?limit=5&q=colombo");
  assert.equal(a.headers.get("ETag"), b.headers.get("ETag"));
});

test("different query params produce different ETags", async () => {
  const a = await app.request("/v1/search?q=colombo");
  const b = await app.request("/v1/search?q=galle");
  assert.notEqual(a.headers.get("ETag"), b.headers.get("ETag"));
});
