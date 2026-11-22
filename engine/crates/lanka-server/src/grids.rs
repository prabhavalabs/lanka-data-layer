//! Grid serving (06 §2 "Grid", 05 §8): layers flagged `serve` in
//! `geo.grid_layers` are memory-mapped from the store at start (and on every
//! catalogue reload); `/v2/point`, `/v2/radius`, `/v2/grids`,
//! `/v2/grids/{id}`, `/v2/grids/{id}/tiles/{z}/{x}/{y}.png` read them with no
//! database round trip, and `/v2/reverse` answers from `gnd_idx` whenever
//! `land_mask` says the cell lies wholly inside one unit (else the PostGIS
//! polygon path).

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use lanka_core::grid::Grid;
use lanka_core::query::{AreaRef, ReverseResult};
use lanka_grid::cog::Overview;
use lanka_grid::lookup::MASK_INTERIOR;
use lanka_grid::tile::{Painter, Style};
use lanka_grid::{CellValues, MappedGrid};
use serde::Serialize;
use sqlx::PgPool;
use utoipa::ToSchema;

/// A row of `geo.grid_layers`, as served.
#[derive(Debug, Clone, Serialize, ToSchema, sqlx::FromRow)]
pub struct GridLayerInfo {
    pub id: String,
    pub period: String,
    pub dataset_id: String,
    pub indicator_id: Option<String>,
    pub dims: String,
    pub dtype: String,
    pub scale: f64,
    pub offset: f64,
    pub nodata: Option<f64>,
    pub unit: String,
    /// Zonal statistic of the layer (`sum`, `mean`, `class_share`, …; `none` for lookups).
    pub zonal: String,
    /// How coarser pixels are made: mean | sum | max | mode.
    pub overview: String,
    /// Mapped by the server (point, radius, tiles).
    pub serve: bool,
    pub source_res_m: f64,
    pub resampling: Option<String>,
    #[schema(value_type = Object)]
    pub stats: Option<serde_json::Value>,
    #[schema(value_type = Option<Object>)]
    pub style: Option<serde_json::Value>,
    /// Store-relative path of the COG.
    pub cog_path: Option<String>,
    #[serde(skip)]
    pub bin_path: String,
    pub sha256: String,
    pub bytes: Option<i64>,
    pub cog_bytes: Option<i64>,
}

/// (key, code, level, name_en, name_si, name_ta, anc_keys)
type UnitTuple = (
    i32,
    String,
    i16,
    String,
    Option<String>,
    Option<String>,
    Vec<i32>,
);

#[derive(Debug, Clone)]
struct UnitInfo {
    area: AreaRef,
    anc_keys: Vec<i32>,
}

struct Lookups {
    gnd: Arc<MappedGrid>,
    mask: Arc<MappedGrid>,
    slot_unit: Vec<i32>,
}

/// Everything the grid routes read.
#[derive(Default)]
pub struct GridSet {
    pub grid: Option<Grid>,
    pub layers: Vec<GridLayerInfo>,
    mapped: HashMap<(String, String), (Arc<MappedGrid>, usize)>,
    lookups: Option<Lookups>,
    units: HashMap<i32, UnitInfo>,
}

impl std::fmt::Debug for GridSet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GridSet")
            .field("layers", &self.layers.len())
            .field("mapped", &self.mapped.len())
            .finish_non_exhaustive()
    }
}

/// How a reverse query was answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Path2 {
    Grid,
    Polygon,
}

impl GridSet {
    /// Maps every `serve` layer whose file exists under `root`.
    pub async fn load(pool: &PgPool, root: &Path, grid: Grid) -> Result<Self> {
        let layers: Vec<GridLayerInfo> = sqlx::query_as(
            "SELECT id, period, dataset_id, indicator_id, dims, dtype, scale, \"offset\" AS offset, nodata, unit, zonal,
                    overview, serve, source_res_m, resampling, stats, style, cog_path, bin_path,
                    encode(sha256, 'hex') AS sha256, bytes, cog_bytes
             FROM geo.grid_layers ORDER BY id, period",
        )
        .fetch_all(pool)
        .await?;
        let mut mapped = HashMap::new();
        for (i, l) in layers.iter().enumerate() {
            if !l.serve {
                continue;
            }
            let p: PathBuf = root.join(&l.bin_path);
            match MappedGrid::open(&p) {
                Ok(m) if m.header.grid == grid => {
                    mapped.insert((l.id.clone(), l.period.clone()), (Arc::new(m), i));
                }
                Ok(_) => {
                    tracing::warn!(layer = %l.id, "grid layer is not on the profile grid; not mapped")
                }
                Err(e) => tracing::warn!(layer = %l.id, error = %e, "grid layer not mapped"),
            }
        }
        // Lookups of the reference boundary set.
        let set: Option<String> = sqlx::query_scalar(
            "SELECT id FROM geo.boundary_sets WHERE is_reference ORDER BY id LIMIT 1",
        )
        .fetch_optional(pool)
        .await?;
        let mut lookups = None;
        let mut units = HashMap::new();
        if let Some(set) = set {
            let g = mapped
                .get(&("grid.gnd_idx".to_owned(), set.clone()))
                .map(|x| x.0.clone());
            let m = mapped
                .get(&("grid.land_mask".to_owned(), set.clone()))
                .map(|x| x.0.clone());
            if let (Some(gnd), Some(mask)) = (g, m) {
                let slots: Vec<(i32, i32)> = sqlx::query_as(
                    "SELECT idx, unit_key FROM geo.grid_lookup_units WHERE layer_id = 'grid.gnd_idx' AND period = $1
                     ORDER BY idx",
                )
                .bind(&set)
                .fetch_all(pool)
                .await?;
                let n = slots.last().map_or(0, |s| s.0);
                let mut slot_unit = vec![0; usize::try_from(n)? + 1];
                for (i, k) in slots {
                    slot_unit[usize::try_from(i)?] = k;
                }
                lookups = Some(Lookups {
                    gnd,
                    mask,
                    slot_unit,
                });
                let rows: Vec<UnitTuple> = sqlx::query_as(
                    "SELECT key, code, level, name_en, name_si, name_ta, anc_keys FROM geo.units WHERE valid_to IS NULL",
                )
                .fetch_all(pool)
                .await?;
                for (key, pcode, level, name_en, name_si, name_ta, anc_keys) in rows {
                    units.insert(
                        key,
                        UnitInfo {
                            area: AreaRef {
                                pcode,
                                level,
                                name_en,
                                name_si,
                                name_ta,
                            },
                            anc_keys,
                        },
                    );
                }
            }
        }
        Ok(Self {
            grid: Some(grid),
            layers,
            mapped,
            lookups,
            units,
        })
    }

    /// Number of mapped layers (`/v2/health` `grids_mapped`).
    #[must_use]
    pub fn mapped_count(&self) -> usize {
        self.mapped.len()
    }

    #[must_use]
    pub fn layer(&self, id: &str, period: &str) -> Option<(&MappedGrid, &GridLayerInfo)> {
        self.mapped
            .get(&(id.to_owned(), period.to_owned()))
            .map(|(m, i)| (m.as_ref(), &self.layers[*i]))
    }

    /// Mapped periods of a layer, ascending.
    #[must_use]
    pub fn periods(&self, id: &str) -> Vec<&str> {
        let mut v: Vec<&str> = self
            .layers
            .iter()
            .filter(|l| l.id == id && self.mapped.contains_key(&(l.id.clone(), l.period.clone())))
            .map(|l| l.period.as_str())
            .collect();
        v.sort_unstable();
        v
    }

    /// Mapped `(layer, info)` pairs, excluding the lookups.
    pub fn served(&self) -> impl Iterator<Item = (&MappedGrid, &GridLayerInfo)> {
        let mut v: Vec<_> = self
            .mapped
            .values()
            .map(|(m, i)| (m.as_ref(), &self.layers[*i]))
            .filter(|(_, l)| l.dataset_id != "grid-lookups")
            .collect();
        v.sort_by(|a, b| (&a.1.id, &a.1.period).cmp(&(&b.1.id, &b.1.period)));
        v.into_iter()
    }

    /// The grid path of reverse geocoding: `Some` only when the cell lies
    /// wholly inside one unit (`land_mask = 1`), so the answer equals the
    /// polygon path for every point of the cell.
    #[must_use]
    pub fn reverse(&self, lat: f64, lon: f64) -> Option<ReverseResult> {
        let g = self.grid?;
        let lk = self.lookups.as_ref()?;
        let cell = g.cell_id(lat, lon)?;
        let i = cell as usize;
        if lk.mask.u8_at(i) != MASK_INTERIOR {
            return None;
        }
        let slot = lk.gnd.u16_at(i) as usize;
        let key = *lk.slot_unit.get(slot)?;
        let u = self.units.get(&key)?;
        let mut ancestors: Vec<AreaRef> = u
            .anc_keys
            .iter()
            .filter_map(|k| self.units.get(k).map(|a| a.area.clone()))
            .collect();
        ancestors.sort_by_key(|a| a.level);
        Some(ReverseResult {
            lat,
            lon,
            cell_id: cell,
            unit: u.area.clone(),
            ancestors,
            match_kind: "contains".to_owned(),
            distance_m: None,
        })
    }

    /// `land_mask` of the cell at a point (diagnostics).
    #[must_use]
    pub fn mask_at(&self, lat: f64, lon: f64) -> Option<u8> {
        let c = self.grid?.cell_id(lat, lon)?;
        Some(self.lookups.as_ref()?.mask.u8_at(c as usize))
    }
}

/// A layer's value at one cell.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct PointValue {
    pub layer: String,
    pub period: String,
    pub indicator: Option<String>,
    pub dims: String,
    pub unit: String,
    /// `null` = no data in this cell.
    pub value: Option<f64>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct PointResult {
    pub lat: f64,
    pub lon: f64,
    pub cell_id: u32,
    /// `[west, south, east, north]` of the cell.
    pub cell_bounds: [f64; 4],
    pub cell_area_m2: f64,
    /// Units at the point (grid path or polygon fallback); `null` off shore.
    pub reverse: Option<ReverseResult>,
    pub values: Vec<PointValue>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct RadiusResult {
    pub lat: f64,
    pub lon: f64,
    pub km: f64,
    pub layer: String,
    pub period: String,
    pub unit: String,
    /// `sum` for count layers (zonal `sum`), else the area-weighted `mean`.
    pub stat: String,
    pub value: Option<f64>,
    /// Cells whose centre lies within the radius.
    pub cells: u64,
    /// … of which have data.
    pub cells_with_data: u64,
    pub area_km2: f64,
}

/// Rounds a decoded value to the decimals its storage scale carries
/// (`41 × 0.1` prints as 4.1, not 4.1000000000000005).
#[must_use]
pub fn round_to_scale(v: f64, scale: f64) -> f64 {
    if scale >= 1.0 || scale <= 0.0 {
        return v;
    }
    for d in 1..=9 {
        let p = 10f64.powi(d);
        let x = scale * p;
        if (x - x.round()).abs() < 1e-9 {
            return (v * p).round() / p;
        }
    }
    v
}

/// Haversine distance, km.
fn km_between(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    const R: f64 = 6_371.008_8;
    let (p1, p2) = (lat1.to_radians(), lat2.to_radians());
    let dp = p2 - p1;
    let dl = (lon2 - lon1).to_radians();
    let a = (dp / 2.0).sin().powi(2) + p1.cos() * p2.cos() * (dl / 2.0).sin().powi(2);
    2.0 * R * a.sqrt().asin()
}

/// Sum / area-weighted mean of a layer over the cells whose centre is within `km`.
#[must_use]
pub fn radius(
    g: &Grid,
    m: &MappedGrid,
    info: &GridLayerInfo,
    lat: f64,
    lon: f64,
    km: f64,
) -> RadiusResult {
    let dlat = km / 111.0;
    let dlon = km / (111.0 * lat.to_radians().cos().max(0.1));
    #[allow(clippy::cast_possible_truncation)]
    let rows = |la: f64| ((g.lat_max - la) / g.step).floor() as i64;
    #[allow(clippy::cast_possible_truncation)]
    let cols = |lo: f64| ((lo - g.lon_min) / g.step).floor() as i64;
    let (r0, r1) = (
        rows(lat + dlat).max(0),
        rows(lat - dlat).min(i64::from(g.rows) - 1),
    );
    let (c0, c1) = (
        cols(lon - dlon).max(0),
        cols(lon + dlon).min(i64::from(g.cols) - 1),
    );
    let sum_layer = info.zonal == "sum";
    let (mut n, mut nv, mut s, mut ws, mut wa, mut area) = (0u64, 0u64, 0.0, 0.0, 0.0, 0.0);
    for r in r0..=r1 {
        #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
        let a = lanka_grid::cell_area_m2(g, r as u32);
        #[allow(clippy::cast_precision_loss)]
        let clat = g.lat_max - (r as f64 + 0.5) * g.step;
        for c in c0..=c1 {
            #[allow(clippy::cast_precision_loss)]
            let clon = g.lon_min + (c as f64 + 0.5) * g.step;
            if km_between(lat, lon, clat, clon) > km {
                continue;
            }
            n += 1;
            area += a;
            #[allow(clippy::cast_sign_loss)]
            if let Some(v) = m.value((r * i64::from(g.cols) + c) as usize) {
                nv += 1;
                s += v;
                ws += v * a;
                wa += a;
            }
        }
    }
    let value = if nv == 0 {
        None
    } else if sum_layer {
        Some(s)
    } else {
        Some(ws / wa)
    };
    RadiusResult {
        lat,
        lon,
        km,
        layer: info.id.clone(),
        period: info.period.clone(),
        unit: info.unit.clone(),
        stat: if sum_layer { "sum" } else { "mean" }.to_owned(),
        value,
        cells: n,
        cells_with_data: nv,
        area_km2: area / 1e6,
    }
}

/// Resolves a layer's style (ramp names via `spec/map/1/ramps.json`).
pub fn painter(info: &GridLayerInfo) -> Result<(Painter, Overview)> {
    let mut style: Style = match &info.style {
        Some(v) => serde_json::from_value(v.clone()).context("layer style")?,
        None => Style::default(),
    };
    if style.stops.is_empty() && style.classes.is_empty() {
        let name = style
            .ramp
            .clone()
            .unwrap_or_else(|| "viridis.viridis".to_owned());
        let ramp = lanka_mapdoc::ramps::REGISTRY
            .ramps
            .get(&name)
            .with_context(|| format!("unknown ramp {name}"))?;
        style.stops = ramp.stops_for(false).to_vec();
    }
    let stat = |k: &str| {
        info.stats
            .as_ref()
            .and_then(|s| s.get(k))
            .and_then(serde_json::Value::as_f64)
    };
    let domain = [stat("min").unwrap_or(0.0), stat("max").unwrap_or(1.0)];
    Ok((
        Painter::new(&style, domain)?,
        Overview::parse(&info.overview)?,
    ))
}

/// Layers grouped by id with their periods (`/v2/grids`).
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct GridSummary {
    pub id: String,
    pub dataset_id: String,
    pub indicator_id: Option<String>,
    pub dims: String,
    pub unit: String,
    pub dtype: String,
    pub zonal: String,
    pub overview: String,
    pub serve: bool,
    pub periods: Vec<String>,
    /// Tile URL template (served layers only).
    pub tiles: Option<String>,
}

#[must_use]
pub fn summaries(gs: &GridSet) -> Vec<GridSummary> {
    let mut m: BTreeMap<&str, GridSummary> = BTreeMap::new();
    for l in &gs.layers {
        let e = m.entry(l.id.as_str()).or_insert_with(|| GridSummary {
            id: l.id.clone(),
            dataset_id: l.dataset_id.clone(),
            indicator_id: l.indicator_id.clone(),
            dims: l.dims.clone(),
            unit: l.unit.clone(),
            dtype: l.dtype.clone(),
            zonal: l.zonal.clone(),
            overview: l.overview.clone(),
            serve: false,
            periods: Vec::new(),
            tiles: None,
        });
        e.periods.push(l.period.clone());
        if l.serve && gs.layer(&l.id, &l.period).is_some() {
            e.serve = true;
            e.tiles = Some(format!("/v2/grids/{}/tiles/{{z}}/{{x}}/{{y}}.png", l.id));
        }
    }
    m.into_values().collect()
}

// ───────────────────────────── handlers ────────────────────────────────────

use axum::extract::{Path as UrlPath, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, Uri, header};
use axum::response::IntoResponse;
use lanka_core::envelope::Link;
use serde::Deserialize;
use utoipa::IntoParams;

use crate::{ApiError, ApiResult, AppState, ErrorBody, with_boundaries};

fn coord(v: Option<&String>, n: &str) -> Result<f64, ApiError> {
    v.and_then(|s| s.trim().parse::<f64>().ok())
        .filter(|x| x.is_finite())
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::BAD_REQUEST,
                format!("{n} is required (decimal degrees)"),
            )
        })
}

fn datasets_of<'a>(it: impl IntoIterator<Item = &'a GridLayerInfo>) -> Vec<&'a str> {
    let mut v: Vec<&str> = it.into_iter().map(|l| l.dataset_id.as_str()).collect();
    v.sort_unstable();
    v.dedup();
    v
}

#[derive(Debug, Deserialize, IntoParams)]
pub struct PointParams {
    pub lat: Option<String>,
    pub lon: Option<String>,
    /// Comma-separated layer ids (default: every served layer, every mapped period).
    pub layers: Option<String>,
}

/// Values of the served grid layers at a point, plus the admin units there.
#[utoipa::path(get, path = "/v2/point", tag = "grids", params(PointParams),
    responses((status = 200, body = lanka_core::Envelope<PointResult>), (status = 400, body = ErrorBody),
              (status = 404, body = ErrorBody)))]
pub async fn point(
    State(st): State<AppState>,
    Query(p): Query<PointParams>,
    headers: HeaderMap,
    uri: Uri,
) -> ApiResult {
    let cat = st.catalog();
    let (lat, lon) = (coord(p.lat.as_ref(), "lat")?, coord(p.lon.as_ref(), "lon")?);
    let gs = st.grids();
    let g = gs.grid.ok_or_else(|| {
        ApiError::new(
            StatusCode::NOT_FOUND,
            "not_found: no grid layers are mapped",
        )
    })?;
    let cell = g
        .cell_id(lat, lon)
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "not_in_coverage"))?;
    let wanted: Option<Vec<&str>> = p
        .layers
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(|s| s.split(',').map(str::trim).collect());
    let mut values = Vec::new();
    let mut touched = Vec::new();
    for (m, l) in gs.served() {
        if wanted.as_ref().is_some_and(|w| !w.contains(&l.id.as_str())) {
            continue;
        }
        values.push(PointValue {
            layer: l.id.clone(),
            period: l.period.clone(),
            indicator: l.indicator_id.clone(),
            dims: l.dims.clone(),
            unit: l.unit.clone(),
            value: m.value(cell as usize).map(|v| round_to_scale(v, l.scale)),
        });
        touched.push(l);
    }
    if let Some(w) = &wanted {
        for id in w {
            if !values.iter().any(|v| v.layer == *id) {
                return Err(ApiError::new(
                    StatusCode::NOT_FOUND,
                    format!("not_found: grid layer {id} is not served"),
                ));
            }
        }
    }
    let reverse = match gs.reverse(lat, lon) {
        Some(r) => Some(r),
        None => match lanka_core::query::reverse(&st.pool, &cat, lat, lon).await {
            Ok(r) => Some(r),
            Err(lanka_core::CoreError::NotInCoverage) => None,
            Err(e) => return Err(ApiError::from(e).with_version(cat.data_version.clone())),
        },
    };
    let (row, _) = lanka_core::grid::row_col(cell).unwrap_or((0, 0));
    let bounds = {
        let (r, c) = (f64::from(cell / g.cols), f64::from(cell % g.cols));
        let north = g.lat_max - r * g.step;
        let west = g.lon_min + c * g.step;
        [west, north - g.step, west + g.step, north]
    };
    let res = PointResult {
        lat,
        lon,
        cell_id: cell,
        cell_bounds: bounds,
        cell_area_m2: lanka_grid::cell_area_m2(&g, row),
        reverse,
        values,
    };
    let ids = datasets_of(touched);
    let meta = cat.meta_for(
        with_boundaries(&cat, ids),
        vec![Link::new("grids", "/v2/grids".to_owned())],
    );
    crate::respond(&headers, &uri, res, meta)
}

#[derive(Debug, Deserialize, IntoParams)]
pub struct RadiusParams {
    pub lat: Option<String>,
    pub lon: Option<String>,
    /// Radius in km (0 < km ≤ 50).
    pub km: Option<String>,
    /// Layer id (a served layer).
    pub layer: Option<String>,
    /// Period (default: the latest mapped).
    pub period: Option<String>,
}

/// Sum (count layers) or area-weighted mean of a grid layer within a radius (≤ 50 km).
#[utoipa::path(get, path = "/v2/radius", tag = "grids", params(RadiusParams),
    responses((status = 200, body = lanka_core::Envelope<RadiusResult>), (status = 400, body = ErrorBody),
              (status = 404, body = ErrorBody)))]
pub async fn radius_handler(
    State(st): State<AppState>,
    Query(p): Query<RadiusParams>,
    headers: HeaderMap,
    uri: Uri,
) -> ApiResult {
    let cat = st.catalog();
    let (lat, lon) = (coord(p.lat.as_ref(), "lat")?, coord(p.lon.as_ref(), "lon")?);
    let km = coord(p.km.as_ref(), "km")?;
    if !(km > 0.0 && km <= 50.0) {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "km must be in (0, 50]",
        ));
    }
    let id = p
        .layer
        .as_deref()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| ApiError::new(StatusCode::BAD_REQUEST, "layer is required"))?;
    let gs = st.grids();
    let g = gs.grid.ok_or_else(|| {
        ApiError::new(
            StatusCode::NOT_FOUND,
            "not_found: no grid layers are mapped",
        )
    })?;
    if g.cell_id(lat, lon).is_none() {
        return Err(ApiError::new(StatusCode::NOT_FOUND, "not_in_coverage"));
    }
    let period = match p.period.as_deref().filter(|s| !s.is_empty()) {
        Some(x) => x.to_owned(),
        None => gs
            .periods(id)
            .last()
            .map(|s| (*s).to_owned())
            .ok_or_else(|| {
                ApiError::new(
                    StatusCode::NOT_FOUND,
                    format!("not_found: grid layer {id} is not served"),
                )
            })?,
    };
    let (m, info) = gs.layer(id, &period).ok_or_else(|| {
        ApiError::new(
            StatusCode::NOT_FOUND,
            format!("not_found: grid layer {id} {period} is not served"),
        )
    })?;
    let r = radius(&g, m, info, lat, lon, km);
    let meta = cat.meta_for([info.dataset_id.as_str()], Vec::new());
    crate::respond(&headers, &uri, r, meta)
}

/// Every grid layer with its periods; `tiles` is set for served layers.
#[utoipa::path(get, path = "/v2/grids", tag = "grids",
    responses((status = 200, body = lanka_core::Envelope<Vec<GridSummary>>)))]
pub async fn list(State(st): State<AppState>, headers: HeaderMap, uri: Uri) -> ApiResult {
    let cat = st.catalog();
    let gs = st.grids();
    let s = summaries(&gs);
    let meta = cat.meta_for(datasets_of(&gs.layers), Vec::new());
    crate::respond(&headers, &uri, s, meta)
}

/// One grid layer: every period with encoding, statistics, style and COG path.
#[utoipa::path(get, path = "/v2/grids/{id}", tag = "grids", params(("id" = String, Path)),
    responses((status = 200, body = lanka_core::Envelope<Vec<GridLayerInfo>>), (status = 404, body = ErrorBody)))]
pub async fn detail(
    State(st): State<AppState>,
    UrlPath(id): UrlPath<String>,
    headers: HeaderMap,
    uri: Uri,
) -> ApiResult {
    let cat = st.catalog();
    let gs = st.grids();
    let rows: Vec<GridLayerInfo> = gs.layers.iter().filter(|l| l.id == id).cloned().collect();
    if rows.is_empty() {
        return Err(ApiError::new(
            StatusCode::NOT_FOUND,
            format!("not_found: grid layer {id}"),
        ));
    }
    let meta = cat.meta_for(datasets_of(&rows), Vec::new());
    crate::respond(&headers, &uri, rows, meta)
}

#[derive(Debug, Deserialize, IntoParams)]
pub struct TileParams {
    /// Period (default: the latest mapped).
    pub period: Option<String>,
}

/// A 256 × 256 PNG tile (web mercator) of a served layer.
#[utoipa::path(get, path = "/v2/grids/{id}/tiles/{z}/{x}/{y}.png", tag = "grids",
    params(("id" = String, Path), ("z" = u8, Path), ("x" = u32, Path), ("y" = String, Path, description = "`<y>.png`"), TileParams),
    responses((status = 200, content_type = "image/png", body = Vec<u8>), (status = 204), (status = 404, body = ErrorBody)))]
pub async fn tile(
    State(st): State<AppState>,
    UrlPath((id, z, x, y)): UrlPath<(String, u8, u32, String)>,
    Query(p): Query<TileParams>,
    headers: HeaderMap,
) -> ApiResult {
    let y: u32 = y
        .strip_suffix(".png")
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "not_found: tiles are <y>.png"))?;
    if z > 18 || x >= (1u32 << z) || y >= (1u32 << z) {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "tile coordinates out of range (z ≤ 18)",
        ));
    }
    let gs = st.grids();
    let period = match p.period.as_deref().filter(|s| !s.is_empty()) {
        Some(v) => v.to_owned(),
        None => gs
            .periods(&id)
            .last()
            .map(|s| (*s).to_owned())
            .ok_or_else(|| {
                ApiError::new(
                    StatusCode::NOT_FOUND,
                    format!("not_found: grid layer {id} is not served"),
                )
            })?,
    };
    let (m, info) = gs.layer(&id, &period).ok_or_else(|| {
        ApiError::new(
            StatusCode::NOT_FOUND,
            format!("not_found: grid layer {id} {period} is not served"),
        )
    })?;
    let etag = format!(
        "\"{}-{z}-{x}-{y}\"",
        &info.sha256[..16.min(info.sha256.len())]
    );
    if headers
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.split(',').any(|t| t.trim() == etag))
    {
        let mut r = StatusCode::NOT_MODIFIED.into_response();
        r.headers_mut().insert(
            header::ETAG,
            HeaderValue::from_str(&etag)
                .map_err(|_| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "etag"))?,
        );
        return Ok(r);
    }
    let g = m.header.grid;
    if !lanka_grid::tile::intersects(&g, z, x, y) {
        let mut r = StatusCode::NO_CONTENT.into_response();
        r.headers_mut().insert(
            header::CACHE_CONTROL,
            HeaderValue::from_static("public, max-age=86400"),
        );
        return Ok(r);
    }
    let (painter, ov) = painter(info).map_err(|e| {
        tracing::error!(error = %e, layer = %id, "tile style");
        ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "internal error")
    })?;
    let png = lanka_grid::tile::render_png(m, &g, z, x, y, ov, &painter).map_err(|e| {
        tracing::error!(error = %e, "tile render");
        ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "internal error")
    })?;
    let mut r = png.into_response();
    let h = r.headers_mut();
    h.insert(header::CONTENT_TYPE, HeaderValue::from_static("image/png"));
    // The layer's content hash is in the ETag: a rebuilt layer is a new tile.
    h.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=3600, stale-while-revalidate=86400"),
    );
    h.insert(
        header::ETAG,
        HeaderValue::from_str(&etag)
            .map_err(|_| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "etag"))?,
    );
    Ok(r)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rounding_follows_the_scale() {
        assert_eq!(round_to_scale(41.0 * 0.1, 0.1), 4.1);
        assert_eq!(round_to_scale(3.0 * 0.004, 0.004), 0.012);
        assert_eq!(round_to_scale(36.117_694, 1.0), 36.117_694);
    }

    #[test]
    fn haversine_is_sane() {
        // Colombo Fort → Kandy ≈ 94 km.
        let d = km_between(6.9344, 79.8428, 7.2906, 80.6337);
        assert!((d - 96.0).abs() < 4.0, "{d}");
    }
}
