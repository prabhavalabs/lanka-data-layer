//! Lanka Data Layer REST v2 (axum). A thin transport over `lanka_core::query`.
//!
//! - Envelope `{success, message, payload, meta}`; `meta` cites data_version,
//!   exact dataset versions, sources and licences for everything touched.
//! - Every GET carries `ETag: "<data_version>-<route hash>"` and
//!   `Cache-Control: public, max-age=300, stale-while-revalidate=86400`;
//!   `If-None-Match` is answered with 304 (cheap revalidation after releases).
//! - Errors: 400 validation, 404 not_found / not_in_coverage, 500 masked.
//! - The catalog snapshot is swapped atomically on `NOTIFY lanka_release`.

use std::hash::{Hash, Hasher};
use std::net::SocketAddr;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use axum::Router;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use lanka_core::envelope::Link;
use lanka_core::query::{self, CatalogQuery, MapQuery, SeriesQuery};
use lanka_core::{Catalog, CoreError, Envelope, Meta, Pcode};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use sqlx::postgres::PgListener;
use tower_http::compression::CompressionLayer;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use utoipa::{IntoParams, OpenApi};

pub mod openapi;

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    catalog: Arc<RwLock<Arc<Catalog>>>,
}

impl AppState {
    pub async fn new(pool: PgPool) -> anyhow::Result<Self> {
        let cat = Catalog::load(&pool).await?;
        Ok(Self {
            pool,
            catalog: Arc::new(RwLock::new(Arc::new(cat))),
        })
    }

    /// The current snapshot (an `Arc` clone; never blocks on reloads).
    #[must_use]
    pub fn catalog(&self) -> Arc<Catalog> {
        match self.catalog.read() {
            Ok(g) => Arc::clone(&g),
            Err(p) => Arc::clone(&p.into_inner()),
        }
    }

    pub async fn reload(&self) -> anyhow::Result<()> {
        let fresh = Arc::new(Catalog::load(&self.pool).await?);
        let dv = fresh.data_version.clone();
        match self.catalog.write() {
            Ok(mut g) => *g = fresh,
            Err(p) => *p.into_inner() = fresh,
        }
        tracing::info!(data_version = ?dv, "catalog reloaded");
        Ok(())
    }

    /// Reloads on `NOTIFY lanka_release`, with a 5-minute safety refresh.
    pub fn spawn_reloader(&self) {
        let state = self.clone();
        tokio::spawn(async move {
            loop {
                match PgListener::connect_with(&state.pool).await {
                    Ok(mut l) => {
                        if let Err(e) = l.listen("lanka_release").await {
                            tracing::warn!(error = %e, "LISTEN failed");
                        } else {
                            loop {
                                match tokio::time::timeout(Duration::from_secs(300), l.recv()).await
                                {
                                    Ok(Ok(n)) => tracing::info!(
                                        payload = n.payload(),
                                        "release notification"
                                    ),
                                    Ok(Err(e)) => {
                                        tracing::warn!(error = %e, "listener error; reconnecting");
                                        break;
                                    }
                                    Err(_) => {} // periodic refresh
                                }
                                if let Err(e) = state.reload().await {
                                    tracing::error!(error = %e, "catalog reload failed");
                                }
                            }
                        }
                    }
                    Err(e) => tracing::warn!(error = %e, "listener connect failed"),
                }
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        });
    }
}

// ───────────────────────────── errors ──────────────────────────────────────

pub struct ApiError {
    status: StatusCode,
    message: String,
    data_version: Option<String>,
}

impl ApiError {
    fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
            data_version: None,
        }
    }
    fn with_version(mut self, dv: Option<String>) -> Self {
        self.data_version = dv;
        self
    }
}

impl From<CoreError> for ApiError {
    fn from(e: CoreError) -> Self {
        match e {
            CoreError::NotFound(m) => Self::new(StatusCode::NOT_FOUND, format!("not_found: {m}")),
            CoreError::NotInCoverage => Self::new(StatusCode::NOT_FOUND, "not_in_coverage"),
            CoreError::BadRequest(m) => Self::new(StatusCode::BAD_REQUEST, m),
            CoreError::Db(err) => {
                tracing::error!(error = %err, "database error");
                Self::new(StatusCode::INTERNAL_SERVER_ERROR, "internal error")
            }
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = Envelope::error(self.message, Meta::bare(self.data_version));
        let mut res = (self.status, axum::Json(body)).into_response();
        res.headers_mut()
            .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
        res
    }
}

type ApiResult = Result<Response, ApiError>;

/// Error response shape (documentation only): `success: false`, `payload: null`.
#[derive(Serialize, utoipa::ToSchema)]
pub struct ErrorBody {
    pub success: bool,
    pub message: String,
    #[schema(value_type = Option<Object>)]
    pub payload: Option<serde_json::Value>,
    pub meta: Meta,
}

/// Serialises an OK envelope with ETag/Cache-Control, honouring If-None-Match.
fn respond<T: Serialize>(
    headers: &HeaderMap,
    uri: &axum::http::Uri,
    payload: T,
    meta: Meta,
) -> ApiResult {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    uri.path().hash(&mut h);
    uri.query().unwrap_or_default().hash(&mut h);
    meta.dataset_versions
        .iter()
        .for_each(|d| d.dsv.hash(&mut h));
    let etag = format!(
        "\"{}-{:016x}\"",
        meta.data_version.as_deref().unwrap_or("0"),
        h.finish()
    );
    let cache = HeaderValue::from_static("public, max-age=300, stale-while-revalidate=86400");
    let etag_val = HeaderValue::from_str(&etag)
        .map_err(|_| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "etag"))?;
    if headers
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.split(',').any(|t| t.trim() == etag || t.trim() == "*"))
    {
        let mut res = StatusCode::NOT_MODIFIED.into_response();
        res.headers_mut().insert(header::ETAG, etag_val);
        res.headers_mut().insert(header::CACHE_CONTROL, cache);
        return Ok(res);
    }
    let mut res = axum::Json(Envelope::ok(payload, meta)).into_response();
    res.headers_mut().insert(header::ETAG, etag_val);
    res.headers_mut().insert(header::CACHE_CONTROL, cache);
    Ok(res)
}

fn parse_pcode(s: &str) -> Result<Pcode, ApiError> {
    Pcode::parse(s).map_err(|e| ApiError::new(StatusCode::BAD_REQUEST, e.to_string()))
}

// ───────────────────────────── handlers ────────────────────────────────────

/// Service and database health.
#[utoipa::path(get, path = "/v2/health", tag = "meta",
    responses((status = 200, body = Envelope<query::Health>), (status = 500, body = ErrorBody)))]
async fn health(State(st): State<AppState>) -> ApiResult {
    let cat = st.catalog();
    let h = query::health(&st.pool, &cat)
        .await
        .map_err(ApiError::from)?;
    let mut res = axum::Json(Envelope::ok(h, Meta::bare(cat.data_version.clone()))).into_response();
    res.headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(res)
}

#[derive(Debug, Deserialize, IntoParams)]
pub struct CatalogParams {
    /// Free-text filter over titles, descriptions and indicator labels.
    pub q: Option<String>,
    /// EU DCAT theme code (ECON, SOCI, REGI …).
    pub theme: Option<String>,
}

/// Search the dataset catalog.
#[utoipa::path(get, path = "/v2/catalog", tag = "catalog", params(CatalogParams),
    responses((status = 200, body = Envelope<Vec<query::CatalogEntry>>)))]
async fn catalog(
    State(st): State<AppState>,
    Query(p): Query<CatalogParams>,
    headers: HeaderMap,
    uri: Uri,
) -> ApiResult {
    let cat = st.catalog();
    let list = query::catalog_list(
        &cat,
        &CatalogQuery {
            q: p.q,
            theme: p.theme,
        },
    );
    let ids: Vec<&str> = list.iter().map(|e| e.id.as_str()).collect();
    let meta = cat.meta_for(ids, Vec::new());
    respond(&headers, &uri, list, meta)
}

/// Dataset card: description, limits, licences, sources, indicators, current version, freshness.
#[utoipa::path(get, path = "/v2/datasets/{id}", tag = "catalog",
    params(("id" = String, Path, description = "Dataset id, e.g. wdi-lka")),
    responses((status = 200, body = Envelope<query::DatasetCard>), (status = 404, body = ErrorBody)))]
async fn dataset(
    State(st): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    uri: Uri,
) -> ApiResult {
    let cat = st.catalog();
    let card = query::dataset_card(&st.pool, &cat, &id)
        .await
        .map_err(|e| ApiError::from(e).with_version(cat.data_version.clone()))?;
    let next = card
        .indicators
        .first()
        .map(|i| {
            vec![Link::new(
                "series",
                format!("/v2/indicators/{}/series?geo=LK", i.id),
            )]
        })
        .unwrap_or_default();
    let meta = cat.meta_for([id.as_str()], next);
    respond(&headers, &uri, card, meta)
}

/// Area profile: hierarchy, children and the latest value of every indicator for a p-code.
#[utoipa::path(get, path = "/v2/areas/{pcode}", tag = "areas",
    params(("pcode" = String, Path, description = "COD-AB p-code: LK, LK1, LK11, LK1103, LK1103005")),
    responses((status = 200, body = Envelope<query::AreaProfile>), (status = 400, body = ErrorBody),
              (status = 404, body = ErrorBody)))]
async fn area(
    State(st): State<AppState>,
    Path(pcode): Path<String>,
    headers: HeaderMap,
    uri: Uri,
) -> ApiResult {
    let cat = st.catalog();
    let pcode = parse_pcode(&pcode)?;
    let (profile, mut touched) = query::area_profile(&st.pool, &cat, &pcode)
        .await
        .map_err(|e| ApiError::from(e).with_version(cat.data_version.clone()))?;
    touched.push(lanka_core::BOUNDARY_DATASET.to_owned());
    let mut next: Vec<Link> = profile
        .indicators
        .iter()
        .take(3)
        .map(|i| {
            Link::new(
                "series",
                format!(
                    "/v2/indicators/{}/series?geo={}",
                    i.indicator_id, profile.pcode
                ),
            )
        })
        .collect();
    if let Some(p) = profile.ancestors.last() {
        next.push(Link::new("parent", format!("/v2/areas/{}", p.pcode)));
    }
    let meta = cat.meta_for(touched.iter().map(String::as_str), next);
    respond(&headers, &uri, profile, meta)
}

#[derive(Debug, Deserialize, IntoParams)]
pub struct SeriesParams {
    /// Comma-separated p-codes (≤ 25), e.g. `LK,LK11`.
    pub geo: Option<String>,
    /// First period (inclusive): YYYY, YYYY-Qn, YYYY-MM or YYYY-MM-DD.
    pub from: Option<String>,
    /// Last period (inclusive).
    pub to: Option<String>,
    /// Canonical dims key, e.g. `sex=female`.
    pub dims: Option<String>,
}

/// Time series of one indicator for up to 25 areas.
#[utoipa::path(get, path = "/v2/indicators/{id}/series", tag = "indicators",
    params(("id" = String, Path, description = "Indicator id, e.g. wdi.sp_pop_totl"), SeriesParams),
    responses((status = 200, body = Envelope<query::Series>), (status = 400, body = ErrorBody),
              (status = 404, body = ErrorBody)))]
async fn series(
    State(st): State<AppState>,
    Path(id): Path<String>,
    Query(p): Query<SeriesParams>,
    headers: HeaderMap,
    uri: Uri,
) -> ApiResult {
    let cat = st.catalog();
    let geos: Vec<String> = p
        .geo
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect();
    let q = SeriesQuery {
        geos,
        from: p.from,
        to: p.to,
        dims: p.dims,
    };
    let s = query::series(&st.pool, &cat, &id, &q)
        .await
        .map_err(|e| ApiError::from(e).with_version(cat.data_version.clone()))?;
    let next = s
        .series
        .iter()
        .take(1)
        .filter_map(|l| l.points.last())
        .map(|pt| {
            Link::new(
                "provenance",
                format!("/v2/observations/{}/provenance", pt.obs_id),
            )
        })
        .collect();
    let meta = cat.meta_for([s.indicator.dataset_id.as_str()], next);
    respond(&headers, &uri, s, meta)
}

#[derive(Debug, Deserialize, IntoParams)]
pub struct MapParams {
    /// Admin level 0–4 (default 2, districts).
    pub level: Option<String>,
    /// Period; defaults to the latest held at that level.
    pub period: Option<String>,
    /// Restrict to units under this p-code.
    pub parent: Option<String>,
    /// Canonical dims key; defaults to the first available.
    pub dims: Option<String>,
}

/// Choropleth arrays (`pcodes[i]` ↔ `values[i]`) plus quantile breaks; no geometry.
#[utoipa::path(get, path = "/v2/indicators/{id}/map", tag = "indicators",
    params(("id" = String, Path, description = "Indicator id"), MapParams),
    responses((status = 200, body = Envelope<query::MapData>), (status = 400, body = ErrorBody),
              (status = 404, body = ErrorBody)))]
async fn indicator_map(
    State(st): State<AppState>,
    Path(id): Path<String>,
    Query(p): Query<MapParams>,
    headers: HeaderMap,
    uri: Uri,
) -> ApiResult {
    let cat = st.catalog();
    let level: i16 = match p.level.as_deref() {
        None | Some("") => 2,
        Some(l) => l
            .parse()
            .map_err(|_| ApiError::new(StatusCode::BAD_REQUEST, "level must be an integer 0..4"))?,
    };
    let q = MapQuery {
        level,
        period: p.period,
        parent: p.parent,
        dims: p.dims,
    };
    let m = query::map(&st.pool, &cat, &id, &q)
        .await
        .map_err(|e| ApiError::from(e).with_version(cat.data_version.clone()))?;
    let ds = cat
        .indicators
        .get(&id)
        .map(|i| i.dataset_id.clone())
        .unwrap_or_default();
    let meta = cat.meta_for([ds.as_str(), lanka_core::BOUNDARY_DATASET], Vec::new());
    respond(&headers, &uri, m, meta)
}

/// Row-level provenance of one observation: artifact sha256, locator, verbatim value, review.
#[utoipa::path(get, path = "/v2/observations/{id}/provenance", tag = "provenance",
    params(("id" = i64, Path, description = "Observation id")),
    responses((status = 200, body = Envelope<query::Provenance>), (status = 404, body = ErrorBody)))]
async fn provenance(
    State(st): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    uri: Uri,
) -> ApiResult {
    let cat = st.catalog();
    let id: i64 = id
        .parse()
        .map_err(|_| ApiError::new(StatusCode::BAD_REQUEST, "observation id must be an integer"))?;
    let p = query::provenance(&st.pool, id)
        .await
        .map_err(|e| ApiError::from(e).with_version(cat.data_version.clone()))?;
    let meta = cat.meta_for([p.dataset_id.as_str()], Vec::new());
    respond(&headers, &uri, p, meta)
}

#[derive(Debug, Deserialize, IntoParams)]
pub struct ReverseParams {
    pub lat: Option<String>,
    pub lon: Option<String>,
}

/// Reverse geocode a WGS84 point to the deepest admin unit and grid cell.
#[utoipa::path(get, path = "/v2/reverse", tag = "areas", params(ReverseParams),
    responses((status = 200, body = Envelope<query::ReverseResult>), (status = 400, body = ErrorBody),
              (status = 404, body = ErrorBody)))]
async fn reverse(
    State(st): State<AppState>,
    Query(p): Query<ReverseParams>,
    headers: HeaderMap,
    uri: Uri,
) -> ApiResult {
    let cat = st.catalog();
    let f = |v: Option<&String>, n: &str| -> Result<f64, ApiError> {
        v.and_then(|s| s.trim().parse::<f64>().ok())
            .filter(|x| x.is_finite())
            .ok_or_else(|| {
                ApiError::new(
                    StatusCode::BAD_REQUEST,
                    format!("{n} is required (decimal degrees)"),
                )
            })
    };
    let (lat, lon) = (f(p.lat.as_ref(), "lat")?, f(p.lon.as_ref(), "lon")?);
    let r = query::reverse(&st.pool, lat, lon)
        .await
        .map_err(|e| ApiError::from(e).with_version(cat.data_version.clone()))?;
    let next = vec![Link::new("area", format!("/v2/areas/{}", r.unit.pcode))];
    let meta = cat.meta_for([lanka_core::BOUNDARY_DATASET], next);
    respond(&headers, &uri, r, meta)
}

async fn openapi_json() -> Response {
    let mut res = axum::Json(openapi::ApiDoc::openapi()).into_response();
    res.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=300"),
    );
    res
}

async fn fallback() -> ApiError {
    ApiError::new(
        StatusCode::NOT_FOUND,
        "not_found: no such route (see /v2/openapi.json)",
    )
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/v2/health", get(health))
        .route("/v2/catalog", get(catalog))
        .route("/v2/datasets/{id}", get(dataset))
        .route("/v2/areas/{pcode}", get(area))
        .route("/v2/indicators/{id}/series", get(series))
        .route("/v2/indicators/{id}/map", get(indicator_map))
        .route("/v2/observations/{id}/provenance", get(provenance))
        .route("/v2/reverse", get(reverse))
        .route("/v2/openapi.json", get(openapi_json))
        .fallback(fallback)
        .with_state(state)
        .layer(CompressionLayer::new())
        .layer(
            CorsLayer::new()
                .allow_origin(tower_http::cors::Any)
                .allow_methods([axum::http::Method::GET]),
        )
        .layer(TraceLayer::new_for_http())
}

/// Binds and serves until SIGINT/SIGTERM.
pub async fn serve(pool: PgPool, bind: SocketAddr) -> anyhow::Result<()> {
    let state = AppState::new(pool).await?;
    state.spawn_reloader();
    let app = router(state);
    let listener = tokio::net::TcpListener::bind(bind).await?;
    tracing::info!(%bind, "lanka-server listening");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown())
        .await?;
    Ok(())
}

async fn shutdown() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let term = async {
        if let Ok(mut s) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            s.recv().await;
        }
    };
    #[cfg(not(unix))]
    let term = std::future::pending::<()>();
    tokio::select! { () = ctrl_c => {}, () = term => {} }
    tracing::info!("shutting down");
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    /// A router over a lazy (never-connected) pool: exercises every path
    /// that must not touch the database.
    fn app() -> Router {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgres://nobody@127.0.0.1:1/none")
            .expect("lazy pool");
        let state = AppState {
            pool,
            catalog: Arc::new(RwLock::new(Arc::new(Catalog::default()))),
        };
        router(state)
    }

    async fn get(uri: &str) -> (StatusCode, serde_json::Value) {
        let res = app()
            .oneshot(
                axum::http::Request::get(uri)
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        let status = res.status();
        let bytes = res.into_body().collect().await.expect("body").to_bytes();
        (status, serde_json::from_slice(&bytes).expect("json"))
    }

    #[tokio::test]
    async fn unknown_route_is_enveloped_404() {
        let (status, v) = get("/v2/nope").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(v["success"], false);
        assert!(v["payload"].is_null());
        assert_eq!(v["meta"]["contract_version"], lanka_core::CONTRACT_VERSION);
    }

    #[tokio::test]
    async fn invalid_pcode_is_400_before_any_query() {
        let (status, v) = get("/v2/areas/XX1").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(
            v["message"]
                .as_str()
                .is_some_and(|m| m.contains("invalid p-code"))
        );
    }

    #[tokio::test]
    async fn catalog_works_from_snapshot() {
        let (status, v) = get("/v2/catalog?q=anything").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(v["payload"], serde_json::json!([]));
    }

    #[tokio::test]
    async fn openapi_lists_all_routes() {
        let (status, v) = get("/v2/openapi.json").await;
        assert_eq!(status, StatusCode::OK);
        for p in [
            "/v2/health",
            "/v2/catalog",
            "/v2/datasets/{id}",
            "/v2/areas/{pcode}",
            "/v2/indicators/{id}/series",
            "/v2/indicators/{id}/map",
            "/v2/observations/{id}/provenance",
            "/v2/reverse",
        ] {
            assert!(v["paths"].get(p).is_some(), "missing {p}");
        }
    }

    #[tokio::test]
    async fn reverse_validates_coordinates() {
        let (status, _) = get("/v2/reverse?lat=abc&lon=80").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }
}
