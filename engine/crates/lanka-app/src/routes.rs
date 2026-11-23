//! `/app/v1` routes of stage 0 (10 §6.1): snapshots, resolution, packages.
//!
//! Every response carries `X-Robots-Tag: noindex`. Errors are the envelope
//! with a `Problem` payload (ui-contract-explore §2) and `no-store`.

use std::net::SocketAddr;

use axum::body::{Body, Bytes};
use axum::extract::{ConnectInfo, Path, Request, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use lanka_core::{Envelope, Meta};
use lanka_mapdoc::resolve::{ResolvedMap, load_context, resolve};
use lanka_mapdoc::{MapDocument, Problem};
use serde::Serialize;
use serde_json::{Value, json};

use crate::AppPlane;
use crate::package::package;
use crate::rate::identify;
use crate::scope::Principal;
use crate::snapshots::{self, Stored, is_snapshot_id};

/// Largest request body read (twice the document limit: a 300 KB document
/// is read and rejected by the validator with a JSON-pointer problem).
pub const MAX_BODY: usize = 2 * lanka_mapdoc::validate::MAX_DOC_BYTES;

// ───────────────────────────── errors ──────────────────────────────────────

/// An error answer (boxed: handlers return it by value).
#[derive(Debug)]
pub struct AppError(Box<AppErrorInner>);

#[derive(Debug)]
pub struct AppErrorInner {
    status: StatusCode,
    kind: &'static str,
    title: String,
    detail: Option<String>,
    problems: Vec<Problem>,
    retry_after: Option<u64>,
    data_version: Option<String>,
}

impl AppError {
    pub fn new(status: StatusCode, kind: &'static str, title: impl Into<String>) -> Self {
        Self(Box::new(AppErrorInner {
            status,
            kind,
            title: title.into(),
            detail: None,
            problems: Vec::new(),
            retry_after: None,
            data_version: None,
        }))
    }
    fn detail(mut self, d: impl Into<String>) -> Self {
        self.0.detail = Some(d.into());
        self
    }
    fn internal(e: impl std::fmt::Display) -> Self {
        tracing::error!(error = %e, "app plane error");
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            "internal error",
        )
    }
    fn not_found(what: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, "not_found", what)
    }
}

impl From<sqlx::Error> for AppError {
    fn from(e: sqlx::Error) -> Self {
        Self::internal(e)
    }
}

impl From<lanka_core::CoreError> for AppError {
    fn from(e: lanka_core::CoreError) -> Self {
        Self::internal(e)
    }
}

#[derive(Serialize)]
struct ProblemBody<'a> {
    #[serde(rename = "type")]
    kind: &'a str,
    title: &'a str,
    status: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<&'a str>,
    #[serde(skip_serializing_if = "<[Problem]>::is_empty")]
    problems: &'a [Problem],
    #[serde(skip_serializing_if = "Option::is_none")]
    hints: Option<Value>,
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let e = *self.0;
        let body = Envelope {
            success: false,
            message: e.title.clone(),
            payload: Some(ProblemBody {
                kind: e.kind,
                title: &e.title,
                status: e.status.as_u16(),
                detail: e.detail.as_deref(),
                problems: &e.problems,
                hints: e.retry_after.map(|s| json!({ "retry_after_s": s })),
            }),
            meta: Meta::bare(e.data_version.clone()),
        };
        let mut res = (e.status, axum::Json(body)).into_response();
        let h = res.headers_mut();
        h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
        h.insert("x-robots-tag", HeaderValue::from_static("noindex"));
        if let Some(s) = e.retry_after
            && let Ok(v) = HeaderValue::from_str(&s.to_string())
        {
            h.insert(header::RETRY_AFTER, v);
        }
        res
    }
}

type AppResult = Result<Response, AppError>;

// ───────────────────────────── helpers ─────────────────────────────────────

const NO_STORE: &str = "no-store";
/// Stored snapshot documents: immutable, but a takedown must take effect.
const CACHE_DOC: &str = "private, max-age=3600";
/// Resolution of a floating document (10 §2.3: 300 s).
const CACHE_FLOATING: &str = "private, max-age=300";
/// Resolution of a document pinned to an existing release.
const CACHE_PINNED: &str = "private, max-age=86400";

fn finish(mut res: Response, cache: &'static str, etag: Option<&str>) -> Response {
    let h = res.headers_mut();
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static(cache));
    h.insert("x-robots-tag", HeaderValue::from_static("noindex"));
    if let Some(v) = etag.and_then(|e| HeaderValue::from_str(e).ok()) {
        h.insert(header::ETAG, v);
    }
    res
}

fn ok<T: Serialize>(
    status: StatusCode,
    payload: T,
    meta: Meta,
    cache: &'static str,
    etag: Option<&str>,
) -> Response {
    finish(
        (status, axum::Json(Envelope::ok(payload, meta))).into_response(),
        cache,
        etag,
    )
}

fn not_modified(headers: &HeaderMap, etag: &str, cache: &'static str) -> Option<Response> {
    let hit = headers
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.split(',').any(|t| t.trim() == etag || t.trim() == "*"));
    hit.then(|| finish(StatusCode::NOT_MODIFIED.into_response(), cache, Some(etag)))
}

/// Reads and validates a document body.
async fn read_document(body: Body) -> Result<(Value, MapDocument), AppError> {
    let bytes: Bytes = axum::body::to_bytes(body, MAX_BODY).await.map_err(|_| {
        AppError::new(
            StatusCode::PAYLOAD_TOO_LARGE,
            "too_large",
            format!("request body over {MAX_BODY} bytes"),
        )
        .detail(format!(
            "a map document is at most {} bytes of canonical JSON",
            lanka_mapdoc::validate::MAX_DOC_BYTES
        ))
    })?;
    let v: Value = serde_json::from_slice(&bytes).map_err(|e| {
        AppError::new(StatusCode::BAD_REQUEST, "invalid_json", "body is not JSON")
            .detail(e.to_string())
    })?;
    match lanka_mapdoc::validate(&v) {
        Ok(doc) => Ok((v, doc)),
        Err(problems) => {
            let too_large = problems.iter().any(|p| p.code == "TOO_LARGE");
            let mut e = if too_large {
                AppError::new(
                    StatusCode::PAYLOAD_TOO_LARGE,
                    "too_large",
                    "map document over the size limit",
                )
            } else {
                AppError::new(
                    StatusCode::BAD_REQUEST,
                    "invalid_document",
                    format!("invalid map document: {} problem(s)", problems.len()),
                )
            };
            e.0.problems = problems;
            Err(e)
        }
    }
}

fn peer(req: &Request) -> Option<SocketAddr> {
    req.extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|c| c.0)
}

impl AppPlane {
    async fn resolved_value(
        &self,
        doc: &MapDocument,
    ) -> Result<(Value, bool, Option<String>), AppError> {
        let cat = (self.catalog)();
        let ctx = load_context(&self.pool, doc).await?;
        let pinned = ctx.release.is_some();
        let resolved = resolve(doc, &cat, &ctx);
        let dv = resolved.data_version.clone();
        let v = serde_json::to_value(ResolvedMap { doc, resolved }).map_err(AppError::internal)?;
        Ok((v, pinned, dv))
    }

    /// A stored snapshot that may be shown, or the error to answer.
    async fn readable(&self, id: &str) -> Result<(Value, MapDocument), AppError> {
        if id.starts_with("m_") || id.contains('@') {
            return Err(AppError::not_found(format!(
                "not_found: {id} — saved maps and versions arrive with accounts (WP-31)"
            )));
        }
        if !is_snapshot_id(id) {
            return Err(AppError::new(
                StatusCode::BAD_REQUEST,
                "invalid_id",
                format!("{id:?} is not a map id (ms_ + 16 hex)"),
            ));
        }
        let Some(s) = snapshots::get(&self.pool, id).await? else {
            return Err(AppError::not_found(format!("not_found: snapshot {id}")));
        };
        if s.withdrawn {
            return Err(AppError::new(
                StatusCode::GONE,
                "withdrawn",
                format!("snapshot {id} was withdrawn"),
            ));
        }
        let doc: MapDocument = serde_json::from_value(s.doc.clone()).map_err(|e| {
            AppError::internal(format!("stored snapshot {} does not parse: {e}", s.id))
        })?;
        Ok((s.doc, doc))
    }
}

// ───────────────────────────── handlers ────────────────────────────────────

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct SnapshotCreated {
    /// `ms_` + 16 hex of the content hash.
    pub id: String,
    /// The permalink page (`/m/{id}`).
    pub url: String,
    /// The document (`/app/v1/maps/{id}`).
    pub document: String,
    /// The embed iframe target.
    pub embed: String,
    /// `false` when the same content was stored before.
    pub created: bool,
    pub bytes: usize,
}

/// Store a document as an immutable, content-addressed snapshot. Idempotent:
/// the same content returns the same id (200) without consuming the rate limit.
#[utoipa::path(post, path = "/app/v1/maps/snapshots", tag = "maps",
    request_body(content = Object, description = "an ldl-map/1 document"),
    responses((status = 201, body = Envelope<SnapshotCreated>), (status = 200, body = Envelope<SnapshotCreated>),
              (status = 400, description = "invalid document (problems with JSON pointers)"),
              (status = 410, description = "the same content was withdrawn"),
              (status = 413, description = "over 256 KB of canonical JSON"),
              (status = 429, description = "snapshot rate limit; Retry-After")))]
pub async fn create_snapshot(State(app): State<AppPlane>, req: Request) -> AppResult {
    let principal = Principal::anonymous(identify(req.headers(), peer(&req), &app.hasher));
    let (v, _doc) = read_document(req.into_body()).await?;
    let (body, created) = app.store_snapshot(&v, &principal).await?;
    let id = body.id.clone();
    let status = if created {
        StatusCode::CREATED
    } else {
        StatusCode::OK
    };
    let mut res = ok(
        status,
        body,
        Meta::bare((app.catalog)().data_version.clone()),
        NO_STORE,
        None,
    );
    if let Ok(v) = HeaderValue::from_str(&format!("/app/v1/maps/{id}")) {
        res.headers_mut().insert(header::LOCATION, v);
    }
    Ok(res)
}

/// What a failed [`AppPlane::create_snapshot`] reports (the transport-neutral
/// face of the route's problem answer).
#[derive(Debug, Clone)]
pub struct SnapshotFailure {
    /// The HTTP status the route answers with.
    pub status: u16,
    /// `invalid_document`, `too_large`, `rate_limited`, `withdrawn`, …
    pub kind: &'static str,
    pub title: String,
    /// JSON-pointer problems of an invalid document.
    pub problems: Vec<Problem>,
    /// Seconds to wait when rate limited.
    pub retry_after_s: Option<u64>,
}

impl From<AppError> for SnapshotFailure {
    fn from(e: AppError) -> Self {
        let e = *e.0;
        Self {
            status: e.status.as_u16(),
            kind: e.kind,
            title: e.title,
            problems: e.problems,
            retry_after_s: e.retry_after,
        }
    }
}

impl AppPlane {
    /// Validates and stores a map document as an immutable snapshot, exactly
    /// as `POST /app/v1/maps/snapshots` does (same validation, content id,
    /// idempotence and rate limit), for in-process callers (the MCP tool
    /// `create_map_link`). `rate` is the caller's rate identity.
    pub async fn create_snapshot(
        &self,
        v: &Value,
        rate: crate::RateIdentity,
    ) -> Result<(SnapshotCreated, bool), SnapshotFailure> {
        match lanka_mapdoc::validate(v) {
            Ok(_) => {}
            Err(problems) => {
                let too_large = problems.iter().any(|p| p.code == "TOO_LARGE");
                let mut e = if too_large {
                    AppError::new(
                        StatusCode::PAYLOAD_TOO_LARGE,
                        "too_large",
                        "map document over the size limit",
                    )
                } else {
                    AppError::new(
                        StatusCode::BAD_REQUEST,
                        "invalid_document",
                        format!("invalid map document: {} problem(s)", problems.len()),
                    )
                };
                e.0.problems = problems;
                return Err(e.into());
            }
        }
        Ok(self.store_snapshot(v, &Principal::anonymous(rate)).await?)
    }

    /// The storing half of snapshot creation for a validated document.
    async fn store_snapshot(
        &self,
        v: &Value,
        principal: &Principal,
    ) -> Result<(SnapshotCreated, bool), AppError> {
        let p = snapshots::prepare(v);
        let existing = snapshots::state(&self.pool, &p.id).await?;
        if existing == Some(true) {
            return Err(AppError::new(
                StatusCode::GONE,
                "withdrawn",
                format!("snapshot {} was withdrawn", p.id),
            ));
        }
        let created = if existing.is_some() {
            false
        } else {
            if let Err(wait) = self.snapshot_limiter.check(&principal.rate) {
                let mut e = AppError::new(
                    StatusCode::TOO_MANY_REQUESTS,
                    "rate_limited",
                    format!(
                        "snapshot limit reached ({} per identity)",
                        self.cfg.snapshot_rate
                    ),
                )
                .detail("sign-in and API keys raise this limit (WP-31)");
                e.0.retry_after = Some(wait.as_secs().max(1));
                return Err(e);
            }
            let stored =
                snapshots::insert(&self.pool, &p, principal.user_id, principal.rate.ip_hash())
                    .await
                    .map_err(AppError::internal)?;
            stored == Stored::Created
        };
        let base = self.public_url();
        Ok((
            SnapshotCreated {
                url: format!("{base}/m/{}", p.id),
                document: format!("/app/v1/maps/{}", p.id),
                embed: format!("{base}/embed/m/{}", p.id),
                id: p.id.clone(),
                created,
                bytes: p.bytes,
            },
            created,
        ))
    }

    /// The hash of an address as a rate identity (this month, salted), for
    /// in-process callers that know their client's address.
    #[must_use]
    pub fn rate_identity_for(&self, ip: std::net::IpAddr) -> crate::RateIdentity {
        crate::RateIdentity::Ip(self.hasher.hash(ip))
    }
}

/// Resolve a document without storing it (anonymous): the document plus its
/// `resolved` block — versions, data URLs, attribution, notices, problems.
#[utoipa::path(post, path = "/app/v1/maps/resolve", tag = "maps",
    request_body(content = Object, description = "an ldl-map/1 document"),
    responses((status = 200, description = "Envelope<ResolvedMap>"), (status = 400, description = "invalid document"),
              (status = 413, description = "over 256 KB of canonical JSON")))]
pub async fn resolve_document(State(app): State<AppPlane>, req: Request) -> AppResult {
    let (_, doc) = read_document(req.into_body()).await?;
    let (v, _, dv) = app.resolved_value(&doc).await?;
    Ok(ok(StatusCode::OK, v, Meta::bare(dv), NO_STORE, None))
}

/// The stored document (`ms_…` snapshot; saved maps `m_…` from WP-31).
#[utoipa::path(get, path = "/app/v1/maps/{id}", tag = "maps",
    params(("id" = String, Path, description = "ms_… snapshot id")),
    responses((status = 200, description = "Envelope<MapDocument>"), (status = 404), (status = 410, description = "withdrawn")))]
pub async fn get_map(
    State(app): State<AppPlane>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> AppResult {
    let etag = format!("\"{id}\"");
    if is_snapshot_id(&id)
        && let Some(r) = not_modified(&headers, &etag, CACHE_DOC)
    {
        // content-addressed: the id is the content (a withdrawn snapshot
        // is caught by the next full read after max-age)
        return Ok(r);
    }
    let (v, _) = app.readable(&id).await?;
    Ok(ok(
        StatusCode::OK,
        v,
        Meta::bare((app.catalog)().data_version.clone()),
        CACHE_DOC,
        Some(&etag),
    ))
}

/// The resolved document of a stored map (10 §2.3).
#[utoipa::path(get, path = "/app/v1/maps/{id}/resolved", tag = "maps",
    params(("id" = String, Path, description = "ms_… snapshot id")),
    responses((status = 200, description = "Envelope<ResolvedMap>"), (status = 304), (status = 404), (status = 410)))]
pub async fn get_resolved(
    State(app): State<AppPlane>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> AppResult {
    let (_, doc) = app.readable(&id).await?;
    let (v, pinned, dv) = app.resolved_value(&doc).await?;
    let cache = if pinned { CACHE_PINNED } else { CACHE_FLOATING };
    let etag = format!("\"{id}-{}\"", dv.as_deref().unwrap_or("0"));
    if let Some(r) = not_modified(&headers, &etag, cache) {
        return Ok(r);
    }
    Ok(ok(StatusCode::OK, v, Meta::bare(dv), cache, Some(&etag)))
}

/// Frictionless `datapackage.json`: per layer, the download URLs of the data
/// behind it and its licence (not enveloped: it is a file format).
#[utoipa::path(get, path = "/app/v1/maps/{id}/package", tag = "maps",
    params(("id" = String, Path, description = "ms_… snapshot id")),
    responses((status = 200, description = "datapackage.json"), (status = 404), (status = 410)))]
pub async fn get_package(State(app): State<AppPlane>, Path(id): Path<String>) -> AppResult {
    let (_, doc) = app.readable(&id).await?;
    let cat = (app.catalog)();
    let ctx = load_context(&app.pool, &doc).await?;
    let pinned = ctx.release.is_some();
    let resolved = resolve(&doc, &cat, &ctx);
    let pkg = package(&id, &doc, &resolved, &cat, &app.public_url());
    let mut res = axum::Json(pkg).into_response();
    res.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_static("inline; filename=\"datapackage.json\""),
    );
    Ok(finish(
        res,
        if pinned { CACHE_PINNED } else { CACHE_FLOATING },
        None,
    ))
}

/// The app plane's OpenAPI document (versioned separately from `/v2`).
pub async fn openapi_json() -> Response {
    use utoipa::OpenApi;
    finish(
        axum::Json(crate::openapi::AppApiDoc::openapi()).into_response(),
        "public, max-age=300",
        None,
    )
}
