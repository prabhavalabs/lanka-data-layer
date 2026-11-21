//! The shared query core. REST v2, the CLI and (next) MCP call these
//! functions; none of them builds SQL of its own.
//!
//! Every query is a bound, prepared statement against an index designed for
//! it (see migrations/0001_foundation.sql `ensure_obs_partition`).

use std::collections::BTreeMap;

use chrono::{DateTime, NaiveDate, Utc};
use serde::Serialize;
use sqlx::{FromRow, PgPool};
use utoipa::ToSchema;

use crate::catalog::{
    Catalog, CurrentVersion, DatasetInfo, IndicatorInfo, LicenceInfo, SourceInfo,
};
use crate::error::{CoreError, CoreResult};
use crate::grid;
use crate::pcode::Pcode;
use crate::period::period_start;

/// Max geographies per series request.
pub const MAX_SERIES_GEOS: usize = 25;

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
    pcode: String,
    level: i16,
    name_en: String,
    name_si: Option<String>,
    name_ta: Option<String>,
    parent_pcode: Option<String>,
    adm1_pcode: Option<String>,
    adm2_pcode: Option<String>,
    adm3_pcode: Option<String>,
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
    pub status: String,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct AreaIndicator {
    pub indicator_id: String,
    pub label: String,
    pub unit: String,
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
    /// Root first: LK, province, district, …
    pub ancestors: Vec<AreaRef>,
    pub children: Vec<AreaRef>,
    pub indicators: Vec<AreaIndicator>,
}

#[derive(Debug, Clone, FromRow)]
struct LatestObs {
    obs_id: i64,
    dataset_id: String,
    indicator_id: String,
    period: String,
    dims: String,
    value: Option<f64>,
    status: String,
}

/// Area profile: hierarchy, children and the latest value of every indicator
/// held for the unit in the current release.
pub async fn area_profile(
    pool: &PgPool,
    cat: &Catalog,
    pcode: &Pcode,
) -> CoreResult<(AreaProfile, Vec<String>)> {
    let row: Option<AreaRow> = sqlx::query_as(
        "SELECT pcode, level, name_en, name_si, name_ta, parent_pcode, adm1_pcode, adm2_pcode, adm3_pcode,
                boundary_set, area_km2, ST_Y(centroid) AS lat, ST_X(centroid) AS lon,
                ST_XMin(bbox) AS west, ST_YMin(bbox) AS south, ST_XMax(bbox) AS east, ST_YMax(bbox) AS north
         FROM geo.admin_units WHERE pcode = $1",
    )
    .bind(pcode.as_str())
    .fetch_optional(pool)
    .await?;
    let row = row.ok_or_else(|| CoreError::not_found(format!("no admin unit \"{pcode}\"")))?;

    let mut chain: Vec<String> = Vec::with_capacity(4);
    if row.level > 0 {
        chain.push("LK".to_owned());
    }
    for (lvl, code) in [
        (1, &row.adm1_pcode),
        (2, &row.adm2_pcode),
        (3, &row.adm3_pcode),
    ] {
        if row.level > lvl
            && let Some(c) = code
        {
            chain.push(c.clone());
        }
    }
    if let Some(p) = &row.parent_pcode
        && !chain.contains(p)
    {
        chain.push(p.clone());
    }

    let versions = cat.current_version_ids();
    let (ancestors, children, latest) = tokio_join3(
        sqlx::query_as::<_, AreaRef>(
            "SELECT pcode, level, name_en, name_si, name_ta FROM geo.admin_units
             WHERE pcode = ANY($1) ORDER BY level",
        )
        .bind(&chain)
        .fetch_all(pool),
        sqlx::query_as::<_, AreaRef>(
            "SELECT pcode, level, name_en, name_si, name_ta FROM geo.admin_units
             WHERE parent_pcode = $1 ORDER BY name_en",
        )
        .bind(pcode.as_str())
        .fetch_all(pool),
        sqlx::query_as::<_, LatestObs>(
            "SELECT DISTINCT ON (indicator_id, dims)
                    obs_id, dataset_id, indicator_id, period, dims, value, status::text AS status
             FROM data.observations
             WHERE geo_code = $1 AND version_id = ANY($2)
             ORDER BY indicator_id, dims, period_start DESC",
        )
        .bind(pcode.as_str())
        .bind(&versions)
        .fetch_all(pool),
    )
    .await;
    let (ancestors, children, latest) = (ancestors?, children?, latest?);

    // Group per indicator; keep only the latest period per indicator so a
    // dims value that stopped being published doesn't mix vintages.
    let mut grouped: BTreeMap<String, Vec<LatestObs>> = BTreeMap::new();
    for o in latest {
        grouped.entry(o.indicator_id.clone()).or_default().push(o);
    }
    let mut touched: Vec<String> = Vec::new();
    let mut indicators = Vec::with_capacity(grouped.len());
    for (ind_id, mut obs) in grouped {
        let Some(max_period) = obs
            .iter()
            .map(|o| o.period.clone())
            .max_by_key(|p| period_start(p).ok())
        else {
            continue;
        };
        obs.retain(|o| o.period == max_period);
        let dataset_id = obs
            .first()
            .map(|o| o.dataset_id.clone())
            .unwrap_or_default();
        let (label, unit) = cat.indicators.get(&ind_id).map_or_else(
            || (ind_id.clone(), String::new()),
            |i| (i.label.clone(), i.unit.clone()),
        );
        if !touched.contains(&dataset_id) {
            touched.push(dataset_id.clone());
        }
        indicators.push(AreaIndicator {
            indicator_id: ind_id,
            label,
            unit,
            dataset_id,
            period: max_period,
            values: obs
                .into_iter()
                .map(|o| AreaValue {
                    obs_id: o.obs_id,
                    dims: o.dims,
                    value: o.value,
                    status: o.status,
                })
                .collect(),
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

async fn tokio_join3<A, B, C>(
    a: impl Future<Output = A>,
    b: impl Future<Output = B>,
    c: impl Future<Output = C>,
) -> (A, B, C) {
    tokio::join!(a, b, c)
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
    adm1_pcode: Option<String>,
    adm2_pcode: Option<String>,
    adm3_pcode: Option<String>,
    distance_m: Option<f64>,
}

/// Point-in-polygon against full-resolution COD-AB geometry (GiST index).
pub async fn reverse(pool: &PgPool, lat: f64, lon: f64) -> CoreResult<ReverseResult> {
    let cell_id = grid::cell_id(lat, lon).ok_or(CoreError::NotInCoverage)?;
    let hit: Option<ReverseHit> = sqlx::query_as(
        "SELECT pcode, level, name_en, name_si, name_ta, adm1_pcode, adm2_pcode, adm3_pcode,
                NULL::float8 AS distance_m
         FROM geo.admin_units
         WHERE level > 0 AND ST_Intersects(geom, ST_SetSRID(ST_MakePoint($2, $1), 4326))
         ORDER BY level DESC LIMIT 1",
    )
    .bind(lat)
    .bind(lon)
    .fetch_optional(pool)
    .await?;
    // Near-shore fallback: KNN over GN polygons (GiST `<->`), then an exact
    // geodesic distance check.
    let hit = match hit {
        Some(h) => Some(h),
        None => sqlx::query_as(
            "SELECT * FROM (
               SELECT pcode, level, name_en, name_si, name_ta, adm1_pcode, adm2_pcode, adm3_pcode,
                      ST_Distance(geom::geography, ST_SetSRID(ST_MakePoint($2, $1), 4326)::geography) AS distance_m
               FROM geo.admin_units WHERE level = 4
               ORDER BY geom <-> ST_SetSRID(ST_MakePoint($2, $1), 4326) LIMIT 3
             ) k WHERE distance_m <= $3 ORDER BY distance_m LIMIT 1",
        )
        .bind(lat)
        .bind(lon)
        .bind(REVERSE_SNAP_M)
        .fetch_optional(pool)
        .await?,
    };
    let ReverseHit {
        unit,
        adm1_pcode: a1,
        adm2_pcode: a2,
        adm3_pcode: a3,
        distance_m,
    } = hit.ok_or(CoreError::NotInCoverage)?;
    let mut chain = vec!["LK".to_owned()];
    chain.extend(
        [a1, a2, a3]
            .into_iter()
            .flatten()
            .filter(|c| *c != unit.pcode),
    );
    let ancestors = sqlx::query_as::<_, AreaRef>(
        "SELECT pcode, level, name_en, name_si, name_ta FROM geo.admin_units WHERE pcode = ANY($1) ORDER BY level",
    )
    .bind(&chain)
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
    pub status: String,
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
    geo_code: String,
    period: String,
    dims: String,
    value: Option<f64>,
    status: String,
}

fn resolve_indicator<'c>(
    cat: &'c Catalog,
    id: &str,
) -> CoreResult<(&'c IndicatorInfo, &'c CurrentVersion)> {
    let ind = cat
        .indicators
        .get(id)
        .ok_or_else(|| CoreError::not_found(format!("no indicator \"{id}\"")))?;
    let ver = cat
        .datasets
        .get(&ind.dataset_id)
        .and_then(|d| d.current.as_ref())
        .ok_or_else(|| {
            CoreError::not_found(format!("indicator \"{id}\" has no published data yet"))
        })?;
    Ok((ind, ver))
}

fn parse_bound(p: Option<&String>) -> CoreResult<Option<NaiveDate>> {
    p.map(|s| period_start(s).map_err(|e| CoreError::bad_request(e.to_string())))
        .transpose()
}

pub async fn series(
    pool: &PgPool,
    cat: &Catalog,
    indicator_id: &str,
    q: &SeriesQuery,
) -> CoreResult<Series> {
    let (ind, ver) = resolve_indicator(cat, indicator_id)?;
    if q.geos.is_empty() {
        return Err(CoreError::bad_request(
            "geo is required (comma-separated p-codes, e.g. geo=LK,LK11)",
        ));
    }
    if q.geos.len() > MAX_SERIES_GEOS {
        return Err(CoreError::bad_request(format!(
            "at most {MAX_SERIES_GEOS} geographies per request"
        )));
    }
    let geos: Vec<String> = q
        .geos
        .iter()
        .map(|g| {
            Pcode::parse(g)
                .map(String::from)
                .map_err(|e| CoreError::bad_request(e.to_string()))
        })
        .collect::<CoreResult<_>>()?;
    let from = parse_bound(q.from.as_ref())?;
    let to = parse_bound(q.to.as_ref())?;

    let rows: Vec<SeriesRow> = sqlx::query_as(
        "SELECT obs_id, geo_code, period, dims, value, status::text AS status
         FROM data.observations
         WHERE dataset_id = $1 AND version_id = $2 AND indicator_id = $3 AND geo_code = ANY($4)
           AND ($5::date IS NULL OR period_start >= $5)
           AND ($6::date IS NULL OR period_start <= $6)
           AND ($7::text IS NULL OR dims = $7)
         ORDER BY geo_code, dims, period_start",
    )
    .bind(&ind.dataset_id)
    .bind(ver.version_id)
    .bind(&ind.id)
    .bind(&geos)
    .bind(from)
    .bind(to)
    .bind(q.dims.as_deref())
    .fetch_all(pool)
    .await?;

    let names: Vec<(String, String)> =
        sqlx::query_as("SELECT pcode, name_en FROM geo.admin_units WHERE pcode = ANY($1)")
            .bind(&geos)
            .fetch_all(pool)
            .await?;
    let names: BTreeMap<String, String> = names.into_iter().collect();

    let mut series: Vec<SeriesLine> = Vec::new();
    for r in rows {
        let point = SeriesPoint {
            period: r.period,
            value: r.value,
            status: r.status,
            obs_id: r.obs_id,
        };
        match series.last_mut() {
            Some(l) if l.geo == r.geo_code && l.dims == r.dims => l.points.push(point),
            _ => series.push(SeriesLine {
                name: names.get(&r.geo_code).cloned(),
                geo: r.geo_code,
                dims: r.dims,
                points: vec![point],
            }),
        }
    }
    Ok(Series {
        indicator: ind.clone(),
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
    pub unit: String,
    pub level: i16,
    pub period: String,
    pub dims: String,
    pub pcodes: Vec<String>,
    pub values: Vec<Option<f64>>,
    /// Quantile class breaks (≤ 5 classes) over non-null values.
    pub breaks: Vec<f64>,
    /// Other dims keys available for this indicator/period/level.
    pub available_dims: Vec<String>,
}

pub async fn map(
    pool: &PgPool,
    cat: &Catalog,
    indicator_id: &str,
    q: &MapQuery,
) -> CoreResult<MapData> {
    let (ind, ver) = resolve_indicator(cat, indicator_id)?;
    if !(0..=4).contains(&q.level) {
        return Err(CoreError::bad_request("level must be 0..4"));
    }
    let parent = q
        .parent
        .as_deref()
        .map(|p| {
            Pcode::parse(p)
                .map(String::from)
                .map_err(|e| CoreError::bad_request(e.to_string()))
        })
        .transpose()?;

    // Period: requested, else the latest held at this level.
    let period: String = match &q.period {
        Some(p) => {
            period_start(p).map_err(|e| CoreError::bad_request(e.to_string()))?;
            p.clone()
        }
        None => sqlx::query_scalar(
            "SELECT period FROM data.observations
             WHERE dataset_id = $1 AND version_id = $2 AND indicator_id = $3 AND geo_level = $4
             ORDER BY period_start DESC LIMIT 1",
        )
        .bind(&ind.dataset_id)
        .bind(ver.version_id)
        .bind(&ind.id)
        .bind(q.level)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| {
            CoreError::not_found(format!("\"{}\" has no data at level {}", ind.id, q.level))
        })?,
    };

    let available_dims: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT dims FROM data.observations
         WHERE dataset_id = $1 AND version_id = $2 AND indicator_id = $3 AND period = $4 AND geo_level = $5
         ORDER BY dims",
    )
    .bind(&ind.dataset_id)
    .bind(ver.version_id)
    .bind(&ind.id)
    .bind(&period)
    .bind(q.level)
    .fetch_all(pool)
    .await?;
    let dims = match &q.dims {
        Some(d) if available_dims.contains(d) => d.clone(),
        Some(d) => {
            return Err(CoreError::bad_request(format!(
                "dims \"{d}\" not available; valid values: {}",
                available_dims.join(" | ")
            )));
        }
        None => available_dims.first().cloned().unwrap_or_default(),
    };

    let rows: Vec<(String, Option<f64>)> = sqlx::query_as(
        "SELECT o.geo_code, o.value FROM data.observations o
         WHERE o.dataset_id = $1 AND o.version_id = $2 AND o.indicator_id = $3
           AND o.period = $4 AND o.geo_level = $5 AND o.dims = $6
           AND ($7::text IS NULL OR EXISTS (
                 SELECT 1 FROM geo.admin_units u WHERE u.pcode = o.geo_code
                   AND $7 IN (u.parent_pcode, u.adm1_pcode, u.adm2_pcode, u.adm3_pcode)))
         ORDER BY o.geo_code",
    )
    .bind(&ind.dataset_id)
    .bind(ver.version_id)
    .bind(&ind.id)
    .bind(&period)
    .bind(q.level)
    .bind(&dims)
    .bind(parent.as_deref())
    .fetch_all(pool)
    .await?;

    let (pcodes, values): (Vec<String>, Vec<Option<f64>>) = rows.into_iter().unzip();
    let breaks = quantile_breaks(&values, 5);
    Ok(MapData {
        indicator_id: ind.id.clone(),
        unit: ind.unit.clone(),
        level: q.level,
        period,
        dims,
        pcodes,
        values,
        breaks,
        available_dims,
    })
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

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct SourceFreshness {
    pub source_id: String,
    pub cadence: String,
    pub last_status: Option<String>,
    pub last_started_at: Option<DateTime<Utc>>,
    pub last_success_at: Option<DateTime<Utc>>,
    pub consecutive_failures: i64,
    /// fresh | due | overdue | failing | never_run
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

#[derive(Debug, Clone, FromRow)]
struct HealthRow {
    source_id: String,
    cadence: String,
    last_status: Option<String>,
    last_started_at: Option<DateTime<Utc>>,
    last_success_at: Option<DateTime<Utc>>,
    consecutive_failures: i64,
    check_every_secs: Option<f64>,
}

/// Freshness state from the monitoring view. A source is `failing` after
/// three consecutive failed runs, `due` once its cadence has elapsed since
/// the last success, and `overdue` at twice the cadence.
pub async fn source_freshness(
    pool: &PgPool,
    source_ids: &[String],
) -> CoreResult<Vec<SourceFreshness>> {
    let rows: Vec<HealthRow> = sqlx::query_as(
        "SELECT source_id, cadence, last_status::text AS last_status, last_started_at, last_success_at,
                consecutive_failures, extract(epoch FROM check_every)::float8 AS check_every_secs
         FROM ops.source_health WHERE source_id = ANY($1) ORDER BY source_id",
    )
    .bind(source_ids)
    .fetch_all(pool)
    .await?;
    let now = Utc::now();
    Ok(rows
        .into_iter()
        .map(|r| {
            let state = freshness_state(
                now,
                r.last_success_at,
                r.check_every_secs,
                r.consecutive_failures,
            );
            SourceFreshness {
                source_id: r.source_id,
                cadence: r.cadence,
                last_status: r.last_status,
                last_started_at: r.last_started_at,
                last_success_at: r.last_success_at,
                consecutive_failures: r.consecutive_failures,
                state: state.to_owned(),
            }
        })
        .collect())
}

#[must_use]
pub fn freshness_state(
    now: DateTime<Utc>,
    last_success: Option<DateTime<Utc>>,
    check_every_secs: Option<f64>,
    consecutive_failures: i64,
) -> &'static str {
    if consecutive_failures >= 3 {
        return "failing";
    }
    let Some(ok) = last_success else {
        return "never_run";
    };
    let Some(every) = check_every_secs else {
        return "fresh";
    };
    #[allow(clippy::cast_precision_loss)]
    let age = (now - ok).num_seconds() as f64;
    if age > 2.0 * every {
        "overdue"
    } else if age > every {
        "due"
    } else {
        "fresh"
    }
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

#[derive(Debug, Clone, FromRow, Serialize, ToSchema)]
pub struct Provenance {
    pub obs_id: i64,
    pub dataset_id: String,
    pub dsv: String,
    pub indicator_id: String,
    pub geo_code: String,
    pub period: String,
    pub dims: String,
    pub value: Option<f64>,
    pub status: String,
    /// The token exactly as published upstream.
    pub verbatim: Option<String>,
    /// Where in the artifact: `{sheet, cell}`, `{url, path}`, `{page, bbox}` …
    #[schema(value_type = Object)]
    pub locator: Option<serde_json::Value>,
    pub method: String,
    pub confidence: Option<f32>,
    pub review: String,
    pub reviewed_by: Option<String>,
    pub reviewed_at: Option<DateTime<Utc>>,
    /// SHA-256 of the archived upstream bytes.
    pub artifact_sha256: Option<String>,
    pub artifact_url: Option<String>,
    pub fetched_at: Option<DateTime<Utc>>,
    pub media_type: Option<String>,
    pub edition: Option<String>,
}

pub async fn provenance(pool: &PgPool, obs_id: i64) -> CoreResult<Provenance> {
    sqlx::query_as(
        "SELECT o.obs_id, o.dataset_id, v.dsv, o.indicator_id, o.geo_code, o.period, o.dims, o.value,
                o.status::text AS status, o.verbatim, o.locator, o.method::text AS method, o.confidence,
                o.review::text AS review, v.reviewed_by, v.reviewed_at,
                encode(a.sha256, 'hex') AS artifact_sha256, a.url AS artifact_url, a.fetched_at,
                a.media_type, a.edition
         FROM data.observations o
         JOIN data.dataset_versions v ON v.id = o.version_id
         LEFT JOIN ops.artifacts a ON a.id = o.artifact_id
         WHERE o.obs_id = $1",
    )
    .bind(obs_id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| CoreError::not_found(format!("no observation {obs_id}")))
}

// ───────────────────────────── health ──────────────────────────────────────

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct Health {
    pub status: String,
    pub database: bool,
    pub postgis: Option<String>,
    pub data_version: Option<String>,
    pub datasets_published: usize,
    pub admin_units: i64,
}

pub async fn health(pool: &PgPool, cat: &Catalog) -> CoreResult<Health> {
    let (postgis, admin_units): (Option<String>, i64) =
        sqlx::query_as("SELECT postgis_lib_version(), (SELECT count(*) FROM geo.admin_units)")
            .fetch_one(pool)
            .await?;
    Ok(Health {
        status: "ok".to_owned(),
        database: true,
        postgis,
        data_version: cat.data_version.clone(),
        datasets_published: cat
            .datasets
            .values()
            .filter(|d| d.current.is_some())
            .count(),
        admin_units,
    })
}

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
    fn freshness_thresholds() {
        let now = Utc::now();
        let day = 86_400.0;
        let ago = |d: i64| Some(now - chrono::Duration::days(d));
        assert_eq!(freshness_state(now, None, Some(day), 0), "never_run");
        assert_eq!(freshness_state(now, ago(0), Some(day), 0), "fresh");
        assert_eq!(
            freshness_state(
                now,
                ago(1).map(|t| t - chrono::Duration::hours(1)),
                Some(day),
                0
            ),
            "due"
        );
        assert_eq!(freshness_state(now, ago(3), Some(day), 0), "overdue");
        assert_eq!(freshness_state(now, ago(0), Some(day), 3), "failing");
        assert_eq!(freshness_state(now, ago(900), None, 0), "fresh");
    }
}
