//! GeoTIFF reading in pure Rust (`tiff` crate): north-up rasters in
//! EPSG:4326, any of the TIFF layouts the upstream sources use (strips or
//! tiles, deflate/LZW, horizontal and floating-point predictors, pixel- or
//! band-interleaved samples).
//!
//! Georeferencing follows GDAL: `ModelPixelScale` + `ModelTiepoint`, and for
//! `GTRasterTypeGeoKey = PixelIsPoint` the origin moves half a pixel up-left
//! (Copernicus DEM tiles are PixelIsPoint, WorldPop/WorldCover/WorldClim are
//! PixelIsArea).

use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use anyhow::{Context, Result, bail, ensure};
use tiff::decoder::{Decoder, DecodingResult, Limits};
use tiff::tags::Tag;

/// North-up affine georeference: pixel `(col, row)` covers
/// `[x0 + col·dx, x0 + (col+1)·dx) × (y0 − (row+1)·dy, y0 − row·dy]`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GeoRef {
    pub x0: f64,
    /// Pixel width, degrees (> 0).
    pub dx: f64,
    pub y0: f64,
    /// Pixel height, degrees (> 0; rows go south).
    pub dy: f64,
    pub width: u32,
    pub height: u32,
}

impl GeoRef {
    /// `[west, south, east, north]` of the raster.
    #[must_use]
    pub fn bounds(&self) -> [f64; 4] {
        [
            self.x0,
            self.y0 - f64::from(self.height) * self.dy,
            self.x0 + f64::from(self.width) * self.dx,
            self.y0,
        ]
    }

    /// Centre of pixel `(col, row)` as `(lon, lat)`.
    #[must_use]
    pub fn centre(&self, col: u32, row: u32) -> (f64, f64) {
        (
            self.x0 + (f64::from(col) + 0.5) * self.dx,
            self.y0 - (f64::from(row) + 0.5) * self.dy,
        )
    }

    /// Pixel containing `(lon, lat)`, if inside.
    #[must_use]
    pub fn pixel_at(&self, lon: f64, lat: f64) -> Option<(u32, u32)> {
        let c = ((lon - self.x0) / self.dx).floor();
        let r = ((self.y0 - lat) / self.dy).floor();
        if c < 0.0 || r < 0.0 || c >= f64::from(self.width) || r >= f64::from(self.height) {
            return None;
        }
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        Some((c as u32, r as u32))
    }
}

/// An opened GeoTIFF (first image of the file).
pub struct GeoTiff {
    dec: Decoder<BufReader<File>>,
    pub georef: GeoRef,
    /// `GDAL_NODATA` (tag 42113), if any.
    pub nodata: Option<f64>,
    /// Samples (bands) per pixel.
    pub bands: u16,
    /// `GTRasterTypeGeoKey` was PixelIsPoint (the origin was shifted).
    pub pixel_is_point: bool,
    /// Raw `ModelPixelScale` and `ModelTiepoint` (for parity with readers
    /// that ignore the raster type, like geotiff.js).
    pub pixel_scale: [f64; 3],
    pub tiepoint: [f64; 6],
    /// Palette (class-coded) images are read chunk by chunk here: the
    /// `tiff` crate refuses `RGBPalette`, but the samples are just u8 codes.
    raw: Option<RawChunks>,
}

#[derive(Debug, Clone)]
struct RawChunks {
    path: std::path::PathBuf,
    offsets: Vec<u64>,
    counts: Vec<u64>,
    compression: u16,
    predictor: u16,
    chunk_w: u32,
}

impl std::fmt::Debug for GeoTiff {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GeoTiff")
            .field("georef", &self.georef)
            .field("nodata", &self.nodata)
            .field("bands", &self.bands)
            .finish_non_exhaustive()
    }
}

/// A decoded chunk (strip or tile) or image, samples as `f32` (integer
/// samples are converted exactly up to 2^24, which covers every source).
#[derive(Debug, Clone)]
pub struct Block {
    /// Pixel offset of the block in the image.
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    /// Row stride in pixels (≥ `width` for padded tiles).
    pub stride: u32,
    pub bands: u16,
    /// Pixel-interleaved samples: `(row · stride + col) · bands + band`.
    pub data: Vec<f32>,
}

impl Block {
    #[must_use]
    pub fn get(&self, col: u32, row: u32, band: u16) -> f32 {
        self.data[((row * self.stride + col) as usize) * self.bands as usize + band as usize]
    }
}

#[allow(clippy::cast_precision_loss)]
fn to_f32(r: DecodingResult) -> Result<Vec<f32>> {
    Ok(match r {
        DecodingResult::U8(v) => v.into_iter().map(f32::from).collect(),
        DecodingResult::U16(v) => v.into_iter().map(f32::from).collect(),
        DecodingResult::I8(v) => v.into_iter().map(f32::from).collect(),
        DecodingResult::I16(v) => v.into_iter().map(f32::from).collect(),
        DecodingResult::U32(v) => v.into_iter().map(|x| x as f32).collect(),
        DecodingResult::I32(v) => v.into_iter().map(|x| x as f32).collect(),
        DecodingResult::F32(v) => v,
        #[allow(clippy::cast_possible_truncation)]
        DecodingResult::F64(v) => v.into_iter().map(|x| x as f32).collect(),
        _ => bail!("unsupported TIFF sample type"),
    })
}

impl GeoTiff {
    pub fn open(path: &Path) -> Result<Self> {
        let f = File::open(path).with_context(|| format!("opening {}", path.display()))?;
        let mut dec = Decoder::new(BufReader::new(f))
            .with_context(|| format!("{}: not a TIFF", path.display()))?
            .with_limits(Limits::unlimited());
        let (width, height) = dec.dimensions()?;
        let scale = dec
            .get_tag_f64_vec(Tag::ModelPixelScaleTag)
            .with_context(|| format!("{}: no ModelPixelScale (not a GeoTIFF?)", path.display()))?;
        let tie = dec
            .get_tag_f64_vec(Tag::ModelTiepointTag)
            .with_context(|| format!("{}: no ModelTiepoint", path.display()))?;
        ensure!(
            scale.len() >= 2 && tie.len() >= 6,
            "{}: malformed georeference",
            path.display()
        );
        ensure!(
            dec.find_tag(Tag::Unknown(34264))?.is_none(),
            "{}: ModelTransformation (rotated rasters) is not supported",
            path.display()
        );
        let keys = dec
            .get_tag_u16_vec(Tag::GeoKeyDirectoryTag)
            .unwrap_or_default();
        let key = |id: u16| -> Option<u16> {
            keys.get(4..)?
                .as_chunks::<4>()
                .0
                .iter()
                .find(|k| k[0] == id && k[1] == 0)
                .map(|k| k[3])
        };
        if let Some(epsg) = key(2048) {
            ensure!(
                epsg == 4326,
                "{}: CRS EPSG:{epsg} (only EPSG:4326 is supported)",
                path.display()
            );
        }
        let pixel_is_point = key(1025) == Some(2);
        let (dx, dy) = (scale[0], scale[1]);
        ensure!(
            dx > 0.0 && dy > 0.0,
            "{}: non north-up raster",
            path.display()
        );
        let mut x0 = tie[3] - tie[0] * dx;
        let mut y0 = tie[4] + tie[1] * dy;
        if pixel_is_point {
            x0 -= dx / 2.0;
            y0 += dy / 2.0;
        }
        let nodata = match dec.find_tag(Tag::GdalNodata)? {
            Some(v) => {
                let s = v.into_string()?;
                let s = s.trim_matches(|c: char| c == '\0' || c.is_whitespace());
                if s.eq_ignore_ascii_case("nan") {
                    Some(f64::NAN)
                } else {
                    Some(
                        s.parse::<f64>()
                            .with_context(|| format!("GDAL_NODATA {s:?}"))?,
                    )
                }
            }
            None => None,
        };
        let bands = dec
            .find_tag_unsigned::<u16>(Tag::SamplesPerPixel)?
            .unwrap_or(1);
        let mut t6 = [0.0; 6];
        t6.copy_from_slice(&tie[..6]);
        let photometric = dec
            .find_tag_unsigned::<u16>(Tag::PhotometricInterpretation)?
            .unwrap_or(1);
        let bits = dec
            .find_tag_unsigned_vec::<u16>(Tag::BitsPerSample)?
            .and_then(|v| v.first().copied())
            .unwrap_or(8);
        let raw = if photometric == 3 {
            ensure!(
                bits == 8 && bands == 1,
                "{}: only 8-bit single-band palette images are supported",
                path.display()
            );
            let tiled = dec.find_tag(Tag::TileOffsets)?.is_some();
            let (ot, ct) = if tiled {
                (Tag::TileOffsets, Tag::TileByteCounts)
            } else {
                (Tag::StripOffsets, Tag::StripByteCounts)
            };
            Some(RawChunks {
                path: path.to_path_buf(),
                offsets: dec.get_tag_u64_vec(ot)?,
                counts: dec.get_tag_u64_vec(ct)?,
                compression: dec.find_tag_unsigned::<u16>(Tag::Compression)?.unwrap_or(1),
                predictor: dec.find_tag_unsigned::<u16>(Tag::Predictor)?.unwrap_or(1),
                chunk_w: dec.chunk_dimensions().0,
            })
        } else {
            None
        };
        Ok(Self {
            dec,
            georef: GeoRef {
                x0,
                dx,
                y0,
                dy,
                width,
                height,
            },
            nodata,
            bands,
            pixel_is_point,
            pixel_scale: [scale[0], scale[1], scale.get(2).copied().unwrap_or(0.0)],
            tiepoint: t6,
            raw,
        })
    }

    /// Is `v` a nodata sample? Compared at the sample precision: GDAL
    /// writes `GDAL_NODATA` as decimal text (`-3.4e+38` for `-FLT_MAX`-like
    /// fills), which only round-trips through `f32`.
    #[must_use]
    pub fn is_nodata(&self, v: f32) -> bool {
        #[allow(clippy::cast_possible_truncation)]
        let hit = self.nodata.is_some_and(|n| !n.is_nan() && v == n as f32);
        v.is_nan() || hit
    }

    /// Decodes the whole image (pixel-interleaved when several bands).
    pub fn read_all(&mut self) -> Result<Block> {
        let planar = self
            .dec
            .find_tag_unsigned::<u16>(Tag::PlanarConfiguration)?
            .unwrap_or(1);
        ensure!(
            planar == 1 || self.bands == 1,
            "band-sequential (planar) TIFFs are not supported"
        );
        let data = to_f32(self.dec.read_image()?)?;
        let GeoRef { width, height, .. } = self.georef;
        ensure!(
            data.len() == width as usize * height as usize * self.bands as usize,
            "decoded {} samples, expected {}",
            data.len(),
            width as usize * height as usize * self.bands as usize
        );
        Ok(Block {
            x: 0,
            y: 0,
            width,
            height,
            stride: width,
            bands: self.bands,
            data,
        })
    }

    /// Number of chunks (strips or tiles).
    pub fn chunk_count(&mut self) -> Result<u32> {
        Ok(match self.dec.get_chunk_type() {
            tiff::decoder::ChunkType::Strip => self.dec.strip_count()?,
            tiff::decoder::ChunkType::Tile => self.dec.tile_count()?,
        })
    }

    /// Pixel rectangle `(x, y, w, h)` of chunk `i`.
    #[must_use]
    pub fn chunk_rect(&self, i: u32) -> (u32, u32, u32, u32) {
        let (cw, ch) = self.dec.chunk_dimensions();
        let across = self.georef.width.div_ceil(cw);
        let (w, h) = self.dec.chunk_data_dimensions(i);
        ((i % across) * cw, (i / across) * ch, w, h)
    }

    /// Decodes chunk `i`.
    pub fn read_chunk(&mut self, i: u32) -> Result<Block> {
        let (x, y, w, h) = self.chunk_rect(i);
        if let Some(r) = &self.raw {
            return read_raw_chunk(r, i, (x, y, w, h));
        }
        let data = to_f32(self.dec.read_chunk(i)?)?;
        let px = data.len() / self.bands as usize;
        let stride = if px == (w * h) as usize {
            w
        } else {
            let (cw, _) = self.dec.chunk_dimensions();
            ensure!(
                px >= (cw * h) as usize,
                "unexpected chunk size {px} for {w}x{h}"
            );
            cw
        };
        Ok(Block {
            x,
            y,
            width: w,
            height: h,
            stride,
            bands: self.bands,
            data,
        })
    }
}

/// Reads one 8-bit chunk without the colour model: inflate (zlib) or raw,
/// then undo the horizontal predictor.
fn read_raw_chunk(r: &RawChunks, i: u32, (x, y, w, h): (u32, u32, u32, u32)) -> Result<Block> {
    use std::io::{Read, Seek, SeekFrom};
    let (off, len) = (r.offsets[i as usize], r.counts[i as usize]);
    let mut f = File::open(&r.path)?;
    f.seek(SeekFrom::Start(off))?;
    let mut comp = vec![0u8; usize::try_from(len)?];
    f.read_exact(&mut comp)?;
    let mut bytes = match r.compression {
        1 => comp,
        8 | 32946 => {
            let mut out = Vec::new();
            flate2::read::ZlibDecoder::new(comp.as_slice()).read_to_end(&mut out)?;
            out
        }
        c => bail!("palette TIFF compression {c} is not supported (deflate or none)"),
    };
    // Strips are `w` wide; tiles are padded to the tile width.
    let stride = if bytes.len() >= (r.chunk_w * h) as usize {
        r.chunk_w
    } else {
        w
    };
    ensure!(bytes.len() >= (stride * h) as usize, "short chunk {i}");
    if r.predictor == 2 {
        for row in bytes.chunks_mut(stride as usize) {
            for k in 1..row.len() {
                row[k] = row[k].wrapping_add(row[k - 1]);
            }
        }
    }
    Ok(Block {
        x,
        y,
        width: w,
        height: h,
        stride,
        bands: 1,
        data: bytes.into_iter().map(f32::from).collect(),
    })
}
