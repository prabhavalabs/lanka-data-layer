import { test } from "node:test";
import assert from "node:assert/strict";
import { cellId, cellCenter, cellBounds, GRID, MAX_CELL_ID } from "../src/grid.ts";

// Pinned test vectors — identical to shared/src/grid.test.ts. Both
// implementations must produce identical values for every case here.
test("Colombo Fort (6.9344, 79.8428)", () => {
  const id = cellId(6.9344, 79.8428);
  // row = floor((10.0 - 6.9344) * 1000) = 3065; col = floor((79.8428 - 79.4) * 1000) = 442
  assert.equal(id, 3065 * 2700 + 442);
});

test("Point Pedro (9.8355, 80.2350), northern tip", () => {
  // col is 834, not 835: (80.235 - 79.4) * 1000 === 834.9999… in IEEE 754.
  // The formula is pure double math with no epsilon — every implementation
  // must reproduce these exact ids.
  assert.equal(cellId(9.8355, 80.235), 164 * 2700 + 834);
});

test("Dondra Head (5.9236, 80.5883), southern tip", () => {
  assert.equal(cellId(5.9236, 80.5883), 4076 * 2700 + 1188);
});

test("out of coverage returns null", () => {
  assert.equal(cellId(4.0, 80.0), null); // south of bbox
  assert.equal(cellId(7.0, 83.0), null); // east of bbox
  assert.equal(cellId(10.0, 80.0), null); // latMax is exclusive
  assert.equal(cellId(NaN, 80.0), null);
});

test("bbox corners", () => {
  assert.equal(cellId(9.9999, 79.4), 0);
  assert.equal(cellId(5.8005, 82.0995), MAX_CELL_ID);
  // exact south edge falls outside (half-open bbox)
  assert.equal(cellId(5.8, 82.0), null);
});

test("cellCenter inverts cellId within one step", () => {
  const id = cellId(6.9344, 79.8428)!;
  const c = cellCenter(id)!;
  assert.ok(Math.abs(c.lat - 6.9344) <= GRID.step);
  assert.ok(Math.abs(c.lon - 79.8428) <= GRID.step);
  assert.equal(cellId(c.lat, c.lon), id);
});

test("cellBounds contains center", () => {
  const id = cellId(7.2906, 80.6337)!; // Kandy
  const [w, s, e, n] = cellBounds(id)!;
  const c = cellCenter(id)!;
  assert.ok(c.lon > w && c.lon < e && c.lat > s && c.lat < n);
});

test("invalid ids return null", () => {
  assert.equal(cellCenter(-1), null);
  assert.equal(cellBounds(MAX_CELL_ID + 1), null);
});
