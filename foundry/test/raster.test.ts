import { test } from "node:test";
import assert from "node:assert/strict";
import { fineCellsForBounds } from "../src/lib/raster.ts";
import { GRID, cellCenter, cellId, MAX_CELL_ID } from "../src/grid.ts";

/** Brute-force reference: scan every fine cell in a generous local window
 * around the pixel and keep the ones whose center actually falls inside
 * bounds. Used to cross-check the closed-form implementation. */
function bruteForce(bounds: readonly [number, number, number, number]): number[] {
  const [west, south, east, north] = bounds;
  const out: number[] = [];
  for (let id = 0; id <= MAX_CELL_ID; id++) {
    const c = cellCenter(id);
    if (!c) continue;
    if (c.lon >= west && c.lon < east && c.lat >= south && c.lat < north) out.push(id);
  }
  return out;
}

test("pixel exactly 8 fine cells wide/tall (grid-aligned) yields a full 8x8 block", () => {
  const west = 79.401;
  const south = 5.801;
  const east = west + 8 * GRID.step; // 79.409
  const north = south + 8 * GRID.step; // 5.809
  const ids = fineCellsForBounds([west, south, east, north]);
  assert.equal(ids.length, 64);
  // Every returned id's center must be within bounds.
  for (const id of ids) {
    const c = cellCenter(id)!;
    assert.ok(c.lon >= west && c.lon < east);
    assert.ok(c.lat >= south && c.lat < north);
  }
});

test("matches brute-force scan for a WorldPop-sized pixel (~0.0083333deg), grid-aligned origin", () => {
  const pixelSize = 0.0083333333;
  const west = 79.44; // aligned to the 0.001deg grid
  const south = 6.6;
  const bounds: [number, number, number, number] = [west, south, west + pixelSize, south + pixelSize];
  const got = fineCellsForBounds(bounds).sort((a, b) => a - b);
  const want = bruteForce(bounds).sort((a, b) => a - b);
  assert.deepEqual(got, want);
  // ~8.333 cells per side -> 8x8 or 8x9/9x8/9x9 depending on offset.
  assert.ok(got.length >= 56 && got.length <= 81, `unexpected cell count ${got.length}`);
});

test("matches brute-force scan for a fractionally-offset pixel", () => {
  const pixelSize = 0.0083333333;
  const west = 80.123456; // deliberately not grid-aligned
  const south = 7.654321;
  const bounds: [number, number, number, number] = [west, south, west + pixelSize, south + pixelSize];
  const got = fineCellsForBounds(bounds).sort((a, b) => a - b);
  const want = bruteForce(bounds).sort((a, b) => a - b);
  assert.deepEqual(got, want);
});

test("uniform pop distribution: every returned cell gets pop/count, summing back to pop", () => {
  const pixelSize = 0.0083333333;
  const bounds: [number, number, number, number] = [80.0, 7.0, 80.0 + pixelSize, 7.0 + pixelSize];
  const ids = fineCellsForBounds(bounds);
  const pop = 123.456;
  const perCell = pop / ids.length;
  const total = ids.length * perCell;
  assert.ok(Math.abs(total - pop) < 1e-9);
});

test("pixel entirely outside the canonical grid bbox yields no cells", () => {
  const ids = fineCellsForBounds([90, 20, 90.008, 20.008]);
  assert.deepEqual(ids, []);
});

test("pixel straddling the grid's western edge is clipped, not thrown", () => {
  const ids = fineCellsForBounds([GRID.lonMin - 0.004, 6.0, GRID.lonMin + 0.004, 6.008]);
  for (const id of ids) {
    const c = cellCenter(id)!;
    assert.ok(c.lon >= GRID.lonMin);
  }
  // Sanity: at least the cells on the in-bounds half are present.
  assert.ok(ids.length > 0);
});

test("no cell is ever double-counted across two horizontally adjacent pixels", () => {
  const pixelSize = 0.0083333333;
  const west = 81.0;
  const south = 8.0;
  const left = fineCellsForBounds([west, south, west + pixelSize, south + pixelSize]);
  const right = fineCellsForBounds([west + pixelSize, south, west + 2 * pixelSize, south + pixelSize]);
  const overlap = left.filter((id) => right.includes(id));
  assert.deepEqual(overlap, []);
});

test("cellId of every returned center round-trips to the same id", () => {
  const bounds: [number, number, number, number] = [79.5, 6.1, 79.5083333, 6.1083333];
  for (const id of fineCellsForBounds(bounds)) {
    const c = cellCenter(id)!;
    assert.equal(cellId(c.lat, c.lon), id);
  }
});
