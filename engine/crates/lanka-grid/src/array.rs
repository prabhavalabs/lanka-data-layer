//! In-memory grid arrays and their `.bin` encoding.

use std::io::Write;
use std::path::Path;

use anyhow::{Context, Result, bail, ensure};
use lanka_core::grid::Grid;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::CellValues;
use crate::header::{HEADER_LEN, Header};

/// Storage type of a layer (`geo.grid_layers.dtype`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Dtype {
    U8,
    U16,
    I16,
    U32,
    F32,
}

impl Dtype {
    #[must_use]
    pub const fn size(self) -> usize {
        match self {
            Self::U8 => 1,
            Self::U16 | Self::I16 => 2,
            Self::U32 | Self::F32 => 4,
        }
    }

    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Self::U8 => 1,
            Self::U16 => 2,
            Self::I16 => 3,
            Self::U32 => 4,
            Self::F32 => 5,
        }
    }

    pub fn from_code(c: u8) -> Result<Self> {
        Ok(match c {
            1 => Self::U8,
            2 => Self::U16,
            3 => Self::I16,
            4 => Self::U32,
            5 => Self::F32,
            _ => bail!("unknown grid dtype code {c}"),
        })
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::U8 => "u8",
            Self::U16 => "u16",
            Self::I16 => "i16",
            Self::U32 => "u32",
            Self::F32 => "f32",
        }
    }

    pub fn parse(s: &str) -> Result<Self> {
        Ok(match s {
            "u8" => Self::U8,
            "u16" => Self::U16,
            "i16" => Self::I16,
            "u32" => Self::U32,
            "f32" => Self::F32,
            _ => bail!("unknown grid dtype {s:?} (u8, u16, i16, u32, f32)"),
        })
    }

    /// Representable raw range.
    #[must_use]
    pub const fn range(self) -> (f64, f64) {
        match self {
            Self::U8 => (0.0, 255.0),
            Self::U16 => (0.0, 65_535.0),
            Self::I16 => (-32_768.0, 32_767.0),
            Self::U32 => (0.0, 4_294_967_295.0),
            Self::F32 => (f64::NEG_INFINITY, f64::INFINITY),
        }
    }
}

/// How raw values map to physical values: `physical = raw · scale + offset`;
/// `nodata` is a raw value (for `f32`, NaN is always nodata as well).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LayerEncoding {
    pub dtype: Dtype,
    pub scale: f64,
    pub offset: f64,
    pub nodata: Option<f64>,
}

impl LayerEncoding {
    #[must_use]
    pub const fn f32() -> Self {
        Self {
            dtype: Dtype::F32,
            scale: 1.0,
            offset: 0.0,
            nodata: None,
        }
    }

    /// Physical value of a raw value.
    #[must_use]
    pub fn physical(&self, raw: f64) -> Option<f64> {
        if raw.is_nan() || self.nodata == Some(raw) {
            None
        } else {
            Some(raw.mul_add(self.scale, self.offset))
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum GridData {
    U8(Vec<u8>),
    U16(Vec<u16>),
    I16(Vec<i16>),
    U32(Vec<u32>),
    F32(Vec<f32>),
}

impl GridData {
    #[must_use]
    pub fn len(&self) -> usize {
        match self {
            Self::U8(v) => v.len(),
            Self::U16(v) => v.len(),
            Self::I16(v) => v.len(),
            Self::U32(v) => v.len(),
            Self::F32(v) => v.len(),
        }
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    #[must_use]
    pub fn raw(&self, i: usize) -> f64 {
        match self {
            Self::U8(v) => f64::from(v[i]),
            Self::U16(v) => f64::from(v[i]),
            Self::I16(v) => f64::from(v[i]),
            Self::U32(v) => f64::from(v[i]),
            Self::F32(v) => f64::from(v[i]),
        }
    }

    /// Little-endian payload bytes.
    #[must_use]
    pub fn to_le_bytes(&self) -> Vec<u8> {
        match self {
            Self::U8(v) => v.clone(),
            Self::U16(v) => v.iter().flat_map(|x| x.to_le_bytes()).collect(),
            Self::I16(v) => v.iter().flat_map(|x| x.to_le_bytes()).collect(),
            Self::U32(v) => v.iter().flat_map(|x| x.to_le_bytes()).collect(),
            Self::F32(v) => v.iter().flat_map(|x| x.to_le_bytes()).collect(),
        }
    }

    pub fn from_le_bytes(dtype: Dtype, b: &[u8]) -> Result<Self> {
        ensure!(
            b.len().is_multiple_of(dtype.size()),
            "payload length not a multiple of the dtype size"
        );
        Ok(match dtype {
            Dtype::U8 => Self::U8(b.to_vec()),
            Dtype::U16 => Self::U16(
                b.as_chunks::<2>()
                    .0
                    .iter()
                    .map(|c| u16::from_le_bytes(*c))
                    .collect(),
            ),
            Dtype::I16 => Self::I16(
                b.as_chunks::<2>()
                    .0
                    .iter()
                    .map(|c| i16::from_le_bytes(*c))
                    .collect(),
            ),
            Dtype::U32 => Self::U32(
                b.as_chunks::<4>()
                    .0
                    .iter()
                    .map(|c| u32::from_le_bytes(*c))
                    .collect(),
            ),
            Dtype::F32 => Self::F32(
                b.as_chunks::<4>()
                    .0
                    .iter()
                    .map(|c| f32::from_le_bytes(*c))
                    .collect(),
            ),
        })
    }
}

/// One layer × period on a grid.
#[derive(Debug, Clone, PartialEq)]
pub struct GridArray {
    pub grid: Grid,
    pub enc: LayerEncoding,
    pub data: GridData,
}

/// Result of writing a `.bin`.
#[derive(Debug, Clone)]
pub struct Written {
    pub sha256: [u8; 32],
    pub bytes: u64,
}

impl GridArray {
    /// Quantises physical values (`NaN` = nodata) into `enc`.
    ///
    /// # Errors
    /// When a value is outside the dtype's range, or `enc` needs a nodata
    /// value to represent a NaN and has none.
    pub fn quantise(grid: Grid, enc: LayerEncoding, values: &[f32]) -> Result<Self> {
        ensure!(
            values.len() == crate::cells(&grid),
            "value count does not match the grid"
        );
        if enc.dtype == Dtype::F32 && enc.scale == 1.0 && enc.offset == 0.0 {
            let nd = enc.nodata.map(|n| n as f32);
            let v = values
                .iter()
                .map(|&x| {
                    if x.is_nan() {
                        nd.unwrap_or(f32::NAN)
                    } else {
                        x
                    }
                })
                .collect();
            return Ok(Self {
                grid,
                enc,
                data: GridData::F32(v),
            });
        }
        let (lo, hi) = enc.dtype.range();
        let mut raws = Vec::with_capacity(values.len());
        for &x in values {
            if x.is_nan() {
                let n = enc
                    .nodata
                    .context("a NaN value needs a nodata value in the layer encoding")?;
                raws.push(n);
                continue;
            }
            let mut r = (f64::from(x) - enc.offset) / enc.scale;
            if enc.dtype != Dtype::F32 {
                r = r.round();
            }
            ensure!(
                (lo..=hi).contains(&r) && Some(r) != enc.nodata,
                "value {x} does not fit {} with scale {} / offset {} (raw {r})",
                enc.dtype.as_str(),
                enc.scale,
                enc.offset
            );
            raws.push(r);
        }
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let data = match enc.dtype {
            Dtype::U8 => GridData::U8(raws.iter().map(|&r| r as u8).collect()),
            Dtype::U16 => GridData::U16(raws.iter().map(|&r| r as u16).collect()),
            Dtype::I16 => GridData::I16(raws.iter().map(|&r| r as i16).collect()),
            Dtype::U32 => GridData::U32(raws.iter().map(|&r| r as u32).collect()),
            Dtype::F32 => GridData::F32(raws.iter().map(|&r| r as f32).collect()),
        };
        Ok(Self { grid, enc, data })
    }

    /// Writes header + payload to `path` (the caller renames it into the
    /// store). The payload sha256 goes into the header.
    pub fn write_bin(&self, path: &Path, json: serde_json::Value) -> Result<Written> {
        let payload = self.data.to_le_bytes();
        let sha256: [u8; 32] = Sha256::digest(&payload).into();
        let header = Header {
            grid: self.grid,
            enc: self.enc,
            sha256,
            json,
        }
        .encode()?;
        let mut f = std::io::BufWriter::new(
            std::fs::File::create(path).with_context(|| format!("creating {}", path.display()))?,
        );
        f.write_all(&header)?;
        f.write_all(&payload)?;
        f.into_inner()
            .map_err(|e| anyhow::anyhow!("flushing {}: {e}", path.display()))?
            .sync_all()?;
        Ok(Written {
            sha256,
            bytes: (header.len() + payload.len()) as u64,
        })
    }

    /// Reads a `.bin` fully (tests, parity tools; the server maps instead).
    pub fn read_bin(path: &Path) -> Result<(Self, Header)> {
        let b = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
        let h = Header::decode(&b)?;
        let payload = &b[HEADER_LEN..];
        ensure!(
            payload.len() == h.payload_len(),
            "{}: payload length mismatch",
            path.display()
        );
        let sha: [u8; 32] = Sha256::digest(payload).into();
        ensure!(
            sha == h.sha256,
            "{}: payload sha256 mismatch",
            path.display()
        );
        let data = GridData::from_le_bytes(h.enc.dtype, payload)?;
        Ok((
            Self {
                grid: h.grid,
                enc: h.enc,
                data,
            },
            h,
        ))
    }

    /// Basic statistics over valid cells: (count, min, max, sum).
    #[must_use]
    pub fn stats(&self) -> Stats {
        let mut s = Stats::default();
        for i in 0..self.data.len() {
            if let Some(v) = self.value(i) {
                s.add(v);
            }
        }
        s
    }
}

impl CellValues for GridArray {
    fn len(&self) -> usize {
        self.data.len()
    }
    fn value(&self, i: usize) -> Option<f64> {
        self.enc.physical(self.data.raw(i))
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
pub struct Stats {
    pub count: u64,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub sum: f64,
}

impl Stats {
    pub fn add(&mut self, v: f64) {
        self.count += 1;
        self.sum += v;
        self.min = Some(self.min.map_or(v, |m| m.min(v)));
        self.max = Some(self.max.map_or(v, |m| m.max(v)));
    }

    #[must_use]
    pub fn mean(&self) -> Option<f64> {
        #[allow(clippy::cast_precision_loss)]
        (self.count > 0).then(|| self.sum / self.count as f64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn small() -> Grid {
        Grid {
            lat_max: 10.0,
            lon_min: 79.4,
            step: 0.001,
            rows: 3,
            cols: 4,
        }
    }

    #[test]
    fn quantise_and_round_trip() {
        let g = small();
        let enc = LayerEncoding {
            dtype: Dtype::I16,
            scale: 0.1,
            offset: 0.0,
            nodata: Some(-32768.0),
        };
        let vals = [
            0.0,
            1.27,
            -3.0,
            f32::NAN,
            100.04,
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
            7.0,
        ];
        let a = GridArray::quantise(g, enc, &vals).expect("quantise");
        assert_eq!(a.value(3), None);
        assert!((a.value(1).expect("v") - 1.3).abs() < 1e-9);
        assert!((a.value(4).expect("v") - 100.0).abs() < 1e-9);
        let dir = std::env::temp_dir().join(format!("lanka-grid-arr-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let p = dir.join("a.bin");
        let w = a
            .write_bin(&p, serde_json::json!({"layer": "x"}))
            .expect("write");
        assert_eq!(w.bytes, 4096 + 24);
        let (back, h) = GridArray::read_bin(&p).expect("read");
        assert_eq!(back, a);
        assert_eq!(h.sha256, w.sha256);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn out_of_range_is_rejected() {
        let enc = LayerEncoding {
            dtype: Dtype::U8,
            scale: 1.0,
            offset: 0.0,
            nodata: Some(255.0),
        };
        let mut vals = vec![0.0f32; 12];
        vals[0] = 300.0;
        assert!(GridArray::quantise(small(), enc, &vals).is_err());
        vals[0] = 255.0; // collides with nodata
        assert!(GridArray::quantise(small(), enc, &vals).is_err());
    }
}
