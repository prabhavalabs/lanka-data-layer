import { test } from "node:test";
import assert from "node:assert/strict";
import { NearestIndex } from "../src/lib/nearest.ts";
import { haversineMeters } from "../src/lib/geo.ts";

interface Pt {
  id: number;
  lat: number;
  lon: number;
}

function bruteForceNearest(points: Pt[], lat: number, lon: number): { id: number; distM: number } | null {
  let best: { id: number; distM: number } | null = null;
  for (const p of points) {
    const d = haversineMeters(lat, lon, p.lat, p.lon);
    if (!best || d < best.distM) best = { id: p.id, distM: d };
  }
  return best;
}

// A pseudo-random-but-deterministic scatter of points across a plausible
// Sri Lanka-sized bbox, mimicking postal_codes/places density.
function scatterPoints(n: number): Pt[] {
  const points: Pt[] = [];
  let seed = 42;
  const rand = () => {
    seed = (seed * 1103515245 + 12345) & 0x7fffffff;
    return seed / 0x7fffffff;
  };
  for (let i = 0; i < n; i++) {
    points.push({
      id: i,
      lat: 5.8 + rand() * 4.2,
      lon: 79.4 + rand() * 2.7,
    });
  }
  return points;
}

test("matches brute-force nearest for a dense scattered point set", () => {
  const points = scatterPoints(500);
  const idx = new NearestIndex<number>(0.1);
  for (const p of points) idx.insert(p.id, p.lat, p.lon);

  const queries: [number, number][] = [
    [6.9344, 79.8428], // Colombo Fort
    [9.8355, 80.235], // Point Pedro
    [5.9236, 80.5883], // Dondra Head
    [7.2906, 80.6337], // Kandy
    [8.5874, 81.2152], // Trincomalee-ish
  ];
  for (const [lat, lon] of queries) {
    const got = idx.nearest(lat, lon);
    const want = bruteForceNearest(points, lat, lon);
    assert.ok(got && want);
    assert.equal(got!.id, want!.id);
    assert.ok(Math.abs(got!.distM - want!.distM) < 1e-6);
  }
});

test("matches brute-force nearest for a sparse point set requiring ring expansion", () => {
  // Only 5 points spread far apart relative to the bucket size — forces
  // nearest() to expand through several empty rings before finding anything.
  const points: Pt[] = [
    { id: 1, lat: 6.0, lon: 79.5 },
    { id: 2, lat: 9.5, lon: 81.9 },
    { id: 3, lat: 5.85, lon: 82.0 },
    { id: 4, lat: 9.9, lon: 79.45 },
    { id: 5, lat: 7.5, lon: 80.6 },
  ];
  const idx = new NearestIndex<number>(0.05);
  for (const p of points) idx.insert(p.id, p.lat, p.lon);

  const queries: [number, number][] = [
    [6.9344, 79.8428],
    [8.9, 81.5],
    [6.0, 79.51],
  ];
  for (const [lat, lon] of queries) {
    const got = idx.nearest(lat, lon);
    const want = bruteForceNearest(points, lat, lon);
    assert.equal(got!.id, want!.id);
    assert.ok(Math.abs(got!.distM - want!.distM) < 1e-6);
  }
});

test("nearest() on an empty index returns null", () => {
  const idx = new NearestIndex<number>(0.1);
  assert.equal(idx.nearest(6.9344, 79.8428), null);
});

test("nearest() finds the exact point when queried at its own coordinates", () => {
  const idx = new NearestIndex<string>(0.1);
  idx.insert("a", 6.9344, 79.8428);
  idx.insert("b", 7.2906, 80.6337);
  const got = idx.nearest(6.9344, 79.8428);
  assert.equal(got!.id, "a");
  assert.ok(got!.distM < 1);
});

test("nearest() breaks ties by whichever point is scanned first at equal distance", () => {
  const idx = new NearestIndex<string>(0.1);
  idx.insert("west", 7.0, 79.9);
  idx.insert("east", 7.0, 80.1);
  const got = idx.nearest(7.0, 80.0);
  assert.ok(got!.id === "west" || got!.id === "east");
  assert.ok(Math.abs(got!.distM - haversineMeters(7.0, 80.0, 7.0, 79.9)) < 1);
});
