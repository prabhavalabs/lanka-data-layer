//! Minimal planar geometry for feature standardisation (WP-04): GeoJSON
//! geometries in WGS84 degrees, written with at most 6 decimals (the
//! platform's coordinate contract), representative points, OSM ring
//! assembly and a fast point-in-coverage test.
//!
//! Deliberately small: no projections and no topology repair — PostGIS
//! validates and repairs at staging (`ST_MakeValid`), and metric lengths and
//! areas are computed there (`::geography`). Everything here is pure and
//! deterministic so `standardise` stays pure.

use std::fmt::Write as _;

use anyhow::{Context, Result, bail};
use serde_json::Value;

/// `[lon, lat]`.
pub type Coord = [f64; 2];

#[derive(Debug, Clone, PartialEq)]
pub enum Geometry {
    Point(Coord),
    MultiPoint(Vec<Coord>),
    LineString(Vec<Coord>),
    MultiLineString(Vec<Vec<Coord>>),
    /// Rings: outer first, then holes. Rings are closed (first == last).
    Polygon(Vec<Vec<Coord>>),
    MultiPolygon(Vec<Vec<Vec<Coord>>>),
}

/// Rounds to 6 decimals (≈ 0.11 m), mapping `-0` to `0`.
#[must_use]
pub fn round6(v: f64) -> f64 {
    round_dp(v, 1e6)
}

fn round_dp(v: f64, scale: f64) -> f64 {
    let r = (v * scale).round() / scale;
    if r == 0.0 { 0.0 } else { r }
}

fn coord(v: &Value) -> Result<Coord> {
    let a = v.as_array().context("coordinate is not an array")?;
    let x = a.first().and_then(Value::as_f64).context("coordinate x")?;
    let y = a.get(1).and_then(Value::as_f64).context("coordinate y")?;
    if !x.is_finite() || !y.is_finite() {
        bail!("non-finite coordinate");
    }
    Ok([x, y])
}

fn coords(v: &Value) -> Result<Vec<Coord>> {
    v.as_array()
        .context("coordinates are not an array")?
        .iter()
        .map(coord)
        .collect()
}

fn rings(v: &Value) -> Result<Vec<Vec<Coord>>> {
    v.as_array()
        .context("rings are not an array")?
        .iter()
        .map(coords)
        .collect()
}

impl Geometry {
    /// Parses a GeoJSON geometry object (not a Feature, not a collection).
    pub fn from_geojson(v: &Value) -> Result<Self> {
        let t = v
            .get("type")
            .and_then(Value::as_str)
            .context("geometry without type")?;
        let c = v
            .get("coordinates")
            .context("geometry without coordinates")?;
        Ok(match t {
            "Point" => Self::Point(coord(c)?),
            "MultiPoint" => Self::MultiPoint(coords(c)?),
            "LineString" => Self::LineString(coords(c)?),
            "MultiLineString" => Self::MultiLineString(rings(c)?),
            "Polygon" => Self::Polygon(rings(c)?),
            "MultiPolygon" => Self::MultiPolygon(
                c.as_array()
                    .context("polygons are not an array")?
                    .iter()
                    .map(rings)
                    .collect::<Result<_>>()?,
            ),
            other => bail!("unsupported geometry type {other:?}"),
        })
    }

    pub fn parse(text: &str) -> Result<Self> {
        Self::from_geojson(&serde_json::from_str(text).context("geometry is not JSON")?)
    }

    #[must_use]
    pub const fn type_name(&self) -> &'static str {
        match self {
            Self::Point(_) => "Point",
            Self::MultiPoint(_) => "MultiPoint",
            Self::LineString(_) => "LineString",
            Self::MultiLineString(_) => "MultiLineString",
            Self::Polygon(_) => "Polygon",
            Self::MultiPolygon(_) => "MultiPolygon",
        }
    }

    /// `point`, `line` or `polygon`.
    #[must_use]
    pub const fn class(&self) -> &'static str {
        match self {
            Self::Point(_) | Self::MultiPoint(_) => "point",
            Self::LineString(_) | Self::MultiLineString(_) => "line",
            Self::Polygon(_) | Self::MultiPolygon(_) => "polygon",
        }
    }

    /// GeoJSON text with every coordinate rounded to 6 decimals and
    /// consecutive duplicate vertices (after rounding) removed. Deterministic:
    /// the same geometry always prints the same bytes.
    #[must_use]
    pub fn to_geojson6(&self) -> String {
        self.to_geojson_scaled(1e6)
    }

    /// GeoJSON text with 7 decimals (OSM's native precision; L1 extracts).
    #[must_use]
    pub fn to_geojson7(&self) -> String {
        self.to_geojson_scaled(1e7)
    }

    fn to_geojson_scaled(&self, scale: f64) -> String {
        let pt = |s: &mut String, c: Coord| {
            let _ = write!(s, "[{},{}]", round_dp(c[0], scale), round_dp(c[1], scale));
        };
        let line = |s: &mut String, l: &[Coord]| {
            s.push('[');
            let mut last: Option<Coord> = None;
            let mut first = true;
            for &c in l {
                let r = [round_dp(c[0], scale), round_dp(c[1], scale)];
                if last == Some(r) {
                    continue;
                }
                if !first {
                    s.push(',');
                }
                first = false;
                pt(s, r);
                last = Some(r);
            }
            s.push(']');
        };
        let many = |s: &mut String, ls: &[Vec<Coord>]| {
            s.push('[');
            for (i, l) in ls.iter().enumerate() {
                if i > 0 {
                    s.push(',');
                }
                line(s, l);
            }
            s.push(']');
        };
        let mut s = format!("{{\"type\":\"{}\",\"coordinates\":", self.type_name());
        match self {
            Self::Point(c) => pt(&mut s, *c),
            Self::MultiPoint(cs) => {
                s.push('[');
                for (i, c) in cs.iter().enumerate() {
                    if i > 0 {
                        s.push(',');
                    }
                    pt(&mut s, *c);
                }
                s.push(']');
            }
            Self::LineString(l) => line(&mut s, l),
            Self::MultiLineString(ls) | Self::Polygon(ls) => many(&mut s, ls),
            Self::MultiPolygon(ps) => {
                s.push('[');
                for (i, p) in ps.iter().enumerate() {
                    if i > 0 {
                        s.push(',');
                    }
                    many(&mut s, p);
                }
                s.push(']');
            }
        }
        s.push('}');
        s
    }

    /// `[west, south, east, north]`.
    #[must_use]
    pub fn bbox(&self) -> [f64; 4] {
        let mut b = [
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        ];
        self.for_each_coord(|c| {
            b[0] = b[0].min(c[0]);
            b[1] = b[1].min(c[1]);
            b[2] = b[2].max(c[0]);
            b[3] = b[3].max(c[1]);
        });
        b
    }

    fn for_each_coord(&self, mut f: impl FnMut(Coord)) {
        match self {
            Self::Point(c) => f(*c),
            Self::MultiPoint(cs) | Self::LineString(cs) => cs.iter().copied().for_each(f),
            Self::MultiLineString(ls) | Self::Polygon(ls) => {
                ls.iter().flatten().copied().for_each(f);
            }
            Self::MultiPolygon(ps) => ps.iter().flatten().flatten().copied().for_each(f),
        }
    }

    /// Number of vertices.
    #[must_use]
    pub fn vertices(&self) -> usize {
        let mut n = 0;
        self.for_each_coord(|_| n += 1);
        n
    }

    /// A point on the geometry: the point itself, the midpoint along a line
    /// (the longest member of a multi-line), an interior point of a polygon
    /// (the largest member of a multi-polygon). `None` for empty geometry.
    #[must_use]
    pub fn rep_point(&self) -> Option<Coord> {
        match self {
            Self::Point(c) => Some(*c),
            Self::MultiPoint(cs) => cs.first().copied(),
            Self::LineString(l) => line_midpoint(l),
            Self::MultiLineString(ls) => ls
                .iter()
                .max_by(|a, b| line_length(a).total_cmp(&line_length(b)))
                .and_then(|l| line_midpoint(l)),
            Self::Polygon(rs) => interior_point(rs),
            Self::MultiPolygon(ps) => ps
                .iter()
                .max_by(|a, b| polygon_area(a).total_cmp(&polygon_area(b)))
                .and_then(|p| interior_point(p)),
        }
    }

    /// Planar length in degrees (lines only; 0 otherwise).
    #[must_use]
    pub fn length_deg(&self) -> f64 {
        match self {
            Self::LineString(l) => line_length(l),
            Self::MultiLineString(ls) => ls.iter().map(|l| line_length(l)).sum(),
            _ => 0.0,
        }
    }

    /// Planar area in square degrees (polygons only; holes subtracted).
    #[must_use]
    pub fn area_deg2(&self) -> f64 {
        match self {
            Self::Polygon(rs) => polygon_area(rs),
            Self::MultiPolygon(ps) => ps.iter().map(|p| polygon_area(p)).sum(),
            _ => 0.0,
        }
    }
}

/// Sum of |dx| + |dy| along a ring — the "rough size" v1 used to drop small
/// unnamed water bodies (`ceylon-hub/scripts/data/fetch-osm-features.mjs`).
#[must_use]
pub fn rough_ring_size(ring: &[Coord]) -> f64 {
    ring.windows(2)
        .map(|w| (w[1][0] - w[0][0]).abs() + (w[1][1] - w[0][1]).abs())
        .sum()
}

fn seg_len(a: Coord, b: Coord) -> f64 {
    (b[0] - a[0]).hypot(b[1] - a[1])
}

fn line_length(l: &[Coord]) -> f64 {
    l.windows(2).map(|w| seg_len(w[0], w[1])).sum()
}

fn line_midpoint(l: &[Coord]) -> Option<Coord> {
    let first = *l.first()?;
    let half = line_length(l) / 2.0;
    if half <= 0.0 {
        return Some(first);
    }
    let mut acc = 0.0;
    for w in l.windows(2) {
        let d = seg_len(w[0], w[1]);
        if acc + d >= half && d > 0.0 {
            let t = (half - acc) / d;
            return Some([
                w[0][0] + t * (w[1][0] - w[0][0]),
                w[0][1] + t * (w[1][1] - w[0][1]),
            ]);
        }
        acc += d;
    }
    l.last().copied()
}

fn ring_area_signed(r: &[Coord]) -> f64 {
    r.windows(2)
        .map(|w| w[0][0] * w[1][1] - w[1][0] * w[0][1])
        .sum::<f64>()
        / 2.0
}

fn polygon_area(rs: &[Vec<Coord>]) -> f64 {
    let mut it = rs.iter();
    let outer = it.next().map_or(0.0, |r| ring_area_signed(r).abs());
    outer - it.map(|r| ring_area_signed(r).abs()).sum::<f64>()
}

/// Interior point of a polygon (GEOS `InteriorPointArea`): a horizontal scan
/// line through the middle of the shape, placed between vertex latitudes so
/// it never passes through a vertex; the midpoint of the widest interior
/// interval. Falls back to the first vertex for degenerate rings.
fn interior_point(rs: &[Vec<Coord>]) -> Option<Coord> {
    let outer = rs.first()?;
    let first = *outer.first()?;
    let (mut ymin, mut ymax) = (f64::INFINITY, f64::NEG_INFINITY);
    for c in outer {
        ymin = ymin.min(c[1]);
        ymax = ymax.max(c[1]);
    }
    let centre = (ymin + ymax) / 2.0;
    // Nearest vertex latitudes strictly below and above the centre.
    let (mut lo, mut hi) = (ymin, ymax);
    for c in rs.iter().flatten() {
        let y = c[1];
        if y <= centre && y > lo {
            lo = y;
        }
        if y > centre && y < hi {
            hi = y;
        }
    }
    let scan = (lo + hi) / 2.0;
    let mut xs: Vec<f64> = Vec::new();
    for r in rs {
        for w in r.windows(2) {
            let (a, b) = (w[0], w[1]);
            if (a[1] <= scan) != (b[1] <= scan) {
                xs.push(a[0] + (scan - a[1]) * (b[0] - a[0]) / (b[1] - a[1]));
            }
        }
    }
    xs.sort_by(f64::total_cmp);
    let mut best: Option<(f64, f64)> = None;
    for pair in xs.as_chunks::<2>().0 {
        let w = pair[1] - pair[0];
        if best.is_none_or(|(bw, _)| w > bw) {
            best = Some((w, (pair[0] + pair[1]) / 2.0));
        }
    }
    Some(best.map_or(first, |(_, x)| [x, scan]))
}

/// Even-odd point-in-ring test over several rings (holes and islands alike).
#[must_use]
pub fn point_in_rings(p: Coord, rs: &[Vec<Coord>]) -> bool {
    let mut inside = false;
    for r in rs {
        for w in r.windows(2) {
            let (a, b) = (w[0], w[1]);
            if (a[1] > p[1]) != (b[1] > p[1])
                && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0]
            {
                inside = !inside;
            }
        }
    }
    inside
}

// ───────────────────────────── OSM ring assembly ───────────────────────────

/// A way's node ids and coordinates, as read from the PBF.
#[derive(Debug, Clone)]
pub struct WayLine<'a> {
    pub nodes: &'a [i64],
    pub coords: &'a [Coord],
}

/// Joins ways end to end (by node id) into closed rings. Ways that cannot
/// be closed are reported as an error (the relation is broken upstream).
pub fn assemble_rings(ways: &[WayLine<'_>]) -> Result<Vec<Vec<Coord>>> {
    let mut used = vec![false; ways.len()];
    let mut out = Vec::new();
    for start in 0..ways.len() {
        if used[start] || ways[start].nodes.len() < 2 {
            continue;
        }
        used[start] = true;
        let mut ids: Vec<i64> = ways[start].nodes.to_vec();
        let mut cs: Vec<Coord> = ways[start].coords.to_vec();
        while ids.first() != ids.last() {
            let end = *ids.last().context("empty way")?;
            let mut found = false;
            for (j, w) in ways.iter().enumerate() {
                if used[j] || w.nodes.len() < 2 {
                    continue;
                }
                if w.nodes.first() == Some(&end) {
                    ids.extend_from_slice(&w.nodes[1..]);
                    cs.extend_from_slice(&w.coords[1..]);
                } else if w.nodes.last() == Some(&end) {
                    ids.extend(w.nodes.iter().rev().skip(1));
                    cs.extend(w.coords.iter().rev().skip(1));
                } else {
                    continue;
                }
                used[j] = true;
                found = true;
                break;
            }
            if !found {
                bail!("ring does not close (open at node {end})");
            }
        }
        if cs.len() >= 4 {
            out.push(cs);
        }
    }
    Ok(out)
}

/// Builds polygons from outer and inner rings: each inner ring goes to the
/// smallest outer ring containing its first vertex (inner rings outside every
/// outer are dropped).
#[must_use]
pub fn polygons_from_rings(outers: Vec<Vec<Coord>>, inners: Vec<Vec<Coord>>) -> Geometry {
    let mut polys: Vec<Vec<Vec<Coord>>> = outers.into_iter().map(|o| vec![o]).collect();
    let areas: Vec<f64> = polys
        .iter()
        .map(|p| ring_area_signed(&p[0]).abs())
        .collect();
    for inner in inners {
        let Some(p) = inner.first().copied() else {
            continue;
        };
        let target = polys
            .iter()
            .enumerate()
            .filter(|(_, poly)| point_in_rings(p, &poly[..1]))
            .min_by(|a, b| areas[a.0].total_cmp(&areas[b.0]))
            .map(|(i, _)| i);
        if let Some(i) = target {
            polys[i].push(inner);
        }
    }
    if polys.len() == 1 {
        Geometry::Polygon(polys.remove(0))
    } else {
        Geometry::MultiPolygon(polys)
    }
}

// ───────────────────────────── coverage ────────────────────────────────────

/// Point-in-area test against a (multi)polygon with many vertices, indexed
/// by latitude bands so each test looks at the few edges crossing its band.
#[derive(Debug, Clone)]
pub struct Coverage {
    ymin: f64,
    band: f64,
    /// Edges `(ax, ay, bx, by)` per band.
    bands: Vec<Vec<[f64; 4]>>,
    bbox: [f64; 4],
}

impl Coverage {
    /// From a Polygon / MultiPolygon GeoJSON geometry.
    pub fn from_geometry(g: &Geometry) -> Result<Self> {
        let rs: Vec<&Vec<Coord>> = match g {
            Geometry::Polygon(rs) => rs.iter().collect(),
            Geometry::MultiPolygon(ps) => ps.iter().flatten().collect(),
            other => bail!("coverage must be a polygon, not {}", other.type_name()),
        };
        let bbox = g.bbox();
        let n_bands = 512usize;
        let height = (bbox[3] - bbox[1]).max(1e-9);
        #[allow(clippy::cast_precision_loss)]
        let band = height / n_bands as f64;
        let mut bands = vec![Vec::new(); n_bands];
        for r in rs {
            for w in r.windows(2) {
                let (a, b) = (w[0], w[1]);
                if a[1] == b[1] {
                    continue;
                }
                let (y0, y1) = (a[1].min(b[1]), a[1].max(b[1]));
                let i0 = Self::band_of(bbox[1], band, n_bands, y0);
                let i1 = Self::band_of(bbox[1], band, n_bands, y1);
                for e in &mut bands[i0..=i1] {
                    e.push([a[0], a[1], b[0], b[1]]);
                }
            }
        }
        Ok(Self {
            ymin: bbox[1],
            band,
            bands,
            bbox,
        })
    }

    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    fn band_of(ymin: f64, band: f64, n: usize, y: f64) -> usize {
        (((y - ymin) / band).floor().max(0.0) as usize).min(n - 1)
    }

    #[must_use]
    pub fn contains(&self, p: Coord) -> bool {
        if p[0] < self.bbox[0] || p[0] > self.bbox[2] || p[1] < self.bbox[1] || p[1] > self.bbox[3]
        {
            return false;
        }
        let i = Self::band_of(self.ymin, self.band, self.bands.len(), p[1]);
        let mut inside = false;
        for e in &self.bands[i] {
            if (e[1] > p[1]) != (e[3] > p[1])
                && p[0] < (e[2] - e[0]) * (p[1] - e[1]) / (e[3] - e[1]) + e[0]
            {
                inside = !inside;
            }
        }
        inside
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sq(x0: f64, y0: f64, d: f64) -> Vec<Coord> {
        vec![
            [x0, y0],
            [x0 + d, y0],
            [x0 + d, y0 + d],
            [x0, y0 + d],
            [x0, y0],
        ]
    }

    #[test]
    fn rounds_to_six_decimals_and_drops_repeats() {
        let g = Geometry::LineString(vec![
            [79.123_456_78, 6.000_000_04],
            [79.123_456_81, 6.000_000_01],
            [80.0, -0.000_000_1],
        ]);
        assert_eq!(
            g.to_geojson6(),
            r#"{"type":"LineString","coordinates":[[79.123457,6],[80,0]]}"#
        );
        let back = Geometry::parse(&g.to_geojson6()).expect("parse");
        assert_eq!(back.to_geojson6(), g.to_geojson6());
    }

    #[test]
    fn representative_points() {
        let l = Geometry::LineString(vec![[0.0, 0.0], [2.0, 0.0], [2.0, 2.0]]);
        assert_eq!(l.rep_point(), Some([2.0, 0.0]));
        // A U shape: the centroid lies outside; the interior point must not.
        let u = vec![
            [0.0, 0.0],
            [3.0, 0.0],
            [3.0, 3.0],
            [2.0, 3.0],
            [2.0, 1.0],
            [1.0, 1.0],
            [1.0, 3.0],
            [0.0, 3.0],
            [0.0, 0.0],
        ];
        let p = Geometry::Polygon(vec![u.clone()])
            .rep_point()
            .expect("point");
        assert!(point_in_rings(p, &[u]), "{p:?}");
        let with_hole = vec![sq(0.0, 0.0, 4.0), sq(1.0, 1.0, 2.0)];
        let p = Geometry::Polygon(with_hole.clone())
            .rep_point()
            .expect("point");
        assert!(point_in_rings(p, &with_hole), "{p:?}");
    }

    #[test]
    fn assembles_rings_from_split_ways() {
        // A square split into two ways, the second reversed.
        let (n1, c1) = (vec![1, 2, 3], vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0]]);
        let (n2, c2) = (vec![1, 4, 3], vec![[0.0, 0.0], [0.0, 1.0], [1.0, 1.0]]);
        let rings = assemble_rings(&[
            WayLine {
                nodes: &n1,
                coords: &c1,
            },
            WayLine {
                nodes: &n2,
                coords: &c2,
            },
        ])
        .expect("rings");
        assert_eq!(rings.len(), 1);
        assert_eq!(rings[0].first(), rings[0].last());
        assert!((polygon_area(&rings) - 1.0).abs() < 1e-12);
        let open = assemble_rings(&[WayLine {
            nodes: &n1,
            coords: &c1,
        }]);
        assert!(open.is_err());
        let g = polygons_from_rings(
            vec![sq(0.0, 0.0, 4.0), sq(10.0, 0.0, 1.0)],
            vec![sq(1.0, 1.0, 1.0)],
        );
        match g {
            Geometry::MultiPolygon(ps) => {
                assert_eq!(ps.len(), 2);
                assert_eq!(ps[0].len(), 2, "hole goes to the containing outer");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn coverage_matches_brute_force() {
        let rings = vec![sq(0.0, 0.0, 10.0), sq(2.0, 2.0, 3.0), sq(20.0, 0.0, 1.0)];
        let g = Geometry::MultiPolygon(vec![rings[..2].to_vec(), vec![rings[2].clone()]]);
        let cov = Coverage::from_geometry(&g).expect("coverage");
        let mut x = 0.123_f64;
        for _ in 0..2000 {
            x = (x * 9301.0 + 49297.0) % 233_280.0;
            let p = [
                x / 233_280.0 * 24.0 - 1.0,
                (x * 7.0 % 233_280.0) / 233_280.0 * 12.0 - 1.0,
            ];
            assert_eq!(cov.contains(p), point_in_rings(p, &rings), "{p:?}");
        }
    }
}
