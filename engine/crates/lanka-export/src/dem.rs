//! The `dem` raster-dem tileset (10 §3.2, WP-27): Terrarium-encoded PNG
//! tiles in one PMTiles archive, built once per elevation layer from the
//! canonical-grid Copernicus DEM layer (`terrain.elevation`, i16 at 0.1 m)
//! for MapLibre's `hillshade` layer.
//!
//! - Zooms [`MIN_ZOOM`]–[`MAX_ZOOM`]; z11 pixels (≈ 76 m) are finer than the
//!   0.001° cells, so z11 samples the cells themselves and MapLibre
//!   over-zooms above. Lower zooms sample the layer's mean pyramid
//!   (`lanka_grid::values::Pyramid`), the same one value tiles use.
//! - Terrarium: `elevation = R·256 + G + B/256 − 32768` (m). Sea and cells
//!   without data are 0 m (hillshade of a flat sea); tiles with no land are
//!   not stored (the client sees no tile = flat).
//! - The archive is content-named (`tiles/dem/<sha16>.pmtiles`) and indexed
//!   in `geo.tilesets` like the vector tilesets; a build is skipped when the
//!   elevation layer's sha256 and the encoder are unchanged.

use std::path::PathBuf;
use std::time::Instant;

use anyhow::{Context, Result, bail};
use lanka_grid::CellValues;
use lanka_grid::cog::Overview;
use lanka_grid::values::{self, MAX_LEVEL, NODATA_Q, Pyramid, Quant};
use lanka_store::{BlobStore, Ns};
use serde_json::json;
use sha2::{Digest, Sha256};
use sqlx::PgPool;

use crate::pmtiles::{self, Compression, PmTiles, WriteSpec};
use crate::tiles::Built;

pub const DEM: &str = "dem";
pub const MIN_ZOOM: u8 = 5;
pub const MAX_ZOOM: u8 = 11;
/// The grid layer the tileset is built from.
pub const ELEVATION_LAYER: &str = "terrain.elevation";
/// Bumped when the encoding or tiling changes.
const DEM_FORMAT: &str = "lanka-dem-terrarium-v1";

/// Terrarium RGB of an elevation in metres.
#[must_use]
pub fn terrarium(h: f64) -> [u8; 3] {
    let v = (h + 32_768.0).clamp(0.0, 65_535.996);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let i = v.floor() as u32;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let b = ((v - f64::from(i)) * 256.0).floor() as u8;
    #[allow(clippy::cast_possible_truncation)]
    [(i >> 8) as u8, (i & 0xff) as u8, b]
}

/// Elevation of a Terrarium pixel.
#[must_use]
pub fn terrarium_height(p: [u8; 3]) -> f64 {
    f64::from(p[0]).mul_add(256.0, f64::from(p[1])) + f64::from(p[2]) / 256.0 - 32_768.0
}

/// One Terrarium tile, or `None` when no pixel has data.
pub fn render_tile<V: CellValues + ?Sized>(
    values: &V,
    g: &lanka_core::grid::Grid,
    quant: &Quant,
    pyramid: &Pyramid,
    z: u8,
    x: u32,
    y: u32,
) -> Result<Option<Vec<u8>>> {
    let pyr = (values::level_for_zoom(g, z, pyramid.depth()) > 0).then_some(pyramid);
    let rgba = values::render_rgba(values, g, quant, pyr, z, x, y)?;
    let mut any = false;
    let mut out = vec![0u8; rgba.len()];
    for (o, p) in out
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip(rgba.as_chunks::<4>().0)
    {
        let q = (u16::from(p[0]) << 8) | u16::from(p[1]);
        let h = match quant.value(q) {
            Some(h) if q != NODATA_Q => {
                any = true;
                h.max(-100.0)
            }
            _ => 0.0,
        };
        let [r, gg, b] = terrarium(h);
        *o = [r, gg, b, 255];
    }
    if !any {
        return Ok(None);
    }
    Ok(Some(values::encode_png(&out)?))
}

/// Builds the `dem` tileset unless the elevation layer is unchanged.
#[allow(clippy::too_many_lines)]
pub async fn build(pool: &PgPool, store: &dyn BlobStore, force: bool) -> Result<Built> {
    let row: Option<(String, String, String, String, Option<i64>)> = sqlx::query_as(
        "SELECT l.period, l.bin_path, encode(l.sha256, 'hex'), l.dataset_id, l.version_id
         FROM geo.grid_layers l WHERE l.id = $1 ORDER BY l.period DESC LIMIT 1",
    )
    .bind(ELEVATION_LAYER)
    .fetch_optional(pool)
    .await?;
    let Some((period, bin_path, sha, dataset, version_id)) = row else {
        bail!("no {ELEVATION_LAYER} grid layer: run `lanka ingest run copernicus-dem` first");
    };
    let mut hh = Sha256::new();
    hh.update(DEM_FORMAT.as_bytes());
    hh.update([0x1f]);
    hh.update(sha.as_bytes());
    hh.update([MIN_ZOOM, MAX_ZOOM]);
    let hash: [u8; 32] = hh.finalize().into();
    let prev: Option<(String, String, i64)> = sqlx::query_as(
        "SELECT sha16, path, bytes FROM geo.current_tilesets WHERE id = $1 AND input_hash = $2",
    )
    .bind(DEM)
    .bind(hash.as_slice())
    .fetch_optional(pool)
    .await?;
    if let Some((sha16, path, bytes)) = prev
        && !force
        && store
            .exists(Ns::Tile, path.trim_start_matches("tiles/"))
            .await?
    {
        return Ok(Built {
            id: DEM.to_owned(),
            sha16,
            path,
            bytes: u64::try_from(bytes)?,
            built: false,
            features: Default::default(),
            export_ms: 0,
            tippecanoe_ms: 0,
            warnings: Vec::new(),
        });
    }
    let root = lanka_ingest::grid::local_root(store)?;
    let work: PathBuf = store.tmp_file("tiles-dem")?;
    tokio::fs::create_dir_all(&work).await?;
    let out = work.join("dem.pmtiles");
    let out2 = out.clone();
    let period2 = period.clone();
    let t0 = Instant::now();
    let (n_tiles, grid) = tokio::task::spawn_blocking(move || -> Result<(usize, lanka_core::grid::Grid)> {
        let arr = lanka_ingest::grid::read_layer(&root, &bin_path)?;
        let g = arr.grid;
        let st = arr.stats();
        let quant = Quant::for_layer(&arr.enc, st.min, st.max);
        let pyr = Pyramid::build(&arr, g, quant, Overview::Mean, MAX_LEVEL);
        let mut tiles = Vec::new();
        for z in MIN_ZOOM..=MAX_ZOOM {
            let (x0, x1, y0, y1) = values::tile_range(&g, z);
            for x in x0..=x1 {
                for y in y0..=y1 {
                    if let Some(png) = render_tile(&arr, &g, &quant, &pyr, z, x, y)? {
                        tiles.push((z, x, y, png));
                    }
                }
            }
        }
        let n = tiles.len();
        let spec = WriteSpec {
            tile_type: 2,
            tile_compression: Compression::None,
            min_zoom: MIN_ZOOM,
            max_zoom: MAX_ZOOM,
            bounds: [g.lon_min, g.lat_min(), g.lon_max(), g.lat_max],
            center: [(g.lon_min + g.lon_max()) / 2.0, (g.lat_min() + g.lat_max) / 2.0],
            center_zoom: 8,
            metadata: json!({
                "name": DEM, "format": "png", "type": "baselayer", "encoding": "terrarium",
                "description": format!("Terrarium elevation from {ELEVATION_LAYER} {period2} (canonical 0.001° grid; 0 m at sea)"),
                "attribution": "Copernicus DEM GLO-30 © DLR e.V. 2010-2014 and © Airbus Defence and Space GmbH 2014-2018, provided under COPERNICUS by the European Union and ESA",
            }),
        };
        pmtiles::write(&out2, &spec, tiles)?;
        Ok((n, g))
    })
    .await??;
    let build_ms = t0.elapsed().as_millis();
    let (fsha, bytes) = lanka_store::FsStore::hash_file(&out).await?;
    let sha16 = hex::encode(&fsha[..8]);
    let key = format!("{DEM}/{sha16}.pmtiles");
    let blob = store.put_named(Ns::Tile, &key, &out).await?;
    let local = store
        .local_path(Ns::Tile, &key)
        .context("tiles need a local store")?;
    let pm = PmTiles::open(&local)?;
    let h = pm.header.clone();
    let rel = blob.rel_path();
    let layers = json!([{ "id": DEM, "dataset": dataset, "encoding": "terrarium",
                          "grid_layer": ELEVATION_LAYER, "period": period, "tiles": n_tiles }]);
    sqlx::query(
        "INSERT INTO geo.tilesets (id, sha16, path, bytes, minzoom, maxzoom, bounds, layers, dataset_ids,
            version_ids, input_hash, tool, attribution, build_ms)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)
         ON CONFLICT (id, sha16) DO UPDATE SET built_at = now(), input_hash = EXCLUDED.input_hash,
            version_ids = EXCLUDED.version_ids, tool = EXCLUDED.tool, build_ms = EXCLUDED.build_ms",
    )
    .bind(DEM)
    .bind(&sha16)
    .bind(&rel)
    .bind(i64::try_from(bytes)?)
    .bind(i16::from(h.min_zoom))
    .bind(i16::from(h.max_zoom))
    .bind(vec![grid.lon_min, grid.lat_min(), grid.lon_max(), grid.lat_max])
    .bind(&layers)
    .bind(vec![dataset.clone()])
    .bind(version_id.map(|v| vec![v]).unwrap_or_default())
    .bind(hash.as_slice())
    .bind(DEM_FORMAT)
    .bind("Copernicus DEM GLO-30 (ESA, EU)")
    .bind(i64::try_from(build_ms)?)
    .execute(pool)
    .await?;
    let _ = tokio::fs::remove_dir_all(&work).await;
    let mut features = std::collections::BTreeMap::new();
    features.insert(DEM.to_owned(), n_tiles as u64);
    Ok(Built {
        id: DEM.to_owned(),
        sha16,
        path: rel,
        bytes,
        built: true,
        features,
        export_ms: build_ms,
        tippecanoe_ms: 0,
        warnings: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terrarium_round_trips() {
        for h in [-100.0, 0.0, 0.5, 2524.3, 8848.0] {
            let back = terrarium_height(terrarium(h));
            assert!((back - h).abs() < 1.0 / 256.0 + 1e-9, "{h} → {back}");
        }
        assert_eq!(terrarium(0.0), [128, 0, 0]);
    }
}
