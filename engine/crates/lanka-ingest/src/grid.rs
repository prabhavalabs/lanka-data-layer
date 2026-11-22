//! The grid stage (WP-05, 05 §8): raster artifacts → canonical grid layers
//! in the store (`grids/<layer>/<period>/<sha16>.{bin,tif}`, indexed in
//! `geo.grid_layers`) → zonal observations published like any other.
//!
//! For rasters the L1 "extract" is the canonical array itself; the run
//! still persists one `records` extract that lists every layer it built
//! (id, period, sha256, statistics) so each zonal observation points at the
//! layer row it was computed from.
//!
//! The lookups of dataset `grid-lookups` (`grid.gnd_idx`, `grid.land_mask`)
//! are built from the reference boundary set in PostGIS by
//! [`build_lookups`] (and on demand by the first raster run).
//!
//! Zonal computation is `lanka_grid::zonal` (pure, over arrays); this module
//! is the adapter that turns its accumulators into `ObsRow`s and stages
//! them. WP-06's `grid_zonal` method wraps the same two calls.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result, bail, ensure};
use lanka_core::grid::Grid;
use lanka_core::{Basis, Flags, Method, Status};
use lanka_grid::array::{Dtype, GridArray, GridData, LayerEncoding};
use lanka_grid::cog::{Overview, write_cog};
use lanka_grid::lookup::{self, Cells};
use lanka_grid::polygon::MultiPolygon;
use lanka_grid::zonal::{Acc, rollup, zonal};
use lanka_store::{BlobStore, Ns};
use serde::Serialize;
use serde_json::json;
use sqlx::PgPool;

use crate::connector::{CellProv, Connector, Fetched, ObsRow};
use crate::drivers::raster::{BuiltLayer, LayerSpec, Raster, ZonalStat, build_layers};
use crate::extract::{ExtractArtifact, ExtractOut, Record};
use crate::pipeline::{Env, RunReport};
use crate::publish::{StageInput, publish_observations_with};
use crate::run::{RunStatus, RunTracker, Stage};

/// Dataset of the lookup layers.
pub const LOOKUPS_DATASET: &str = "grid-lookups";
pub const GND_IDX: &str = "grid.gnd_idx";
pub const LAND_MASK: &str = "grid.land_mask";

/// A layer written to the store.
#[derive(Debug, Clone, Serialize)]
pub struct StoredLayer {
    pub id: String,
    pub period: String,
    pub sha256: String,
    pub bin_path: String,
    pub cog_path: String,
    pub bytes: u64,
    pub cog_bytes: u64,
    pub count: u64,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub sum: f64,
}

/// Metadata of one layer for `geo.grid_layers`.
pub struct LayerRow<'a> {
    pub id: &'a str,
    pub period: &'a str,
    pub dataset_id: &'a str,
    pub indicator_id: Option<&'a str>,
    pub dims: &'a str,
    pub enc: LayerEncoding,
    pub unit: &'a str,
    pub zonal: &'a str,
    pub overview: Overview,
    pub serve: bool,
    pub source_res_m: f64,
    pub resampling: &'a str,
    pub style: Option<serde_json::Value>,
    pub version_id: Option<i64>,
}

/// Quantises, writes `.bin` + COG into the store (CPU work on a blocking thread).
#[allow(clippy::too_many_arguments)]
pub async fn store_layer(
    store: &dyn BlobStore,
    grid: Grid,
    id: &str,
    period: &str,
    enc: LayerEncoding,
    overview: Overview,
    values: Vec<f32>,
    json: serde_json::Value,
) -> Result<(StoredLayer, GridArray)> {
    let tmp_bin = store.tmp_file("grid.bin")?;
    let tmp_tif = store.tmp_file("grid.tif")?;
    let (tb, tt) = (tmp_bin.clone(), tmp_tif.clone());
    let (arr, written, cog_bytes, stats) = tokio::task::spawn_blocking(move || -> Result<_> {
        let arr = GridArray::quantise(grid, enc, &values)?;
        let w = arr.write_bin(&tb, json)?;
        let c = write_cog(&tt, &arr, overview)?;
        let s = arr.stats();
        Ok((arr, w, c, s))
    })
    .await??;
    let sha16 = &hex::encode(written.sha256)[..16];
    let bin = store
        .put_named(Ns::Grid, &format!("{id}/{period}/{sha16}.bin"), &tmp_bin)
        .await?;
    let tif = store
        .put_named(Ns::Grid, &format!("{id}/{period}/{sha16}.tif"), &tmp_tif)
        .await?;
    let _ = tokio::fs::remove_file(&tmp_bin).await;
    let _ = tokio::fs::remove_file(&tmp_tif).await;
    Ok((
        StoredLayer {
            id: id.to_owned(),
            period: period.to_owned(),
            sha256: hex::encode(written.sha256),
            bin_path: bin.rel_path(),
            cog_path: tif.rel_path(),
            bytes: written.bytes,
            cog_bytes,
            count: stats.count,
            min: stats.min,
            max: stats.max,
            sum: stats.sum,
        },
        arr,
    ))
}

/// Upserts a layer row.
pub async fn upsert_layer(pool: &PgPool, r: &LayerRow<'_>, s: &StoredLayer) -> Result<()> {
    let sha = hex::decode(&s.sha256)?;
    sqlx::query(
        "INSERT INTO geo.grid_layers AS t (id, period, dataset_id, indicator_id, dtype, scale, \"offset\", nodata,
            unit, zonal, source_res_m, bin_path, cog_path, sha256, version_id, dims, overview, serve, resampling,
            stats, style, bytes, cog_bytes)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20, $21, $22, $23)
         ON CONFLICT (id, period) DO UPDATE SET dataset_id = EXCLUDED.dataset_id, indicator_id = EXCLUDED.indicator_id,
            dtype = EXCLUDED.dtype, scale = EXCLUDED.scale, \"offset\" = EXCLUDED.\"offset\", nodata = EXCLUDED.nodata,
            unit = EXCLUDED.unit, zonal = EXCLUDED.zonal, source_res_m = EXCLUDED.source_res_m,
            bin_path = EXCLUDED.bin_path, cog_path = EXCLUDED.cog_path, sha256 = EXCLUDED.sha256,
            version_id = COALESCE(EXCLUDED.version_id, t.version_id), dims = EXCLUDED.dims,
            overview = EXCLUDED.overview, serve = EXCLUDED.serve, resampling = EXCLUDED.resampling,
            stats = EXCLUDED.stats, style = EXCLUDED.style, bytes = EXCLUDED.bytes, cog_bytes = EXCLUDED.cog_bytes,
            created_at = CASE WHEN t.sha256 = EXCLUDED.sha256 THEN t.created_at ELSE now() END",
    )
    .bind(r.id)
    .bind(r.period)
    .bind(r.dataset_id)
    .bind(r.indicator_id)
    .bind(r.enc.dtype.as_str())
    .bind(r.enc.scale)
    .bind(r.enc.offset)
    .bind(r.enc.nodata)
    .bind(r.unit)
    .bind(r.zonal)
    .bind(r.source_res_m)
    .bind(&s.bin_path)
    .bind(&s.cog_path)
    .bind(sha)
    .bind(r.version_id)
    .bind(r.dims)
    .bind(r.overview.as_str())
    .bind(r.serve)
    .bind(r.resampling)
    .bind(json!({"count": s.count, "min": s.min, "max": s.max, "sum": s.sum}))
    .bind(&r.style)
    .bind(i64::try_from(s.bytes)?)
    .bind(i64::try_from(s.cog_bytes)?)
    .execute(pool)
    .await?;
    Ok(())
}

// ───────────────────────────── lookups ─────────────────────────────────────

/// What [`build_lookups`] produced.
#[derive(Debug, Clone, Serialize)]
pub struct LookupBuild {
    pub boundary_set: String,
    pub units: usize,
    pub vertices: usize,
    pub report: lookup::LookupReport,
    pub gnd_idx: StoredLayer,
    pub land_mask: StoredLayer,
    pub build_ms: u128,
}

async fn reference_set(pool: &PgPool) -> Result<String> {
    sqlx::query_scalar("SELECT id FROM geo.boundary_sets WHERE is_reference ORDER BY id LIMIT 1")
        .fetch_optional(pool)
        .await?
        .context("no reference boundary set (load the boundaries first)")
}

/// Builds `grid.gnd_idx` + `grid.land_mask` from the deepest level of the
/// reference boundary set (units in p-code order = slots 1…n).
pub async fn build_lookups(env: &Env<'_>) -> Result<LookupBuild> {
    let set = reference_set(env.pool).await?;
    let level = i16::from(env.profile.admin.max_level());
    let rows: Vec<(i32, String, Vec<u8>)> = sqlx::query_as(
        "SELECT u.key, u.code, ST_AsBinary(g.geom) FROM geo.unit_geoms g JOIN geo.units u ON u.key = g.unit_key
         WHERE g.boundary_set = $1 AND u.level = $2 AND u.valid_to IS NULL ORDER BY u.key",
    )
    .bind(&set)
    .bind(level)
    .fetch_all(env.pool)
    .await?;
    ensure!(
        !rows.is_empty(),
        "boundary set {set} has no level-{level} geometry"
    );
    ensure!(
        rows.len() < 65_535,
        "{} units do not fit a u16 index",
        rows.len()
    );
    let grid = env.profile.grid;
    let geoms: Vec<MultiPolygon> = rows
        .iter()
        .map(|(_, code, wkb)| {
            MultiPolygon::from_wkb(wkb).with_context(|| format!("geometry of {code}"))
        })
        .collect::<Result<_>>()?;
    let vertices = geoms.iter().map(MultiPolygon::vertices).sum();
    let t0 = Instant::now();
    let built =
        tokio::task::spawn_blocking(move || lookup::build(&grid, &geoms, Cells::All)).await?;
    let build_ms = t0.elapsed().as_millis();
    let meta = |id: &str| json!({"layer": id, "period": set, "dataset": LOOKUPS_DATASET});
    #[allow(clippy::cast_precision_loss)]
    let gnd_vals: Vec<f32> = built.gnd_idx.iter().map(|&v| f32::from(v)).collect();
    let mask_vals: Vec<f32> = built.land_mask.iter().map(|&v| f32::from(v)).collect();
    let u16enc = LayerEncoding {
        dtype: Dtype::U16,
        scale: 1.0,
        offset: 0.0,
        nodata: None,
    };
    let u8enc = LayerEncoding {
        dtype: Dtype::U8,
        ..u16enc
    };
    let (gs, _) = store_layer(
        env.store,
        grid,
        GND_IDX,
        &set,
        u16enc,
        Overview::Mode,
        gnd_vals,
        meta(GND_IDX),
    )
    .await?;
    let (ms, _) = store_layer(
        env.store,
        grid,
        LAND_MASK,
        &set,
        u8enc,
        Overview::Max,
        mask_vals,
        meta(LAND_MASK),
    )
    .await?;
    let mut tx = env.pool.begin().await?;
    for (id, enc, s, res) in [
        (GND_IDX, u16enc, &gs, "pip_centre_v1"),
        (LAND_MASK, u8enc, &ms, "boundary_supercover"),
    ] {
        let row = LayerRow {
            id,
            period: &set,
            dataset_id: LOOKUPS_DATASET,
            indicator_id: None,
            dims: "",
            enc,
            unit: "1",
            zonal: "none",
            overview: if id == GND_IDX {
                Overview::Mode
            } else {
                Overview::Max
            },
            serve: true,
            source_res_m: 111.0,
            resampling: res,
            style: None,
            version_id: None,
        };
        // Same statements as upsert_layer, inside the transaction.
        upsert_layer_tx(&mut tx, &row, s).await?;
    }
    sqlx::query("DELETE FROM geo.grid_lookup_units WHERE layer_id = $1 AND period = $2")
        .bind(GND_IDX)
        .bind(&set)
        .execute(&mut *tx)
        .await?;
    let idx: Vec<i32> = (1..=rows.len())
        .map(i32::try_from)
        .collect::<std::result::Result<_, _>>()?;
    let keys: Vec<i32> = rows.iter().map(|r| r.0).collect();
    sqlx::query(
        "INSERT INTO geo.grid_lookup_units (layer_id, period, idx, unit_key, boundary_set)
         SELECT $1, $2, i, k, $2 FROM unnest($3::int[], $4::int[]) AS t(i, k)",
    )
    .bind(GND_IDX)
    .bind(&set)
    .bind(&idx)
    .bind(&keys)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(LookupBuild {
        boundary_set: set,
        units: rows.len(),
        vertices,
        report: built.report,
        gnd_idx: gs,
        land_mask: ms,
        build_ms,
    })
}

async fn upsert_layer_tx(
    tx: &mut sqlx::PgConnection,
    r: &LayerRow<'_>,
    s: &StoredLayer,
) -> Result<()> {
    sqlx::query("DELETE FROM geo.grid_layers WHERE id = $1 AND period = $2 AND sha256 <> $3")
        .bind(r.id)
        .bind(r.period)
        .bind(hex::decode(&s.sha256)?)
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "INSERT INTO geo.grid_layers (id, period, dataset_id, indicator_id, dtype, scale, \"offset\", nodata,
            unit, zonal, source_res_m, bin_path, cog_path, sha256, version_id, dims, overview, serve, resampling,
            stats, style, bytes, cog_bytes)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20, $21, $22, $23)
         ON CONFLICT (id, period) DO NOTHING",
    )
    .bind(r.id)
    .bind(r.period)
    .bind(r.dataset_id)
    .bind(r.indicator_id)
    .bind(r.enc.dtype.as_str())
    .bind(r.enc.scale)
    .bind(r.enc.offset)
    .bind(r.enc.nodata)
    .bind(r.unit)
    .bind(r.zonal)
    .bind(r.source_res_m)
    .bind(&s.bin_path)
    .bind(&s.cog_path)
    .bind(hex::decode(&s.sha256)?)
    .bind(r.version_id)
    .bind(r.dims)
    .bind(r.overview.as_str())
    .bind(r.serve)
    .bind(r.resampling)
    .bind(json!({"count": s.count, "min": s.min, "max": s.max, "sum": s.sum}))
    .bind(&r.style)
    .bind(i64::try_from(s.bytes)?)
    .bind(i64::try_from(s.cog_bytes)?)
    .execute(&mut *tx)
    .await?;
    Ok(())
}

/// The lookup arrays and slot table, loaded for zonal passes.
pub struct Lookups {
    pub boundary_set: String,
    pub gnd_idx: Vec<u16>,
    /// `gnd_idx` restricted to cells whose centre is inside a unit
    /// (`land_mask` 1 or 2); other cells are slot 0.
    pub gnd_inside: Vec<u16>,
    /// `slot_unit[s]` = `geo.units.key` of slot `s` (index 0 unused).
    pub slot_unit: Vec<i32>,
}

/// Loads the current `gnd_idx` of the reference set; builds it when missing.
pub async fn load_lookups(env: &Env<'_>, root: &Path) -> Result<Lookups> {
    let set = reference_set(env.pool).await?;
    let q = |id: &'static str| {
        sqlx::query_scalar::<_, String>(
            "SELECT bin_path FROM geo.grid_layers WHERE id = $1 AND period = $2",
        )
        .bind(id)
        .bind(set.clone())
        .fetch_optional(env.pool)
    };
    let (mut gpath, mut mpath) = (q(GND_IDX).await?, q(LAND_MASK).await?);
    if gpath.is_none() || mpath.is_none() {
        let b = build_lookups(env).await?;
        gpath = Some(b.gnd_idx.bin_path);
        mpath = Some(b.land_mask.bin_path);
    }
    let gpath = root.join(gpath.context("gnd_idx")?);
    let mpath = root.join(mpath.context("land_mask")?);
    let (gnd_idx, gnd_inside) =
        tokio::task::spawn_blocking(move || -> Result<(Vec<u16>, Vec<u16>)> {
            let (g, _) = GridArray::read_bin(&gpath)?;
            let (m, _) = GridArray::read_bin(&mpath)?;
            let (GridData::U16(g), GridData::U8(m)) = (g.data, m.data) else {
                bail!("gnd_idx/land_mask have unexpected dtypes");
            };
            let inside = g
                .iter()
                .zip(&m)
                .map(|(&s, &k)| if lookup::centre_inside(k) { s } else { 0 })
                .collect();
            Ok((g, inside))
        })
        .await??;
    let slots: Vec<(i32, i32)> = sqlx::query_as(
        "SELECT idx, unit_key FROM geo.grid_lookup_units WHERE layer_id = $1 AND period = $2 ORDER BY idx",
    )
    .bind(GND_IDX)
    .bind(&set)
    .fetch_all(env.pool)
    .await?;
    let n = slots.last().map_or(0, |s| s.0);
    let mut slot_unit = vec![0; usize::try_from(n)? + 1];
    for (i, k) in slots {
        slot_unit[usize::try_from(i)?] = k;
    }
    Ok(Lookups {
        boundary_set: set,
        gnd_idx,
        gnd_inside,
        slot_unit,
    })
}

// ───────────────────────────── the raster run ──────────────────────────────

/// Builds the layers of a raster source from its archived artifacts,
/// registers them and publishes their zonal observations.
pub async fn build_and_publish(
    env: &Env<'_>,
    run: &RunTracker,
    c: &dyn Connector,
    r: &Raster,
    fetched: &[Fetched],
) -> Result<RunReport> {
    let root = local_root(env.store)?;
    let grid = env.profile.grid;
    let paths = r.paths(fetched)?;
    let spec = r.spec.clone();
    let t0 = Instant::now();
    let built: Vec<BuiltLayer> =
        tokio::task::spawn_blocking(move || build_layers(&spec, &grid, &paths)).await??;
    let build_ms = t0.elapsed().as_millis();
    run.info(
        Stage::Extract,
        &format!("{} grid layer(s) resampled in {build_ms} ms", built.len()),
        Some(json!(
            built
                .iter()
                .map(|b| json!({"layer": b.spec.id, "period": b.spec.period, "note": b.note}))
                .collect::<Vec<_>>()
        )),
    )
    .await;

    // Store every layer; keep the quantised arrays for the zonal pass.
    let t1 = Instant::now();
    let mut stored: Vec<(LayerSpec, StoredLayer, GridArray)> = Vec::with_capacity(built.len());
    for b in built {
        let enc = b.spec.encoding()?;
        let ov = Overview::parse(&b.spec.overview)?;
        let meta = json!({
            "layer": b.spec.id, "period": b.spec.period, "dataset": r.spec.dataset,
            "indicator": b.spec.indicator, "dims": b.spec.dims, "unit": b.spec.unit,
        });
        let (s, arr) = store_layer(
            env.store,
            grid,
            &b.spec.id,
            &b.spec.period,
            enc,
            ov,
            b.values,
            meta,
        )
        .await?;
        stored.push((b.spec, s, arr));
    }
    let store_ms = t1.elapsed().as_millis();

    // L1 record per layer (the zonal observations point at these rows).
    let records: Vec<Record> = stored
        .iter()
        .enumerate()
        .map(|(i, (l, s, _))| Record {
            table_id: "grid_layers".to_owned(),
            record_idx: u32::try_from(i).unwrap_or(u32::MAX),
            path: s.bin_path.clone(),
            fields: BTreeMap::from([
                ("layer".to_owned(), l.id.clone()),
                ("period".to_owned(), l.period.clone()),
                (
                    "indicator".to_owned(),
                    l.indicator.clone().unwrap_or_default(),
                ),
                ("dims".to_owned(), l.dims.clone()),
                ("resample".to_owned(), l.resample.as_str().to_owned()),
                ("sha256".to_owned(), s.sha256.clone()),
                ("cells".to_owned(), s.count.to_string()),
                ("sum".to_owned(), s.sum.to_string()),
            ]),
            artifact_idx: 0,
        })
        .collect();
    let artifacts: Vec<ExtractArtifact> = fetched
        .iter()
        .map(|f| ExtractArtifact {
            artifact_id: f.artifact_id,
            key: f.key.clone(),
            url: f.url.clone(),
            sha256: f.sha256,
        })
        .collect();
    let ext =
        crate::pipeline::persist_extract(env, run, c, ExtractOut::Records(records), &artifacts)
            .await?;

    // Zonal pass.
    let lk = load_lookups(env, &root).await?;
    let t2 = Instant::now();
    let (rows, zreport) = zonal_rows(env.pool, &r.spec.zonal, &stored, &lk, &grid).await?;
    let zonal_ms = t2.elapsed().as_millis();
    run.info(
        Stage::Standardise,
        &format!(
            "zonal: {} observation(s) from {} layer pass(es) in {zonal_ms} ms ({} unit(s) without cells skipped)",
            rows.len(),
            zreport.passes,
            zreport.empty_units
        ),
        Some(json!(zreport)),
    )
    .await;

    let artifact_ids: Vec<i64> = fetched.iter().map(|f| f.artifact_id).collect();
    let mut published = Vec::new();
    let mut written = 0;
    let mut version_id = None;
    if !rows.is_empty() {
        // The dataset's expectations (03 §5.2), as for any other observations.
        let lookup = crate::pipeline::load_lookup(env.pool, env.profile).await?;
        let opts = crate::pipeline::expectations(env, run, &lookup, &r.spec.dataset, &rows).await?;
        let p = publish_observations_with(
            env.pool,
            env.registry,
            env.profile,
            StageInput {
                run_id: run.run_id,
                dataset_id: &r.spec.dataset,
                vintage: None,
                rows: &rows,
                artifact_ids: &artifact_ids,
                extract_ids: &[ext.id],
            },
            &opts,
        )
        .await
        .with_context(|| format!("publishing {}", r.spec.dataset))?;
        run.info(
            Stage::Publish,
            &format!("{}: {} — {}", p.dataset_id, p.outcome, p.note),
            Some(json!(p)),
        )
        .await;
        written = p.rows_written;
        version_id = Some(p.version_id);
        published.push(p);
    }
    for (l, s, _) in &stored {
        let style = l.style.as_ref().map(serde_json::to_value).transpose()?;
        upsert_layer(
            env.pool,
            &LayerRow {
                id: &l.id,
                period: &l.period,
                dataset_id: &r.spec.dataset,
                indicator_id: l.indicator.as_deref(),
                dims: &l.dims,
                enc: l.encoding()?,
                unit: &l.unit,
                zonal: &l.zonal,
                overview: Overview::parse(&l.overview)?,
                serve: l.serve,
                source_res_m: l.source_res_m,
                resampling: l.resample.as_str(),
                style,
                version_id,
            },
            s,
        )
        .await?;
    }
    run.info(
        Stage::Publish,
        &format!(
            "{} grid layer(s) registered ({} bytes .bin, {} bytes COG; store {store_ms} ms)",
            stored.len(),
            stored.iter().map(|s| s.1.bytes).sum::<u64>(),
            stored.iter().map(|s| s.1.cog_bytes).sum::<u64>()
        ),
        Some(json!(stored.iter().map(|s| &s.1).collect::<Vec<_>>())),
    )
    .await;
    run.add_counts(
        i32::try_from(fetched.len())?,
        i64::try_from(rows.len())?,
        written,
    )
    .await?;
    let status = if published.iter().any(|p| p.outcome == "in_review") {
        RunStatus::NeedsReview
    } else if published.iter().all(|p| p.outcome == "unchanged") {
        RunStatus::NoChange
    } else {
        RunStatus::Succeeded
    };
    Ok(RunReport {
        run_id: 0,
        source_id: c.source_id().to_owned(),
        status,
        artifacts: fetched.len(),
        fetched: 0,
        extracts: 1,
        published,
        crosswalks: 0,
        short_circuit: None,
    })
}

/// The store root (grids are read through local paths).
pub fn local_root(store: &dyn BlobStore) -> Result<PathBuf> {
    let p = store
        .local_path(Ns::Grid, "x")
        .context("grid layers need a local store (mmap)")?;
    Ok(p.parent()
        .and_then(Path::parent)
        .context("store root")?
        .to_path_buf())
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ZonalReport {
    pub passes: usize,
    /// Units with no cell centre (no value published).
    pub empty_units: usize,
    /// Per layer: grid total, Σ of the deepest-level units, unassigned cells' sum.
    pub totals: Vec<serde_json::Value>,
    pub pass_ms: Vec<(String, u128)>,
}

#[derive(sqlx::FromRow)]
struct UnitRow {
    key: i32,
    code: String,
    level: i16,
    anc_keys: Vec<i32>,
}

/// Zonal observations for a raster spec's `zonal` entries.
pub async fn zonal_rows(
    pool: &PgPool,
    specs: &[crate::drivers::raster::ZonalSpec],
    stored: &[(LayerSpec, StoredLayer, GridArray)],
    lk: &Lookups,
    grid: &Grid,
) -> Result<(Vec<ObsRow>, ZonalReport)> {
    let units: Vec<UnitRow> =
        sqlx::query_as("SELECT key, code, level, anc_keys FROM geo.units WHERE valid_to IS NULL")
            .fetch_all(pool)
            .await?;
    let by_key: HashMap<i32, &UnitRow> = units.iter().map(|u| (u.key, u)).collect();
    // Slot → the unit and every ancestor.
    let parents: Vec<Vec<i32>> = lk
        .slot_unit
        .iter()
        .map(|&k| {
            let mut v = vec![k];
            if let Some(u) = by_key.get(&k) {
                v.extend(&u.anc_keys);
            }
            v
        })
        .collect();
    let slots = lk.slot_unit.len().saturating_sub(1);
    let areas = lanka_grid::row_areas(grid);
    let mut rep = ZonalReport::default();
    // A zonal entry names a layer id; its `period` picks the layer period
    // (required when the id has several).
    let resolve = |id: &str, period: Option<&str>| -> Result<usize> {
        let hits: Vec<usize> = stored
            .iter()
            .enumerate()
            .filter(|(_, s)| s.0.id == id && period.is_none_or(|p| s.0.period == p))
            .map(|(i, _)| i)
            .collect();
        match hits.as_slice() {
            [i] => Ok(*i),
            [] => bail!("zonal: layer {id} {period:?} not built"),
            _ => bail!("zonal: layer {id} has several periods; give `period`"),
        }
    };
    // One pass per layer used.
    let mut accs: HashMap<(usize, bool), Vec<(i32, Acc)>> = HashMap::new();
    for z in specs {
        let inside = z.inside_only()?;
        let mut used: Vec<usize> = Vec::new();
        for (_, l) in z.series() {
            used.push(resolve(&l, z.period.as_deref())?);
        }
        for o in &z.of {
            used.push(resolve(o, z.period.as_deref())?);
        }
        for i in used {
            if accs.contains_key(&(i, inside)) {
                continue;
            }
            let (lay, _, arr) = &stored[i];
            let label = format!(
                "{} {}{}",
                lay.id,
                lay.period,
                if inside { " (centre inside)" } else { "" }
            );
            let idx = if inside { &lk.gnd_inside } else { &lk.gnd_idx };
            let t = Instant::now();
            let a = zonal(idx, arr, grid.cols, &areas, slots);
            rep.pass_ms.push((label.clone(), t.elapsed().as_millis()));
            rep.passes += 1;
            let grid_total: f64 = arr.stats().sum;
            let leaves: f64 = a.iter().skip(1).map(|x| x.sum).sum();
            ensure!(
                (leaves + a[0].sum - grid_total).abs() <= 1e-6 * grid_total.abs().max(1.0),
                "zonal pass of {label} does not preserve the grid total"
            );
            rep.totals.push(json!({"layer": label, "grid_total": grid_total, "units_total": leaves, "unassigned": a[0].sum, "unassigned_cells": a[0].n}));
            accs.insert((i, inside), rollup(&a, &parents));
        }
    }
    // Unit areas over every assigned cell (valid or not), for `density`: a
    // constrained population raster is nodata outside mapped settlement, and
    // a density divides by the unit's land, not by its settled cells.
    let mut unit_area: HashMap<bool, HashMap<i32, f64>> = HashMap::new();
    for z in specs.iter().filter(|z| z.stat == ZonalStat::Density) {
        let inside = z.inside_only()?;
        if unit_area.contains_key(&inside) {
            continue;
        }
        let idx = if inside { &lk.gnd_inside } else { &lk.gnd_idx };
        let ones = vec![0.0f32; idx.len()];
        let a = zonal(idx, ones.as_slice(), grid.cols, &areas, slots);
        unit_area.insert(
            inside,
            rollup(&a, &parents)
                .into_iter()
                .map(|(k, a)| (k, a.area))
                .collect(),
        );
    }
    let mut rows = Vec::new();
    let mut empty = std::collections::BTreeSet::new();
    for z in specs {
        let levels = z.levels.clone().unwrap_or_else(|| vec![1, 2, 3, 4]);
        let basis = match z.basis.as_deref() {
            None | Some("computed") => Basis::Computed,
            Some("estimated") => Basis::Estimated,
            Some("reported") => Basis::Reported,
            Some(b) => bail!("zonal {}: basis {b:?}", z.indicator),
        };
        let factor = z.factor.unwrap_or(1.0);
        let inside = z.inside_only()?;
        // Denominator for shares: Σ area_sum over `of`, per unit.
        let mut den: HashMap<i32, f64> = HashMap::new();
        for o in &z.of {
            for (k, a) in &accs[&(resolve(o, z.period.as_deref())?, inside)] {
                *den.entry(*k).or_default() += a.wsum;
            }
        }
        for (dims, l) in z.series() {
            let li = resolve(&l, z.period.as_deref())?;
            let up = &accs[&(li, inside)];
            let lay = &stored[li].0;
            let period = z.period.clone().unwrap_or_else(|| lay.period.clone());
            for (k, a) in up {
                let Some(u) = by_key.get(k) else { continue };
                if !levels.contains(&u.level) {
                    continue;
                }
                let v = match z.stat {
                    ZonalStat::Share => {
                        let d = den.get(k).copied().unwrap_or(0.0);
                        if a.n == 0 || d <= 0.0 {
                            None
                        } else {
                            Some(a.wsum / d)
                        }
                    }
                    // Persons (or any count) per km² of the unit's valid cells.
                    // Σ v per km² of all the unit's (inside) cells.
                    ZonalStat::Density => {
                        let area = unit_area
                            .get(&inside)
                            .and_then(|m| m.get(k))
                            .copied()
                            .unwrap_or(0.0);
                        (area > 0.0).then(|| a.sum / (area / 1e6))
                    }
                    s => a.stat(s.acc_stat()),
                };
                let Some(v) = v else {
                    empty.insert(*k);
                    continue;
                };
                rows.push(ObsRow {
                    indicator_id: z.indicator.clone(),
                    geo_code: u.code.clone(),
                    period: period.clone(),
                    dims: dims.clone(),
                    value: Some(v * factor),
                    lo: None,
                    hi: None,
                    basis,
                    status: Status::Normal,
                    flags: Flags::EMPTY,
                    artifact: 0,
                    prov: Some(CellProv {
                        extract: 0,
                        row: u32::try_from(li)?,
                    }),
                    locator: json!({"layer": lay.id, "period": lay.period, "stat": format!("{:?}", z.stat).to_lowercase(), "cells": a.n}),
                    verbatim: None,
                    method: Method::Spatial,
                    confidence: None,
                });
            }
        }
    }
    rep.empty_units = empty.len();
    Ok((rows, rep))
}

/// Re-reads a stored layer (tools, parity).
pub fn read_layer(root: &Path, bin_path: &str) -> Result<GridArray> {
    Ok(GridArray::read_bin(&root.join(bin_path))?.0)
}
