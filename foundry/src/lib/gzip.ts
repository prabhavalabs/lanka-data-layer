import { gunzipSync, gzipSync } from "node:zlib";

/** Gzips a UTF-8 buffer/string — thin wrapper so `downloads.ts` and its tests share one code path. */
export function gzipBuffer(data: Buffer | string): Buffer {
  return gzipSync(data);
}

/** Inverse of `gzipBuffer`, decoded as UTF-8 text — used by tests to assert round-trip integrity. */
export function gunzipToText(data: Buffer): string {
  return gunzipSync(data).toString("utf8");
}
