//! PMTiles v3 reader (https://github.com/protomaps/PMTiles/blob/main/spec/v3/spec.md):
//! header, root and leaf directories (cached), metadata, tiles by `z/x/y`.
//!
//! Reads with positioned reads on one open file handle, so concurrent tile
//! requests share it without locking; directories are decoded once and kept
//! in memory (05 §6: "PMTiles directories cached in RAM"). Tile bytes are
//! returned as stored (`tile_compression` says how: tippecanoe writes gzip).

use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::os::unix::fs::FileExt;
use std::path::Path;
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result, bail, ensure};

pub const HEADER_LEN: usize = 127;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Compression {
    Unknown,
    None,
    Gzip,
    Brotli,
    Zstd,
}

impl Compression {
    const fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::None,
            2 => Self::Gzip,
            3 => Self::Brotli,
            4 => Self::Zstd,
            _ => Self::Unknown,
        }
    }

    /// HTTP `Content-Encoding` value, if any.
    #[must_use]
    pub const fn content_encoding(self) -> Option<&'static str> {
        match self {
            Self::Gzip => Some("gzip"),
            Self::Brotli => Some("br"),
            Self::Zstd => Some("zstd"),
            Self::None | Self::Unknown => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Header {
    pub root_offset: u64,
    pub root_length: u64,
    pub metadata_offset: u64,
    pub metadata_length: u64,
    pub leaf_offset: u64,
    pub leaf_length: u64,
    pub data_offset: u64,
    pub data_length: u64,
    pub addressed_tiles: u64,
    pub tile_entries: u64,
    pub tile_contents: u64,
    pub clustered: bool,
    pub internal_compression: Compression,
    pub tile_compression: Compression,
    /// 1 = MVT.
    pub tile_type: u8,
    pub min_zoom: u8,
    pub max_zoom: u8,
    /// `[west, south, east, north]`.
    pub bounds: [f64; 4],
    pub center_zoom: u8,
    pub center: [f64; 2],
}

fn u64_at(b: &[u8], i: usize) -> u64 {
    let mut a = [0u8; 8];
    a.copy_from_slice(&b[i..i + 8]);
    u64::from_le_bytes(a)
}

fn i32_at(b: &[u8], i: usize) -> f64 {
    let mut a = [0u8; 4];
    a.copy_from_slice(&b[i..i + 4]);
    f64::from(i32::from_le_bytes(a)) / 1e7
}

impl Header {
    pub fn parse(b: &[u8]) -> Result<Self> {
        ensure!(b.len() >= HEADER_LEN, "PMTiles header too short");
        ensure!(&b[0..7] == b"PMTiles", "not a PMTiles file");
        ensure!(b[7] == 3, "PMTiles version {} (only v3 is read)", b[7]);
        Ok(Self {
            root_offset: u64_at(b, 8),
            root_length: u64_at(b, 16),
            metadata_offset: u64_at(b, 24),
            metadata_length: u64_at(b, 32),
            leaf_offset: u64_at(b, 40),
            leaf_length: u64_at(b, 48),
            data_offset: u64_at(b, 56),
            data_length: u64_at(b, 64),
            addressed_tiles: u64_at(b, 72),
            tile_entries: u64_at(b, 80),
            tile_contents: u64_at(b, 88),
            clustered: b[96] == 1,
            internal_compression: Compression::from_u8(b[97]),
            tile_compression: Compression::from_u8(b[98]),
            tile_type: b[99],
            min_zoom: b[100],
            max_zoom: b[101],
            bounds: [
                i32_at(b, 102),
                i32_at(b, 106),
                i32_at(b, 110),
                i32_at(b, 114),
            ],
            center_zoom: b[118],
            center: [i32_at(b, 119), i32_at(b, 123)],
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entry {
    pub tile_id: u64,
    pub offset: u64,
    pub length: u32,
    /// 0 = the entry points at a leaf directory.
    pub run_length: u32,
}

fn varint(b: &[u8], pos: &mut usize) -> Result<u64> {
    let mut v: u64 = 0;
    let mut shift = 0;
    loop {
        let byte = *b.get(*pos).context("truncated varint")?;
        *pos += 1;
        v |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Ok(v);
        }
        shift += 7;
        ensure!(shift < 64, "varint too long");
    }
}

/// Decodes a (decompressed) directory.
pub fn decode_directory(b: &[u8]) -> Result<Vec<Entry>> {
    let mut pos = 0;
    let n = usize::try_from(varint(b, &mut pos)?)?;
    ensure!(n <= 10_000_000, "directory with {n} entries");
    let mut es = vec![
        Entry {
            tile_id: 0,
            offset: 0,
            length: 0,
            run_length: 0
        };
        n
    ];
    let mut last = 0u64;
    for e in &mut es {
        last += varint(b, &mut pos)?;
        e.tile_id = last;
    }
    for e in &mut es {
        e.run_length = u32::try_from(varint(b, &mut pos)?)?;
    }
    for e in &mut es {
        e.length = u32::try_from(varint(b, &mut pos)?)?;
    }
    for i in 0..n {
        let v = varint(b, &mut pos)?;
        es[i].offset = if v == 0 && i > 0 {
            es[i - 1].offset + u64::from(es[i - 1].length)
        } else {
            v.checked_sub(1)
                .context("directory offset 0 on the first entry")?
        };
    }
    Ok(es)
}

/// Hilbert tile id of `z/x/y` (PMTiles v3).
#[must_use]
pub fn tile_id(z: u8, x: u64, y: u64) -> u64 {
    let base: u64 = ((1u64 << (2 * u32::from(z))) - 1) / 3;
    let n: u64 = 1 << z;
    let (mut x, mut y) = (x, y);
    let mut d = 0u64;
    let mut s = n / 2;
    while s > 0 {
        let rx = u64::from(x & s > 0);
        let ry = u64::from(y & s > 0);
        d += s * s * ((3 * rx) ^ ry);
        if ry == 0 {
            if rx == 1 {
                x = n - 1 - x;
                y = n - 1 - y;
            }
            std::mem::swap(&mut x, &mut y);
        }
        s /= 2;
    }
    base + d
}

fn find(entries: &[Entry], id: u64) -> Option<Entry> {
    let i = entries
        .partition_point(|e| e.tile_id <= id)
        .checked_sub(1)?;
    let e = entries[i];
    if e.run_length == 0 || id < e.tile_id + u64::from(e.run_length) {
        Some(e)
    } else {
        None
    }
}

fn decompress(c: Compression, b: Vec<u8>) -> Result<Vec<u8>> {
    match c {
        Compression::None | Compression::Unknown => Ok(b),
        Compression::Gzip => {
            let mut out = Vec::with_capacity(b.len() * 4);
            flate2::read::GzDecoder::new(b.as_slice())
                .read_to_end(&mut out)
                .context("gzip directory")?;
            Ok(out)
        }
        other => bail!("{other:?} internal compression is not supported"),
    }
}

/// An open PMTiles archive.
pub struct PmTiles {
    file: File,
    pub header: Header,
    root: Vec<Entry>,
    leaves: Mutex<HashMap<u64, Arc<Vec<Entry>>>>,
}

impl PmTiles {
    pub fn open(path: &Path) -> Result<Self> {
        let file = File::open(path).with_context(|| format!("opening {}", path.display()))?;
        let mut h = vec![0u8; HEADER_LEN];
        file.read_exact_at(&mut h, 0)?;
        let header = Header::parse(&h)?;
        let root = decode_directory(&decompress(
            header.internal_compression,
            read_at(&file, header.root_offset, header.root_length)?,
        )?)?;
        Ok(Self {
            file,
            header,
            root,
            leaves: Mutex::new(HashMap::new()),
        })
    }

    /// The metadata JSON (tippecanoe: `vector_layers`, `tilestats`, …).
    pub fn metadata(&self) -> Result<serde_json::Value> {
        let raw = read_at(
            &self.file,
            self.header.metadata_offset,
            self.header.metadata_length,
        )?;
        let b = decompress(self.header.internal_compression, raw)?;
        Ok(serde_json::from_slice(&b)?)
    }

    fn leaf(&self, offset: u64, length: u64) -> Result<Arc<Vec<Entry>>> {
        if let Some(l) = self
            .leaves
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(&offset)
        {
            return Ok(Arc::clone(l));
        }
        let es = Arc::new(decode_directory(&decompress(
            self.header.internal_compression,
            read_at(&self.file, self.header.leaf_offset + offset, length)?,
        )?)?);
        self.leaves
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(offset, Arc::clone(&es));
        Ok(es)
    }

    /// The stored bytes of tile `z/x/y`, or `None` when the archive has none.
    pub fn tile(&self, z: u8, x: u64, y: u64) -> Result<Option<Vec<u8>>> {
        if z < self.header.min_zoom || z > self.header.max_zoom || z > 31 {
            return Ok(None);
        }
        if x >= (1u64 << z) || y >= (1u64 << z) {
            return Ok(None);
        }
        let id = tile_id(z, x, y);
        let mut dir: Arc<Vec<Entry>> = Arc::new(Vec::new());
        let mut entries: &[Entry] = &self.root;
        for _ in 0..4 {
            let Some(e) = find(entries, id) else {
                return Ok(None);
            };
            if e.run_length > 0 {
                return Ok(Some(read_at(
                    &self.file,
                    self.header.data_offset + e.offset,
                    u64::from(e.length),
                )?));
            }
            dir = self.leaf(e.offset, u64::from(e.length))?;
            entries = &dir;
        }
        drop(dir);
        bail!("PMTiles directory deeper than 4 levels")
    }
}

fn read_at(f: &File, offset: u64, len: u64) -> Result<Vec<u8>> {
    let mut b = vec![0u8; usize::try_from(len)?];
    f.read_exact_at(&mut b, offset)?;
    Ok(b)
}

// ───────────────────────────── writer (WP-27) ──────────────────────────────

fn put_varint(v: &mut Vec<u8>, mut x: u64) {
    loop {
        #[allow(clippy::cast_possible_truncation)]
        let b = (x & 0x7f) as u8;
        x >>= 7;
        if x == 0 {
            v.push(b);
            return;
        }
        v.push(b | 0x80);
    }
}

/// Encodes a directory (uncompressed), the inverse of [`decode_directory`].
#[must_use]
pub fn encode_directory(es: &[Entry]) -> Vec<u8> {
    let mut b = Vec::new();
    put_varint(&mut b, es.len() as u64);
    let mut last = 0;
    for e in es {
        put_varint(&mut b, e.tile_id - last);
        last = e.tile_id;
    }
    for e in es {
        put_varint(&mut b, u64::from(e.run_length));
    }
    for e in es {
        put_varint(&mut b, u64::from(e.length));
    }
    for (i, e) in es.iter().enumerate() {
        if i > 0 && e.offset == es[i - 1].offset + u64::from(es[i - 1].length) {
            put_varint(&mut b, 0);
        } else {
            put_varint(&mut b, e.offset + 1);
        }
    }
    b
}

fn gzip(b: &[u8]) -> Result<Vec<u8>> {
    use std::io::Write;
    let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    e.write_all(b)?;
    Ok(e.finish()?)
}

/// What [`write`] needs besides the tiles.
#[derive(Debug, Clone)]
pub struct WriteSpec {
    /// 2 = PNG, 1 = MVT.
    pub tile_type: u8,
    pub tile_compression: Compression,
    pub min_zoom: u8,
    pub max_zoom: u8,
    /// `[west, south, east, north]`.
    pub bounds: [f64; 4],
    pub center: [f64; 2],
    pub center_zoom: u8,
    pub metadata: serde_json::Value,
}

/// Writes a clustered PMTiles v3 archive with a single (root) directory;
/// identical tile contents are stored once and consecutive equal tiles are
/// run-length encoded. `tiles` is `(z, x, y, bytes)` in any order.
pub fn write(path: &Path, spec: &WriteSpec, mut tiles: Vec<(u8, u32, u32, Vec<u8>)>) -> Result<()> {
    use sha2::{Digest, Sha256};
    use std::io::Write;
    let mut ids: Vec<(u64, Vec<u8>)> = tiles
        .drain(..)
        .map(|(z, x, y, b)| (tile_id(z, u64::from(x), u64::from(y)), b))
        .collect();
    ids.sort_by_key(|t| t.0);
    ensure!(
        ids.windows(2).all(|w| w[0].0 != w[1].0),
        "duplicate tile in PMTiles input"
    );
    let mut data: Vec<u8> = Vec::new();
    let mut seen: HashMap<[u8; 32], (u64, u32)> = HashMap::new();
    let mut entries: Vec<Entry> = Vec::new();
    for (id, b) in &ids {
        let h: [u8; 32] = Sha256::digest(b).into();
        let (offset, length) = match seen.get(&h) {
            Some(&v) => v,
            None => {
                let v = (data.len() as u64, u32::try_from(b.len())?);
                data.extend_from_slice(b);
                seen.insert(h, v);
                v
            }
        };
        if let Some(last) = entries.last_mut()
            && last.offset == offset
            && last.tile_id + u64::from(last.run_length) == *id
        {
            last.run_length += 1;
            continue;
        }
        entries.push(Entry {
            tile_id: *id,
            offset,
            length,
            run_length: 1,
        });
    }
    let root = gzip(&encode_directory(&entries))?;
    ensure!(
        HEADER_LEN + root.len() <= 16_384,
        "root directory of {} bytes needs leaf directories (not implemented)",
        root.len()
    );
    let meta = gzip(&serde_json::to_vec(&spec.metadata)?)?;
    let root_offset = HEADER_LEN as u64;
    let meta_offset = root_offset + root.len() as u64;
    let leaf_offset = meta_offset + meta.len() as u64;
    let data_offset = leaf_offset;
    let mut h = vec![0u8; HEADER_LEN];
    h[0..7].copy_from_slice(b"PMTiles");
    h[7] = 3;
    let put = |h: &mut [u8], at: usize, v: u64| h[at..at + 8].copy_from_slice(&v.to_le_bytes());
    put(&mut h, 8, root_offset);
    put(&mut h, 16, root.len() as u64);
    put(&mut h, 24, meta_offset);
    put(&mut h, 32, meta.len() as u64);
    put(&mut h, 40, leaf_offset);
    put(&mut h, 48, 0);
    put(&mut h, 56, data_offset);
    put(&mut h, 64, data.len() as u64);
    put(&mut h, 72, ids.len() as u64);
    put(&mut h, 80, entries.len() as u64);
    put(&mut h, 88, seen.len() as u64);
    h[96] = 1;
    h[97] = 2; // internal compression: gzip
    h[98] = match spec.tile_compression {
        Compression::Gzip => 2,
        Compression::Brotli => 3,
        Compression::Zstd => 4,
        Compression::None | Compression::Unknown => 1,
    };
    h[99] = spec.tile_type;
    h[100] = spec.min_zoom;
    h[101] = spec.max_zoom;
    #[allow(clippy::cast_possible_truncation)]
    let e7 = |v: f64| ((v * 1e7).round() as i32).to_le_bytes();
    h[102..106].copy_from_slice(&e7(spec.bounds[0]));
    h[106..110].copy_from_slice(&e7(spec.bounds[1]));
    h[110..114].copy_from_slice(&e7(spec.bounds[2]));
    h[114..118].copy_from_slice(&e7(spec.bounds[3]));
    h[118] = spec.center_zoom;
    h[119..123].copy_from_slice(&e7(spec.center[0]));
    h[123..127].copy_from_slice(&e7(spec.center[1]));
    let mut f = std::io::BufWriter::new(
        File::create(path).with_context(|| format!("creating {}", path.display()))?,
    );
    f.write_all(&h)?;
    f.write_all(&root)?;
    f.write_all(&meta)?;
    f.write_all(&data)?;
    f.into_inner()
        .map_err(|e| anyhow::anyhow!("flushing {}: {e}", path.display()))?
        .sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hilbert_ids_match_the_spec_examples() {
        // From the PMTiles v3 spec / reference implementations.
        assert_eq!(tile_id(0, 0, 0), 0);
        assert_eq!(tile_id(1, 0, 0), 1);
        assert_eq!(tile_id(1, 0, 1), 2);
        assert_eq!(tile_id(1, 1, 1), 3);
        assert_eq!(tile_id(1, 1, 0), 4);
        assert_eq!(tile_id(2, 0, 0), 5);
        assert_eq!(tile_id(12, 3423, 1763), 19_078_479);
    }

    #[test]
    fn directories_round_trip() {
        fn put(v: &mut Vec<u8>, mut x: u64) {
            loop {
                let b = (x & 0x7f) as u8;
                x >>= 7;
                if x == 0 {
                    v.push(b);
                    break;
                }
                v.push(b | 0x80);
            }
        }
        // three entries: ids 5, 6 (run 2), 10 (leaf); offsets contiguous then explicit
        let mut b = Vec::new();
        put(&mut b, 3);
        for d in [5, 1, 4] {
            put(&mut b, d);
        }
        for r in [1, 2, 0] {
            put(&mut b, r);
        }
        for l in [100, 50, 30] {
            put(&mut b, l);
        }
        for o in [1, 0, 1001] {
            put(&mut b, o);
        }
        let es = decode_directory(&b).expect("decode");
        assert_eq!(
            es[0],
            Entry {
                tile_id: 5,
                offset: 0,
                length: 100,
                run_length: 1
            }
        );
        assert_eq!(
            es[1],
            Entry {
                tile_id: 6,
                offset: 100,
                length: 50,
                run_length: 2
            }
        );
        assert_eq!(
            es[2],
            Entry {
                tile_id: 10,
                offset: 1000,
                length: 30,
                run_length: 0
            }
        );
        assert_eq!(
            find(&es, 7).map(|e| e.tile_id),
            Some(6),
            "run-length covers 6..8"
        );
        assert_eq!(find(&es, 8), None);
        assert_eq!(find(&es, 4), None);
        assert_eq!(find(&es, 1234).map(|e| e.run_length), Some(0), "leaf");
    }

    #[test]
    fn written_archives_read_back() {
        let dir = std::env::temp_dir().join(format!("lanka-pmw-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let p = dir.join("t.pmtiles");
        let mut tiles = Vec::new();
        for z in 0..=6u8 {
            for x in 0..(1u32 << z).min(5) {
                for y in 0..(1u32 << z).min(4) {
                    // many identical tiles (dedup + runs) and some unique
                    let b = if (x + y) % 3 == 0 {
                        vec![7u8; 10]
                    } else {
                        format!("{z}/{x}/{y}").into_bytes()
                    };
                    tiles.push((z, x, y, b));
                }
            }
        }
        let want: Vec<_> = tiles.clone();
        let spec = WriteSpec {
            tile_type: 2,
            tile_compression: Compression::None,
            min_zoom: 0,
            max_zoom: 6,
            bounds: [79.5, 5.9, 81.9, 9.9],
            center: [80.6, 7.8],
            center_zoom: 7,
            metadata: serde_json::json!({"encoding": "terrarium"}),
        };
        write(&p, &spec, tiles).expect("write");
        let pm = PmTiles::open(&p).expect("open");
        assert_eq!(pm.header.tile_type, 2);
        assert_eq!(pm.header.max_zoom, 6);
        assert_eq!(pm.metadata().expect("meta")["encoding"], "terrarium");
        for (z, x, y, b) in want {
            assert_eq!(
                pm.tile(z, u64::from(x), u64::from(y)).expect("read"),
                Some(b),
                "{z}/{x}/{y}"
            );
        }
        assert_eq!(pm.tile(6, 40, 40).expect("read"), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
