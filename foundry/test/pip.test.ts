import { test } from "node:test";
import assert from "node:assert/strict";
import {
  pointInRing,
  pointInPolygon,
  pointInGeometry,
  minBoundaryDistanceMeters,
  PolygonIndex,
  type PolygonCoords,
  type PolygonalGeometry,
} from "../src/lib/pip.ts";

// A 1deg x 1deg square, lon/lat in [0,1] x [0,1].
const OUTER_SQUARE: [number, number][] = [
  [0, 0],
  [1, 0],
  [1, 1],
  [0, 1],
  [0, 0],
];

// A 0.2deg square hole centered in the outer square: [0.4,0.6] x [0.4,0.6].
const HOLE: [number, number][] = [
  [0.4, 0.4],
  [0.6, 0.4],
  [0.6, 0.6],
  [0.4, 0.6],
  [0.4, 0.4],
];

test("pointInRing: inside/outside a simple square", () => {
  assert.equal(pointInRing(0.5, 0.5, OUTER_SQUARE), true);
  assert.equal(pointInRing(0.01, 0.01, OUTER_SQUARE), true);
  assert.equal(pointInRing(1.5, 0.5, OUTER_SQUARE), false);
  assert.equal(pointInRing(-0.5, 0.5, OUTER_SQUARE), false);
  assert.equal(pointInRing(0.5, 1.5, OUTER_SQUARE), false);
});

test("pointInPolygon: hole is subtracted from the outer ring", () => {
  const polygon: PolygonCoords = [OUTER_SQUARE, HOLE];
  // Inside outer, outside hole -> true.
  assert.equal(pointInPolygon(0.1, 0.1, polygon), true);
  assert.equal(pointInPolygon(0.9, 0.9, polygon), true);
  // Inside the hole -> false, even though inside the outer ring.
  assert.equal(pointInPolygon(0.5, 0.5, polygon), false);
  // Outside the outer ring entirely -> false.
  assert.equal(pointInPolygon(2, 2, polygon), false);
  // Exactly at hole boundary region but just outside the hole -> true.
  assert.equal(pointInPolygon(0.39, 0.5, polygon), true);
});

test("pointInPolygon without holes behaves like pointInRing on the outer ring", () => {
  const polygon: PolygonCoords = [OUTER_SQUARE];
  assert.equal(pointInPolygon(0.5, 0.5, polygon), true);
  assert.equal(pointInPolygon(1.5, 0.5, polygon), false);
});

test("pointInGeometry: MultiPolygon matches if inside any member polygon", () => {
  const farSquare: [number, number][] = [
    [10, 10],
    [11, 10],
    [11, 11],
    [10, 11],
    [10, 10],
  ];
  const geom: PolygonalGeometry = {
    type: "MultiPolygon",
    coordinates: [[OUTER_SQUARE, HOLE], [farSquare]],
  };
  assert.equal(pointInGeometry(0.1, 0.1, geom), true); // in first polygon
  assert.equal(pointInGeometry(10.5, 10.5, geom), true); // in second polygon
  assert.equal(pointInGeometry(0.5, 0.5, geom), false); // in first polygon's hole
  assert.equal(pointInGeometry(5, 5, geom), false); // in neither
});

test("minBoundaryDistanceMeters is ~0 exactly on the boundary (incl. mid-edge, not just vertices) and grows with distance", () => {
  const geom: PolygonalGeometry = { type: "Polygon", coordinates: [OUTER_SQUARE] };
  const onVertex = minBoundaryDistanceMeters(0, 0, geom);
  assert.ok(onVertex < 1, `expected ~0m at a vertex, got ${onVertex}`);
  // Mid-edge point, far from any vertex but right on the boundary.
  const onEdgeMidpoint = minBoundaryDistanceMeters(0, 0.5, geom);
  assert.ok(onEdgeMidpoint < 1, `expected ~0m at an edge midpoint, got ${onEdgeMidpoint}`);
  const near = minBoundaryDistanceMeters(-0.001, 0.5, geom); // just outside the western edge
  const far = minBoundaryDistanceMeters(-0.05, 0.5, geom);
  assert.ok(near < far);
});

test("PolygonIndex.contains: bbox-bucketed lookup finds the containing polygon incl. holes", () => {
  const idx = new PolygonIndex<string>(0.25);
  idx.insert("square-with-hole", { type: "Polygon", coordinates: [OUTER_SQUARE, HOLE] });
  const farSquare: [number, number][] = [
    [10, 10],
    [11, 10],
    [11, 11],
    [10, 11],
    [10, 10],
  ];
  idx.insert("far-square", { type: "Polygon", coordinates: [farSquare] });

  assert.equal(idx.contains(0.1, 0.1), "square-with-hole");
  assert.equal(idx.contains(10.5, 10.5), "far-square");
  assert.equal(idx.contains(0.5, 0.5), null); // in the hole
  assert.equal(idx.contains(5, 5), null); // in neither
});

test("PolygonIndex.nearestWithin: assigns the nearest polygon for points just outside it", () => {
  const idx = new PolygonIndex<string>(0.25);
  idx.insert("square", { type: "Polygon", coordinates: [OUTER_SQUARE] });

  // ~111m outside the western edge (0.001deg ~ 111m at the equator).
  const justOutside = idx.nearestWithin(-0.001, 0.5, 2000);
  assert.equal(justOutside, "square");

  // Far outside any reasonable threshold.
  const tooFar = idx.nearestWithin(-5, 0.5, 2000);
  assert.equal(tooFar, null);
});

test("PolygonIndex.nearestWithin: point already inside is not returned by contains but is trivially near", () => {
  const idx = new PolygonIndex<string>(0.25);
  idx.insert("square", { type: "Polygon", coordinates: [OUTER_SQUARE] });
  assert.equal(idx.contains(0.5, 0.5), "square");
});
