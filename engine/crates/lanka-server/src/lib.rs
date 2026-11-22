//! Lanka Data Layer REST v2 (axum). A thin transport over `lanka_core::query`.
//!
//! - Envelope `{success, message, payload, meta}`; `meta` cites data_version,
//!   exact dataset versions, sources and licences for everything touched.
//! - Every GET carries `ETag: "<data_version>-<hash(route, query, dsvs)>"`
//!   and `Cache-Control: public, max-age=300, stale-while-revalidate=86400`
//!   (a day for release- or version-pinned frames); `If-None-Match` is
//!   answered with 304 — for current-release frames before any query.
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
use lanka_core::status;
use lanka_core::{Catalog, CoreError, Envelope, Meta, Pcode};

use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use sqlx::postgres::PgListener;
use tower_http::compression::CompressionLayer;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use utoipa::{IntoParams, OpenApi};

pub mod frame_codec;
pub mod openapi;

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    catalog: Arc<RwLock<Arc<Catalog>>>,
    started: std::time::Instant,
    /// Blob store root (read-only for the server); checked by /v2/health.
    store_dir: Option<std::path::PathBuf>,
}

impl AppState {
    pub async fn new(pool: PgPool) -> anyhow::Result<Self> {
        let cat = Catalog::load(&pool).await?;
        Ok(Self {
            pool,
            catalog: Arc::new(RwLock::new(Arc::new(cat))),
            started: std::time::Instant::now(),
            store_dir: std::env::var_os("LANKA_STORE_DIR")
                .or_else(|| std::env::var_os("LANKA_DATA_DIR"))
                .filter(|v| !v.is_empty())
                .map(std::path::PathBuf::from),
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

/// `"<data_version>-<hash(path, query, dsvs touched)>"`.
fn etag_of(uri: &Uri, meta: &Meta) -> String {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    uri.path().hash(&mut h);
    uri.query().unwrap_or_default().hash(&mut h);
    meta.dataset_versions
        .iter()
        .for_each(|d| d.dsv.hash(&mut h));
    format!(
        "\"{}-{:016x}\"",
        meta.data_version.as_deref().unwrap_or("0"),
        h.finish()
    )
}

const CACHE_FLOATING: &str = "public, max-age=300, stale-while-revalidate=86400";
/// Responses pinned to a release or exact versions: their data never
/// changes (boundary geometry is release-pinned with WP-10, hence not
/// `immutable` yet).
const CACHE_PINNED: &str = "public, max-age=86400, stale-while-revalidate=604800";

/// 304 when `If-None-Match` carries `etag`.
fn not_modified(headers: &HeaderMap, etag: &str, cache: &'static str) -> Option<Response> {
    let hit = headers
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.split(',').any(|t| t.trim() == etag || t.trim() == "*"));
    if !hit {
        return None;
    }
    let mut res = StatusCode::NOT_MODIFIED.into_response();
    if let Ok(v) = HeaderValue::from_str(etag) {
        res.headers_mut().insert(header::ETAG, v);
    }
    res.headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static(cache));
    Some(res)
}

fn with_cache(mut res: Response, etag: &str, cache: &'static str) -> ApiResult {
    let etag_val = HeaderValue::from_str(etag)
        .map_err(|_| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "etag"))?;
    res.headers_mut().insert(header::ETAG, etag_val);
    res.headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static(cache));
    Ok(res)
}

/// Serialises an OK envelope with ETag/Cache-Control, honouring If-None-Match.
fn respond<T: Serialize>(
    headers: &HeaderMap,
    uri: &axum::http::Uri,
    payload: T,
    meta: Meta,
) -> ApiResult {
    respond_cached(headers, uri, payload, meta, CACHE_FLOATING)
}

fn respond_cached<T: Serialize>(
    headers: &HeaderMap,
    uri: &axum::http::Uri,
    payload: T,
    meta: Meta,
    cache: &'static str,
) -> ApiResult {
    let etag = etag_of(uri, &meta);
    if let Some(r) = not_modified(headers, &etag, cache) {
        return Ok(r);
    }
    with_cache(
        axum::Json(Envelope::ok(payload, meta)).into_response(),
        &etag,
        cache,
    )
}

fn parse_pcode(cat: &Catalog, s: &str) -> Result<Pcode, ApiError> {
    query::parse_pcode(cat, s).map_err(|e| ApiError::from(e).with_version(cat.data_version.clone()))
}

/// Dataset ids touched by a response plus the boundary dataset (from the profile).
fn with_boundaries<'a>(cat: &'a Catalog, ids: impl IntoIterator<Item = &'a str>) -> Vec<&'a str> {
    let mut v: Vec<&str> = ids.into_iter().collect();
    v.extend(cat.boundary_dataset());
    v
}

// ───────────────────────────── handlers ────────────────────────────────────

#[derive(Debug, Deserialize, IntoParams)]
pub struct HealthParams {
    /// `1` adds PostGIS version, migration version, replica lag and counts.
    pub deep: Option<String>,
}

fn no_store(mut res: Response) -> Response {
    res.headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    res
}

/// Liveness + readiness: 200 when Postgres answers and a release is loaded, else 503.
#[utoipa::path(get, path = "/v2/health", tag = "ops", params(HealthParams),
    responses((status = 200, body = Envelope<status::Health>), (status = 503, body = Envelope<status::Health>)))]
async fn health(State(st): State<AppState>, Query(p): Query<HealthParams>) -> Response {
    let cat = st.catalog();
    let latency = status::db_latency_ms(&st.pool).await;
    let deep = if latency.is_some() && matches!(p.deep.as_deref(), Some("1" | "true")) {
        let scheme = cat.profile.as_deref().map_or("admin", |p| p.admin_scheme());
        match status::deep_health(&st.pool, scheme).await {
            Ok((postgis, migration_version, replica_lag_s, admin_units)) => {
                Some(status::DeepHealth {
                    postgis,
                    migration_version,
                    replica_lag_s,
                    admin_units,
                    datasets_published: cat
                        .datasets
                        .values()
                        .filter(|d| d.current.is_some())
                        .count(),
                })
            }
            Err(e) => {
                tracing::warn!(error = %e, "deep health failed");
                None
            }
        }
    } else {
        None
    };
    let ready = latency.is_some() && cat.data_version.is_some();
    let h = status::Health {
        status: if ready { "ok" } else { "unavailable" }.to_owned(),
        data_version: cat.data_version.clone(),
        db: status::DbHealth {
            ok: latency.is_some(),
            latency_ms: latency,
        },
        store: status::StoreHealth {
            ok: st.store_dir.as_ref().is_none_or(|d| d.is_dir()),
        },
        grids_mapped: 0,
        uptime_s: st.started.elapsed().as_secs(),
        engine_version: lanka_core::ENGINE_VERSION.to_owned(),
        git_rev: lanka_core::git_rev(),
        country: cat.profile.as_deref().map(|p| p.spec.id.clone()),
        deep,
    };
    let code = if ready {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    let body = Envelope::ok(h, Meta::bare(cat.data_version.clone()));
    no_store((code, axum::Json(body)).into_response())
}

/// Ops reads are never cached.
fn ops_ok<T: Serialize>(cat: &Catalog, payload: T) -> Response {
    no_store(
        axum::Json(Envelope::ok(payload, Meta::bare(cat.data_version.clone()))).into_response(),
    )
}

/// One-screen platform summary with the computed `overall` state. Always 200: the body says what is wrong.
#[utoipa::path(get, path = "/v2/status", tag = "ops",
    responses((status = 200, body = Envelope<status::StatusSummary>)))]
async fn status_summary(State(st): State<AppState>) -> Response {
    let cat = st.catalog();
    match status::summary(&st.pool).await {
        Ok(s) => ops_ok(&cat, s),
        Err(e) => {
            tracing::error!(error = %e, "status summary failed");
            ops_ok(
                &cat,
                serde_json::json!({
                    "overall": "failing",
                    "reasons": ["database unreachable or status query failed"],
                    "generated_at": sqlx::types::chrono::Utc::now(),
                }),
            )
        }
    }
}

#[derive(Debug, Deserialize, IntoParams)]
pub struct SourcesParams {
    /// fresh | due | overdue | failing | in_review | expected_missing | never_run | paused | retired
    pub state: Option<String>,
    /// `1` = only sources with an implemented connector.
    pub schedulable: Option<String>,
}

/// Per-source freshness, last run and next check.
#[utoipa::path(get, path = "/v2/status/sources", tag = "ops", params(SourcesParams),
    responses((status = 200, body = Envelope<Vec<status::SourceStatus>>), (status = 400, body = ErrorBody)))]
async fn status_sources(State(st): State<AppState>, Query(p): Query<SourcesParams>) -> ApiResult {
    let cat = st.catalog();
    let sched = p.schedulable.as_deref().map(|v| matches!(v, "1" | "true"));
    let rows = status::sources(
        &st.pool,
        p.state.as_deref().filter(|s| !s.is_empty()),
        sched,
    )
    .await
    .map_err(|e| ApiError::from(e).with_version(cat.data_version.clone()))?;
    Ok(ops_ok(&cat, rows))
}

#[derive(Debug, Deserialize, IntoParams)]
pub struct DatasetsStatusParams {
    /// published | approved | in_review | unpublished
    pub state: Option<String>,
}

/// Per-dataset currency: released version, pending (approved / in review) version.
#[utoipa::path(get, path = "/v2/status/datasets", tag = "ops", params(DatasetsStatusParams),
    responses((status = 200, body = Envelope<Vec<status::DatasetStatus>>), (status = 400, body = ErrorBody)))]
async fn status_datasets(
    State(st): State<AppState>,
    Query(p): Query<DatasetsStatusParams>,
) -> ApiResult {
    let cat = st.catalog();
    let rows = status::datasets(&st.pool, p.state.as_deref().filter(|s| !s.is_empty()))
        .await
        .map_err(|e| ApiError::from(e).with_version(cat.data_version.clone()))?;
    Ok(ops_ok(&cat, rows))
}

#[derive(Debug, Deserialize, IntoParams)]
pub struct RunsParams {
    /// RFC 3339 timestamp: runs started at or after it.
    pub since: Option<String>,
    pub source: Option<String>,
    /// running | succeeded | no_change | needs_review | failed
    pub status: Option<String>,
    /// `next_cursor` of the previous page.
    pub cursor: Option<i64>,
    /// 1–200 (default 50).
    pub limit: Option<i64>,
}

/// Run history, newest first, cursor-paged.
#[utoipa::path(get, path = "/v2/status/runs", tag = "ops", params(RunsParams),
    responses((status = 200, body = Envelope<status::RunPage>), (status = 400, body = ErrorBody)))]
async fn status_runs(State(st): State<AppState>, Query(p): Query<RunsParams>) -> ApiResult {
    let cat = st.catalog();
    let since = match p.since.as_deref().filter(|s| !s.is_empty()) {
        None => None,
        Some(s) => Some(
            s.parse::<sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>>()
                .map_err(|_| {
                    ApiError::new(
                        StatusCode::BAD_REQUEST,
                        "since must be an RFC 3339 timestamp",
                    )
                })?,
        ),
    };
    let page = status::runs(
        &st.pool,
        since,
        p.source.as_deref().filter(|s| !s.is_empty()),
        p.status.as_deref().filter(|s| !s.is_empty()),
        p.cursor,
        p.limit.unwrap_or(50),
    )
    .await
    .map_err(|e| ApiError::from(e).with_version(cat.data_version.clone()))?;
    Ok(ops_ok(&cat, page))
}

#[derive(Debug, Deserialize, IntoParams)]
pub struct EventsParams {
    /// Last event id seen (exclusive); 0 or absent = from the beginning.
    pub after: Option<i64>,
    /// Minimum level: debug | info | warn | error.
    pub level: Option<String>,
    pub source: Option<String>,
    /// 1–1000 (default 100).
    pub limit: Option<i64>,
}

/// Append-only event feed with strictly increasing ids (`Accept: application/x-ndjson` for NDJSON).
#[utoipa::path(get, path = "/v2/status/events", tag = "ops", params(EventsParams),
    responses((status = 200, body = Envelope<Vec<status::EventRecord>>), (status = 400, body = ErrorBody)))]
async fn status_events(
    State(st): State<AppState>,
    Query(p): Query<EventsParams>,
    headers: HeaderMap,
) -> ApiResult {
    let cat = st.catalog();
    let rows = status::events(
        &st.pool,
        p.after.unwrap_or(0),
        p.level.as_deref().filter(|s| !s.is_empty()),
        p.source.as_deref().filter(|s| !s.is_empty()),
        p.limit.unwrap_or(100),
    )
    .await
    .map_err(|e| ApiError::from(e).with_version(cat.data_version.clone()))?;
    let ndjson = headers
        .get(header::ACCEPT)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.contains("application/x-ndjson"));
    if ndjson {
        let mut body = String::new();
        for r in &rows {
            body.push_str(
                &serde_json::to_string(r).map_err(|_| {
                    ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "internal error")
                })?,
            );
            body.push('\n');
        }
        let mut res = body.into_response();
        res.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/x-ndjson"),
        );
        return Ok(no_store(res));
    }
    Ok(ops_ok(&cat, rows))
}

#[derive(Debug, Deserialize, IntoParams)]
pub struct IncidentsParams {
    /// `1` = open incidents only.
    pub open: Option<String>,
    pub limit: Option<i64>,
}

/// Persistent conditions, opened and closed automatically.
#[utoipa::path(get, path = "/v2/status/incidents", tag = "ops", params(IncidentsParams),
    responses((status = 200, body = Envelope<Vec<status::Incident>>)))]
async fn status_incidents(
    State(st): State<AppState>,
    Query(p): Query<IncidentsParams>,
) -> ApiResult {
    let cat = st.catalog();
    let rows = status::incidents(
        &st.pool,
        matches!(p.open.as_deref(), Some("1" | "true")),
        p.limit.unwrap_or(100),
    )
    .await
    .map_err(|e| ApiError::from(e).with_version(cat.data_version.clone()))?;
    Ok(ops_ok(&cat, rows))
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
    let pcode = parse_pcode(&cat, &pcode)?;
    let (profile, touched) = query::area_profile(&st.pool, &cat, &pcode)
        .await
        .map_err(|e| ApiError::from(e).with_version(cat.data_version.clone()))?;
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
    let meta = cat.meta_for(
        with_boundaries(&cat, touched.iter().map(String::as_str)),
        next,
    );
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
    let meta = cat.meta_for(with_boundaries(&cat, [ds.as_str()]), Vec::new());
    respond(&headers, &uri, m, meta)
}

#[derive(Debug, Default, Deserialize, IntoParams)]
pub struct FrameParams {
    /// Comma-separated indicator ids (≤ 8), e.g. `census24.pop_total,census24.sex_ratio`.
    pub indicators: Option<String>,
    /// Admin level (0 = national … 4).
    pub level: Option<String>,
    /// Geography scheme (default: the admin scheme; the only one served today).
    pub scheme: Option<String>,
    /// Restrict to units under this p-code.
    pub parent: Option<String>,
    /// Comma-separated period codes, or `latest` (default: the latest held at the level, per indicator).
    pub periods: Option<String>,
    /// First period (inclusive), instead of `periods`.
    pub from: Option<String>,
    /// Last period (inclusive).
    pub to: Option<String>,
    /// `[<indicator>:]<dims>` entries separated by `|`, e.g. `census24.pop_total:sex=total|sex=female`;
    /// an entry selects the combinations containing its pairs (default: every combination).
    pub dims: Option<String>,
    /// Comma-separated exact dataset versions (`dsv_…`) to read instead of the release's.
    pub dsv: Option<String>,
    /// `json` (default) | `arrow` (Arrow IPC stream) | `csv`.
    pub format: Option<String>,
}

fn split_list(s: Option<&str>) -> Vec<String> {
    s.unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}

async fn frame_response(
    st: &AppState,
    release: Option<String>,
    p: FrameParams,
    headers: &HeaderMap,
    uri: &Uri,
) -> ApiResult {
    let cat = st.catalog();
    let dv = || cat.data_version.clone();
    let level: i16 = match p.level.as_deref().map(str::trim) {
        None | Some("") => {
            return Err(
                ApiError::new(StatusCode::BAD_REQUEST, "level is required (0..4)")
                    .with_version(dv()),
            );
        }
        Some(l) => l.parse().map_err(|_| {
            ApiError::new(StatusCode::BAD_REQUEST, "level must be an integer 0..4")
                .with_version(dv())
        })?,
    };
    let format = p.format.as_deref().unwrap_or("json");
    if !matches!(format, "json" | "arrow" | "csv") {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            format!("format \"{format}\": valid values: json | arrow | csv (parquet with WP-09)"),
        )
        .with_version(dv()));
    }
    let q = lanka_core::frame::FrameQuery {
        indicators: split_list(p.indicators.as_deref()),
        level,
        scheme: p.scheme.filter(|s| !s.is_empty()),
        parent: p.parent.filter(|s| !s.is_empty()),
        periods: split_list(p.periods.as_deref()),
        from: p.from.filter(|s| !s.is_empty()),
        to: p.to.filter(|s| !s.is_empty()),
        dims: p.dims.filter(|s| !s.is_empty()),
        pin: lanka_core::frame::FramePin {
            release,
            dsvs: split_list(p.dsv.as_deref()),
        },
    };
    let pinned = !q.pin.is_current();
    let cache = if pinned { CACHE_PINNED } else { CACHE_FLOATING };
    // Current release: the dsv set is known from the snapshot, so a
    // revalidation is answered without touching Postgres.
    if !pinned {
        let ds: Vec<&str> = q
            .indicators
            .iter()
            .filter_map(|i| cat.indicators.get(i).map(|x| x.dataset_id.as_str()))
            .collect();
        let etag = etag_of(uri, &cat.meta_for(with_boundaries(&cat, ds), Vec::new()));
        if let Some(r) = not_modified(headers, &etag, cache) {
            return Ok(r);
        }
    }
    let f = lanka_core::frame::frame(&st.pool, &cat, &q)
        .await
        .map_err(|e| ApiError::from(e).with_version(dv()))?;
    let mut meta = cat.meta_for(
        with_boundaries(&cat, f.versions.iter().map(|v| v.dataset.as_str())),
        Vec::new(),
    );
    if pinned {
        meta.data_version.clone_from(&f.data_version);
        for v in &f.versions {
            if let Some(m) = meta
                .dataset_versions
                .iter_mut()
                .find(|m| m.dataset == v.dataset)
            {
                *m = v.clone();
            }
        }
    }
    match format {
        "arrow" | "csv" => {
            let etag = etag_of(uri, &meta);
            if let Some(r) = not_modified(headers, &etag, cache) {
                return Ok(r);
            }
            let (body, ctype) = if format == "arrow" {
                let b = frame_codec::arrow(&f, &meta).map_err(|e| {
                    tracing::error!(error = %e, "arrow encode");
                    ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "internal error")
                })?;
                (b, frame_codec::ARROW_STREAM)
            } else {
                (frame_codec::csv(&f).into_bytes(), "text/csv; charset=utf-8")
            };
            let mut res = body.into_response();
            let h = res.headers_mut();
            h.insert(header::CONTENT_TYPE, HeaderValue::from_static(ctype));
            if let Some(v) = meta
                .data_version
                .as_deref()
                .and_then(|v| HeaderValue::from_str(v).ok())
            {
                h.insert("x-data-version", v);
            }
            let versions: Vec<String> = meta
                .dataset_versions
                .iter()
                .map(|d| format!("{}={}", d.dataset, d.dsv))
                .collect();
            if let Ok(v) = HeaderValue::from_str(&versions.join(",")) {
                h.insert("x-dataset-versions", v);
            }
            with_cache(res, &etag, cache)
        }
        _ => respond_cached(headers, uri, f, meta, cache),
    }
}

/// A frame: values of up to 8 indicators on one admin level, across periods and dims, as one code array
/// plus one value array per (indicator, dims, period) — for MapLibre `feature-state` joins (10 §3.1).
/// `codes` are every unit of the reference boundary set at the level (under `parent`); `null` = no value.
/// `format=arrow` returns the same table as an Arrow IPC stream (Float64 values), `format=csv` a CSV.
/// Limits: 64 columns, 64 periods per indicator, codes × columns ≤ 500,000.
#[utoipa::path(get, path = "/v2/frame", tag = "indicators", params(FrameParams),
    responses((status = 200, description = "Frame (JSON envelope; Arrow IPC stream with `format=arrow`; CSV with `format=csv`)",
               content((Envelope<lanka_core::frame::Frame> = "application/json"),
                       (Vec<u8> = "application/vnd.apache.arrow.stream"), (String = "text/csv"))),
              (status = 304, description = "Not modified (ETag tied to the release and dataset versions)"),
              (status = 400, body = ErrorBody), (status = 404, body = ErrorBody)))]
async fn frame(
    State(st): State<AppState>,
    Query(p): Query<FrameParams>,
    headers: HeaderMap,
    uri: Uri,
) -> ApiResult {
    frame_response(&st, None, p, &headers, &uri).await
}

/// A frame pinned to a release (`data_version`): the values that release served, cacheable for a day.
#[utoipa::path(get, path = "/v2/r/{data_version}/frame", tag = "indicators",
    params(("data_version" = String, Path, description = "Release id, e.g. 20261122.1"), FrameParams),
    responses((status = 200, description = "Frame at that release",
               content((Envelope<lanka_core::frame::Frame> = "application/json"),
                       (Vec<u8> = "application/vnd.apache.arrow.stream"), (String = "text/csv"))),
              (status = 304, description = "Not modified"),
              (status = 400, body = ErrorBody), (status = 404, body = ErrorBody)))]
async fn frame_release(
    State(st): State<AppState>,
    Path(data_version): Path<String>,
    Query(p): Query<FrameParams>,
    headers: HeaderMap,
    uri: Uri,
) -> ApiResult {
    let valid = data_version.split_once('.').is_some_and(|(d, n)| {
        d.len() == 8
            && d.bytes().all(|b| b.is_ascii_digit())
            && !n.is_empty()
            && n.bytes().all(|b| b.is_ascii_digit())
    });
    if !valid {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "data_version must be YYYYMMDD.N",
        ));
    }
    frame_response(&st, Some(data_version), p, &headers, &uri).await
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
    let p = query::provenance(&st.pool, &cat, id)
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
    let r = query::reverse(&st.pool, &cat, lat, lon)
        .await
        .map_err(|e| ApiError::from(e).with_version(cat.data_version.clone()))?;
    let next = vec![Link::new("area", format!("/v2/areas/{}", r.unit.pcode))];
    let meta = cat.meta_for(with_boundaries(&cat, []), next);
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
        .route("/v2/status", get(status_summary))
        .route("/v2/status/sources", get(status_sources))
        .route("/v2/status/datasets", get(status_datasets))
        .route("/v2/status/runs", get(status_runs))
        .route("/v2/status/events", get(status_events))
        .route("/v2/status/incidents", get(status_incidents))
        .route("/v2/catalog", get(catalog))
        .route("/v2/datasets/{id}", get(dataset))
        .route("/v2/areas/{pcode}", get(area))
        .route("/v2/indicators/{id}/series", get(series))
        .route("/v2/indicators/{id}/map", get(indicator_map))
        .route("/v2/frame", get(frame))
        .route("/v2/r/{data_version}/frame", get(frame_release))
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
            .acquire_timeout(Duration::from_millis(500))
            .connect_lazy("postgres://nobody@127.0.0.1:1/none")
            .expect("lazy pool");
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../registry");
        let profile = lanka_core::Profile::discover(&root)
            .and_then(lanka_core::Profile::load)
            .expect("repository profile");
        let cat = Catalog {
            profile: Some(Arc::new(profile)),
            ..Catalog::default()
        };
        let state = AppState {
            pool,
            catalog: Arc::new(RwLock::new(Arc::new(cat))),
            started: std::time::Instant::now(),
            store_dir: None,
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
            "/v2/status",
            "/v2/status/sources",
            "/v2/status/datasets",
            "/v2/status/runs",
            "/v2/status/events",
            "/v2/status/incidents",
            "/v2/catalog",
            "/v2/datasets/{id}",
            "/v2/areas/{pcode}",
            "/v2/indicators/{id}/series",
            "/v2/indicators/{id}/map",
            "/v2/frame",
            "/v2/r/{data_version}/frame",
            "/v2/observations/{id}/provenance",
            "/v2/reverse",
        ] {
            assert!(v["paths"].get(p).is_some(), "missing {p}");
        }
    }

    #[tokio::test]
    async fn health_is_503_without_database_or_release() {
        let (status, v) = get("/v2/health").await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(v["payload"]["status"], "unavailable");
        assert_eq!(v["payload"]["db"]["ok"], false);
    }

    #[tokio::test]
    async fn status_is_200_even_when_the_database_is_down() {
        let (status, v) = get("/v2/status").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(v["payload"]["overall"], "failing");
    }

    #[tokio::test]
    async fn frame_validates_before_any_query() {
        for (uri, needle) in [
            ("/v2/frame?indicators=a.b", "level is required"),
            (
                "/v2/frame?indicators=a.b&level=x",
                "level must be an integer",
            ),
            ("/v2/frame?indicators=a.b&level=2&format=xml", "format"),
            ("/v2/r/latest/frame?indicators=a.b&level=2", "YYYYMMDD.N"),
        ] {
            let (status, v) = get(uri).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{uri}");
            assert!(
                v["message"].as_str().is_some_and(|m| m.contains(needle)),
                "{uri}: {v}"
            );
        }
    }

    #[tokio::test]
    async fn reverse_validates_coordinates() {
        let (status, _) = get("/v2/reverse?lat=abc&lon=80").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }
}
