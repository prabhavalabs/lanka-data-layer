import { test } from "node:test";
import assert from "node:assert/strict";
import { simplifyLine, simplifyRing } from "../src/lib/simplify.ts";
import { roundCoordinates, simplifyPolygonalGeometry } from "../src/lib/geojson.ts";

test("simplifyLine drops near-collinear points but keeps corners", () => {
  const line: [number, number][] = [
    [0, 0],
    [1, 0.0001], // nearly on the line from (0,0) to (2,0)
    [2, 0],
    [2, 2], // sharp corner
    [0, 2],
  ];
  const out = simplifyLine(line, 0.001);
  assert.deepEqual(out[0], [0, 0]);
  assert.deepEqual(out[out.length - 1], [0, 2]);
  assert.ok(out.length < line.length);
  // The corner at (2,2) must survive — it's far from any chord.
  assert.ok(out.some(([x, y]) => x === 2 && y === 2));
});

test("simplifyLine with tolerance 0 keeps every point (well below any perpendicular distance)", () => {
  const line: [number, number][] = [
    [0, 0],
    [1, 0.5],
    [2, 0],
  ];
  const out = simplifyLine(line, 0);
  assert.deepEqual(out, line);
});

test("simplifyRing returns a closed ring (first === last)", () => {
  // A near-circular octagon-ish ring with a couple of redundant near-collinear points.
  const ring: [number, number][] = [
    [0, 1],
    [0.001, 0.999],
    [0.707, 0.707],
    [1, 0],
    [0.707, -0.707],
    [0, -1],
    [-0.707, -0.707],
    [-1, 0],
    [-0.707, 0.707],
    [0, 1],
  ];
  const out = simplifyRing(ring, 0.01);
  assert.deepEqual(out[0], out[out.length - 1]);
  assert.ok(out.length >= 4);
  assert.ok(out.length <= ring.length);
});

test("simplifyRing falls back to the original ring when it would degenerate below a valid polygon", () => {
  const tinyTriangle: [number, number][] = [
    [0, 0],
    [0.0001, 0],
    [0.00005, 0.0001],
    [0, 0],
  ];
  const out = simplifyRing(tinyTriangle, 0.0005); // tolerance bigger than the whole triangle
  assert.deepEqual(out, tinyTriangle);
});

test("simplifyRing leaves already-minimal rings (<=4 points) untouched", () => {
  const square: [number, number][] = [
    [0, 0],
    [1, 0],
    [1, 1],
    [0, 0],
  ];
  assert.deepEqual(simplifyRing(square, 0.5), square);
});

test("simplifyPolygonalGeometry simplifies holes independently and preserves winding structure", () => {
  const outer: [number, number][] = [
    [0, 0],
    [0.001, 0],
    [10, 0],
    [10, 10],
    [0, 10],
    [0, 0],
  ];
  const hole: [number, number][] = [
    [4, 4],
    [4.001, 4],
    [6, 4],
    [6, 6],
    [4, 6],
    [4, 4],
  ];
  const out = simplifyPolygonalGeometry({ type: "Polygon", coordinates: [outer, hole] }, 0.01);
  assert.equal(out.type, "Polygon");
  assert.equal((out as { coordinates: unknown[] }).coordinates.length, 2); // outer + hole both survive
});

test("roundCoordinates rounds every leaf coordinate regardless of nesting depth", () => {
  const point = roundCoordinates([79.842812345, 6.934412345], 6);
  assert.deepEqual(point, [79.842812, 6.934412]);

  const polygon = roundCoordinates(
    [
      [
        [79.1234564, 6.1234564],
        [79.1234566, 6.1234566],
      ],
    ],
    6,
  );
  assert.deepEqual(polygon, [
    [
      [79.123456, 6.123456],
      [79.123457, 6.123457],
    ],
  ]);
});
