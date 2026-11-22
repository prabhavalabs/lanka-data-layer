//! Read-only memory maps of grid `.bin` files — the one audited `unsafe`
//! module of the workspace (ADR 0013).
//!
//! # Safety argument
//!
//! `memmap2::Mmap::map` is `unsafe` because the mapped file could be
//! truncated or rewritten by another process while mapped, which would turn
//! reads into SIGBUS or data races. Grid files are immutable named objects
//! of the blob store (`grids/<layer>/<period>/<sha16>.bin`): they are written
//! to a temporary file and renamed into place once, `put_named` refuses to
//! overwrite a key with other content, and the server opens them read-only
//! (its store mount is read-only in production, 07 §1). A rename that
//! replaces a path leaves the mapped inode intact. Every read goes through
//! bounds-checked slice indexing; no pointer casts are made.

#![allow(unsafe_code)]

use std::fs::File;
use std::path::Path;

use anyhow::{Context, Result, ensure};
use memmap2::Mmap;

use crate::CellValues;
use crate::array::Dtype;
use crate::header::{HEADER_LEN, Header};

/// A mapped grid layer.
pub struct MappedGrid {
    map: Mmap,
    pub header: Header,
}

impl std::fmt::Debug for MappedGrid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MappedGrid")
            .field("header", &self.header)
            .finish_non_exhaustive()
    }
}

impl MappedGrid {
    /// Maps `path` read-only and validates the header and payload length.
    /// The payload hash is not re-verified here (that reads every page);
    /// [`MappedGrid::verify`] does it.
    pub fn open(path: &Path) -> Result<Self> {
        let f = File::open(path).with_context(|| format!("opening {}", path.display()))?;
        // SAFETY: see the module documentation — grid files are immutable
        // once renamed into the store and are opened read-only.
        let map =
            unsafe { Mmap::map(&f) }.with_context(|| format!("mapping {}", path.display()))?;
        let header = Header::decode(&map)?;
        ensure!(
            map.len() == HEADER_LEN + header.payload_len(),
            "{}: file length {} does not match its header",
            path.display(),
            map.len()
        );
        Ok(Self { map, header })
    }

    /// Recomputes the payload sha256 and compares it with the header.
    pub fn verify(&self) -> Result<()> {
        use sha2::{Digest, Sha256};
        let sha: [u8; 32] = Sha256::digest(self.payload()).into();
        ensure!(sha == self.header.sha256, "payload sha256 mismatch");
        Ok(())
    }

    #[must_use]
    pub fn payload(&self) -> &[u8] {
        &self.map[HEADER_LEN..]
    }

    /// Raw stored value of cell `i` (no scale/offset, nodata not resolved).
    #[must_use]
    pub fn raw(&self, i: usize) -> f64 {
        let p = self.payload();
        match self.header.enc.dtype {
            Dtype::U8 => f64::from(p[i]),
            Dtype::U16 => f64::from(u16::from_le_bytes([p[2 * i], p[2 * i + 1]])),
            Dtype::I16 => f64::from(i16::from_le_bytes([p[2 * i], p[2 * i + 1]])),
            Dtype::U32 => {
                let b = &p[4 * i..4 * i + 4];
                f64::from(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            }
            Dtype::F32 => {
                let b = &p[4 * i..4 * i + 4];
                f64::from(f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            }
        }
    }

    /// `u16` value of cell `i` for index layers (`gnd_idx`).
    #[must_use]
    pub fn u16_at(&self, i: usize) -> u16 {
        let p = self.payload();
        u16::from_le_bytes([p[2 * i], p[2 * i + 1]])
    }

    #[must_use]
    pub fn u8_at(&self, i: usize) -> u8 {
        self.payload()[i]
    }
}

impl CellValues for MappedGrid {
    fn len(&self) -> usize {
        crate::cells(&self.header.grid)
    }
    fn value(&self, i: usize) -> Option<f64> {
        self.header.enc.physical(self.raw(i))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::array::{GridArray, LayerEncoding};
    use lanka_core::grid::Grid;

    #[test]
    fn mapped_equals_owned() {
        let g = Grid {
            lat_max: 10.0,
            lon_min: 79.4,
            step: 0.001,
            rows: 5,
            cols: 7,
        };
        let vals: Vec<f32> = (0..35)
            .map(|i| if i == 3 { f32::NAN } else { i as f32 * 1.5 })
            .collect();
        let enc = LayerEncoding {
            dtype: Dtype::U16,
            scale: 0.5,
            offset: 0.0,
            nodata: Some(65_535.0),
        };
        let a = GridArray::quantise(g, enc, &vals).expect("q");
        let dir = std::env::temp_dir().join(format!("lanka-grid-mm-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let p = dir.join("m.bin");
        a.write_bin(&p, serde_json::Value::Null).expect("write");
        let m = MappedGrid::open(&p).expect("map");
        m.verify().expect("verify");
        for i in 0..35 {
            assert_eq!(m.value(i), a.value(i), "cell {i}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
