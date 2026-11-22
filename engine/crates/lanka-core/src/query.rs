//! The shared query core. REST v2, the CLI and (next) MCP call these
//! functions; none of them builds SQL of its own.
//!
//! Every data query is a bound, prepared statement on the integer-keyed,
//! range-versioned `data.observations` (migration 0002): the dataset key is
//! always a bound constant (partition pruning at plan time), current rows are
//! read through the partial indexes `WHERE vto = 2147483647`, and staged rows
//! (`vfrom` above the released sequence) are excluded. Codes are resolved to
//! surrogate keys from the catalog snapshot and rendered back for output.

use std::collections::{BTreeMap, HashMap};

use chrono::{DateTime, NaiveDate, Utc};
use serde::Serialize;
use sqlx::{FromRow, PgPool};
use utoipa::ToSchema;

use crate::VTO_OPEN;
use crate::basis::{Basis, Method, Review, Status};
use crate::catalog::{
    Catalog, CurrentVersion, DatasetInfo, IndicatorInfo, LicenceInfo, SourceInfo,
};
use crate::dims::Dims;
use crate::error::{CoreError, CoreResult};
use crate::flags::Flags;
use crate::geo_code::{GeoCode, Pcode};
use crate::period::{Freq, Period};

/// Max geographies per series request.
pub const MAX_SERIES_GEOS: usize = 25;

/// Bits of `obs_id` below the dataset key.
const OBS_KEY_SHIFT: u32 = 40;

fn basis_of(c: i16) -> Basis {
    Basis::from_code(c).unwrap_or(Basis::Reported)
}

fn status_of(c: i16) -> Status {
    Status::from_code(c).unwrap_or(Status::Normal)
}

fn render_period(cat: &Catalog, freq: i8, start: NaiveDate) -> CoreResult<String> {
    let p = cat.profile()?;
    let f = Freq::from_db(freq)
        .ok_or_else(|| CoreError::bad_request(format!("unknown frequency code {freq}")))?;
    Ok(Period { freq: f, start }.render(&p.calendars))
}

fn dims_text(cat: &Catalog, key: i32) -> String {
    cat.dims.text(key).unwrap_or_default().to_owned()
}

// ───────────────────────────── areas ───────────────────────────────────────

#[derive(Debug, Clone, FromRow, Serialize, ToSchema)]
pub struct AreaRef {
    pub pcode: String,
    pub level: i16,
    pub name_en: String,
    pub name_si: Option<String>,
    pub name_ta: Option<String>,
}

#[derive(Debug, Clone, FromRow)]
struct AreaRow {
    key: i32,
    pcode: String,
    level: i16,
    name_en: String,
    name_si: Option<String>,
    name_ta: Option<String>,
    anc_keys: Vec<i32>,
    boundary_set: String,
    area_km2: Option<f64>,
    lat: Option<f64>,
    lon: Option<f64>,
    west: Option<f64>,
    south: Option<f64>,
    east: Option<f64>,
    north: Option<f64>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct AreaValue {
    pub obs_id: i64,
    /// Canonical dimension key, e.g. `sex=female`; empty for undimensioned.
    pub dims: String,
    pub value: Option<f64>,
    pub lo: Option<f64>,
    pub hi: Option<f64>,
    /// reported | computed | estimated | forecast
    pub basis: String,
    /// normal | provisional | revised | break | low_reliability | missing | nil | suppressed
    pub status: String,
    /// Registered quality/method flag names.
    #[schema(value_type = Vec<String>)]
    pub flags: Flags,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct AreaIndicator {
    pub indicator_id: String,
    pub label: String,
    pub unit: Option<String>,
    pub unit_mult: i16,
    pub class: String,
    pub dataset_id: String,
    /// Latest period held for this unit.
    pub period: String,
    pub values: Vec<AreaValue>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct AreaProfile {
    pub pcode: String,
    pub level: i16,
    pub name_en: String,
    pub name_si: Option<String>,
    pub name_ta: Option<String>,
    pub boundary_set: String,
    pub area_km2: Option<f64>,
    /// `[lat, lon]` of a point guaranteed inside the unit.
    pub centroid: Option<[f64; 2]>,
    /// `[west, south, east, north]`.
    pub bbox: Option<[f64; 4]>,
    /// Root first: country, province, district, …
    pub ancestors: Vec<AreaRef>,
    pub children: Vec<AreaRef>,
    pub indicators: Vec<AreaIndicator>,
}

/// A row of `serve.area_latest` (migration 0005).
#[derive(Debug, Clone, FromRow)]
struct OpenObs {
    obs_id: i64,
    indicator_key: i32,
    dims_key: i32,
    freq: i8,
    time_start: NaiveDate,
    value: Option<f64>,
    lo: Option<f64>,
    hi: Option<f64>,
    basis: i16,
    status: i16,
    flags: i32,
}

const AREA_SQL: &str = "SELECT u.key, u.code AS pcode, u.level, u.name_en, u.name_si, u.name_ta, u.anc_keys,
        g.boundary_set, g.area_km2, ST_Y(g.centroid) AS lat, ST_X(g.centroid) AS lon,
        ST_XMin(g.bbox) AS west, ST_YMin(g.bbox) AS south, ST_XMax(g.bbox) AS east, ST_YMax(g.bbox) AS north
 FROM geo.units u JOIN geo.unit_geoms g ON g.unit_key = u.key AND g.boundary_set = $2
 WHERE u.code = $1";

fn reference_set(cat: &Catalog) -> CoreResult<&str> {
    cat.reference_set
        .as_deref()
        .ok_or_else(|| CoreError::not_found("reference boundary set (load the boundaries first)"))
}

/// Area profile: hierarchy, children and the latest value of every indicator
/// held for the unit in the current release.
pub async fn area_profile(
    pool: &PgPool,
    cat: &Catalog,
    pcode: &Pcode,
) -> CoreResult<(AreaProfile, Vec<String>)> {
    let set = reference_set(cat)?;
    let row: AreaRow = sqlx::query_as(AREA_SQL)
        .bind(pcode.as_str())
        .bind(set)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| CoreError::not_found(format!("no admin unit \"{pcode}\"")))?;

    let ds_keys: Vec<i32> = cat.current_seqs().iter().map(|(k, _)| *k).collect();
    let (ancestors, children, open) = tokio::join!(
        sqlx::query_as::<_, AreaRef>(
            "SELECT code AS pcode, level, name_en, name_si, name_ta FROM geo.units
             WHERE key = ANY($1) ORDER BY level",
        )
        .bind(&row.anc_keys)
        .fetch_all(pool),
        sqlx::query_as::<_, AreaRef>(
            "SELECT u.code AS pcode, u.level, u.name_en, u.name_si, u.name_ta
             FROM geo.units u JOIN geo.unit_geoms g ON g.unit_key = u.key AND g.boundary_set = $2
             WHERE u.parent_key = $1 ORDER BY u.name_en",
        )
        .bind(row.key)
        .bind(set)
        .fetch_all(pool),
        // One primary-key range scan: the release train keeps the latest
        // released row per (geo, indicator, dims) in serve.area_latest.
        sqlx::query_as::<_, OpenObs>(
            "SELECT obs_id, indicator_key, dims_key, freq, time_start, value, lo, hi, basis, status, flags
             FROM serve.area_latest
             WHERE geo_key = $1 AND dataset_key = ANY($2)",
        )
        .bind(row.key)
        .bind(&ds_keys)
        .fetch_all(pool),
    );
    let (ancestors, children, open) = (ancestors?, children?, open?);

    // Latest released row per (indicator, dims); then keep only the latest
    // period per indicator so a dims value that stopped being published does
    // not mix vintages.
    let mut latest: BTreeMap<(String, i32), OpenObs> = BTreeMap::new();
    for o in open {
        let Some(ind) = cat.indicator_by_key.get(&o.indicator_key) else {
            continue;
        };
        let k = (ind.clone(), o.dims_key);
        if latest.get(&k).is_none_or(|p| p.time_start < o.time_start) {
            latest.insert(k, o);
        }
    }
    let mut grouped: BTreeMap<String, Vec<OpenObs>> = BTreeMap::new();
    for ((ind, _), o) in latest {
        grouped.entry(ind).or_default().push(o);
    }
    let mut touched: Vec<String> = Vec::new();
    let mut indicators = Vec::with_capacity(grouped.len());
    for (ind_id, mut obs) in grouped {
        let Some(max) = obs.iter().map(|o| o.time_start).max() else {
            continue;
        };
        obs.retain(|o| o.time_start == max);
        let Some(info) = cat.indicators.get(&ind_id) else {
            continue;
        };
        let period = render_period(cat, obs[0].freq, max)?;
        if !touched.contains(&info.dataset_id) {
            touched.push(info.dataset_id.clone());
        }
        let mut values: Vec<AreaValue> = obs
            .into_iter()
            .map(|o| AreaValue {
                obs_id: o.obs_id,
                dims: dims_text(cat, o.dims_key),
                value: o.value,
                lo: o.lo,
                hi: o.hi,
                basis: basis_of(o.basis).as_str().to_owned(),
                status: status_of(o.status).as_str().to_owned(),
                flags: Flags::from_db_lossy(o.flags),
            })
            .collect();
        values.sort_by(|a, b| a.dims.cmp(&b.dims));
        indicators.push(AreaIndicator {
            indicator_id: ind_id,
            label: info.label.clone(),
            unit: info.unit.clone(),
            unit_mult: info.unit_mult,
            class: info.class.clone(),
            dataset_id: info.dataset_id.clone(),
            period,
            values,
        });
    }

    // Coordinates leave the API with at most 6 decimals (docs/architecture.md).
    let centroid = row.lat.zip(row.lon).map(|(a, b)| [round6(a), round6(b)]);
    let bbox = match (row.west, row.south, row.east, row.north) {
        (Some(w), Some(s), Some(e), Some(n)) => Some([round6(w), round6(s), round6(e), round6(n)]),
        _ => None,
    };
    Ok((
        AreaProfile {
            pcode: row.pcode,
            level: row.level,
            name_en: row.name_en,
            name_si: row.name_si,
            name_ta: row.name_ta,
            boundary_set: row.boundary_set,
            area_km2: row.area_km2,
            centroid,
            bbox,
            ancestors,
            children,
            indicators,
        },
        touched,
    ))
}

fn round6(x: f64) -> f64 {
    (x * 1e6).round() / 1e6
}

// ───────────────────────────── reverse geocode ─────────────────────────────

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ReverseResult {
    pub lat: f64,
    pub lon: f64,
    /// Canonical 0.001° grid cell (docs/architecture.md §1).
    pub cell_id: u32,
    /// Deepest unit containing the point, then its ancestors (root first).
    pub unit: AreaRef,
    pub ancestors: Vec<AreaRef>,
    /// `contains` (point-in-polygon) or `nearest` (near-shore point within
    /// [`REVERSE_SNAP_M`] of a unit, e.g. a jetty or a coastline vertex gap).
    #[serde(rename = "match")]
    pub match_kind: String,
    /// Distance to the unit for `nearest` matches, metres.
    pub distance_m: Option<f64>,
}

/// Max snapping distance for points just off the coastline polygons.
pub const REVERSE_SNAP_M: f64 = 2000.0;

#[derive(FromRow)]
struct ReverseHit {
    #[sqlx(flatten)]
    unit: AreaRef,
    anc_keys: Vec<i32>,
    distance_m: Option<f64>,
}

/// Point-in-polygon against full-resolution reference geometry (GiST index).
pub async fn reverse(
    pool: &PgPool,
    cat: &Catalog,
    lat: f64,
    lon: f64,
) -> CoreResult<ReverseResult> {
    let profile = cat.profile()?;
    let cell_id = profile
        .grid
        .cell_id(lat, lon)
        .ok_or(CoreError::NotInCoverage)?;
    let set = reference_set(cat)?;
    let hit: Option<ReverseHit> = sqlx::query_as(
        "SELECT u.code AS pcode, u.level, u.name_en, u.name_si, u.name_ta, u.anc_keys,
                NULL::float8 AS distance_m
         FROM geo.unit_geoms g JOIN geo.units u ON u.key = g.unit_key
         WHERE g.boundary_set = $3 AND u.level > 0
           AND ST_Intersects(g.geom, ST_SetSRID(ST_MakePoint($2, $1), 4326))
         ORDER BY u.level DESC LIMIT 1",
    )
    .bind(lat)
    .bind(lon)
    .bind(set)
    .fetch_optional(pool)
    .await?;
    // Near-shore fallback: KNN over the deepest level (GiST `<->`), then an
    // exact geodesic distance check.
    let hit = match hit {
        Some(h) => Some(h),
        None => sqlx::query_as(
            "SELECT * FROM (
               SELECT u.code AS pcode, u.level, u.name_en, u.name_si, u.name_ta, u.anc_keys,
                      ST_Distance(g.geom::geography, ST_SetSRID(ST_MakePoint($2, $1), 4326)::geography) AS distance_m
               FROM geo.unit_geoms g JOIN geo.units u ON u.key = g.unit_key
               WHERE g.boundary_set = $4 AND u.level = $5
               ORDER BY g.geom <-> ST_SetSRID(ST_MakePoint($2, $1), 4326) LIMIT 3
             ) k WHERE distance_m <= $3 ORDER BY distance_m LIMIT 1",
        )
        .bind(lat)
        .bind(lon)
        .bind(REVERSE_SNAP_M)
        .bind(set)
        .bind(i16::from(profile.admin.max_level()))
        .fetch_optional(pool)
        .await?,
    };
    let ReverseHit {
        unit,
        anc_keys,
        distance_m,
    } = hit.ok_or(CoreError::NotInCoverage)?;
    let ancestors = sqlx::query_as::<_, AreaRef>(
        "SELECT code AS pcode, level, name_en, name_si, name_ta FROM geo.units
         WHERE key = ANY($1) ORDER BY level",
    )
    .bind(&anc_keys)
    .fetch_all(pool)
    .await?;
    Ok(ReverseResult {
        lat,
        lon,
        cell_id,
        unit,
        ancestors,
        match_kind: if distance_m.is_some() {
            "nearest"
        } else {
            "contains"
        }
        .to_owned(),
        distance_m: distance_m.map(|d| d.round()),
    })
}

// ───────────────────────────── series ──────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct SeriesQuery {
    pub geos: Vec<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub dims: Option<String>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct SeriesPoint {
    pub period: String,
    pub value: Option<f64>,
    pub lo: Option<f64>,
    pub hi: Option<f64>,
    pub basis: String,
    pub status: String,
    /// Registered quality/method flag names.
    #[schema(value_type = Vec<String>)]
    pub flags: Flags,
    pub obs_id: i64,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct SeriesLine {
    pub geo: String,
    pub name: Option<String>,
    pub dims: String,
    pub points: Vec<SeriesPoint>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct Series {
    pub indicator: IndicatorInfo,
    pub series: Vec<SeriesLine>,
}

#[derive(Debug, Clone, FromRow)]
struct SeriesRow {
    obs_id: i64,
    geo_key: i32,
    dims_key: i32,
    freq: i8,
    time_start: NaiveDate,
    value: Option<f64>,
    lo: Option<f64>,
    hi: Option<f64>,
    basis: i16,
    status: i16,
    flags: i32,
}

struct Resolved<'c> {
    ind: &'c IndicatorInfo,
    dataset_key: i32,
    ver: &'c CurrentVersion,
}

fn resolve_indicator<'c>(cat: &'c Catalog, id: &str) -> CoreResult<Resolved<'c>> {
    let ind = cat
        .indicators
        .get(id)
        .ok_or_else(|| CoreError::not_found(format!("no indicator \"{id}\"")))?;
    let ds = cat.datasets.get(&ind.dataset_id);
    let ver = ds.and_then(|d| d.current.as_ref()).ok_or_else(|| {
        CoreError::not_found(format!("indicator \"{id}\" has no published data yet"))
    })?;
    Ok(Resolved {
        ind,
        dataset_key: ds.map_or(0, |d| d.key),
        ver,
    })
}

fn parse_bound(cat: &Catalog, p: Option<&String>) -> CoreResult<Option<NaiveDate>> {
    let cal = &cat.profile()?.calendars;
    p.map(|s| {
        Period::infer(s, cal)
            .map(|p| p.start)
            .map_err(|e| CoreError::bad_request(e.to_string()))
    })
    .transpose()
}

/// Dims filter → interned key. `Ok(None)` = no filter; `Err` = no such
/// combination in the catalogue (nothing can match).
fn dims_filter(cat: &Catalog, dims: Option<&str>) -> CoreResult<Result<Option<i32>, ()>> {
    let Some(d) = dims else { return Ok(Ok(None)) };
    let canon = Dims::parse(d)
        .map_err(|e| CoreError::bad_request(e.to_string()))?
        .canonical();
    Ok(cat.dims.key(&canon).map(Some).ok_or(()))
}

/// Geography codes → `(key, code, name)` of the units that exist.
async fn resolve_geos(pool: &PgPool, codes: &[String]) -> CoreResult<Vec<(i32, String, String)>> {
    Ok(
        sqlx::query_as("SELECT key, code, name_en FROM geo.units WHERE code = ANY($1)")
            .bind(codes)
            .fetch_all(pool)
            .await?,
    )
}

pub async fn series(
    pool: &PgPool,
    cat: &Catalog,
    indicator_id: &str,
    q: &SeriesQuery,
) -> CoreResult<Series> {
    let r = resolve_indicator(cat, indicator_id)?;
    let profile = cat.profile()?;
    if q.geos.is_empty() {
        return Err(CoreError::bad_request(format!(
            "geo is required (comma-separated codes, e.g. geo={0},{0}1)",
            profile.admin.root()
        )));
    }
    if q.geos.len() > MAX_SERIES_GEOS {
        return Err(CoreError::bad_request(format!(
            "at most {MAX_SERIES_GEOS} geographies per request"
        )));
    }
    let codes: Vec<String> = q
        .geos
        .iter()
        .map(|g| {
            profile
                .admin
                .parse_geo(g)
                .map(|g| g.to_code())
                .map_err(|e| CoreError::bad_request(e.to_string()))
        })
        .collect::<CoreResult<_>>()?;
    let from = parse_bound(cat, q.from.as_ref())?;
    let to = parse_bound(cat, q.to.as_ref())?;
    let Ok(dims_key) = dims_filter(cat, q.dims.as_deref())? else {
        return Ok(Series {
            indicator: r.ind.clone(),
            series: Vec::new(),
        });
    };
    let units = resolve_geos(pool, &codes).await?;
    let keys: Vec<i32> = units.iter().map(|u| u.0).collect();
    let by_key: HashMap<i32, (&str, &str)> = units
        .iter()
        .map(|(k, c, n)| (*k, (c.as_str(), n.as_str())))
        .collect();

    let mut rows: Vec<SeriesRow> = sqlx::query_as(
        "SELECT obs_id, geo_key, dims_key, freq, time_start, value, lo, hi, basis, status, flags
         FROM data.observations
         WHERE dataset_key = $1 AND indicator_key = $2 AND geo_key = ANY($3)
           AND vto = 2147483647 AND vfrom <= $4
           AND ($5::date IS NULL OR time_start >= $5)
           AND ($6::date IS NULL OR time_start <= $6)
           AND ($7::int4 IS NULL OR dims_key = $7)",
    )
    .bind(r.dataset_key)
    .bind(r.ind.key)
    .bind(&keys)
    .bind(r.ver.seq)
    .bind(from)
    .bind(to)
    .bind(dims_key)
    .fetch_all(pool)
    .await?;
    let code_of = |k: i32| by_key.get(&k).map_or("", |c| c.0);
    rows.sort_by(|a, b| {
        (code_of(a.geo_key), dims_text(cat, a.dims_key), a.time_start).cmp(&(
            code_of(b.geo_key),
            dims_text(cat, b.dims_key),
            b.time_start,
        ))
    });

    let mut series: Vec<SeriesLine> = Vec::new();
    for row in rows {
        let point = SeriesPoint {
            period: render_period(cat, row.freq, row.time_start)?,
            value: row.value,
            lo: row.lo,
            hi: row.hi,
            basis: basis_of(row.basis).as_str().to_owned(),
            status: status_of(row.status).as_str().to_owned(),
            flags: Flags::from_db_lossy(row.flags),
            obs_id: row.obs_id,
        };
        let (code, name) = by_key.get(&row.geo_key).copied().unwrap_or(("", ""));
        let dims = dims_text(cat, row.dims_key);
        match series.last_mut() {
            Some(l) if l.geo == code && l.dims == dims => l.points.push(point),
            _ => series.push(SeriesLine {
                geo: code.to_owned(),
                name: Some(name.to_owned()),
                dims,
                points: vec![point],
            }),
        }
    }
    Ok(Series {
        indicator: r.ind.clone(),
        series,
    })
}

// ───────────────────────────── choropleth map ──────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct MapQuery {
    pub level: i16,
    pub period: Option<String>,
    pub parent: Option<String>,
    pub dims: Option<String>,
}

/// Compact columnar payload for choropleths: join `pcodes[i]` to the
/// `admin.pmtiles` feature id client-side; no geometry on the wire.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct MapData {
    pub indicator_id: String,
    pub unit: Option<String>,
    pub unit_mult: i16,
    pub level: i16,
    pub period: String,
    pub dims: String,
    pub pcodes: Vec<String>,
    pub values: Vec<Option<f64>>,
    pub lo: Vec<Option<f64>>,
    pub hi: Vec<Option<f64>>,
    pub basis: Vec<String>,
    pub status: Vec<String>,
    /// Quantile class breaks (≤ 5 classes) over non-null values.
    pub breaks: Vec<f64>,
    /// Other dims keys available for this indicator/period/level.
    pub available_dims: Vec<String>,
}

type MapRow = (String, Option<f64>, Option<f64>, Option<f64>, i16, i16);

pub async fn map(
    pool: &PgPool,
    cat: &Catalog,
    indicator_id: &str,
    q: &MapQuery,
) -> CoreResult<MapData> {
    let r = resolve_indicator(cat, indicator_id)?;
    let profile = cat.profile()?;
    let max_level = i16::from(profile.admin.max_level());
    if !(0..=max_level).contains(&q.level) {
        return Err(CoreError::bad_request(format!(
            "level must be 0..{max_level}"
        )));
    }
    let freq = r.ind.frequency()?;
    let parent_key: Option<i32> = match q.parent.as_deref() {
        None => None,
        Some(p) => {
            let p = profile
                .admin
                .parse(p)
                .map_err(|e| CoreError::bad_request(e.to_string()))?;
            let k: Option<i32> = sqlx::query_scalar("SELECT key FROM geo.units WHERE code = $1")
                .bind(p.as_str())
                .fetch_optional(pool)
                .await?;
            Some(k.ok_or_else(|| CoreError::not_found(format!("no admin unit \"{p}\"")))?)
        }
    };

    // Period: requested, else the latest held at this level.
    let start: NaiveDate = match &q.period {
        Some(p) => {
            Period::parse(freq, p, &profile.calendars)
                .map_err(|e| CoreError::bad_request(e.to_string()))?
                .start
        }
        None => sqlx::query_scalar(
            "SELECT time_start FROM data.observations
             WHERE dataset_key = $1 AND indicator_key = $2 AND geo_level = $3
               AND vto = 2147483647 AND vfrom <= $4
             ORDER BY time_start DESC LIMIT 1",
        )
        .bind(r.dataset_key)
        .bind(r.ind.key)
        .bind(q.level)
        .bind(r.ver.seq)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| {
            CoreError::not_found(format!("\"{}\" has no data at level {}", r.ind.id, q.level))
        })?,
    };
    let period = Period { freq, start }.render(&profile.calendars);

    let dims_keys: Vec<i32> = sqlx::query_scalar(
        "SELECT DISTINCT dims_key FROM data.observations
         WHERE dataset_key = $1 AND indicator_key = $2 AND time_start = $3 AND freq = $4
           AND geo_level = $5 AND vto = 2147483647 AND vfrom <= $6",
    )
    .bind(r.dataset_key)
    .bind(r.ind.key)
    .bind(start)
    .bind(freq.as_db())
    .bind(q.level)
    .bind(r.ver.seq)
    .fetch_all(pool)
    .await?;
    let mut available: Vec<(String, i32)> = dims_keys
        .into_iter()
        .map(|k| (dims_text(cat, k), k))
        .collect();
    available.sort();
    let (dims, dims_key) = match q.dims.as_deref() {
        Some(d) => {
            let canon = Dims::parse(d)
                .map_err(|e| CoreError::bad_request(e.to_string()))?
                .canonical();
            match available.iter().find(|(t, _)| *t == canon) {
                Some((t, k)) => (t.clone(), *k),
                None => {
                    return Err(CoreError::bad_request(format!(
                        "dims \"{d}\" not available; valid values: {}",
                        available
                            .iter()
                            .map(|a| a.0.as_str())
                            .collect::<Vec<_>>()
                            .join(" | ")
                    )));
                }
            }
        }
        None => available.first().cloned().unwrap_or_default(),
    };

    let rows: Vec<MapRow> = sqlx::query_as(
        "SELECT u.code, o.value, o.lo, o.hi, o.basis, o.status
         FROM data.observations o JOIN geo.units u ON u.key = o.geo_key
         WHERE o.dataset_key = $1 AND o.indicator_key = $2 AND o.time_start = $3 AND o.freq = $4
           AND o.geo_level = $5 AND o.dims_key = $6 AND o.vto = 2147483647 AND o.vfrom <= $7
           AND ($8::int4 IS NULL OR u.anc_keys @> ARRAY[$8::int4])
         ORDER BY u.code",
    )
    .bind(r.dataset_key)
    .bind(r.ind.key)
    .bind(start)
    .bind(freq.as_db())
    .bind(q.level)
    .bind(dims_key)
    .bind(r.ver.seq)
    .bind(parent_key)
    .fetch_all(pool)
    .await?;

    let n = rows.len();
    let mut m = MapData {
        indicator_id: r.ind.id.clone(),
        unit: r.ind.unit.clone(),
        unit_mult: r.ind.unit_mult,
        level: q.level,
        period,
        dims,
        pcodes: Vec::with_capacity(n),
        values: Vec::with_capacity(n),
        lo: Vec::with_capacity(n),
        hi: Vec::with_capacity(n),
        basis: Vec::with_capacity(n),
        status: Vec::with_capacity(n),
        breaks: Vec::new(),
        available_dims: available.into_iter().map(|a| a.0).collect(),
    };
    for (code, v, lo, hi, b, s) in rows {
        m.pcodes.push(code);
        m.values.push(v);
        m.lo.push(lo);
        m.hi.push(hi);
        m.basis.push(basis_of(b).as_str().to_owned());
        m.status.push(status_of(s).as_str().to_owned());
    }
    m.breaks = quantile_breaks(&m.values, 5);
    Ok(m)
}

/// Quantile breaks: the upper bounds of the first `classes - 1` classes,
/// deduplicated. Empty for fewer than two distinct values.
#[must_use]
pub fn quantile_breaks(values: &[Option<f64>], classes: usize) -> Vec<f64> {
    let mut v: Vec<f64> = values
        .iter()
        .flatten()
        .copied()
        .filter(|x| x.is_finite())
        .collect();
    if v.len() < 2 || classes < 2 {
        return Vec::new();
    }
    v.sort_by(f64::total_cmp);
    let n = v.len();
    let mut out: Vec<f64> = (1..classes)
        .map(|k| v[(k * n / classes).min(n - 1)])
        .collect();
    out.dedup();
    out
}

// ───────────────────────────── catalog / datasets ──────────────────────────

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct CatalogEntry {
    pub id: String,
    pub title: String,
    pub description: String,
    pub kind: String,
    pub publication_class: String,
    pub themes: Vec<String>,
    pub licence_statuses: Vec<String>,
    pub indicator_count: usize,
    pub published: bool,
    pub dsv: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct CatalogQuery {
    pub q: Option<String>,
    pub theme: Option<String>,
}

#[must_use]
pub fn catalog_list(cat: &Catalog, q: &CatalogQuery) -> Vec<CatalogEntry> {
    let needle =
        q.q.as_deref()
            .map(str::to_lowercase)
            .filter(|s| !s.is_empty());
    let theme = q.theme.as_deref().map(str::to_uppercase);
    cat.dataset_order
        .iter()
        .filter_map(|id| cat.datasets.get(id))
        .filter_map(|d| {
            let sources: Vec<&SourceInfo> = d
                .source_ids
                .iter()
                .filter_map(|s| cat.sources.get(s))
                .collect();
            let mut themes: Vec<String> = sources
                .iter()
                .flat_map(|s| s.themes.iter().cloned())
                .collect();
            themes.sort();
            themes.dedup();
            if let Some(t) = &theme
                && !themes.contains(t)
            {
                return None;
            }
            if let Some(n) = &needle {
                let hit = d.title.to_lowercase().contains(n)
                    || d.description.to_lowercase().contains(n)
                    || d.indicator_ids.iter().any(|i| {
                        i.contains(n.as_str())
                            || cat
                                .indicators
                                .get(i)
                                .is_some_and(|ii| ii.label.to_lowercase().contains(n))
                    });
                if !hit {
                    return None;
                }
            }
            let mut licence_statuses: Vec<String> = sources
                .iter()
                .filter_map(|s| cat.licences.get(&s.licence_id).map(|l| l.status.clone()))
                .collect();
            licence_statuses.sort();
            licence_statuses.dedup();
            Some(CatalogEntry {
                id: d.id.clone(),
                title: d.title.clone(),
                description: d.description.clone(),
                kind: d.kind.clone(),
                publication_class: d.publication_class.clone(),
                themes,
                licence_statuses,
                indicator_count: d.indicator_ids.len(),
                published: d.current.is_some(),
                dsv: d.current.as_ref().map(|c| c.dsv.clone()),
            })
        })
        .collect()
}

#[derive(Debug, Clone, Serialize, ToSchema, FromRow)]
pub struct SourceFreshness {
    pub source_id: String,
    pub cadence: String,
    pub last_status: Option<String>,
    pub last_started_at: Option<DateTime<Utc>>,
    pub last_success_at: Option<DateTime<Utc>>,
    pub consecutive_failures: i64,
    /// fresh | due | overdue | failing | in_review | expected_missing | never_run | paused | retired
    pub state: String,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct DatasetCard {
    #[serde(flatten)]
    pub dataset: DatasetInfo,
    pub sources: Vec<SourceInfo>,
    pub licences: Vec<LicenceInfo>,
    pub indicators: Vec<IndicatorInfo>,
    pub freshness: Vec<SourceFreshness>,
}

/// Freshness of the given sources from `ops.source_freshness` (migration
/// 0004): `fresh | due | overdue | failing | in_review | expected_missing |
/// never_run | paused | retired`, computed in SQL so the scheduler, the
/// status API and dataset cards agree.
pub async fn source_freshness(
    pool: &PgPool,
    source_ids: &[String],
) -> CoreResult<Vec<SourceFreshness>> {
    Ok(sqlx::query_as(
        "SELECT source_id, cadence, last_status, last_run_at AS last_started_at, last_success_at,
                consecutive_failures, state
         FROM ops.source_freshness WHERE source_id = ANY($1) ORDER BY source_id",
    )
    .bind(source_ids)
    .fetch_all(pool)
    .await?)
}

pub async fn dataset_card(pool: &PgPool, cat: &Catalog, id: &str) -> CoreResult<DatasetCard> {
    let d = cat
        .datasets
        .get(id)
        .ok_or_else(|| CoreError::not_found(format!("no dataset \"{id}\"")))?;
    let sources: Vec<SourceInfo> = d
        .source_ids
        .iter()
        .filter_map(|s| cat.sources.get(s).cloned())
        .collect();
    let mut licences: Vec<LicenceInfo> = sources
        .iter()
        .filter_map(|s| cat.licences.get(&s.licence_id).cloned())
        .collect();
    licences.sort_by(|a, b| a.id.cmp(&b.id));
    licences.dedup_by(|a, b| a.id == b.id);
    let indicators = d
        .indicator_ids
        .iter()
        .filter_map(|i| cat.indicators.get(i).cloned())
        .collect();
    let freshness = source_freshness(pool, &d.source_ids).await?;
    Ok(DatasetCard {
        dataset: d.clone(),
        sources,
        licences,
        indicators,
        freshness,
    })
}

// ───────────────────────────── provenance ──────────────────────────────────

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct Provenance {
    pub obs_id: i64,
    pub dataset_id: String,
    /// Version that introduced this row (`vfrom`).
    pub dsv: String,
    /// Version that closed it, when superseded or removed.
    pub closed_by_dsv: Option<String>,
    pub indicator_id: String,
    pub geo_code: String,
    pub period: String,
    pub dims: String,
    pub value: Option<f64>,
    pub lo: Option<f64>,
    pub hi: Option<f64>,
    pub basis: String,
    pub status: String,
    /// Registered quality/method flag names.
    #[schema(value_type = Vec<String>)]
    pub flags: Flags,
    /// The token exactly as published upstream.
    pub verbatim: Option<String>,
    /// Where in the artifact: `{sheet, cell}`, `{url, path}`, `{page, bbox}` …
    #[schema(value_type = Object)]
    pub locator: Option<serde_json::Value>,
    pub method: Option<String>,
    pub confidence: Option<f32>,
    pub review: Option<String>,
    pub reviewed_by: Option<String>,
    pub reviewed_at: Option<DateTime<Utc>>,
    /// SHA-256 of the archived upstream bytes.
    pub artifact_sha256: Option<String>,
    pub artifact_url: Option<String>,
    pub fetched_at: Option<DateTime<Utc>>,
    pub media_type: Option<String>,
    pub edition: Option<String>,
}

#[derive(Debug, Clone, FromRow)]
struct ProvRow {
    obs_id: i64,
    dsv: String,
    closed_by_dsv: Option<String>,
    indicator_key: i32,
    geo_code: String,
    dims_key: i32,
    freq: i8,
    time_start: NaiveDate,
    value: Option<f64>,
    lo: Option<f64>,
    hi: Option<f64>,
    basis: i16,
    status: i16,
    flags: i32,
    verbatim: Option<String>,
    locator: Option<serde_json::Value>,
    method: Option<i16>,
    confidence: Option<f32>,
    review: Option<i16>,
    reviewed_by: Option<String>,
    reviewed_at: Option<DateTime<Utc>>,
    artifact_sha256: Option<String>,
    artifact_url: Option<String>,
    fetched_at: Option<DateTime<Utc>>,
    media_type: Option<String>,
    edition: Option<String>,
}

pub async fn provenance(pool: &PgPool, cat: &Catalog, obs_id: i64) -> CoreResult<Provenance> {
    let missing = || CoreError::not_found(format!("no observation {obs_id}"));
    let dataset_key = i32::try_from(obs_id >> OBS_KEY_SHIFT).map_err(|_| missing())?;
    let dataset_id = cat.dataset_by_key.get(&dataset_key).ok_or_else(missing)?;
    let r: ProvRow = sqlx::query_as(
        "SELECT o.obs_id, vf.dsv, vt.dsv AS closed_by_dsv, o.indicator_key, u.code AS geo_code,
                o.dims_key, o.freq, o.time_start, o.value, o.lo, o.hi, o.basis, o.status, o.flags,
                p.verbatim, p.locator, p.method, p.confidence, p.review, vf.reviewed_by, vf.reviewed_at,
                encode(a.sha256, 'hex') AS artifact_sha256, a.url AS artifact_url, a.fetched_at,
                a.media_type, a.edition
         FROM data.observations o
         JOIN geo.units u ON u.key = o.geo_key
         JOIN data.dataset_versions vf ON vf.dataset_id = $3 AND vf.seq = o.vfrom
         -- a release of seq n closes rows with vto = n - 1
         LEFT JOIN data.dataset_versions vt ON vt.dataset_id = $3 AND vt.seq - 1 = o.vto
         LEFT JOIN data.obs_provenance p ON p.dataset_key = o.dataset_key AND p.obs_id = o.obs_id
         LEFT JOIN ops.artifacts a ON a.id = p.artifact_id
         WHERE o.dataset_key = $1 AND o.obs_id = $2",
    )
    .bind(dataset_key)
    .bind(obs_id)
    .bind(dataset_id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(missing)?;
    let indicator_id = cat
        .indicator_by_key
        .get(&r.indicator_key)
        .cloned()
        .unwrap_or_default();
    Ok(Provenance {
        obs_id: r.obs_id,
        dataset_id: dataset_id.clone(),
        dsv: r.dsv,
        closed_by_dsv: r.closed_by_dsv,
        indicator_id,
        geo_code: r.geo_code,
        period: render_period(cat, r.freq, r.time_start)?,
        dims: dims_text(cat, r.dims_key),
        value: r.value,
        lo: r.lo,
        hi: r.hi,
        basis: basis_of(r.basis).as_str().to_owned(),
        status: status_of(r.status).as_str().to_owned(),
        flags: Flags::from_db_lossy(r.flags),
        verbatim: r.verbatim,
        locator: r.locator,
        method: r
            .method
            .and_then(Method::from_code)
            .map(|m| m.as_str().to_owned()),
        confidence: r.confidence,
        review: r
            .review
            .and_then(Review::from_code)
            .map(|m| m.as_str().to_owned()),
        reviewed_by: r.reviewed_by,
        reviewed_at: r.reviewed_at,
        artifact_sha256: r.artifact_sha256,
        artifact_url: r.artifact_url,
        fetched_at: r.fetched_at,
        media_type: r.media_type,
        edition: r.edition,
    })
}

/// Parses a p-code with the catalog's profile (transport helper).
pub fn parse_pcode(cat: &Catalog, s: &str) -> CoreResult<Pcode> {
    cat.profile()?
        .admin
        .parse(s)
        .map_err(|e| CoreError::bad_request(e.to_string()))
}

/// Parses any geography code with the catalog's profile.
pub fn parse_geo(cat: &Catalog, s: &str) -> CoreResult<GeoCode> {
    cat.profile()?
        .admin
        .parse_geo(s)
        .map_err(|e| CoreError::bad_request(e.to_string()))
}

/// Upper bound of the version range of a current row (re-exported for SQL
/// literals in documentation and tests).
pub const OPEN_RANGE: i32 = VTO_OPEN;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn breaks_are_monotonic_and_deduped() {
        let v: Vec<Option<f64>> = (1..=100).map(|x| Some(f64::from(x))).collect();
        assert_eq!(quantile_breaks(&v, 5), vec![21.0, 41.0, 61.0, 81.0]);
        assert!(quantile_breaks(&[Some(1.0), Some(1.0), Some(1.0)], 5).len() <= 1);
        assert!(quantile_breaks(&[Some(1.0), None], 5).is_empty());
    }

    #[test]
    fn open_range_is_the_sql_literal() {
        assert_eq!(OPEN_RANGE, 2_147_483_647);
    }
}
