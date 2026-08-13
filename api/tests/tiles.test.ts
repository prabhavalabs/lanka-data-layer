import { test, after } from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, writeFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { buildApp } from "../src/app.ts";
import { buildFixtureDb, DATA_VERSION } from "./fixture.ts";

const db = buildFixtureDb();
const app = buildApp(db);

// A small deterministic file (bytes 0..255 repeated) so range slices are easy to assert on exactly.
const FILE_SIZE = 300;
const FILE_BYTES = Buffer.from(Array.from({ length: FILE_SIZE }, (_, i) => i % 256));

const tilesDir = mkdtempSync(join(tmpdir(), "lanka-tiles-test-"));
writeFileSync(join(tilesDir, "admin.pmtiles"), FILE_BYTES);
process.env.LANKA_TILES_DIR = tilesDir;

after(() => {
  rmSync(tilesDir, { recursive: true, force: true });
});

async function bodyBuffer(res: Response): Promise<Buffer> {
  return Buffer.from(await res.arrayBuffer());
}

test("GET /v1/tiles/:file streams the full file with the right headers", async () => {
  const res = await app.request("/v1/tiles/admin.pmtiles");
  assert.equal(res.status, 200);
  assert.equal(res.headers.get("Content-Type"), "application/octet-stream");
  assert.equal(res.headers.get("Content-Length"), String(FILE_SIZE));
  assert.equal(res.headers.get("Accept-Ranges"), "bytes");
  assert.equal(res.headers.get("Cache-Control"), "public, max-age=31536000, immutable");
  const body = await bodyBuffer(res);
  assert.equal(body.length, FILE_SIZE);
  assert.ok(body.equals(FILE_BYTES));
});

test("GET /v1/tiles/:file sets a data_version-based ETag", async () => {
  const res = await app.request("/v1/tiles/admin.pmtiles");
  const etag = res.headers.get("ETag");
  assert.ok(etag);
  assert.ok(etag!.startsWith(`"${DATA_VERSION}-`));
});

test("If-None-Match with the current ETag returns 304", async () => {
  const first = await app.request("/v1/tiles/admin.pmtiles");
  const etag = first.headers.get("ETag")!;
  const second = await app.request("/v1/tiles/admin.pmtiles", { headers: { "If-None-Match": etag } });
  assert.equal(second.status, 304);
});

test("a middle byte range slices the file exactly", async () => {
  const res = await app.request("/v1/tiles/admin.pmtiles", { headers: { Range: "bytes=10-19" } });
  assert.equal(res.status, 206);
  assert.equal(res.headers.get("Content-Range"), `bytes 10-19/${FILE_SIZE}`);
  assert.equal(res.headers.get("Content-Length"), "10");
  const body = await bodyBuffer(res);
  assert.ok(body.equals(FILE_BYTES.subarray(10, 20)));
});

test("an open-ended range (bytes=N-) returns from N to the end", async () => {
  const res = await app.request("/v1/tiles/admin.pmtiles", { headers: { Range: "bytes=290-" } });
  assert.equal(res.status, 206);
  assert.equal(res.headers.get("Content-Range"), `bytes 290-299/${FILE_SIZE}`);
  const body = await bodyBuffer(res);
  assert.ok(body.equals(FILE_BYTES.subarray(290, 300)));
});

test("a suffix range (bytes=-N) returns the last N bytes", async () => {
  const res = await app.request("/v1/tiles/admin.pmtiles", { headers: { Range: "bytes=-10" } });
  assert.equal(res.status, 206);
  assert.equal(res.headers.get("Content-Range"), `bytes 290-299/${FILE_SIZE}`);
  const body = await bodyBuffer(res);
  assert.ok(body.equals(FILE_BYTES.subarray(290, 300)));
});

test("an unsatisfiable range is a 416 with Content-Range: bytes */size", async () => {
  const res = await app.request("/v1/tiles/admin.pmtiles", { headers: { Range: "bytes=100000-200000" } });
  assert.equal(res.status, 416);
  assert.equal(res.headers.get("Content-Range"), `bytes */${FILE_SIZE}`);
});

test("a missing file is a 404, not a crash (tiles may not be built yet)", async () => {
  const res = await app.request("/v1/tiles/electoral.pmtiles");
  assert.equal(res.status, 404);
});

test("filenames outside the [a-z0-9-]+.pmtiles whitelist are rejected as 404", async () => {
  const upper = await app.request("/v1/tiles/ADMIN.pmtiles");
  assert.equal(upper.status, 404);

  const wrongExt = await app.request("/v1/tiles/admin.txt");
  assert.equal(wrongExt.status, 404);

  const traversalEncoded = await app.request("/v1/tiles/..%2Fadmin.pmtiles");
  assert.equal(traversalEncoded.status, 404);

  const traversalPlain = await app.request("/v1/tiles/..%2F..%2Fpackage.json");
  assert.equal(traversalPlain.status, 404);
});
