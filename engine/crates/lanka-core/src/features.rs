//! Feature reads (WP-04, 06 §2): GeoJSON pages of a `features` dataset in
//! the current release, one feature by key, nearest features, and the
//! tileset index.
//!
//! "Current" is the range-versioned read of 05 §2: `vfrom <= released seq
//! AND vto = open`, on the partial indexes of the dataset's partition.

use serde::Serialize;
use serde_json::value::RawValue;
use sqlx::PgPool;
use utoipa::ToSchema;

use crate::catalog::{Catalog, DatasetInfo};
use crate::{CoreError, CoreResult, Flags};

/// Page size limit (06 §2: 2,000 features per page).
pub const MAX_PAGE: i64 = 2000;
pub const DEFAULT_PAGE: i64 = 500;
/// `/v2/nearest` limits.
pub const MAX_NEAREST: i64 = 50;
pub const MAX_RADIUS_KM: f64 = 50.0;

#[derive(Debug, Clone, Default)]
pub struct FeatureQuery {
    /// `[west, south, east, north]`.
    pub bbox: Option<[f64; 4]>,
    /// Admin unit code: features whose unit is it or below it.
    pub within: Option<String>,
    pub kinds: Vec<String>,
    /// Name substring (case-insensitive).
    pub q: Option<String>,
    /// `next_cursor` of the previous page.
    pub cursor: Option<i64>,
    pub limit: Option<i64>,
}

/// Feature properties: **one flat schema** for GeoJSON, vector tiles and
/// columnar points (10 §3.3) — `key`, `kind`, `name` (English), `name_si`,
/// `name_ta`, `flags`, then the dataset's fields at the top level (never
/// nested under `attrs`). GeoJSON adds `unit`, `unit_match`, `cell_id`,
/// `rep`. A source field named like a core property is shadowed by it.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct FeatureProps {
    /// Upstream key (`way/123456789`); the feature's id.
    pub key: String,
    pub kind: String,
    /// English name (`name_en` in the store).
    pub name: Option<String>,
    pub name_si: Option<String>,
    pub name_ta: Option<String>,
    /// Deepest admin unit containing the representative point (or nearest).
    pub unit: Option<String>,
    /// `contains` | `nearest`
    pub unit_match: Option<String>,
    /// Canonical grid cell of the representative point.
    pub cell_id: i32,
    /// Representative point `[lon, lat]`.
    pub rep: [f64; 2],
    /// Flag names (`UNOFFICIAL_GEOM`, …).
    #[schema(value_type = Vec<String>)]
    pub flags: Flags,
    /// The dataset's fields, flattened into the properties.
    #[serde(flatten)]
    #[schema(value_type = Object)]
    pub attrs: serde_json::Map<String, serde_json::Value>,
}

/// Property names of [`FeatureProps`] a source field cannot take.
pub const CORE_PROPS: &[&str] = &[
    "key",
    "kind",
    "name",
    "name_si",
    "name_ta",
    "unit",
    "unit_match",
    "cell_id",
    "rep",
    "flags",
];

fn flat_attrs(v: serde_json::Value) -> serde_json::Map<String, serde_json::Value> {
    match v {
        serde_json::Value::Object(mut m) => {
            m.retain(|k, _| !CORE_PROPS.contains(&k.as_str()));
            m
        }
        _ => serde_json::Map::new(),
    }
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct GeoFeature {
    #[serde(rename = "type")]
    pub ty: &'static str,
    pub id: String,
    #[schema(value_type = Object)]
    pub geometry: Box<RawValue>,
    pub properties: FeatureProps,
    /// Distance from the query point, metres (`/v2/nearest` only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub distance_m: Option<f64>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct FeatureCollection {
    #[serde(rename = "type")]
    pub ty: &'static str,
    pub dataset: String,
    pub features: Vec<GeoFeature>,
    #[serde(rename = "numberReturned")]
    pub number_returned: usize,
    /// Pass as `cursor=` for the next page; absent on the last page.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<i64>,
}

#[derive(sqlx::FromRow)]
struct Row {
    feature_id: i64,
    feature_key: String,
    kind: String,
    name_en: Option<String>,
    name_si: Option<String>,
    name_ta: Option<String>,
    unit: Option<String>,
    unit_match: Option<i16>,
    cell_id: i32,
    lon: f64,
    lat: f64,
    flags: i32,
    attrs: serde_json::Value,
    geojson: String,
    distance_m: Option<f64>,
}

impl Row {
    fn into_feature(self) -> CoreResult<(i64, GeoFeature)> {
        let geometry = RawValue::from_string(self.geojson)
            .map_err(|e| CoreError::bad_request(format!("stored geometry: {e}")))?;
        Ok((
            self.feature_id,
            GeoFeature {
                ty: "Feature",
                id: self.feature_key.clone(),
                geometry,
                properties: FeatureProps {
                    key: self.feature_key,
                    kind: self.kind,
                    name: self.name_en,
                    name_si: self.name_si,
                    name_ta: self.name_ta,
                    unit: self.unit,
                    unit_match: self
                        .unit_match
                        .map(|m| if m == 0 { "contains" } else { "nearest" }.to_owned()),
                    cell_id: self.cell_id,
                    rep: [self.lon, self.lat],
                    flags: Flags::from_db_lossy(self.flags),
                    attrs: flat_attrs(self.attrs),
                },
                distance_m: self.distance_m.map(f64::round),
            },
        ))
    }
}

const SELECT: &str = "SELECT f.feature_id, f.feature_key, f.kind, f.name_en, f.name_si, f.name_ta,
        u.code AS unit, f.unit_match, f.cell_id, ST_X(f.rep) AS lon, ST_Y(f.rep) AS lat, f.flags, f.attrs,
        ST_AsGeoJSON(f.geom, 6) AS geojson";

/// The released features dataset and its current seq.
pub fn released<'a>(cat: &'a Catalog, dataset: &str) -> CoreResult<(&'a DatasetInfo, i32)> {
    let d = cat
        .datasets
        .get(dataset)
        .ok_or_else(|| CoreError::not_found(format!("dataset {dataset}")))?;
    if d.kind != "features" {
        return Err(CoreError::bad_request(format!(
            "dataset {dataset} is of kind {}, not features",
            d.kind
        )));
    }
    let seq = d.current.as_ref().map(|c| c.seq).ok_or_else(|| {
        CoreError::not_found(format!("dataset {dataset} has no released version"))
    })?;
    Ok((d, seq))
}

fn escape_like(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    o.push('%');
    for c in s.chars() {
        if matches!(c, '%' | '_' | '\\') {
            o.push('\\');
        }
        o.push(c);
    }
    o.push('%');
    o
}

/// One page of features (GeoJSON), ordered by feature id.
pub async fn features(
    pool: &PgPool,
    cat: &Catalog,
    dataset: &str,
    q: &FeatureQuery,
) -> CoreResult<FeatureCollection> {
    let (d, seq) = released(cat, dataset)?;
    let limit = q.limit.unwrap_or(DEFAULT_PAGE);
    if !(1..=MAX_PAGE).contains(&limit) {
        return Err(CoreError::bad_request(format!(
            "limit must be 1..{MAX_PAGE} (bulk downloads: /dl/datasets/{dataset}/)"
        )));
    }
    if let Some(b) = q.bbox
        && !(b.iter().all(|v| v.is_finite()) && b[0] < b[2] && b[1] < b[3])
    {
        return Err(CoreError::bad_request("bbox must be west,south,east,north"));
    }
    let within: Option<i32> = match q.within.as_deref() {
        None => None,
        Some(code) => {
            let pc = crate::query::parse_geo(cat, code)?;
            Some(
                sqlx::query_scalar("SELECT key FROM geo.units WHERE code = $1")
                    .bind(pc.to_code())
                    .fetch_optional(pool)
                    .await?
                    .ok_or_else(|| CoreError::not_found(format!("area {code}")))?,
            )
        }
    };
    let bbox = q.bbox.map(|b| b.to_vec());
    let rows: Vec<Row> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "{SELECT}, NULL::float8 AS distance_m
         FROM geo.features f LEFT JOIN geo.units u ON u.key = f.unit_key
         WHERE f.dataset_key = $1 AND f.vto = 2147483647 AND f.vfrom <= $2
           AND ($3::int8 IS NULL OR f.feature_id > $3)
           AND (cardinality($4::text[]) = 0 OR f.kind = ANY($4))
           AND ($5::float8[] IS NULL OR (f.geom && ST_MakeEnvelope($5[1], $5[2], $5[3], $5[4], 4326)
                AND ST_Intersects(f.geom, ST_MakeEnvelope($5[1], $5[2], $5[3], $5[4], 4326))))
           AND ($6::int4 IS NULL OR f.unit_key = $6 OR $6 = ANY(u.anc_keys))
           AND ($7::text IS NULL OR f.name_en ILIKE $7)
         ORDER BY f.feature_id LIMIT $8"
    )))
    .bind(d.key)
    .bind(seq)
    .bind(q.cursor)
    .bind(&q.kinds)
    .bind(bbox)
    .bind(within)
    .bind(
        q.q.as_deref()
            .filter(|s| !s.trim().is_empty())
            .map(|s| escape_like(s.trim())),
    )
    .bind(limit + 1)
    .fetch_all(pool)
    .await?;
    let more = i64::try_from(rows.len()).unwrap_or(i64::MAX) > limit;
    let mut out = Vec::with_capacity(rows.len());
    let mut last = None;
    for r in rows.into_iter().take(usize::try_from(limit).unwrap_or(0)) {
        let (id, f) = r.into_feature()?;
        last = Some(id);
        out.push(f);
    }
    Ok(FeatureCollection {
        ty: "FeatureCollection",
        dataset: dataset.to_owned(),
        number_returned: out.len(),
        features: out,
        next_cursor: if more { last } else { None },
    })
}

/// One feature by its upstream key.
pub async fn feature(
    pool: &PgPool,
    cat: &Catalog,
    dataset: &str,
    key: &str,
) -> CoreResult<GeoFeature> {
    let (d, seq) = released(cat, dataset)?;
    let r: Option<Row> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "{SELECT}, NULL::float8 AS distance_m
         FROM geo.features f LEFT JOIN geo.units u ON u.key = f.unit_key
         WHERE f.dataset_key = $1 AND f.vto = 2147483647 AND f.vfrom <= $2 AND f.feature_key = $3"
    )))
    .bind(d.key)
    .bind(seq)
    .bind(key)
    .fetch_optional(pool)
    .await?;
    r.ok_or_else(|| CoreError::not_found(format!("feature {dataset}/{key}")))?
        .into_feature()
        .map(|(_, f)| f)
}

#[derive(Debug, Clone, Default)]
pub struct NearestQuery {
    pub lat: f64,
    pub lon: f64,
    pub datasets: Vec<String>,
    pub kinds: Vec<String>,
    pub limit: Option<i64>,
    pub radius_km: Option<f64>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct NearestHit {
    pub dataset: String,
    #[serde(flatten)]
    pub feature: GeoFeature,
}

/// K nearest features (≤ 50) of one or more datasets, by distance to the
/// geometry (GiST KNN, then exact geodesic distance).
pub async fn nearest(
    pool: &PgPool,
    cat: &Catalog,
    q: &NearestQuery,
) -> CoreResult<Vec<NearestHit>> {
    if !(q.lat.is_finite() && q.lon.is_finite())
        || !(-90.0..=90.0).contains(&q.lat)
        || !(-180.0..=180.0).contains(&q.lon)
    {
        return Err(CoreError::bad_request("lat/lon out of range"));
    }
    let limit = q.limit.unwrap_or(10);
    if !(1..=MAX_NEAREST).contains(&limit) {
        return Err(CoreError::bad_request(format!(
            "limit must be 1..{MAX_NEAREST}"
        )));
    }
    let radius = q.radius_km.unwrap_or(MAX_RADIUS_KM);
    if !(radius > 0.0 && radius <= MAX_RADIUS_KM) {
        return Err(CoreError::bad_request(format!(
            "radius_km must be in (0, {MAX_RADIUS_KM}]"
        )));
    }
    if q.datasets.is_empty() || q.datasets.len() > 8 {
        return Err(CoreError::bad_request(
            "dataset: 1–8 comma-separated features datasets",
        ));
    }
    let mut hits: Vec<NearestHit> = Vec::new();
    for ds in &q.datasets {
        let (d, seq) = released(cat, ds)?;
        // KNN on the GiST index; candidates are 4× the limit so the exact
        // geodesic order (which can differ from the planar one) holds.
        let rows: Vec<Row> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "SELECT * FROM (
               {SELECT}, ST_Distance(f.geom::geography, ST_SetSRID(ST_MakePoint($4, $3), 4326)::geography) AS distance_m
               FROM (SELECT * FROM geo.features f
                     WHERE f.dataset_key = $1 AND f.vto = 2147483647 AND f.vfrom <= $2
                       AND (cardinality($5::text[]) = 0 OR f.kind = ANY($5))
                     ORDER BY f.geom <-> ST_SetSRID(ST_MakePoint($4, $3), 4326) LIMIT $6 * 4) f
               LEFT JOIN geo.units u ON u.key = f.unit_key) k
             WHERE distance_m <= $7 ORDER BY distance_m, feature_id LIMIT $6"
        )))
        .bind(d.key)
        .bind(seq)
        .bind(q.lat)
        .bind(q.lon)
        .bind(&q.kinds)
        .bind(limit)
        .bind(radius * 1000.0)
        .fetch_all(pool)
        .await?;
        for r in rows {
            hits.push(NearestHit {
                dataset: ds.clone(),
                feature: r.into_feature()?.1,
            });
        }
    }
    hits.sort_by(|a, b| {
        a.feature
            .distance_m
            .unwrap_or(f64::INFINITY)
            .total_cmp(&b.feature.distance_m.unwrap_or(f64::INFINITY))
    });
    hits.truncate(usize::try_from(limit).unwrap_or(0));
    Ok(hits)
}

/// A built tileset (newest build of each id).
#[derive(Debug, Clone, Serialize, ToSchema, sqlx::FromRow)]
pub struct Tileset {
    pub id: String,
    pub sha16: String,
    /// Path relative to the store root (`tiles/<id>/<sha16>.pmtiles`).
    #[serde(skip)]
    pub path: String,
    pub bytes: i64,
    pub minzoom: i16,
    pub maxzoom: i16,
    pub bounds: Option<Vec<f64>>,
    #[schema(value_type = Object)]
    pub layers: serde_json::Value,
    pub dataset_ids: Vec<String>,
    pub attribution: Option<String>,
    pub built_at: chrono::DateTime<chrono::Utc>,
}

pub async fn tilesets(pool: &PgPool) -> CoreResult<Vec<Tileset>> {
    Ok(sqlx::query_as(
        "SELECT id, sha16, path, bytes, minzoom, maxzoom, bounds, layers, dataset_ids, attribution, built_at
         FROM geo.current_tilesets ORDER BY id",
    )
    .fetch_all(pool)
    .await?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn like_patterns_are_escaped() {
        assert_eq!(escape_like("50%_a\\b"), "%50\\%\\_a\\\\b%");
    }
}
