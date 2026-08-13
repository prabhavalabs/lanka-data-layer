import type { Hono } from "hono";
import type Database from "better-sqlite3";
import { createReadStream } from "node:fs";
import { stat } from "node:fs/promises";
import { isAbsolute, join, resolve, sep } from "node:path";
import { Readable } from "node:stream";
import { getDataVersion } from "../lib/envelope.ts";
import { NotFoundError } from "../lib/errors.ts";

/** Whitelist, exactly as specified by contract §4/task spec — rejects traversal, dotfiles, and non-pmtiles names outright. */
const FILENAME_RE = /^[a-z0-9-]+\.pmtiles$/;

function tilesDir(): string {
  return resolve(process.env.LANKA_TILES_DIR ?? "../foundry/data/artifacts/tiles");
}

interface RangeResult {
  start: number;
  end: number;
}

/** Parses a single-range `Range: bytes=start-end` header (RFC 9110 §14.1.2) against a known file size. Returns null for anything unparseable or unsatisfiable — caller responds 416. */
function parseRange(header: string, size: number): RangeResult | null {
  const match = /^bytes=(\d*)-(\d*)$/.exec(header.trim());
  if (!match) return null;
  const [, startRaw, endRaw] = match;
  if (!startRaw && !endRaw) return null;

  let start: number;
  let end: number;
  if (!startRaw) {
    // Suffix range "bytes=-N": the last N bytes.
    const suffixLength = Number.parseInt(endRaw ?? "0", 10);
    if (suffixLength <= 0) return null;
    start = Math.max(0, size - suffixLength);
    end = size - 1;
  } else {
    start = Number.parseInt(startRaw, 10);
    end = endRaw ? Number.parseInt(endRaw, 10) : size - 1;
  }
  if (end >= size) end = size - 1;
  if (!Number.isFinite(start) || !Number.isFinite(end) || start > end || start < 0 || start >= size) return null;
  return { start, end };
}

/**
 * GET /v1/tiles/:file — static PMTiles passthrough (contract §4: "the api
 * serves foundry/data/artifacts/tiles/*.pmtiles as static files with
 * range-request support"). pmtiles.js and other PMTiles clients fetch
 * these with byte-range requests (they only ever pull the header + the
 * directory entries + the specific tile's bytes, never the whole file), so
 * Range support isn't an optimization here — without it the protocol
 * doesn't work at all.
 *
 * Deliberately outside the {success, message, payload, meta} envelope
 * (raw octet-stream body, same reasoning as /v1/admin/:pcode/geometry
 * being a bare GeoJSON Feature): pmtiles clients expect to `fetch()` this
 * URL directly and read bytes off the Response, not unwrap JSON first.
 *
 * The tiles directory may legitimately be empty or not yet exist — a
 * concurrent foundry build step produces these files — so a missing file
 * is a normal 404, not a startup error; this route never touches the
 * database except to read data_version for the ETag.
 */
export function mountTilesRoute(app: Hono, db: Database.Database): void {
  app.get("/v1/tiles/:file", async (c) => {
    const file = c.req.param("file");
    if (!FILENAME_RE.test(file)) throw new NotFoundError("not found");

    const dir = tilesDir();
    const filePath = join(dir, file);
    // Defense in depth beyond the regex whitelist: the resolved path must stay inside `dir`.
    if (!isAbsolute(filePath) || (filePath !== dir && !filePath.startsWith(dir + sep))) {
      throw new NotFoundError("not found");
    }

    let stats;
    try {
      stats = await stat(filePath);
    } catch {
      throw new NotFoundError(`tile file "${file}" not found`);
    }
    if (!stats.isFile()) throw new NotFoundError(`tile file "${file}" not found`);

    const dataVersion = getDataVersion(db);
    const etag = `"${dataVersion}-${stats.mtimeMs.toString(36)}-${stats.size.toString(36)}"`;
    // Not `immutable`: the tileset URL carries no version, and a data
    // release rewrites the file in place. The mtime-based ETag makes
    // revalidation cheap; an hour of freshness keeps tile traffic light.
    c.header("Cache-Control", "public, max-age=3600, stale-while-revalidate=86400");
    c.header("ETag", etag);
    c.header("Accept-Ranges", "bytes");

    if (c.req.header("If-None-Match") === etag) {
      return c.body(null, 304);
    }

    c.header("Content-Type", "application/octet-stream");

    const rangeHeader = c.req.header("Range");
    if (rangeHeader) {
      const range = parseRange(rangeHeader, stats.size);
      if (!range) {
        c.header("Content-Range", `bytes */${stats.size}`);
        return c.body(null, 416);
      }
      c.header("Content-Range", `bytes ${range.start}-${range.end}/${stats.size}`);
      c.header("Content-Length", String(range.end - range.start + 1));
      const nodeStream = createReadStream(filePath, { start: range.start, end: range.end });
      return c.body(Readable.toWeb(nodeStream) as ReadableStream, 206);
    }

    c.header("Content-Length", String(stats.size));
    const nodeStream = createReadStream(filePath);
    return c.body(Readable.toWeb(nodeStream) as ReadableStream, 200);
  });
}
