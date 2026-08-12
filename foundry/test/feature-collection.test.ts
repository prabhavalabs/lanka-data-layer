import { test } from "node:test";
import assert from "node:assert/strict";
import { roundCoord, roundCoordinates, roundGeometry } from "../src/lib/feature-collection.ts";

test("roundCoord rounds to 6 decimal places", () => {
  assert.equal(roundCoord(80.54889171234), 80.548892);
  assert.equal(roundCoord(5.94494499999), 5.944945);
});

test("roundCoord is a no-op on values already at 6 dp", () => {
  assert.equal(roundCoord(79.4), 79.4);
});

test("roundCoordinates rounds a single position, leaving extra dimensions alone", () => {
  assert.deepEqual(roundCoordinates([80.54889171234, 5.94494499999]), [80.548892, 5.944945]);
  assert.deepEqual(roundCoordinates([80.54889171234, 5.94494499999, 12.3]), [80.548892, 5.944945, 12.3]);
});

test("roundCoordinates recurses through LineString-depth arrays", () => {
  const line = [
    [80.5488917123, 5.9449449999],
    [80.5498847001, 5.9447015002],
  ];
  assert.deepEqual(roundCoordinates(line), [
    [80.548892, 5.944945],
    [80.549885, 5.944702],
  ]);
});

test("roundCoordinates recurses through Polygon-depth (ring-of-positions) arrays", () => {
  const polygon = [
    [
      [80.001234567, 6.001234567],
      [80.002234567, 6.002234567],
      [80.001234567, 6.001234567],
    ],
  ];
  const rounded = roundCoordinates(polygon);
  assert.deepEqual(rounded, [
    [
      [80.001235, 6.001235],
      [80.002235, 6.002235],
      [80.001235, 6.001235],
    ],
  ]);
});

test("roundGeometry rounds a Point geometry and preserves its type", () => {
  const geom = roundGeometry({ type: "Point" as const, coordinates: [80.123456789, 6.987654321] });
  assert.equal(geom.type, "Point");
  assert.deepEqual(geom.coordinates, [80.123457, 6.987654]);
});

test("roundGeometry rounds a MultiPolygon (country boundary depth)", () => {
  const geom = roundGeometry({
    type: "MultiPolygon" as const,
    coordinates: [[[[79.78336969999997, 8.2629394], [79.78542959999996, 8.26786590000004]]]],
  });
  assert.deepEqual(geom.coordinates, [[[[79.78337, 8.262939], [79.78543, 8.267866]]]]);
});
