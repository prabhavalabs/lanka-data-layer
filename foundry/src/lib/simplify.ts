/**
 * Douglas-Peucker polyline/ring simplification, dependency-free (same
 * "no turf" convention as lib/geo.ts). Operates directly on lon/lat degrees
 * — for the ~0.0005deg tolerance `admin-geometry` uses, planar distance in
 * degree-space is an adequate stand-in for a proper geodesic distance.
 */

type Pos = [number, number];

function perpendicularDistance(p: Pos, a: Pos, b: Pos): number {
  const [x, y] = p;
  const [x1, y1] = a;
  const [x2, y2] = b;
  const dx = x2 - x1;
  const dy = y2 - y1;
  if (dx === 0 && dy === 0) return Math.hypot(x - x1, y - y1);
  const t = ((x - x1) * dx + (y - y1) * dy) / (dx * dx + dy * dy);
  const cx = x1 + t * dx;
  const cy = y1 + t * dy;
  return Math.hypot(x - cx, y - cy);
}

/** Classic Douglas-Peucker over an open polyline (distinct endpoints). */
export function simplifyLine(points: Pos[], tolerance: number): Pos[] {
  if (points.length <= 2) return points;
  const first = points[0]!;
  const last = points[points.length - 1]!;
  let maxDist = -1;
  let index = 0;
  for (let i = 1; i < points.length - 1; i++) {
    const d = perpendicularDistance(points[i]!, first, last);
    if (d > maxDist) {
      maxDist = d;
      index = i;
    }
  }
  if (maxDist > tolerance) {
    const left = simplifyLine(points.slice(0, index + 1), tolerance);
    const right = simplifyLine(points.slice(index), tolerance);
    return left.slice(0, -1).concat(right);
  }
  return [first, last];
}

function planarDist(a: Pos, b: Pos): number {
  return Math.hypot(a[0] - b[0], a[1] - b[1]);
}

/**
 * Simplifies a closed GeoJSON ring (first point === last point). Standard
 * trick for running open-polyline DP on a loop: split the ring at the point
 * farthest from the start into two arcs with real (distinct) endpoints,
 * simplify each arc independently, then splice back together.
 *
 * Returns the original ring unchanged if simplification would degenerate it
 * below a valid ring (fewer than 4 points, i.e. a triangle + closing point)
 * — callers get a guaranteed-valid ring back, never a broken one.
 */
export function simplifyRing(ring: Pos[], tolerance: number): Pos[] {
  if (ring.length <= 4) return ring;
  const pts = ring.slice(0, -1); // drop the duplicated closing point
  if (pts.length <= 3) return ring;

  let splitIndex = 1;
  let maxDist = -1;
  for (let i = 1; i < pts.length; i++) {
    const d = planarDist(pts[i]!, pts[0]!);
    if (d > maxDist) {
      maxDist = d;
      splitIndex = i;
    }
  }

  const arc1 = pts.slice(0, splitIndex + 1);
  const arc2 = pts.slice(splitIndex).concat([pts[0]!]);
  const s1 = simplifyLine(arc1, tolerance);
  const s2 = simplifyLine(arc2, tolerance);
  let merged = s1.slice(0, -1).concat(s2);

  const start = merged[0]!;
  const end = merged[merged.length - 1]!;
  if (start[0] !== end[0] || start[1] !== end[1]) merged = merged.concat([start]);

  // Degenerate (collapsed to a line/point, or too few points for a valid
  // polygon ring) — fall back to the original ring rather than emit broken
  // geometry. At a 0.0005deg tolerance against real admin-boundary rings
  // this is expected to be rare (tiny slivers only).
  if (merged.length < 4 || isDegenerateRing(merged)) return ring;
  return merged;
}

function isDegenerateRing(ring: Pos[]): boolean {
  // Shoelace formula; near-zero enclosed area means the ring collapsed.
  let area = 0;
  for (let i = 0; i < ring.length - 1; i++) {
    const [x1, y1] = ring[i]!;
    const [x2, y2] = ring[i + 1]!;
    area += x1 * y2 - x2 * y1;
  }
  return Math.abs(area) < 1e-12;
}
