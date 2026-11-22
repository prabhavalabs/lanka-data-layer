//! The 4 KiB self-describing header of a grid `.bin` (05 §8).
//!
//! | offset | size | field |
//! |---|---|---|
//! | 0 | 8 | magic `LKGRID01` |
//! | 8 | 1 | dtype code (1 u8, 2 u16, 3 i16, 4 u32, 5 f32) |
//! | 9 | 1 | flags (bit 0: has nodata) |
//! | 10 | 2 | header JSON length (little-endian u16) |
//! | 12 | 4 | rows (u32) |
//! | 16 | 4 | cols (u32) |
//! | 24 | 32 | bbox west, south, east, north (f64) |
//! | 56 | 8 | step, degrees (f64) |
//! | 64 | 8 | nodata (raw value, f64; NaN when none) |
//! | 72 | 8 | scale (f64) |
//! | 80 | 8 | offset (f64) |
//! | 88 | 32 | sha256 of the payload |
//! | 120 | ≤ 3976 | JSON (layer, period, unit, …), UTF-8 |
//! | 4096 | rows·cols·size | payload, little-endian, row-major from the north-west |
//!
//! All multi-byte fields are little-endian.

use anyhow::{Result, bail, ensure};
use lanka_core::grid::Grid;

use crate::array::{Dtype, LayerEncoding};

pub const MAGIC: &[u8; 8] = b"LKGRID01";
/// Payload offset (page-aligned for mmap).
pub const HEADER_LEN: usize = 4096;
const JSON_AT: usize = 120;

#[derive(Debug, Clone, PartialEq)]
pub struct Header {
    pub grid: Grid,
    pub enc: LayerEncoding,
    pub sha256: [u8; 32],
    /// Free-form description (layer id, period, unit); not part of the payload hash.
    pub json: serde_json::Value,
}

fn f64_at(b: &[u8], at: usize) -> f64 {
    let mut a = [0u8; 8];
    a.copy_from_slice(&b[at..at + 8]);
    f64::from_le_bytes(a)
}

fn u32_at(b: &[u8], at: usize) -> u32 {
    let mut a = [0u8; 4];
    a.copy_from_slice(&b[at..at + 4]);
    u32::from_le_bytes(a)
}

impl Header {
    /// Encodes to exactly [`HEADER_LEN`] bytes.
    pub fn encode(&self) -> Result<Vec<u8>> {
        let mut b = vec![0u8; HEADER_LEN];
        b[0..8].copy_from_slice(MAGIC);
        b[8] = self.enc.dtype.code();
        b[9] = u8::from(self.enc.nodata.is_some());
        let json = serde_json::to_vec(&self.json)?;
        ensure!(
            json.len() <= HEADER_LEN - JSON_AT,
            "grid header JSON too long ({} bytes)",
            json.len()
        );
        b[10..12].copy_from_slice(&u16::try_from(json.len())?.to_le_bytes());
        b[12..16].copy_from_slice(&self.grid.rows.to_le_bytes());
        b[16..20].copy_from_slice(&self.grid.cols.to_le_bytes());
        let g = &self.grid;
        for (i, v) in [g.lon_min, g.lat_min(), g.lon_max(), g.lat_max, g.step]
            .into_iter()
            .enumerate()
        {
            b[24 + i * 8..32 + i * 8].copy_from_slice(&v.to_le_bytes());
        }
        b[64..72].copy_from_slice(&self.enc.nodata.unwrap_or(f64::NAN).to_le_bytes());
        b[72..80].copy_from_slice(&self.enc.scale.to_le_bytes());
        b[80..88].copy_from_slice(&self.enc.offset.to_le_bytes());
        b[88..120].copy_from_slice(&self.sha256);
        b[JSON_AT..JSON_AT + json.len()].copy_from_slice(&json);
        Ok(b)
    }

    pub fn decode(b: &[u8]) -> Result<Self> {
        ensure!(b.len() >= HEADER_LEN, "grid file shorter than its header");
        if &b[0..8] != MAGIC {
            bail!("not a grid file (bad magic)");
        }
        let dtype = Dtype::from_code(b[8])?;
        let has_nodata = b[9] & 1 == 1;
        let json_len = usize::from(u16::from_le_bytes([b[10], b[11]]));
        ensure!(
            json_len <= HEADER_LEN - JSON_AT,
            "grid header JSON overflows"
        );
        let rows = u32_at(b, 12);
        let cols = u32_at(b, 16);
        let grid = Grid {
            lat_max: f64_at(b, 48),
            lon_min: f64_at(b, 24),
            step: f64_at(b, 56),
            rows,
            cols,
        };
        let nodata = has_nodata.then(|| f64_at(b, 64));
        let mut sha256 = [0u8; 32];
        sha256.copy_from_slice(&b[88..120]);
        let json = if json_len == 0 {
            serde_json::Value::Null
        } else {
            serde_json::from_slice(&b[JSON_AT..JSON_AT + json_len])?
        };
        Ok(Self {
            grid,
            enc: LayerEncoding {
                dtype,
                scale: f64_at(b, 72),
                offset: f64_at(b, 80),
                nodata,
            },
            sha256,
            json,
        })
    }

    /// Payload length in bytes.
    #[must_use]
    pub fn payload_len(&self) -> usize {
        crate::cells(&self.grid) * self.enc.dtype.size()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_round_trips() {
        let h = Header {
            grid: Grid::CANONICAL,
            enc: LayerEncoding {
                dtype: Dtype::I16,
                scale: 0.1,
                offset: 0.0,
                nodata: Some(-32768.0),
            },
            sha256: [7; 32],
            json: serde_json::json!({"layer": "terrain.elevation_mean", "period": "2015"}),
        };
        let b = h.encode().expect("encode");
        assert_eq!(b.len(), HEADER_LEN);
        let back = Header::decode(&b).expect("decode");
        assert_eq!(back.grid.rows, 4200);
        assert_eq!(back.grid.cols, 2700);
        assert_eq!(back.grid.lon_min, 79.4);
        assert_eq!(back.grid.lat_max, 10.0);
        assert_eq!(back.grid.step, 0.001);
        assert_eq!(back, h);
        let mut bad = b.clone();
        bad[0] = b'X';
        assert!(Header::decode(&bad).is_err());
    }
}
