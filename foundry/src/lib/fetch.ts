import { createWriteStream } from "node:fs";
import { mkdir, stat } from "node:fs/promises";
import path from "node:path";
import { Readable } from "node:stream";
import { finished } from "node:stream/promises";

/**
 * Downloads `url` to `destPath` unless it already exists (cache is keyed by
 * presence on disk — raw downloads are treated as immutable snapshots).
 * Returns whether a download actually happened.
 */
export async function downloadIfMissing(url: string, destPath: string): Promise<boolean> {
  if (await exists(destPath)) return false;
  await mkdir(path.dirname(destPath), { recursive: true });
  const res = await fetch(url);
  if (!res.ok || !res.body) {
    throw new Error(`Failed to download ${url}: ${res.status} ${res.statusText}`);
  }
  const tmpPath = `${destPath}.part`;
  const out = createWriteStream(tmpPath);
  await finished(Readable.fromWeb(res.body as import("node:stream/web").ReadableStream).pipe(out));
  const { rename } = await import("node:fs/promises");
  await rename(tmpPath, destPath);
  return true;
}

export async function exists(p: string): Promise<boolean> {
  try {
    await stat(p);
    return true;
  } catch {
    return false;
  }
}
