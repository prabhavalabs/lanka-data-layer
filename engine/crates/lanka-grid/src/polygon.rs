//! Polygons, point-in-polygon and the bucket index — a port of v1
//! `foundry/src/lib/{pip,spatial-index}.ts` with the same semantics (even-odd
//! ray casting, holes = every ring after the first, first candidate in
//! insertion order wins, nearest polygon by boundary distance on a local
//! equirectangular projection, ring-by-ring widening with the same stop
//! rule), so `gnd_idx` reproduces v1 `cell_lookup`.

use std::collections::HashMap;

use anyhow::{Result, bail, ensure};

/// `[lon, lat]`.
pub type Pt = [f64; 2];
/// Rings of one polygon: outer first, then holes.
pub type Poly = Vec<Vec<Pt>>;

/// A (multi)polygon.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MultiPolygon(pub Vec<Poly>);

impl MultiPolygon {
    /// `[west, south, east, north]` over every coordinate (v1 `bboxOf`).
    #[must_use]
    pub fn bbox(&self) -> [f64; 4] {
        let mut b = [
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        ];
        for p in &self.0 {
            for ring in p {
                for &[x, y] in ring {
                    b[0] = b[0].min(x);
                    b[1] = b[1].min(y);
                    b[2] = b[2].max(x);
                    b[3] = b[3].max(y);
                }
            }
        }
        b
    }

    /// Parses ISO/OGC WKB (`ST_AsBinary`): Polygon or MultiPolygon, either
    /// byte order, 2-D (Z/M ordinates are skipped).
    pub fn from_wkb(b: &[u8]) -> Result<Self> {
        let mut r = Wkb { b, at: 0 };
        let g = r.geometry()?;
        ensure!(r.at == b.len(), "trailing bytes after WKB geometry");
        Ok(g)
    }

    /// Total vertex count.
    #[must_use]
    pub fn vertices(&self) -> usize {
        self.0.iter().flatten().map(Vec::len).sum()
    }
}

struct Wkb<'a> {
    b: &'a [u8],
    at: usize,
}

impl Wkb<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8]> {
        ensure!(self.at + n <= self.b.len(), "truncated WKB");
        let s = &self.b[self.at..self.at + n];
        self.at += n;
        Ok(s)
    }
    fn u32(&mut self, le: bool) -> Result<u32> {
        let s = self.take(4)?;
        let a = [s[0], s[1], s[2], s[3]];
        Ok(if le {
            u32::from_le_bytes(a)
        } else {
            u32::from_be_bytes(a)
        })
    }
    fn f64(&mut self, le: bool) -> Result<f64> {
        let s = self.take(8)?;
        let mut a = [0u8; 8];
        a.copy_from_slice(s);
        Ok(if le {
            f64::from_le_bytes(a)
        } else {
            f64::from_be_bytes(a)
        })
    }
    fn header(&mut self) -> Result<(bool, u32, usize)> {
        let le = match self.take(1)?[0] {
            0 => false,
            1 => true,
            o => bail!("bad WKB byte order {o}"),
        };
        let t = self.u32(le)?;
        // ISO (1000/2000/3000 offsets) and EWKB (high flag bits) dimensions.
        let ewkb_z = t & 0x8000_0000 != 0;
        let ewkb_m = t & 0x4000_0000 != 0;
        ensure!(
            t & 0x2000_0000 == 0,
            "EWKB with SRID is not expected (use ST_AsBinary)"
        );
        let base = t & 0x0fff_ffff;
        let (kind, dims) = match base / 1000 {
            0 => (base, 2 + usize::from(ewkb_z) + usize::from(ewkb_m)),
            1 | 2 => (base % 1000, 3),
            3 => (base % 1000, 4),
            _ => bail!("bad WKB type {t}"),
        };
        Ok((le, kind, dims))
    }
    fn polygon_body(&mut self, le: bool, dims: usize) -> Result<Poly> {
        let nrings = self.u32(le)? as usize;
        let mut rings = Vec::with_capacity(nrings);
        for _ in 0..nrings {
            let n = self.u32(le)? as usize;
            let mut ring = Vec::with_capacity(n);
            for _ in 0..n {
                let x = self.f64(le)?;
                let y = self.f64(le)?;
                for _ in 2..dims {
                    self.f64(le)?;
                }
                ring.push([x, y]);
            }
            rings.push(ring);
        }
        Ok(rings)
    }
    fn geometry(&mut self) -> Result<MultiPolygon> {
        let (le, kind, dims) = self.header()?;
        match kind {
            3 => Ok(MultiPolygon(vec![self.polygon_body(le, dims)?])),
            6 => {
                let n = self.u32(le)? as usize;
                let mut polys = Vec::with_capacity(n);
                for _ in 0..n {
                    let (le2, k2, d2) = self.header()?;
                    ensure!(k2 == 3, "MultiPolygon member is not a Polygon");
                    polys.push(self.polygon_body(le2, d2)?);
                }
                Ok(MultiPolygon(polys))
            }
            k => bail!("WKB geometry type {k} is not polygonal"),
        }
    }
}

/// Even-odd ray casting against one ring (v1 `pointInRing`).
#[must_use]
pub fn point_in_ring(lon: f64, lat: f64, ring: &[Pt]) -> bool {
    let mut inside = false;
    let n = ring.len();
    if n == 0 {
        return false;
    }
    let mut j = n - 1;
    for i in 0..n {
        let [xi, yi] = ring[i];
        let [xj, yj] = ring[j];
        if (yi > lat) != (yj > lat) {
            let x_int = ((xj - xi) * (lat - yi)) / (yj - yi) + xi;
            if lon < x_int {
                inside = !inside;
            }
        }
        j = i;
    }
    inside
}

/// Inside the outer ring and outside every hole (v1 `pointInPolygon`).
#[must_use]
pub fn point_in_polygon(lon: f64, lat: f64, p: &Poly) -> bool {
    let Some(outer) = p.first() else {
        return false;
    };
    if !point_in_ring(lon, lat, outer) {
        return false;
    }
    !p[1..].iter().any(|h| point_in_ring(lon, lat, h))
}

#[must_use]
pub fn point_in_geometry(lon: f64, lat: f64, g: &MultiPolygon) -> bool {
    g.0.iter().any(|p| point_in_polygon(lon, lat, p))
}

/// v1 `pointToSegmentMeters` (local equirectangular, 111,320 m/degree).
fn point_to_segment_m(lon: f64, lat: f64, a: Pt, b: Pt) -> f64 {
    let m_lat = 111_320.0;
    let m_lon = 111_320.0 * ((lat * std::f64::consts::PI) / 180.0).cos();
    let (px, py) = (lon * m_lon, lat * m_lat);
    let (ax, ay) = (a[0] * m_lon, a[1] * m_lat);
    let (bx, by) = (b[0] * m_lon, b[1] * m_lat);
    let (dx, dy) = (bx - ax, by - ay);
    let len2 = dx * dx + dy * dy;
    let mut t = if len2 == 0.0 {
        0.0
    } else {
        ((px - ax) * dx + (py - ay) * dy) / len2
    };
    t = t.clamp(0.0, 1.0);
    let (cx, cy) = (ax + t * dx, ay + t * dy);
    (px - cx).hypot(py - cy)
}

/// v1 `minBoundaryDistanceMeters`: every edge of every ring.
#[must_use]
pub fn min_boundary_distance_m(lon: f64, lat: f64, g: &MultiPolygon) -> f64 {
    let mut min = f64::INFINITY;
    for p in &g.0 {
        for ring in p {
            for w in ring.windows(2) {
                let d = point_to_segment_m(lon, lat, w[0], w[1]);
                if d < min {
                    min = d;
                }
            }
        }
    }
    min
}

/// v1 `GridIndex` + `PolygonIndex`: items registered in every bucket their
/// bbox overlaps; lookups return candidates in insertion order.
#[derive(Debug, Clone)]
pub struct PolygonIndex {
    cell: f64,
    buckets: HashMap<(i64, i64), Vec<u32>>,
    pub geoms: Vec<MultiPolygon>,
    bboxes: Vec<[f64; 4]>,
}

/// Lower bound of [`min_boundary_distance_m`] to anything inside `b`
/// (the same local projection scales each axis linearly, so the projected
/// bbox contains every projected segment).
fn bbox_distance_m(lon: f64, lat: f64, b: &[f64; 4]) -> f64 {
    let m_lat = 111_320.0;
    let m_lon = 111_320.0 * ((lat * std::f64::consts::PI) / 180.0).cos();
    let dx = if lon < b[0] {
        b[0] - lon
    } else if lon > b[2] {
        lon - b[2]
    } else {
        0.0
    };
    let dy = if lat < b[1] {
        b[1] - lat
    } else if lat > b[3] {
        lat - b[3]
    } else {
        0.0
    };
    (dx * m_lon).hypot(dy * m_lat)
}

impl PolygonIndex {
    #[must_use]
    pub fn new(cell_size_deg: f64) -> Self {
        Self {
            cell: cell_size_deg,
            buckets: HashMap::new(),
            geoms: Vec::new(),
            bboxes: Vec::new(),
        }
    }

    #[allow(clippy::cast_possible_truncation)]
    fn b(&self, v: f64) -> i64 {
        (v / self.cell).floor() as i64
    }

    /// Inserts a geometry; returns its id (insertion order).
    pub fn insert(&mut self, g: MultiPolygon) -> u32 {
        let id = u32::try_from(self.geoms.len()).unwrap_or(u32::MAX);
        let [w, s, e, n] = g.bbox();
        if w.is_finite() {
            for bx in self.b(w)..=self.b(e) {
                for by in self.b(s)..=self.b(n) {
                    self.buckets.entry((bx, by)).or_default().push(id);
                }
            }
        }
        self.bboxes.push([w, s, e, n]);
        self.geoms.push(g);
        id
    }

    fn query_point(&self, lon: f64, lat: f64) -> &[u32] {
        self.buckets
            .get(&(self.b(lon), self.b(lat)))
            .map_or(&[], Vec::as_slice)
    }

    /// v1 `queryRing`: the square ring of buckets at Chebyshev distance `r`.
    fn query_ring(&self, lon: f64, lat: f64, r: i64, out: &mut Vec<u32>) {
        out.clear();
        let (cx, cy) = (self.b(lon), self.b(lat));
        if r == 0 {
            out.extend_from_slice(self.query_point(lon, lat));
            return;
        }
        for bx in cx - r..=cx + r {
            for by in cy - r..=cy + r {
                let on_ring = bx == cx - r || bx == cx + r || by == cy - r || by == cy + r;
                if !on_ring {
                    continue;
                }
                if let Some(v) = self.buckets.get(&(bx, by)) {
                    out.extend_from_slice(v);
                }
            }
        }
    }

    /// First candidate (insertion order) containing the point.
    #[must_use]
    pub fn contains(&self, lon: f64, lat: f64) -> Option<u32> {
        self.query_point(lon, lat).iter().copied().find(|&id| {
            let b = &self.bboxes[id as usize];
            // Outside the bbox no ring can contain the point (same answer, cheaper).
            lon >= b[0]
                && lon <= b[2]
                && lat >= b[1]
                && lat <= b[3]
                && point_in_geometry(lon, lat, &self.geoms[id as usize])
        })
    }

    /// v1 `nearestWithin`: nearest polygon boundary within `max_m`.
    ///
    /// Same result as v1 (first candidate wins ties, strict `<`), with two
    /// shortcuts that cannot change it: a polygon already measured is not
    /// measured again (it would give the same distance, never strictly
    /// smaller), and a polygon whose bbox is already at least as far as the
    /// best distance is skipped.
    #[must_use]
    pub fn nearest_within(&self, lon: f64, lat: f64, max_m: f64) -> Option<(u32, f64)> {
        let m_per_deg = 111_320.0;
        #[allow(clippy::cast_possible_truncation)]
        let max_radius = (max_m / (self.cell * m_per_deg)).ceil() as i64 + 1;
        let mut best: Option<(u32, f64)> = None;
        let mut cands = Vec::new();
        let mut seen: Vec<u32> = Vec::new();
        for r in 0..=max_radius {
            self.query_ring(lon, lat, r, &mut cands);
            for &id in &cands {
                if seen.contains(&id) {
                    continue;
                }
                seen.push(id);
                if let Some((_, bd)) = best
                    && bbox_distance_m(lon, lat, &self.bboxes[id as usize]) * (1.0 - 1e-12) >= bd
                {
                    continue;
                }
                let d = min_boundary_distance_m(lon, lat, &self.geoms[id as usize]);
                if best.is_none_or(|(_, bd)| d < bd) {
                    best = Some((id, d));
                }
            }
            if let Some((_, bd)) = best {
                #[allow(clippy::cast_precision_loss)]
                let floor = r as f64 * self.cell * m_per_deg;
                if floor >= bd {
                    break;
                }
            }
        }
        best.filter(|&(_, d)| d <= max_m)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(x0: f64, y0: f64, s: f64) -> MultiPolygon {
        MultiPolygon(vec![vec![vec![
            [x0, y0],
            [x0 + s, y0],
            [x0 + s, y0 + s],
            [x0, y0 + s],
            [x0, y0],
        ]]])
    }

    #[test]
    fn pip_with_holes() {
        let mut g = square(80.0, 7.0, 1.0);
        g.0[0].push(vec![
            [80.4, 7.4],
            [80.6, 7.4],
            [80.6, 7.6],
            [80.4, 7.6],
            [80.4, 7.4],
        ]);
        assert!(point_in_geometry(80.1, 7.1, &g));
        assert!(!point_in_geometry(80.5, 7.5, &g));
        assert!(!point_in_geometry(81.5, 7.5, &g));
    }

    #[test]
    fn wkb_round_trip() {
        // Little-endian Polygon((0 0, 1 0, 1 1, 0 0)).
        let mut b = vec![1u8];
        b.extend(3u32.to_le_bytes());
        b.extend(1u32.to_le_bytes());
        b.extend(4u32.to_le_bytes());
        for (x, y) in [(0.0f64, 0.0f64), (1.0, 0.0), (1.0, 1.0), (0.0, 0.0)] {
            b.extend(x.to_le_bytes());
            b.extend(y.to_le_bytes());
        }
        let g = MultiPolygon::from_wkb(&b).expect("wkb");
        assert_eq!(g.vertices(), 4);
        // Big-endian MultiPolygon wrapping it.
        let mut m = vec![0u8];
        m.extend(6u32.to_be_bytes());
        m.extend(1u32.to_be_bytes());
        m.push(0);
        m.extend(3u32.to_be_bytes());
        m.extend(1u32.to_be_bytes());
        m.extend(4u32.to_be_bytes());
        for (x, y) in [(0.0f64, 0.0f64), (1.0, 0.0), (1.0, 1.0), (0.0, 0.0)] {
            m.extend(x.to_be_bytes());
            m.extend(y.to_be_bytes());
        }
        assert_eq!(MultiPolygon::from_wkb(&m).expect("wkb"), g);
        assert!(MultiPolygon::from_wkb(&m[..m.len() - 1]).is_err());
    }

    #[test]
    fn index_contains_and_nearest() {
        let mut idx = PolygonIndex::new(0.03);
        let a = idx.insert(square(80.0, 7.0, 0.05));
        let b = idx.insert(square(80.05, 7.0, 0.05));
        assert_eq!(idx.contains(80.01, 7.01), Some(a));
        assert_eq!(idx.contains(80.07, 7.01), Some(b));
        assert_eq!(idx.contains(80.2, 7.01), None);
        // 0.005° east of b ≈ 552 m.
        let (id, d) = idx.nearest_within(80.105, 7.02, 2000.0).expect("near");
        assert_eq!(id, b);
        assert!((d - 552.0).abs() < 5.0, "{d}");
        assert!(idx.nearest_within(80.3, 7.02, 2000.0).is_none());
    }
}
