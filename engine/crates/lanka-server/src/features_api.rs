//! Features and vector tiles (WP-04, 06 §2, ADR 0014).
//!
//! | Route | |
//! |---|---|
//! | `GET /v2/features/{dataset}?bbox=&within=&kind=&q=&cursor=&limit=&format=` | GeoJSON page (≤ 2,000) in `payload`; `format=geojson` = bare FeatureCollection |
//! | `GET /v2/features/{dataset}/{key}` | one feature by upstream key (`way/123`) |
//! | `GET /v2/nearest?lat=&lon=&dataset=&kind=&limit=&radius_km=` | KNN ≤ 50 within ≤ 50 km |
//! | `GET /v2/tiles` | tileset index |
//! | `GET /v2/tiles/{tileset}.json` | TileJSON 3.0.0 |
//! | `GET /v2/tiles/{tileset}/{z}/{x}/{y}.mvt` | one MVT tile read from the PMTiles file (gzip, as stored) |
//! | `GET /dl/tiles/{tileset}/{sha16}.pmtiles` | the file itself, with HTTP range requests (nginx serves this path in production) |
//!
//! PMTiles archives are opened once and their directories cached; the
//! tileset index (`geo.current_tilesets`) is re-read at most every 30 s.

use std::collections::HashMap;
use std::path::{Path as FsPath, PathBuf};
use std::sync::{Arc, LazyLock, Mutex, RwLock};
use std::time::{Duration, Instant};

use axum::Router;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, Uri, header};
use axum::response::IntoResponse;
use axum::routing::get;
use lanka_core::Envelope;
use lanka_core::envelope::Link;
use lanka_core::features::{
    self, FeatureCollection, FeatureQuery, GeoFeature, NearestHit, NearestQuery, Tileset,
};
use lanka_export::pmtiles::PmTiles;
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, OpenApi, ToSchema};

use crate::{ApiError, ApiResult, AppState, ErrorBody, respond};

const CACHE_TILE: &str = "public, max-age=300, stale-while-revalidate=86400";
const CACHE_FILE: &str = "public, max-age=31536000, immutable";

#[derive(OpenApi)]
#[openapi(
    paths(features_list, feature_one, nearest, tiles_index, tilejson, tile),
    tags((name = "features", description = "Points, lines and polygons (OSM, agency GIS) and vector tiles"))
)]
pub struct FeaturesDoc;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/v2/features/{dataset}", get(features_list))
        .route("/v2/features/{dataset}/{*key}", get(feature_one))
        .route("/v2/nearest", get(nearest))
        .route("/v2/tiles", get(tiles_index))
        .route("/v2/tiles/{file}", get(tilejson))
        .route("/v2/tiles/{tileset}/{z}/{x}/{y}", get(tile))
        .route("/dl/tiles/{tileset}/{file}", get(pmtiles_file))
}

fn err(st: &AppState, e: lanka_core::CoreError) -> ApiError {
    ApiError::from(e).with_version(st.catalog().data_version.clone())
}

fn list(s: Option<&str>) -> Vec<String> {
    s.unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}

fn bad(msg: impl Into<String>) -> ApiError {
    ApiError::new(StatusCode::BAD_REQUEST, msg)
}

#[derive(Debug, Deserialize, IntoParams)]
pub struct FeaturesParams {
    /// `west,south,east,north` (WGS84).
    pub bbox: Option<String>,
    /// Admin unit code: features in it or below it.
    pub within: Option<String>,
    /// Comma-separated kinds, e.g. `hospital,clinic`.
    pub kind: Option<String>,
    /// Name contains (case-insensitive).
    pub q: Option<String>,
    /// `next_cursor` of the previous page.
    pub cursor: Option<i64>,
    /// 1–2000 (default 500).
    pub limit: Option<i64>,
    /// `json` (default, enveloped) | `geojson` (bare FeatureCollection).
    pub format: Option<String>,
}

/// Features of a dataset in the current release, as GeoJSON.
#[utoipa::path(get, path = "/v2/features/{dataset}", tag = "features",
    params(("dataset" = String, Path, description = "Features dataset, e.g. osm-pois-amenities"), FeaturesParams),
    responses((status = 200, body = Envelope<FeatureCollection>), (status = 400, body = ErrorBody),
              (status = 404, body = ErrorBody)))]
async fn features_list(
    State(st): State<AppState>,
    Path(dataset): Path<String>,
    Query(p): Query<FeaturesParams>,
    headers: HeaderMap,
    uri: Uri,
) -> ApiResult {
    let cat = st.catalog();
    let bbox = match p.bbox.as_deref().filter(|s| !s.is_empty()) {
        None => None,
        Some(b) => {
            let v: Vec<f64> = b
                .split(',')
                .map(|x| x.trim().parse::<f64>())
                .collect::<Result<_, _>>()
                .map_err(|_| bad("bbox must be four numbers: west,south,east,north"))?;
            let a: [f64; 4] = v
                .try_into()
                .map_err(|_| bad("bbox must be four numbers: west,south,east,north"))?;
            Some(a)
        }
    };
    let q = FeatureQuery {
        bbox,
        within: p.within.filter(|s| !s.is_empty()),
        kinds: list(p.kind.as_deref()),
        q: p.q,
        cursor: p.cursor,
        limit: p.limit,
    };
    let fc = features::features(&st.pool, &cat, &dataset, &q)
        .await
        .map_err(|e| err(&st, e))?;
    let mut next = Vec::new();
    if let Some(c) = fc.next_cursor {
        let base = uri.query().unwrap_or_default();
        let kept: Vec<&str> = base
            .split('&')
            .filter(|kv| !kv.is_empty() && !kv.starts_with("cursor="))
            .collect();
        let mut qs = kept.join("&");
        if !qs.is_empty() {
            qs.push('&');
        }
        next.push(Link::new("next", format!("{}?{qs}cursor={c}", uri.path())));
    }
    let meta = cat.meta_for([dataset.as_str()], next);
    if matches!(p.format.as_deref(), Some("geojson")) {
        let etag = crate::etag_of(&uri, &meta);
        if let Some(r) = crate::not_modified(&headers, &etag, crate::CACHE_FLOATING) {
            return Ok(r);
        }
        let mut res = axum::Json(&fc).into_response();
        res.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/geo+json"),
        );
        return crate::with_cache(res, &etag, crate::CACHE_FLOATING);
    }
    respond(&headers, &uri, fc, meta)
}

/// One feature by its upstream key (e.g. `way/123456789`).
#[utoipa::path(get, path = "/v2/features/{dataset}/{key}", tag = "features",
    params(("dataset" = String, Path, description = "Features dataset"),
           ("key" = String, Path, description = "Upstream key, e.g. node/123 or way/456")),
    responses((status = 200, body = Envelope<GeoFeature>), (status = 404, body = ErrorBody)))]
async fn feature_one(
    State(st): State<AppState>,
    Path((dataset, key)): Path<(String, String)>,
    headers: HeaderMap,
    uri: Uri,
) -> ApiResult {
    let cat = st.catalog();
    let f = features::feature(&st.pool, &cat, &dataset, &key)
        .await
        .map_err(|e| err(&st, e))?;
    let next = f
        .properties
        .unit
        .as_ref()
        .map(|u| vec![Link::new("area", format!("/v2/areas/{u}"))])
        .unwrap_or_default();
    let meta = cat.meta_for([dataset.as_str()], next);
    respond(&headers, &uri, f, meta)
}

#[derive(Debug, Deserialize, IntoParams)]
pub struct NearestParams {
    pub lat: Option<f64>,
    pub lon: Option<f64>,
    /// Comma-separated features datasets (1–8).
    pub dataset: Option<String>,
    /// Comma-separated kinds.
    pub kind: Option<String>,
    /// 1–50 (default 10).
    pub limit: Option<i64>,
    /// Search radius, ≤ 50 km (default 50).
    pub radius_km: Option<f64>,
}

/// Nearest features to a point, by geodesic distance to the geometry.
#[utoipa::path(get, path = "/v2/nearest", tag = "features", params(NearestParams),
    responses((status = 200, body = Envelope<Vec<NearestHit>>), (status = 400, body = ErrorBody),
              (status = 404, body = ErrorBody)))]
async fn nearest(
    State(st): State<AppState>,
    Query(p): Query<NearestParams>,
    headers: HeaderMap,
    uri: Uri,
) -> ApiResult {
    let cat = st.catalog();
    let (Some(lat), Some(lon)) = (p.lat, p.lon) else {
        return Err(bad("lat and lon are required"));
    };
    let datasets = list(p.dataset.as_deref());
    let q = NearestQuery {
        lat,
        lon,
        datasets: datasets.clone(),
        kinds: list(p.kind.as_deref()),
        limit: p.limit,
        radius_km: p.radius_km,
    };
    let hits = features::nearest(&st.pool, &cat, &q)
        .await
        .map_err(|e| err(&st, e))?;
    let meta = cat.meta_for(datasets.iter().map(String::as_str), Vec::new());
    respond(&headers, &uri, hits, meta)
}

// ───────────────────────────── tiles ───────────────────────────────────────

struct Index {
    at: Instant,
    sets: Arc<Vec<Tileset>>,
}

static INDEX: LazyLock<RwLock<Option<Index>>> = LazyLock::new(|| RwLock::new(None));
static OPEN: LazyLock<Mutex<HashMap<PathBuf, Arc<PmTiles>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

async fn tileset_index(st: &AppState) -> Result<Arc<Vec<Tileset>>, ApiError> {
    if let Ok(g) = INDEX.read()
        && let Some(i) = g.as_ref()
        && i.at.elapsed() < Duration::from_secs(30)
    {
        return Ok(Arc::clone(&i.sets));
    }
    let sets = Arc::new(features::tilesets(&st.pool).await.map_err(|e| err(st, e))?);
    if let Ok(mut g) = INDEX.write() {
        *g = Some(Index {
            at: Instant::now(),
            sets: Arc::clone(&sets),
        });
    }
    Ok(sets)
}

fn store_root(st: &AppState) -> PathBuf {
    st.store_dir
        .clone()
        .unwrap_or_else(|| PathBuf::from("data/v2"))
}

fn open_archive(path: &FsPath) -> Result<Arc<PmTiles>, ApiError> {
    let mut g = OPEN
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(p) = g.get(path) {
        return Ok(Arc::clone(p));
    }
    let pm = Arc::new(PmTiles::open(path).map_err(|e| {
        tracing::error!(error = %e, path = %path.display(), "opening tileset");
        ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "internal error")
    })?);
    g.insert(path.to_owned(), Arc::clone(&pm));
    Ok(pm)
}

async fn find_set(st: &AppState, id: &str) -> Result<Tileset, ApiError> {
    tileset_index(st)
        .await?
        .iter()
        .find(|t| t.id == id)
        .cloned()
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, format!("not_found: tileset {id}")))
}

fn base_url(headers: &HeaderMap) -> String {
    let host = headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("127.0.0.1:8601");
    let proto = headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("http");
    format!("{proto}://{host}")
}

#[derive(Debug, Serialize, ToSchema)]
pub struct TilesetEntry {
    #[serde(flatten)]
    pub tileset: Tileset,
    /// TileJSON 3.0.0 document.
    pub tilejson: String,
    /// `{z}/{x}/{y}` template.
    pub tiles: String,
    /// The PMTiles file (range requests).
    pub pmtiles: String,
}

/// Built vector tilesets (newest build of each).
#[utoipa::path(get, path = "/v2/tiles", tag = "features",
    responses((status = 200, body = Envelope<Vec<TilesetEntry>>)))]
async fn tiles_index(State(st): State<AppState>, headers: HeaderMap, uri: Uri) -> ApiResult {
    let cat = st.catalog();
    let sets = tileset_index(&st).await?;
    let out: Vec<TilesetEntry> = sets
        .iter()
        .map(|t| TilesetEntry {
            tilejson: format!("/v2/tiles/{}.json", t.id),
            tiles: format!("/v2/tiles/{}/{{z}}/{{x}}/{{y}}.mvt", t.id),
            pmtiles: format!("/dl/tiles/{}/{}.pmtiles", t.id, t.sha16),
            tileset: t.clone(),
        })
        .collect();
    let meta = cat.meta_for(
        sets.iter()
            .flat_map(|t| t.dataset_ids.iter().map(String::as_str)),
        Vec::new(),
    );
    respond(&headers, &uri, out, meta)
}

/// TileJSON 3.0.0 of a tileset (`/v2/tiles/{tileset}.json`).
#[utoipa::path(get, path = "/v2/tiles/{tileset}.json", tag = "features",
    params(("tileset" = String, Path, description = "Tileset id, e.g. admin")),
    responses((status = 200, description = "TileJSON 3.0.0"), (status = 404, body = ErrorBody)))]
async fn tilejson(
    State(st): State<AppState>,
    Path(file): Path<String>,
    headers: HeaderMap,
) -> ApiResult {
    let Some(id) = file.strip_suffix(".json") else {
        return Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "not_found: /v2/tiles/{tileset}.json or /v2/tiles/{tileset}/{z}/{x}/{y}.mvt",
        ));
    };
    let t = find_set(&st, id).await?;
    let etag = format!("\"{}-tilejson\"", t.sha16);
    if let Some(r) = crate::not_modified(&headers, &etag, CACHE_TILE) {
        return Ok(r);
    }
    let pm = open_archive(&store_root(&st).join(&t.path))?;
    let meta = pm.metadata().unwrap_or_default();
    let h = &pm.header;
    let base = base_url(&headers);
    let doc = serde_json::json!({
        "tilejson": "3.0.0",
        "name": t.id,
        "version": t.sha16,
        "scheme": "xyz",
        "tiles": [format!("{base}/v2/tiles/{}/{{z}}/{{x}}/{{y}}.mvt", t.id)],
        "minzoom": h.min_zoom,
        "maxzoom": h.max_zoom,
        "bounds": h.bounds,
        "center": [h.center[0], h.center[1], h.center_zoom],
        "attribution": t.attribution,
        "vector_layers": meta.get("vector_layers").cloned().unwrap_or_default(),
        "pmtiles": format!("{base}/dl/tiles/{}/{}.pmtiles", t.id, t.sha16),
        "lanka": { "layers": t.layers, "datasets": t.dataset_ids, "built_at": t.built_at },
    });
    crate::with_cache(axum::Json(doc).into_response(), &etag, CACHE_TILE)
}

/// One Mapbox Vector Tile (`.mvt`), gzip-encoded as stored; 204 when empty.
#[utoipa::path(get, path = "/v2/tiles/{tileset}/{z}/{x}/{y}.mvt", tag = "features",
    params(("tileset" = String, Path, description = "Tileset id"), ("z" = u8, Path, description = "Zoom"),
           ("x" = u64, Path, description = "Column"), ("y" = String, Path, description = "Row, with .mvt")),
    responses((status = 200, description = "application/vnd.mapbox-vector-tile"), (status = 204, description = "no tile"),
              (status = 404, body = ErrorBody)))]
async fn tile(
    State(st): State<AppState>,
    Path((tileset, z, x, y)): Path<(String, String, String, String)>,
    headers: HeaderMap,
) -> ApiResult {
    let y = y
        .strip_suffix(".mvt")
        .or_else(|| y.strip_suffix(".pbf"))
        .unwrap_or(&y);
    let (Ok(z), Ok(x), Ok(y)) = (z.parse::<u8>(), x.parse::<u64>(), y.parse::<u64>()) else {
        return Err(bad("tile coordinates must be integers: /{z}/{x}/{y}.mvt"));
    };
    let t = find_set(&st, &tileset).await?;
    let etag = format!("\"{}-{z}-{x}-{y}\"", t.sha16);
    if let Some(r) = crate::not_modified(&headers, &etag, CACHE_TILE) {
        return Ok(r);
    }
    let pm = open_archive(&store_root(&st).join(&t.path))?;
    let enc = pm.header.tile_compression.content_encoding();
    let data = tokio::task::spawn_blocking(move || pm.tile(z, x, y))
        .await
        .map_err(|_| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "internal error"))?
        .map_err(|e| {
            tracing::error!(error = %e, "reading tile");
            ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "internal error")
        })?;
    let mut res = match data {
        None => StatusCode::NO_CONTENT.into_response(),
        Some(b) => {
            let mut r = b.into_response();
            r.headers_mut().insert(
                header::CONTENT_TYPE,
                HeaderValue::from_static("application/vnd.mapbox-vector-tile"),
            );
            if let Some(e) = enc {
                r.headers_mut()
                    .insert(header::CONTENT_ENCODING, HeaderValue::from_static(e));
            }
            r
        }
    };
    res.headers_mut().insert(
        "x-tileset-version",
        HeaderValue::from_str(&t.sha16).unwrap_or(HeaderValue::from_static("0")),
    );
    crate::with_cache(res, &etag, CACHE_TILE)
}

/// `bytes=a-b` | `bytes=a-` | `bytes=-n` → `[start, end]` inclusive.
fn parse_range(h: &str, len: u64) -> Option<(u64, u64)> {
    let spec = h.strip_prefix("bytes=")?;
    if spec.contains(',') || len == 0 {
        return None;
    }
    let (a, b) = spec.split_once('-')?;
    let (start, end) = match (a.trim(), b.trim()) {
        ("", n) => {
            let n: u64 = n.parse().ok()?;
            (len.saturating_sub(n), len - 1)
        }
        (s, "") => (s.parse().ok()?, len - 1),
        (s, e) => (s.parse().ok()?, e.parse::<u64>().ok()?.min(len - 1)),
    };
    (start <= end && start < len).then_some((start, end))
}

/// The PMTiles file, immutable by name, with single-range requests.
async fn pmtiles_file(
    State(st): State<AppState>,
    Path((tileset, file)): Path<(String, String)>,
    headers: HeaderMap,
) -> ApiResult {
    let not_found = || ApiError::new(StatusCode::NOT_FOUND, "not_found: no such tileset file");
    let sha16 = file.strip_suffix(".pmtiles").ok_or_else(not_found)?;
    if !(sha16.len() == 16 && sha16.bytes().all(|b| b.is_ascii_hexdigit()))
        || !tileset
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err(not_found());
    }
    let path = store_root(&st).join("tiles").join(&tileset).join(&file);
    let meta = tokio::fs::metadata(&path).await.map_err(|_| not_found())?;
    let len = meta.len();
    let range = headers
        .get(header::RANGE)
        .and_then(|v| v.to_str().ok())
        .map(|r| parse_range(r, len));
    let etag = format!("\"{sha16}\"");
    let (status, start, end) = match range {
        None => (StatusCode::OK, 0, len.saturating_sub(1)),
        Some(Some((s, e))) => (StatusCode::PARTIAL_CONTENT, s, e),
        Some(None) => {
            let mut r = StatusCode::RANGE_NOT_SATISFIABLE.into_response();
            if let Ok(v) = HeaderValue::from_str(&format!("bytes */{len}")) {
                r.headers_mut().insert(header::CONTENT_RANGE, v);
            }
            return Ok(r);
        }
    };
    let n = if len == 0 { 0 } else { end - start + 1 };
    let body = tokio::task::spawn_blocking(move || -> std::io::Result<Vec<u8>> {
        use std::os::unix::fs::FileExt;
        let f = std::fs::File::open(&path)?;
        let mut b = vec![0u8; usize::try_from(n).unwrap_or(0)];
        f.read_exact_at(&mut b, start)?;
        Ok(b)
    })
    .await
    .map_err(|_| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "internal error"))?
    .map_err(|_| not_found())?;
    let mut res = (status, body).into_response();
    let h = res.headers_mut();
    h.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/vnd.pmtiles"),
    );
    h.insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
    // Already compressed internally: keep the compression layer off ranges.
    h.insert(
        header::CONTENT_ENCODING,
        HeaderValue::from_static("identity"),
    );
    if status == StatusCode::PARTIAL_CONTENT
        && let Ok(v) = HeaderValue::from_str(&format!("bytes {start}-{end}/{len}"))
    {
        h.insert(header::CONTENT_RANGE, v);
    }
    if let Ok(v) = HeaderValue::from_str(&etag) {
        h.insert(header::ETAG, v);
    }
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static(CACHE_FILE));
    Ok(res)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use http_body_util::BodyExt;
    use lanka_core::Catalog;
    use tower::ServiceExt;

    /// The full router over a never-connected pool: these paths must answer
    /// without a database.
    async fn get(uri: &str) -> (StatusCode, serde_json::Value) {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .acquire_timeout(Duration::from_millis(300))
            .connect_lazy("postgres://nobody@127.0.0.1:1/none")
            .expect("lazy pool");
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../registry");
        let profile = lanka_core::Profile::discover(&root)
            .and_then(lanka_core::Profile::load)
            .expect("profile");
        let cat = Catalog {
            profile: Some(Arc::new(profile)),
            ..Catalog::default()
        };
        let state = AppState {
            pool,
            catalog: Arc::new(RwLock::new(Arc::new(cat))),
            started: Instant::now(),
            store_dir: None,
            app: None,
            grids: Arc::new(RwLock::new(Arc::new(crate::grids::GridSet::default()))),
            analysis: None,
        };
        let res = crate::router(state)
            .oneshot(
                axum::http::Request::get(uri)
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        let status = res.status();
        let bytes = res.into_body().collect().await.expect("body").to_bytes();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
        )
    }

    #[tokio::test]
    async fn validation_happens_before_any_query() {
        for (uri, code) in [
            (
                "/v2/features/osm-pois-amenities?bbox=1,2",
                StatusCode::BAD_REQUEST,
            ),
            ("/v2/features/osm-pois-amenities", StatusCode::NOT_FOUND),
            (
                "/v2/features/osm-pois-amenities/node/1",
                StatusCode::NOT_FOUND,
            ),
            ("/v2/nearest?lon=80", StatusCode::BAD_REQUEST),
            ("/v2/nearest?lat=7&lon=80", StatusCode::BAD_REQUEST),
            ("/v2/tiles/admin/z/1/2.mvt", StatusCode::BAD_REQUEST),
            ("/v2/tiles/admin", StatusCode::NOT_FOUND),
            ("/dl/tiles/admin/not-a-hash.pmtiles", StatusCode::NOT_FOUND),
            (
                "/dl/tiles/..%2Fetc/0123456789abcdef.pmtiles",
                StatusCode::NOT_FOUND,
            ),
        ] {
            let (status, v) = get(uri).await;
            assert_eq!(status, code, "{uri}: {v}");
            assert_eq!(v["success"], false, "{uri}");
        }
    }

    #[tokio::test]
    async fn openapi_lists_the_feature_routes() {
        let (status, v) = get("/v2/openapi.json").await;
        assert_eq!(status, StatusCode::OK);
        for p in [
            "/v2/features/{dataset}",
            "/v2/features/{dataset}/{key}",
            "/v2/nearest",
            "/v2/tiles",
            "/v2/tiles/{tileset}.json",
            "/v2/tiles/{tileset}/{z}/{x}/{y}.mvt",
        ] {
            assert!(v["paths"].get(p).is_some(), "missing {p}");
        }
    }

    #[test]
    fn ranges() {
        assert_eq!(parse_range("bytes=0-126", 1000), Some((0, 126)));
        assert_eq!(parse_range("bytes=900-", 1000), Some((900, 999)));
        assert_eq!(parse_range("bytes=-100", 1000), Some((900, 999)));
        assert_eq!(parse_range("bytes=990-5000", 1000), Some((990, 999)));
        assert_eq!(parse_range("bytes=1000-", 1000), None);
        assert_eq!(parse_range("bytes=5-1", 1000), None);
        assert_eq!(parse_range("bytes=0-1,5-6", 1000), None);
        assert_eq!(parse_range("items=0-1", 1000), None);
    }
}
