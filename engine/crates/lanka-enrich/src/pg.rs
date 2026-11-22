//! [`Inputs`] over pinned dataset versions in Postgres.
//!
//! A pinned version `seq` of a dataset is the set of rows with
//! `vfrom <= seq <= vto`, minus — for a version that is approved but not
//! released yet — the rows it will close at release (`data.version_closes`).
//! Reads are one indexed range scan per port, cached for the run.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, bail, ensure};
use async_trait::async_trait;
use lanka_core::{Basis, Calendars, Flags, Freq, Status};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tokio::sync::Mutex;

use crate::method::{Feature, GridInput, IndicatorMeta, Inputs, Row, Unit, Units};

/// `geo.grid_layers` ids of the lookup arrays (WP-05, `lanka-ingest::grid`).
const GND_IDX: &str = "grid.gnd_idx";
const LAND_MASK: &str = "grid.land_mask";

/// (port, layer, period) of a loaded grid layer.
type GridKey = (String, String, Option<String>);

/// `gnd_idx` of the reference set as unit indices (`u32::MAX` = no unit).
pub struct GridLookup {
    pub unit_idx: Arc<[u32]>,
    pub unit_idx_inside: Arc<[u32]>,
    pub units: Arc<[String]>,
    pub cols: u32,
    pub row_area: Arc<[f64]>,
}

/// One input port bound to a dataset version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pinned {
    pub port: String,
    pub dataset_id: String,
    pub dataset_key: i32,
    pub version_id: i64,
    pub seq: i32,
    pub dsv: String,
    /// `catalog.datasets.kind`
    pub kind: String,
    /// Indicator filter of the recipe input (empty = all).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub indicators: Vec<String>,
}

pub struct PgInputs {
    pool: PgPool,
    calendars: Calendars,
    reference_set: String,
    scheme: String,
    pinned: HashMap<String, Pinned>,
    indicators: HashMap<String, IndicatorMeta>,
    obs: Mutex<HashMap<String, Arc<Vec<Row>>>>,
    features: Mutex<HashMap<String, Arc<Vec<Feature>>>>,
    units: Mutex<Option<Arc<Units>>>,
    /// Blob store root for grid layers (`grid_zonal`).
    store_root: Option<PathBuf>,
    lookup: Mutex<Option<Arc<GridLookup>>>,
    grids: Mutex<HashMap<GridKey, Arc<GridInput>>>,
}

type ObsDbRow = (
    i64,
    String,
    String,
    i16,
    String,
    chrono::NaiveDate,
    String,
    Option<f64>,
    Option<f64>,
    Option<f64>,
    i16,
    i16,
    i32,
);

type IndRow = (
    String,
    String,
    String,
    String,
    String,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<f32>,
    serde_json::Value,
);

/// Every indicator of the catalogue.
pub async fn load_indicators(pool: &PgPool) -> Result<HashMap<String, IndicatorMeta>> {
    let rows: Vec<IndRow> = sqlx::query_as(
        "SELECT id, dataset_id, freq::text, measure_type, aggregation, class, numerator_id,
                denominator_id, weight_id, ci_level, dim_defs
         FROM catalog.indicators",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .filter_map(|r| {
            let freq = r.2.chars().next().and_then(Freq::from_char)?;
            let dims =
                r.10.as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|d| d["id"].as_str().map(str::to_owned))
                            .collect()
                    })
                    .unwrap_or_default();
            Some((
                r.0.clone(),
                IndicatorMeta {
                    id: r.0,
                    dataset_id: r.1,
                    freq,
                    measure_type: r.3,
                    aggregation: r.4,
                    class: r.5,
                    numerator: r.6,
                    denominator: r.7,
                    weight: r.8,
                    ci_level: r.9,
                    dims,
                },
            ))
        })
        .collect())
}

impl PgInputs {
    pub async fn new(
        pool: PgPool,
        calendars: Calendars,
        reference_set: &str,
        scheme: &str,
        pinned: Vec<Pinned>,
    ) -> Result<Self> {
        let indicators = load_indicators(&pool).await?;
        Ok(Self {
            pool,
            calendars,
            reference_set: reference_set.to_owned(),
            scheme: scheme.to_owned(),
            pinned: pinned.into_iter().map(|p| (p.port.clone(), p)).collect(),
            indicators,
            obs: Mutex::new(HashMap::new()),
            features: Mutex::new(HashMap::new()),
            units: Mutex::new(None),
            store_root: None,
            lookup: Mutex::new(None),
            grids: Mutex::new(HashMap::new()),
        })
    }

    /// Where grid layers (`geo.grid_layers.bin_path`) are read from.
    #[must_use]
    pub fn with_store_root(mut self, root: Option<&Path>) -> Self {
        self.store_root = root.map(Path::to_path_buf);
        self
    }

    fn store_root(&self) -> Result<&Path> {
        self.store_root
            .as_deref()
            .context("grid inputs need the blob store root (set LANKA_STORE_DIR)")
    }

    /// `gnd_idx` + `land_mask` of the reference set and their slot table.
    /// The reference set's cell → unit lookup (read once per `PgInputs`).
    /// Lets a caller that already holds a layer's cells (the server's mapped
    /// grids) align them without reading the layer from the store.
    pub async fn lookup(&self) -> Result<Arc<GridLookup>> {
        let mut cache = self.lookup.lock().await;
        if let Some(l) = cache.as_ref() {
            return Ok(l.clone());
        }
        let root = self.store_root()?.to_path_buf();
        let path = |id: &'static str| {
            sqlx::query_scalar::<_, String>(
                "SELECT bin_path FROM geo.grid_layers WHERE id = $1 AND period = $2",
            )
            .bind(id)
            .bind(self.reference_set.clone())
            .fetch_optional(&self.pool)
        };
        let g = path(GND_IDX).await?.with_context(|| {
            format!(
                "no {GND_IDX} for boundary set {} (run `lanka grid lookups`)",
                self.reference_set
            )
        })?;
        let m = path(LAND_MASK)
            .await?
            .with_context(|| format!("no {LAND_MASK} for {}", self.reference_set))?;
        let slots: Vec<(i32, String)> = sqlx::query_as(
            "SELECT l.idx, u.code FROM geo.grid_lookup_units l JOIN geo.units u ON u.key = l.unit_key
             WHERE l.layer_id = $1 AND l.period = $2 ORDER BY l.idx",
        )
        .bind(GND_IDX)
        .bind(&self.reference_set)
        .fetch_all(&self.pool)
        .await?;
        let n = usize::try_from(slots.last().map_or(0, |s| s.0))?;
        // slot s (1-based) → unit index s - 1; holes keep an empty code.
        let mut units = vec![String::new(); n];
        for (i, code) in slots {
            let i = usize::try_from(i)?;
            ensure!(i >= 1, "grid_lookup_units: slot 0 is reserved");
            units[i - 1] = code;
        }
        let l = tokio::task::spawn_blocking(move || -> Result<GridLookup> {
            use lanka_grid::{GridArray, GridData, lookup::centre_inside};
            let (ga, _) = GridArray::read_bin(&root.join(&g))?;
            let (ma, _) = GridArray::read_bin(&root.join(&m))?;
            let grid = ga.grid;
            let (GridData::U16(gi), GridData::U8(mk)) = (ga.data, ma.data) else {
                bail!("gnd_idx/land_mask have unexpected dtypes");
            };
            ensure!(
                gi.len() == mk.len(),
                "gnd_idx and land_mask differ in length"
            );
            let to = |s: u16| if s == 0 { u32::MAX } else { u32::from(s) - 1 };
            let unit_idx: Vec<u32> = gi.iter().map(|&s| to(s)).collect();
            let inside: Vec<u32> = gi
                .iter()
                .zip(&mk)
                .map(|(&s, &k)| if centre_inside(k) { to(s) } else { u32::MAX })
                .collect();
            Ok(GridLookup {
                unit_idx: Arc::from(unit_idx),
                unit_idx_inside: Arc::from(inside),
                units: Arc::from(units),
                cols: grid.cols,
                row_area: Arc::from(lanka_grid::row_areas(&grid)),
            })
        })
        .await??;
        let l = Arc::new(l);
        *cache = Some(l.clone());
        Ok(l)
    }

    fn pin(&self, port: &str) -> Result<&Pinned> {
        self.pinned
            .get(port)
            .with_context(|| format!("input {port:?} is not bound to a dataset version"))
    }
}

#[async_trait]
impl Inputs for PgInputs {
    async fn obs(&self, port: &str) -> Result<Arc<Vec<Row>>> {
        let mut cache = self.obs.lock().await;
        if let Some(t) = cache.get(port) {
            return Ok(t.clone());
        }
        let p = self.pin(port)?;
        let filter: Option<Vec<String>> = (!p.indicators.is_empty()).then(|| p.indicators.clone());
        let rows: Vec<ObsDbRow> = sqlx::query_as(
            "SELECT o.obs_id, i.id, u.code, o.geo_level, o.freq::text, o.time_start, ds.dims,
                    o.value, o.lo, o.hi, o.basis, o.status, o.flags
             FROM data.observations o
             JOIN catalog.indicators i ON i.key = o.indicator_key
             JOIN geo.units u ON u.key = o.geo_key
             JOIN catalog.dim_sets ds ON ds.key = o.dims_key
             WHERE o.dataset_key = $1 AND o.vfrom <= $2 AND o.vto >= $2
               AND NOT EXISTS (SELECT 1 FROM data.version_closes c
                               WHERE c.version_id = $3 AND c.obs_id = o.obs_id)
               AND ($4::text[] IS NULL OR i.id = ANY($4))
             ORDER BY i.id, u.code, o.freq, o.time_start, ds.dims",
        )
        .bind(p.dataset_key)
        .bind(p.seq)
        .bind(p.version_id)
        .bind(filter)
        .fetch_all(&self.pool)
        .await
        .with_context(|| format!("reading input {port} ({} {})", p.dataset_id, p.dsv))?;
        let t: Vec<Row> = rows
            .into_iter()
            .map(|r| {
                Ok(Row {
                    obs_id: Some(r.0),
                    indicator: r.1,
                    geo: r.2,
                    level: r.3,
                    freq: r
                        .4
                        .chars()
                        .next()
                        .and_then(Freq::from_char)
                        .context("freq")?,
                    start: r.5,
                    dims: r.6,
                    value: r.7,
                    lo: r.8,
                    hi: r.9,
                    basis: Basis::from_code(r.10).context("basis code")?,
                    status: Status::from_code(r.11).context("status code")?,
                    flags: Flags::from_db_lossy(r.12),
                })
            })
            .collect::<Result<_>>()?;
        let t = Arc::new(t);
        cache.insert(port.to_owned(), t.clone());
        Ok(t)
    }

    /// `geo.features` (WP-04, migration 0007) at the pinned version: features
    /// share `data.version_closes` with observations (`obs_id` = `feature_id`),
    /// so an approved, unreleased version hides the rows it will close.
    async fn features(&self, port: &str) -> Result<Arc<Vec<Feature>>> {
        let mut cache = self.features.lock().await;
        if let Some(t) = cache.get(port) {
            return Ok(t.clone());
        }
        let p = self.pin(port)?;
        type FRow = (
            String,
            String,
            Option<String>,
            Option<serde_json::Value>,
            i32,
            Option<f64>,
            Option<f64>,
        );
        let rows: Vec<FRow> = sqlx::query_as(
            "SELECT f.feature_key, f.kind, u.code, f.attrs, f.flags,
                    CASE WHEN GeometryType(f.geom) LIKE '%LINESTRING' THEN ST_Length(f.geom::geography) / 1000 END,
                    CASE WHEN GeometryType(f.geom) LIKE '%POLYGON' THEN ST_Area(f.geom::geography) / 1e6 END
             FROM geo.features f LEFT JOIN geo.units u ON u.key = f.unit_key
             WHERE f.dataset_key = $1 AND f.vfrom <= $2 AND f.vto >= $2
               AND NOT EXISTS (SELECT 1 FROM data.version_closes c
                               WHERE c.version_id = $3 AND c.obs_id = f.feature_id)
             ORDER BY f.feature_key",
        )
        .bind(p.dataset_key)
        .bind(p.seq)
        .bind(p.version_id)
        .fetch_all(&self.pool)
        .await
        .with_context(|| {
            format!(
                "reading features {port} ({} {})",
                p.dataset_id, p.dsv
            )
        })?;
        let t = Arc::new(
            rows.into_iter()
                .map(|r| Feature {
                    key: r.0,
                    kind: r.1,
                    unit: r.2,
                    attrs: r.3.unwrap_or(serde_json::Value::Null),
                    flags: Flags::from_db_lossy(r.4),
                    length_km: r.5,
                    area_km2: r.6,
                })
                .collect(),
        );
        cache.insert(port.to_owned(), Arc::clone(&t));
        Ok(t)
    }

    /// A layer of `geo.grid_layers` owned by the port's dataset, aligned to
    /// the reference set's `gnd_idx`. Grid layers are current, not
    /// release-pinned (WP-05): the port pins the dataset version, the layer
    /// file is the one registered now.
    async fn grid_layer(
        &self,
        port: &str,
        layer: &str,
        period: Option<&str>,
    ) -> Result<Arc<GridInput>> {
        let key = (port.to_owned(), layer.to_owned(), period.map(str::to_owned));
        if let Some(g) = self.grids.lock().await.get(&key) {
            return Ok(g.clone());
        }
        let p = self.pin(port)?;
        let hits: Vec<(String, String, Option<String>)> = sqlx::query_as(
            "SELECT period, bin_path, dataset_id FROM geo.grid_layers
             WHERE id = $1 AND ($2::text IS NULL OR period = $2) ORDER BY period",
        )
        .bind(layer)
        .bind(period)
        .fetch_all(&self.pool)
        .await?;
        let (lp, bin, ds) = match hits.as_slice() {
            [h] => h.clone(),
            [] => bail!("grid layer {layer} {period:?} is not registered"),
            _ => bail!("grid layer {layer} has several periods; give layer_period"),
        };
        ensure!(
            ds.as_deref() == Some(p.dataset_id.as_str()),
            "grid layer {layer} {lp} belongs to {ds:?}, not to input {port} ({})",
            p.dataset_id
        );
        let lk = self.lookup().await?;
        let path = self.store_root()?.join(&bin);
        let values: Vec<f32> = tokio::task::spawn_blocking(move || -> Result<Vec<f32>> {
            use lanka_grid::CellValues;
            let (a, _) = lanka_grid::GridArray::read_bin(&path)?;
            #[allow(clippy::cast_possible_truncation)]
            Ok((0..a.len())
                .map(|i| a.value(i).map_or(f32::NAN, |v| v as f32))
                .collect())
        })
        .await??;
        ensure!(
            values.len() == lk.unit_idx.len(),
            "grid layer {layer} {lp}: {} cells, gnd_idx has {}",
            values.len(),
            lk.unit_idx.len()
        );
        let g = Arc::new(GridInput {
            layer: format!("{layer} {lp}"),
            values: Arc::from(values),
            unit_idx: lk.unit_idx.clone(),
            units: lk.units.clone(),
            unit_idx_inside: Some(lk.unit_idx_inside.clone()),
            cols: lk.cols,
            row_area: lk.row_area.clone(),
            basis: Basis::Computed,
            flags: Flags::EMPTY,
        });
        self.grids.lock().await.insert(key, g.clone());
        Ok(g)
    }

    async fn units(&self) -> Result<Arc<Units>> {
        let mut cache = self.units.lock().await;
        if let Some(u) = cache.as_ref() {
            return Ok(u.clone());
        }
        let rows: Vec<(String, i16, Option<String>, String, Option<f64>)> = sqlx::query_as(
            "SELECT u.code, u.level, p.code, u.name_en, g.area_km2
             FROM geo.units u
             LEFT JOIN geo.units p ON p.key = u.parent_key
             LEFT JOIN geo.unit_geoms g ON g.unit_key = u.key AND g.boundary_set = $1
             WHERE u.scheme_id = $2 AND u.valid_to IS NULL",
        )
        .bind(&self.reference_set)
        .bind(&self.scheme)
        .fetch_all(&self.pool)
        .await?;
        let u = Arc::new(Units::new(
            rows.into_iter()
                .map(|r| Unit {
                    code: r.0,
                    level: r.1,
                    parent: r.2,
                    name: r.3,
                    area_km2: r.4,
                })
                .collect(),
        ));
        *cache = Some(u.clone());
        Ok(u)
    }

    fn indicator(&self, id: &str) -> Option<IndicatorMeta> {
        self.indicators.get(id).cloned()
    }

    fn calendars(&self) -> &Calendars {
        &self.calendars
    }
}
