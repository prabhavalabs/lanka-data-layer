//! Columnar points (10 §3.3, ADR 0019, WP-27):
//! `GET /v2/features/{dataset}/points?fields=&kind=&within=&bbox=&format=arrow|json`.
//!
//! One row per feature of the current release at its representative point
//! (the point itself for point datasets; on-geometry points for lines and
//! polygons). Arrow IPC stream: `lon`, `lat` (`Float32`), `key` (`Utf8`),
//! then the requested fields — numbers as `Float32`, everything else
//! dictionary-encoded (`Dictionary<Int32, Utf8>`); `kind` and `name` are
//! always requestable. Field metadata `ldl:field` = the declared field
//! (`name, type, unit, measure_type`), schema metadata `ldl:points`
//! (dataset, count, query) and `ldl:meta` (the envelope meta, so
//! attribution travels with the bytes). JSON (≤ 2,000 rows):
//! `{lon[], lat[], key[], fields: {name: [...]}}` in the usual envelope.
//!
//! Limits: 250,000 rows (more → 413 with a pointer to the tileset). Encoded
//! responses are kept in a small in-process cache keyed by the ETag
//! (`"<dsv>-<hash(query)>"`), so a repeated request costs neither the query
//! nor the encoding; `If-None-Match` is answered before either.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};

use anyhow::Result;
use arrow_array::types::Int32Type;
use arrow_array::{ArrayRef, DictionaryArray, Float32Array, RecordBatch, StringArray};
use arrow_ipc::writer::StreamWriter;
use arrow_schema::{DataType, Field, Schema};
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, Uri, header};
use axum::response::IntoResponse;
use lanka_core::Meta;
use lanka_core::envelope::Link;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::Row;
use utoipa::{IntoParams, ToSchema};

use crate::frame_codec::ARROW_STREAM;
use crate::{ApiError, ApiResult, AppState, ErrorBody};

/// Rows per response (10 §3.3).
pub const MAX_POINTS: usize = 250_000;
/// Rows of the JSON form.
pub const MAX_JSON_POINTS: usize = 2_000;
const CACHE_BYTES: usize = 96 << 20;
const CACHE_FLOATING: &str = "public, max-age=300, stale-while-revalidate=86400";

/// A declared feature field (`registry/datasets/<id>.yaml` `fields:`).
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct FieldDecl {
    pub name: String,
    /// string | integer | number | boolean (`kind`, `name`: string)
    #[serde(rename = "type")]
    pub ty: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub measure_type: Option<String>,
}

impl FieldDecl {
    fn numeric(&self) -> bool {
        matches!(self.ty.as_str(), "integer" | "number")
    }
}

/// One requested column.
#[derive(Debug, Clone)]
pub enum FieldCol {
    Num(Vec<Option<f32>>),
    Text(Vec<Option<String>>),
}

/// Columnar points ready to encode.
#[derive(Debug, Clone, Default)]
pub struct PointCols {
    pub lon: Vec<f32>,
    pub lat: Vec<f32>,
    pub key: Vec<String>,
    pub fields: Vec<(FieldDecl, FieldCol)>,
}

impl PointCols {
    #[must_use]
    pub fn len(&self) -> usize {
        self.lon.len()
    }
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.lon.is_empty()
    }
}

/// Arrow IPC stream of the points.
pub fn arrow(p: &PointCols, points_md: &Value, meta: &Meta) -> Result<Vec<u8>> {
    let mut fields = vec![
        Field::new("lon", DataType::Float32, false),
        Field::new("lat", DataType::Float32, false),
        Field::new("key", DataType::Utf8, false),
    ];
    let mut arrays: Vec<ArrayRef> = vec![
        Arc::new(Float32Array::from(p.lon.clone())),
        Arc::new(Float32Array::from(p.lat.clone())),
        Arc::new(StringArray::from_iter_values(&p.key)),
    ];
    for (decl, col) in &p.fields {
        let md = HashMap::from([(
            "ldl:field".to_owned(),
            serde_json::to_string(decl).unwrap_or_default(),
        )]);
        match col {
            FieldCol::Num(v) => {
                fields.push(Field::new(&decl.name, DataType::Float32, true).with_metadata(md));
                arrays.push(Arc::new(Float32Array::from(v.clone())));
            }
            FieldCol::Text(v) => {
                let d: DictionaryArray<Int32Type> = v.iter().map(Option::as_deref).collect();
                fields.push(
                    Field::new(
                        &decl.name,
                        DataType::Dictionary(Box::new(DataType::Int32), Box::new(DataType::Utf8)),
                        true,
                    )
                    .with_metadata(md),
                );
                arrays.push(Arc::new(d));
            }
        }
    }
    let schema = Arc::new(Schema::new(fields).with_metadata(HashMap::from([
        ("ldl:points".to_owned(), points_md.to_string()),
        (
            "ldl:meta".to_owned(),
            serde_json::to_string(meta).unwrap_or_default(),
        ),
    ])));
    let batch = RecordBatch::try_new(Arc::clone(&schema), arrays)?;
    let mut out = Vec::with_capacity(p.len() * 24 + 4096);
    {
        let mut w = StreamWriter::try_new(&mut out, &schema)?;
        w.write(&batch)?;
        w.finish()?;
    }
    Ok(out)
}

/// The JSON form of the payload.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct PointsJson {
    pub dataset: String,
    pub count: usize,
    pub lon: Vec<f32>,
    pub lat: Vec<f32>,
    pub key: Vec<String>,
    #[schema(value_type = Object)]
    pub fields: serde_json::Map<String, Value>,
    pub field_info: Vec<FieldDecl>,
}

fn to_json(dataset: &str, p: PointCols) -> PointsJson {
    let mut fields = serde_json::Map::new();
    let mut info = Vec::new();
    for (d, c) in p.fields {
        let v = match c {
            FieldCol::Num(v) => json!(v),
            FieldCol::Text(v) => json!(v),
        };
        fields.insert(d.name.clone(), v);
        info.push(d);
    }
    PointsJson {
        dataset: dataset.to_owned(),
        count: p.lon.len(),
        lon: p.lon,
        lat: p.lat,
        key: p.key,
        fields,
        field_info: info,
    }
}

#[derive(Debug, Deserialize, IntoParams)]
pub struct PointsParams {
    /// Comma-separated declared fields (plus `kind`, `name`).
    pub fields: Option<String>,
    /// Comma-separated kinds.
    pub kind: Option<String>,
    /// Admin unit code: features in it or below it.
    pub within: Option<String>,
    /// `west,south,east,north`.
    pub bbox: Option<String>,
    /// `arrow` (default) | `json` (≤ 2,000 rows).
    pub format: Option<String>,
}

fn bad(m: impl Into<String>) -> ApiError {
    ApiError::new(StatusCode::BAD_REQUEST, m)
}

type Cache = (usize, HashMap<String, Arc<Vec<u8>>>);
static CACHE: LazyLock<Mutex<Cache>> = LazyLock::new(|| Mutex::new((0, HashMap::new())));

fn cached(etag: &str) -> Option<Arc<Vec<u8>>> {
    CACHE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .1
        .get(etag)
        .cloned()
}

fn remember(etag: String, body: Arc<Vec<u8>>) {
    let mut g = CACHE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if body.len() > CACHE_BYTES / 4 {
        return;
    }
    if g.0 + body.len() > CACHE_BYTES {
        g.1.clear();
        g.0 = 0;
    }
    g.0 += body.len();
    g.1.insert(etag, body);
}

fn hash16(s: &str) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(&Sha256::digest(s.as_bytes())[..8])
}

/// The declared fields of a features dataset.
async fn declared(st: &AppState, dataset: &str) -> Result<Vec<FieldDecl>, ApiError> {
    let v: Option<Value> =
        sqlx::query_scalar("SELECT spec -> 'fields' FROM catalog.datasets WHERE id = $1")
            .bind(dataset)
            .fetch_optional(&st.pool)
            .await
            .map_err(|e| ApiError::from(lanka_core::CoreError::from(e)))?
            .flatten();
    Ok(v.and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default())
}

/// Every feature of a dataset as columnar points (Arrow IPC or JSON).
#[utoipa::path(get, path = "/v2/features/{dataset}/points", tag = "features",
    params(("dataset" = String, Path, description = "Features dataset"), PointsParams),
    responses((status = 200, description = "Arrow IPC stream (lon, lat, key, fields)",
               content(("application/vnd.apache.arrow.stream"), (lanka_core::Envelope<PointsJson> = "application/json"))),
              (status = 400, body = ErrorBody), (status = 404, body = ErrorBody), (status = 413, body = ErrorBody)))]
#[allow(clippy::too_many_lines)]
pub async fn points(
    State(st): State<AppState>,
    Path(dataset): Path<String>,
    Query(p): Query<PointsParams>,
    headers: HeaderMap,
    uri: Uri,
) -> ApiResult {
    let cat = st.catalog();
    let format = p.format.as_deref().unwrap_or("arrow");
    if !matches!(format, "arrow" | "json") {
        return Err(bad("format must be arrow or json"));
    }
    let (d, seq) = lanka_core::features::released(&cat, &dataset)
        .map_err(|e| ApiError::from(e).with_version(cat.data_version.clone()))?;
    let dsv = d
        .current
        .as_ref()
        .map(|c| c.dsv.clone())
        .unwrap_or_default();
    let decl = declared(&st, &dataset).await?;
    let list = |s: Option<&str>| -> Vec<String> {
        s.unwrap_or_default()
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect()
    };
    let wanted = list(p.fields.as_deref());
    let mut fields: Vec<FieldDecl> = Vec::new();
    for w in &wanted {
        let f = match w.as_str() {
            "kind" | "name" => FieldDecl {
                name: w.clone(),
                ty: "string".to_owned(),
                unit: None,
                measure_type: (w == "kind").then(|| "category".to_owned()),
            },
            _ => decl.iter().find(|d| &d.name == w).cloned().ok_or_else(|| {
                bad(format!(
                    "unknown field {w:?}: {dataset} declares {}",
                    std::iter::once("kind, name".to_owned())
                        .chain(decl.iter().map(|d| d.name.clone()))
                        .collect::<Vec<_>>()
                        .join(", ")
                ))
            })?,
        };
        if !fields.iter().any(|x| x.name == f.name) {
            fields.push(f);
        }
    }
    let kinds = list(p.kind.as_deref());
    let bbox: Option<Vec<f64>> = match p.bbox.as_deref().filter(|s| !s.is_empty()) {
        None => None,
        Some(b) => {
            let v: Vec<f64> = b
                .split(',')
                .map(|x| x.trim().parse::<f64>())
                .collect::<Result<_, _>>()
                .map_err(|_| bad("bbox must be four numbers: west,south,east,north"))?;
            if !(v.len() == 4 && v.iter().all(|x| x.is_finite()) && v[0] < v[2] && v[1] < v[3]) {
                return Err(bad("bbox must be west,south,east,north"));
            }
            Some(v)
        }
    };
    let within = p.within.clone().filter(|s| !s.is_empty());
    // The cache key: the version read plus the normalised query.
    let norm = format!(
        "{dataset}|{seq}|{}|{}|{}|{:?}|{format}",
        fields
            .iter()
            .map(|f| f.name.as_str())
            .collect::<Vec<_>>()
            .join(","),
        kinds.join(","),
        within.as_deref().unwrap_or(""),
        bbox
    );
    let etag = format!("\"{dsv}-{}\"", hash16(&norm));
    if let Some(r) = crate::not_modified(&headers, &etag, CACHE_FLOATING) {
        return Ok(r);
    }
    if format == "arrow"
        && let Some(body) = cached(&etag)
    {
        return arrow_response(body.as_ref().clone(), ARROW_STREAM, &etag, &cat, &dataset);
    }
    let within_key: Option<i32> = match within.as_deref() {
        None => None,
        Some(code) => {
            let pc = lanka_core::query::parse_geo(&cat, code).map_err(ApiError::from)?;
            Some(
                sqlx::query_scalar("SELECT key FROM geo.units WHERE code = $1")
                    .bind(pc.to_code())
                    .fetch_optional(&st.pool)
                    .await
                    .map_err(|e| ApiError::from(lanka_core::CoreError::from(e)))?
                    .ok_or_else(|| {
                        ApiError::new(StatusCode::NOT_FOUND, format!("not_found: area {code}"))
                    })?,
            )
        }
    };
    // Field expressions: bound names, never interpolated text.
    let mut cols = String::new();
    for (i, f) in fields.iter().enumerate() {
        let e = match f.name.as_str() {
            "kind" => "f.kind".to_owned(),
            "name" => "f.name_en".to_owned(),
            _ => format!("f.attrs ->> ${}", 7 + i),
        };
        cols.push_str(&format!(", {e} AS c{i}"));
    }
    let sql = format!(
        "SELECT ST_X(f.rep)::float4 AS lon, ST_Y(f.rep)::float4 AS lat, f.feature_key{cols}
         FROM geo.features f LEFT JOIN geo.units u ON u.key = f.unit_key
         WHERE f.dataset_key = $1 AND f.vto = 2147483647 AND f.vfrom <= $2
           AND (cardinality($3::text[]) = 0 OR f.kind = ANY($3))
           AND ($4::float8[] IS NULL OR f.rep && ST_MakeEnvelope($4[1], $4[2], $4[3], $4[4], 4326))
           AND ($5::int4 IS NULL OR f.unit_key = $5 OR $5 = ANY(u.anc_keys))
         ORDER BY f.feature_id LIMIT $6"
    );
    let max = if format == "json" {
        MAX_JSON_POINTS
    } else {
        MAX_POINTS
    };
    let mut q = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(d.key)
        .bind(seq)
        .bind(&kinds)
        .bind(bbox)
        .bind(within_key)
        .bind(i64::try_from(max + 1).unwrap_or(i64::MAX));
    for f in &fields {
        if !matches!(f.name.as_str(), "kind" | "name") {
            q = q.bind(f.name.clone());
        }
    }
    let rows = q
        .fetch_all(&st.pool)
        .await
        .map_err(|e| ApiError::from(lanka_core::CoreError::from(e)))?;
    if rows.len() > max {
        let hint = if format == "json" {
            "use format=arrow (≤ 250,000 points)".to_owned()
        } else {
            format!(
                "use the vector tileset (/v2/tiles/{dataset}.json) or narrow with kind=, within=, bbox="
            )
        };
        return Err(ApiError::new(
            StatusCode::PAYLOAD_TOO_LARGE,
            format!("too_large: more than {max} points; {hint}"),
        ));
    }
    let n = rows.len();
    let mut pc = PointCols {
        lon: Vec::with_capacity(n),
        lat: Vec::with_capacity(n),
        key: Vec::with_capacity(n),
        fields: fields
            .iter()
            .map(|f| {
                (
                    f.clone(),
                    if f.numeric() {
                        FieldCol::Num(Vec::with_capacity(n))
                    } else {
                        FieldCol::Text(Vec::with_capacity(n))
                    },
                )
            })
            .collect(),
    };
    for r in &rows {
        pc.lon.push(r.try_get::<f32, _>(0).unwrap_or(f32::NAN));
        pc.lat.push(r.try_get::<f32, _>(1).unwrap_or(f32::NAN));
        pc.key.push(r.try_get::<String, _>(2).unwrap_or_default());
        for (i, (_, col)) in pc.fields.iter_mut().enumerate() {
            let s: Option<String> = r.try_get(3 + i).unwrap_or(None);
            match col {
                FieldCol::Num(v) => v.push(s.and_then(|x| x.parse::<f32>().ok())),
                FieldCol::Text(v) => v.push(s),
            }
        }
    }
    drop(rows);
    let meta = cat.meta_for(
        [dataset.as_str()],
        vec![Link::new("tiles", format!("/v2/tiles/{dataset}.json"))],
    );
    if format == "json" {
        return crate::respond(&headers, &uri, to_json(&dataset, pc), meta);
    }
    let md = json!({ "dataset": dataset, "dsv": dsv, "count": n,
                     "kind": kinds, "within": within, "fields": fields });
    let body = tokio::task::spawn_blocking(move || arrow(&pc, &md, &meta))
        .await
        .map_err(|_| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "internal error"))?
        .map_err(|e| {
            tracing::error!(error = %e, "points arrow");
            ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "internal error")
        })?;
    let body = Arc::new(body);
    remember(etag.clone(), Arc::clone(&body));
    arrow_response(body.as_ref().clone(), ARROW_STREAM, &etag, &cat, &dataset)
}

fn arrow_response(
    body: Vec<u8>,
    ct: &'static str,
    etag: &str,
    cat: &lanka_core::Catalog,
    dataset: &str,
) -> ApiResult {
    let mut r = body.into_response();
    let h = r.headers_mut();
    h.insert(header::CONTENT_TYPE, HeaderValue::from_static(ct));
    if let Some(dv) = cat
        .data_version
        .as_deref()
        .and_then(|v| HeaderValue::from_str(v).ok())
    {
        h.insert("x-data-version", dv);
    }
    if let Some(v) = cat
        .datasets
        .get(dataset)
        .and_then(|d| d.current.as_ref())
        .and_then(|c| HeaderValue::from_str(&format!("{dataset}@{}", c.dsv)).ok())
    {
        h.insert("x-dataset-versions", v);
    }
    crate::with_cache(r, etag, CACHE_FLOATING)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 250,000 points with an OSM-like key and two fields: the Arrow stream
    /// stays under 5 MB on the wire and encodes in < 150 ms (release).
    #[test]
    fn quarter_million_points_are_small_and_fast() {
        let n = MAX_POINTS;
        #[allow(clippy::cast_precision_loss)]
        let p = PointCols {
            lon: (0..n).map(|i| 79.5 + (i % 2400) as f32 * 0.001).collect(),
            lat: (0..n).map(|i| 5.9 + (i / 2400) as f32 * 0.001).collect(),
            key: (0..n)
                .map(|i| format!("n{}", 1_000_000_000 + i * 7))
                .collect(),
            fields: vec![
                (
                    FieldDecl {
                        name: "kind".into(),
                        ty: "string".into(),
                        unit: None,
                        measure_type: None,
                    },
                    FieldCol::Text(
                        (0..n)
                            .map(|i| Some(["school", "hospital", "bank"][i % 3].to_owned()))
                            .collect(),
                    ),
                ),
                (
                    FieldDecl {
                        name: "beds".into(),
                        ty: "integer".into(),
                        unit: None,
                        measure_type: None,
                    },
                    FieldCol::Num(
                        (0..n)
                            .map(|i| (i % 5 == 0).then_some((i % 300) as f32))
                            .collect(),
                    ),
                ),
            ],
        };
        let meta = Meta::bare(None);
        let t = std::time::Instant::now();
        let b = arrow(&p, &json!({"count": n}), &meta).expect("arrow");
        let ms = t.elapsed().as_millis();
        let gz = {
            use std::io::Write;
            let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
            e.write_all(&b).expect("gz");
            e.finish().expect("gz").len()
        };
        eprintln!("250k points: {} bytes raw, {gz} gzip, {ms} ms", b.len());
        // raw: 4 + 4 + (4 + 11) key + 4 kind + 4 beds (+ validity) ≈ 31 B/pt
        assert!(gz < 5_000_000, "gzip {gz}");
        if !cfg!(debug_assertions) {
            assert!(ms < 150, "{ms} ms");
        }
        let r =
            arrow_ipc::reader::StreamReader::try_new(std::io::Cursor::new(b), None).expect("read");
        let batches: Vec<_> = r.map(|x| x.expect("batch")).collect();
        assert_eq!(batches[0].num_rows(), n);
        assert_eq!(batches[0].schema().field(0).data_type(), &DataType::Float32);
    }
}
