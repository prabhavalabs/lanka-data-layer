//! Cloud Optimized GeoTIFF writer (classic TIFF, little-endian).
//!
//! Layout (what GDAL's `validate_cloud_optimized_geotiff.py` checks): the
//! main IFD at offset 8, the overview IFDs after it in decreasing size, then
//! the tile data from the smallest overview to the full resolution, tiles
//! row-major within each level. 512 × 512 tiles, Adobe Deflate (zlib) with
//! the horizontal predictor for integer types, overviews by halving until
//! both sides are ≤ 512. GeoKeys: geographic WGS 84 (EPSG:4326),
//! PixelIsArea; `GDAL_NODATA` when the layer has one.

use std::io::Write;
use std::path::Path;

use anyhow::{Context, Result, ensure};
use flate2::Compression;
use flate2::write::ZlibEncoder;
use serde::{Deserialize, Serialize};

use crate::array::{Dtype, GridArray};

pub const TILE: usize = 512;

/// How a coarser pixel is made from 2 × 2 finer ones (`geo.grid_layers.overview`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Overview {
    Mean,
    /// Counts: overview pixels hold the mean per cell (a density), so the
    /// rendered ramp is the same at every zoom; the sum is `mean × cells`.
    Sum,
    Max,
    /// Categories: the most frequent valid value (ties → the smaller).
    Mode,
}

impl Overview {
    pub fn parse(s: &str) -> Result<Self> {
        Ok(match s {
            "mean" => Self::Mean,
            "sum" => Self::Sum,
            "max" => Self::Max,
            "mode" => Self::Mode,
            _ => anyhow::bail!("unknown overview {s:?} (mean, sum, max, mode)"),
        })
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Mean => "mean",
            Self::Sum => "sum",
            Self::Max => "max",
            Self::Mode => "mode",
        }
    }
}

/// A level: raw values (NaN = nodata), `w × h`.
struct Level {
    w: usize,
    h: usize,
    raw: Vec<f64>,
}

fn halve(l: &Level, ov: Overview, integer: bool) -> Level {
    let (w, h) = (l.w.div_ceil(2), l.h.div_ceil(2));
    let mut raw = vec![f64::NAN; w * h];
    let mut vals = Vec::with_capacity(4);
    for y in 0..h {
        for x in 0..w {
            vals.clear();
            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                let (sx, sy) = (2 * x + dx, 2 * y + dy);
                if sx < l.w && sy < l.h {
                    let v = l.raw[sy * l.w + sx];
                    if !v.is_nan() {
                        vals.push(v);
                    }
                }
            }
            if vals.is_empty() {
                continue;
            }
            #[allow(clippy::cast_precision_loss)]
            let v = match ov {
                Overview::Mean | Overview::Sum => {
                    let m = vals.iter().sum::<f64>() / vals.len() as f64;
                    if integer { m.round() } else { m }
                }
                Overview::Max => vals.iter().copied().fold(f64::NEG_INFINITY, f64::max),
                Overview::Mode => {
                    vals.sort_by(f64::total_cmp);
                    let (mut best, mut best_n, mut i) = (vals[0], 0, 0);
                    while i < vals.len() {
                        let mut j = i;
                        while j < vals.len() && vals[j] == vals[i] {
                            j += 1;
                        }
                        if j - i > best_n {
                            best = vals[i];
                            best_n = j - i;
                        }
                        i = j;
                    }
                    best
                }
            };
            raw[y * w + x] = v;
        }
    }
    Level { w, h, raw }
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn encode_tile(l: &Level, tx: usize, ty: usize, dtype: Dtype, fill: f64) -> Result<Vec<u8>> {
    let mut samples = vec![fill; TILE * TILE];
    for y in 0..TILE {
        let sy = ty * TILE + y;
        if sy >= l.h {
            break;
        }
        for x in 0..TILE {
            let sx = tx * TILE + x;
            if sx >= l.w {
                break;
            }
            let v = l.raw[sy * l.w + sx];
            samples[y * TILE + x] = if v.is_nan() { fill } else { v };
        }
    }
    let mut bytes = Vec::with_capacity(TILE * TILE * dtype.size());
    for row in samples.chunks(TILE) {
        match dtype {
            Dtype::U8 => {
                let mut prev = 0u8;
                for &v in row {
                    let c = v as u8;
                    bytes.push(c.wrapping_sub(prev));
                    prev = c;
                }
            }
            Dtype::U16 => {
                let mut prev = 0u16;
                for &v in row {
                    let c = v as u16;
                    bytes.extend(c.wrapping_sub(prev).to_le_bytes());
                    prev = c;
                }
            }
            Dtype::I16 => {
                let mut prev = 0i16;
                for &v in row {
                    let c = v as i16;
                    bytes.extend(c.wrapping_sub(prev).to_le_bytes());
                    prev = c;
                }
            }
            Dtype::U32 => {
                let mut prev = 0u32;
                for &v in row {
                    let c = v as u32;
                    bytes.extend(c.wrapping_sub(prev).to_le_bytes());
                    prev = c;
                }
            }
            Dtype::F32 => {
                for &v in row {
                    bytes.extend((v as f32).to_le_bytes());
                }
            }
        }
    }
    let mut z = ZlibEncoder::new(Vec::new(), Compression::new(6));
    z.write_all(&bytes)?;
    Ok(z.finish()?)
}

/// One IFD entry: tag, type, count, value bytes (little-endian).
struct Entry {
    tag: u16,
    typ: u16,
    count: u32,
    data: Vec<u8>,
}

const SHORT: u16 = 3;
const LONG: u16 = 4;
const ASCII: u16 = 2;
const DOUBLE: u16 = 12;

fn short(tag: u16, v: &[u16]) -> Entry {
    Entry {
        tag,
        typ: SHORT,
        count: u32::try_from(v.len()).unwrap_or(u32::MAX),
        data: v.iter().flat_map(|x| x.to_le_bytes()).collect(),
    }
}
fn long(tag: u16, v: &[u32]) -> Entry {
    Entry {
        tag,
        typ: LONG,
        count: u32::try_from(v.len()).unwrap_or(u32::MAX),
        data: v.iter().flat_map(|x| x.to_le_bytes()).collect(),
    }
}
fn double(tag: u16, v: &[f64]) -> Entry {
    Entry {
        tag,
        typ: DOUBLE,
        count: u32::try_from(v.len()).unwrap_or(u32::MAX),
        data: v.iter().flat_map(|x| x.to_le_bytes()).collect(),
    }
}
fn ascii(tag: u16, s: &str) -> Entry {
    let mut d = s.as_bytes().to_vec();
    d.push(0);
    Entry {
        tag,
        typ: ASCII,
        count: u32::try_from(d.len()).unwrap_or(u32::MAX),
        data: d,
    }
}

fn ifd_size(entries: &[Entry]) -> usize {
    let mut n = 2 + 12 * entries.len() + 4;
    for e in entries {
        if e.data.len() > 4 {
            n += e.data.len().next_multiple_of(2);
        }
    }
    n.next_multiple_of(2)
}

/// Writes `arr` as a COG. Returns the file size.
pub fn write_cog(path: &Path, arr: &GridArray, ov: Overview) -> Result<u64> {
    let g = arr.grid;
    let (w, h) = (g.cols as usize, g.rows as usize);
    let dtype = arr.enc.dtype;
    let integer = dtype != Dtype::F32;
    let fill = arr
        .enc
        .nodata
        .unwrap_or(if integer { 0.0 } else { f64::NAN });
    let raw: Vec<f64> = (0..w * h)
        .map(|i| {
            let r = arr.data.raw(i);
            if r.is_nan() || arr.enc.nodata == Some(r) {
                f64::NAN
            } else {
                r
            }
        })
        .collect();
    let mut levels = vec![Level { w, h, raw }];
    while levels.last().is_some_and(|l| l.w > TILE || l.h > TILE) {
        let next = halve(levels.last().context("level")?, ov, integer);
        levels.push(next);
    }

    // Compress every tile of every level.
    let mut tiles: Vec<Vec<Vec<u8>>> = Vec::with_capacity(levels.len());
    for l in &levels {
        let (ta, td) = (l.w.div_ceil(TILE), l.h.div_ceil(TILE));
        let mut v = Vec::with_capacity(ta * td);
        for ty in 0..td {
            for tx in 0..ta {
                v.push(encode_tile(l, tx, ty, dtype, fill)?);
            }
        }
        tiles.push(v);
    }

    let (bits, fmt) = match dtype {
        Dtype::U8 => (8u16, 1u16),
        Dtype::U16 => (16, 1),
        Dtype::I16 => (16, 2),
        Dtype::U32 => (32, 1),
        Dtype::F32 => (32, 3),
    };
    let predictor = if integer { 2 } else { 1 };

    // Data offsets depend on the IFD sizes, which do not depend on the
    // offsets' values: build the entries with placeholder offsets first.
    let build = |li: usize, offsets: &[u32]| -> Result<Vec<Entry>> {
        let l = &levels[li];
        let counts: Vec<u32> = tiles[li]
            .iter()
            .map(|t| u32::try_from(t.len()))
            .collect::<std::result::Result<_, _>>()?;
        let mut e = Vec::new();
        if li > 0 {
            e.push(long(254, &[1]));
        }
        e.push(long(256, &[u32::try_from(l.w)?]));
        e.push(long(257, &[u32::try_from(l.h)?]));
        e.push(short(258, &[bits]));
        e.push(short(259, &[8]));
        e.push(short(262, &[1]));
        e.push(short(277, &[1]));
        e.push(short(284, &[1]));
        e.push(short(317, &[predictor]));
        e.push(short(322, &[u16::try_from(TILE)?]));
        e.push(short(323, &[u16::try_from(TILE)?]));
        e.push(long(324, offsets));
        e.push(long(325, &counts));
        e.push(short(339, &[fmt]));
        if li == 0 {
            e.push(double(33550, &[g.step, g.step, 0.0]));
            e.push(double(33922, &[0.0, 0.0, 0.0, g.lon_min, g.lat_max, 0.0]));
            e.push(short(
                34735,
                &[
                    1, 1, 0, 4, 1024, 0, 1, 2, 1025, 0, 1, 1, 2048, 0, 1, 4326, 2054, 0, 1, 9102,
                ],
            ));
        }
        if let Some(n) = arr.enc.nodata {
            e.push(ascii(42113, &format_nodata(n)));
        }
        Ok(e)
    };
    let mut sizes = Vec::with_capacity(levels.len());
    for (li, t) in tiles.iter().enumerate() {
        sizes.push(ifd_size(&build(li, &vec![0u32; t.len()])?));
    }
    let ifds_end = 8 + sizes.iter().sum::<usize>();
    // Tile data: smallest overview first, full resolution last.
    let mut offsets: Vec<Vec<u32>> = vec![Vec::new(); levels.len()];
    let mut at = ifds_end;
    for li in (0..levels.len()).rev() {
        for t in &tiles[li] {
            offsets[li].push(u32::try_from(at).context("COG larger than 4 GiB")?);
            at += t.len();
        }
    }
    ensure!(u32::try_from(at).is_ok(), "COG larger than 4 GiB");

    let mut out: Vec<u8> = Vec::with_capacity(at);
    out.extend(b"II");
    out.extend(42u16.to_le_bytes());
    out.extend(8u32.to_le_bytes());
    let mut ifd_at = 8usize;
    for li in 0..levels.len() {
        let entries = build(li, &offsets[li])?;
        let next = if li + 1 < levels.len() {
            ifd_at + sizes[li]
        } else {
            0
        };
        write_ifd(&mut out, ifd_at, &entries, next)?;
        ensure!(out.len() == ifd_at + sizes[li], "IFD size mismatch");
        ifd_at += sizes[li];
    }
    for li in (0..levels.len()).rev() {
        for t in &tiles[li] {
            out.extend_from_slice(t);
        }
    }
    let mut f =
        std::fs::File::create(path).with_context(|| format!("creating {}", path.display()))?;
    f.write_all(&out)?;
    f.sync_all()?;
    Ok(out.len() as u64)
}

fn write_ifd(out: &mut Vec<u8>, at: usize, entries: &[Entry], next: usize) -> Result<()> {
    ensure!(out.len() == at, "IFD written out of place");
    let mut extra_at = at + 2 + 12 * entries.len() + 4;
    let mut extra = Vec::new();
    out.extend(u16::try_from(entries.len())?.to_le_bytes());
    for e in entries {
        out.extend(e.tag.to_le_bytes());
        out.extend(e.typ.to_le_bytes());
        out.extend(e.count.to_le_bytes());
        if e.data.len() <= 4 {
            let mut v = e.data.clone();
            v.resize(4, 0);
            out.extend(v);
        } else {
            out.extend(u32::try_from(extra_at)?.to_le_bytes());
            let mut d = e.data.clone();
            if d.len() % 2 == 1 {
                d.push(0);
            }
            extra_at += d.len();
            extra.extend(d);
        }
    }
    out.extend(u32::try_from(next)?.to_le_bytes());
    out.extend(extra);
    if out.len() % 2 == 1 {
        out.push(0);
    }
    Ok(())
}

fn format_nodata(n: f64) -> String {
    if n.is_nan() {
        "nan".to_owned()
    } else if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{n:.0}")
    } else {
        format!("{n}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::array::LayerEncoding;
    use crate::geotiff::GeoTiff;
    use lanka_core::grid::Grid;

    #[test]
    fn cog_reads_back_with_the_geotiff_reader() {
        let g = Grid {
            lat_max: 10.0,
            lon_min: 79.4,
            step: 0.001,
            rows: 700,
            cols: 600,
        };
        #[allow(clippy::cast_precision_loss)]
        let vals: Vec<f32> = (0..g.rows * g.cols)
            .map(|i| {
                if i % 97 == 0 {
                    f32::NAN
                } else {
                    (i % 1000) as f32
                }
            })
            .collect();
        let enc = LayerEncoding {
            dtype: Dtype::U16,
            scale: 1.0,
            offset: 0.0,
            nodata: Some(65535.0),
        };
        let a = GridArray::quantise(g, enc, &vals).expect("q");
        let dir = std::env::temp_dir().join(format!("lanka-grid-cog-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let p = dir.join("t.tif");
        write_cog(&p, &a, Overview::Mean).expect("cog");
        let mut t = GeoTiff::open(&p).expect("open");
        assert_eq!(t.georef.width, 600);
        assert_eq!(t.georef.height, 700);
        assert_eq!(t.georef.x0, 79.4);
        assert_eq!(t.georef.y0, 10.0);
        assert_eq!(t.nodata, Some(65535.0));
        let b = t.read_all().expect("read");
        for i in [0usize, 1, 2, 511, 512, 600 * 513 + 7, 419_999] {
            let want = if i % 97 == 0 {
                65535.0
            } else {
                (i % 1000) as f32
            };
            #[allow(clippy::cast_possible_truncation)]
            let got = b.get((i % 600) as u32, (i / 600) as u32, 0);
            assert_eq!(got, want, "pixel {i}");
        }
        // Main IFD at 8, then one overview level (300 × 350 ≤ 512).
        let bytes = std::fs::read(&p).expect("bytes");
        assert_eq!(&bytes[0..8], &[b'I', b'I', 42, 0, 8, 0, 0, 0]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn mode_overview_picks_the_most_frequent() {
        let l = Level {
            w: 2,
            h: 2,
            raw: vec![40.0, 10.0, 40.0, f64::NAN],
        };
        assert_eq!(halve(&l, Overview::Mode, true).raw, vec![40.0]);
        assert_eq!(halve(&l, Overview::Max, true).raw, vec![40.0]);
        assert_eq!(halve(&l, Overview::Mean, true).raw, vec![30.0]);
    }
}
