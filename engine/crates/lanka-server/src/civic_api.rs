//! The civic record (13 §2.1): ministries and the institutions in their
//! charge over time, and the archived gazettes they come from.
//!
//! | Route | Payload |
//! |---|---|
//! | `GET /v2/civic/instruments` | every instrument (baselines and amendments, with `status`), newest first |
//! | `GET /v2/civic/instruments/{doc_id}/changes` | what an amending gazette changes, change by change, and whether each applied |
//! | `GET /v2/civic/structure?at=` | the ministries in force on a date (default today), with functions, institutions, laws |
//! | `GET /v2/civic/diff?from=&to=` | institutions that moved, were added or removed between two dates |
//! | `GET /v2/civic/entities?q=&limit=` | institutions by name |
//! | `GET /v2/civic/entities/{key}` | one institution and the ministries it sat under |
//! | `GET /v2/documents/{doc_id}` | an archived gazette: files, pages, hashes, the publisher's URL |
//! | `GET /v2/documents/{doc_id}/{lang}.pdf` | the archived PDF, with `Link: <source>; rel="via"` |
//!
//! Civic routes serve `validated` and `approved` instruments only, except
//! the instrument list. Every response cites the gazette collection and its
//! licence in `meta`; the ETag is a hash of the payload, because the
//! structure changes with `lanka civic build`, not with a data release.

use std::hash::{Hash, Hasher};
use std::path::PathBuf;

use axum::Router;
use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use chrono::NaiveDate;
use futures::stream;
use lanka_core::civic::{
    self, AmendmentChanges, Diff, Document, EntityDetail, EntityRow, InstrumentRow, Structure,
};
use lanka_core::envelope::Link;
use lanka_core::{CoreError, Envelope, Meta};
use serde::{Deserialize, Serialize};
use tokio::io::AsyncReadExt;
use utoipa::{IntoParams, OpenApi};

use crate::{ApiError, ApiResult, AppState, CACHE_FLOATING, ErrorBody};

#[derive(OpenApi)]
#[openapi(
    paths(instruments, amendment_changes, structure, diff, entities, entity, document, document_pdf),
    tags(
        (name = "civic", description = "Ministries and the institutions in their charge, over time (Article 44 gazettes)"),
        (name = "documents", description = "The archived gazettes: record, files, PDF"),
    )
)]
pub struct CivicDoc;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/v2/civic/instruments", get(instruments))
        .route(
            "/v2/civic/instruments/{doc_id}/changes",
            get(amendment_changes),
        )
        .route("/v2/civic/structure", get(structure))
        .route("/v2/civic/diff", get(diff))
        .route("/v2/civic/entities", get(entities))
        .route("/v2/civic/entities/{key}", get(entity))
        .route("/v2/documents/{doc_id}", get(document))
        .route("/v2/documents/{doc_id}/{file}", get(document_pdf))
}

fn fail(st: &AppState, e: CoreError) -> ApiError {
    ApiError::from(e).with_version(st.catalog().data_version.clone())
}

/// A 404 for a date before the first parsed schedule, with the first date in
/// `payload.problem.earliest`.
async fn fail_dated(st: &AppState, e: CoreError) -> ApiError {
    let CoreError::NotFound(msg) = &e else {
        return fail(st, e);
    };
    let earliest = civic::earliest(&st.pool).await.ok().flatten();
    ApiError {
        problem: Some(serde_json::json!({
            "code": "not_in_coverage",
            "earliest": earliest.map(|d| d.to_string()),
        })),
        ..ApiError::new(StatusCode::NOT_FOUND, format!("not_found: {msg}"))
            .with_version(st.catalog().data_version.clone())
    }
}

fn bad(st: &AppState, msg: impl Into<String>) -> ApiError {
    ApiError::new(StatusCode::BAD_REQUEST, msg).with_version(st.catalog().data_version.clone())
}

fn meta(st: &AppState, next: Vec<Link>) -> Meta {
    let mut m = Meta::bare(st.catalog().data_version.clone());
    civic::gazette_meta(&mut m);
    m.next = next;
    m
}

fn parse_date(st: &AppState, name: &str, v: &str) -> Result<NaiveDate, ApiError> {
    NaiveDate::parse_from_str(v.trim(), "%Y-%m-%d").map_err(|_| {
        bad(
            st,
            format!("{name}: expected a date as YYYY-MM-DD, got {v:?}"),
        )
    })
}

fn today() -> NaiveDate {
    chrono::Utc::now().date_naive()
}

/// An OK envelope whose ETag is a hash of the payload (the data changes with
/// `lanka civic build`, which no release id tracks).
fn respond_payload<T: Serialize>(headers: &HeaderMap, payload: T, meta: Meta) -> ApiResult {
    let body = serde_json::to_vec(&Envelope::ok(payload, meta))
        .map_err(|_| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "internal error"))?;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    body.hash(&mut h);
    let etag = format!("\"civic-{:016x}\"", h.finish());
    if let Some(r) = crate::not_modified(headers, &etag, CACHE_FLOATING) {
        return Ok(r);
    }
    let mut res = Response::new(Body::from(body));
    res.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    crate::with_cache(res, &etag, CACHE_FLOATING)
}

/// Every instrument, newest first, with its status.
#[utoipa::path(get, path = "/v2/civic/instruments", tag = "civic",
    responses((status = 200, body = Envelope<Vec<InstrumentRow>>), (status = 500, body = ErrorBody)))]
async fn instruments(State(st): State<AppState>, headers: HeaderMap) -> ApiResult {
    let rows = civic::instruments(&st.pool)
        .await
        .map_err(|e| fail(&st, e))?;
    let next = rows
        .iter()
        .find(|r| r.kind == "baseline" && matches!(r.status.as_str(), "validated" | "approved"))
        .map(|r| {
            vec![Link::new(
                "structure",
                format!("/v2/civic/structure?at={}", r.effective_from),
            )]
        })
        .unwrap_or_default();
    respond_payload(&headers, rows, meta(&st, next))
}

#[derive(Debug, Deserialize, IntoParams)]
pub struct StructureParams {
    /// `YYYY-MM-DD`; default today.
    pub at: Option<String>,
}

/// The ministries in force on a date: functions, institutions and laws, with the amendments since.
#[utoipa::path(get, path = "/v2/civic/structure", tag = "civic", params(StructureParams),
    responses((status = 200, body = Envelope<Structure>), (status = 400, body = ErrorBody),
              (status = 404, body = ErrorBody)))]
async fn structure(
    State(st): State<AppState>,
    Query(p): Query<StructureParams>,
    headers: HeaderMap,
) -> ApiResult {
    let at = match p.at.as_deref().filter(|s| !s.is_empty()) {
        Some(v) => parse_date(&st, "at", v)?,
        None => today(),
    };
    let s = match civic::structure(&st.pool, at).await {
        Ok(s) => s,
        Err(e) => return Err(fail_dated(&st, e).await),
    };
    let next = vec![Link::new(
        "document",
        format!("/v2/documents/{}", s.instrument.doc_id),
    )];
    respond_payload(&headers, s, meta(&st, next))
}

#[derive(Debug, Deserialize, IntoParams)]
pub struct DiffParams {
    /// `YYYY-MM-DD`
    pub from: Option<String>,
    /// `YYYY-MM-DD`; default today.
    pub to: Option<String>,
}

/// What changed between the schedules in force on two dates.
#[utoipa::path(get, path = "/v2/civic/diff", tag = "civic", params(DiffParams),
    responses((status = 200, body = Envelope<Diff>), (status = 400, body = ErrorBody),
              (status = 404, body = ErrorBody)))]
async fn diff(
    State(st): State<AppState>,
    Query(p): Query<DiffParams>,
    headers: HeaderMap,
) -> ApiResult {
    let Some(from) = p.from.as_deref().filter(|s| !s.is_empty()) else {
        return Err(bad(
            &st,
            "from is required: /v2/civic/diff?from=YYYY-MM-DD&to=YYYY-MM-DD",
        ));
    };
    let from = parse_date(&st, "from", from)?;
    let to = match p.to.as_deref().filter(|s| !s.is_empty()) {
        Some(v) => parse_date(&st, "to", v)?,
        None => today(),
    };
    let d = match civic::diff(&st.pool, from, to).await {
        Ok(d) => d,
        Err(e) => return Err(fail_dated(&st, e).await),
    };
    let next = vec![
        Link::new("from", format!("/v2/civic/structure?at={from}")),
        Link::new("to", format!("/v2/civic/structure?at={to}")),
    ];
    respond_payload(&headers, d, meta(&st, next))
}

#[derive(Debug, Deserialize, IntoParams)]
pub struct EntitiesParams {
    /// Case-insensitive substring of the name, an alias or the key.
    pub q: Option<String>,
    /// 1–500 (default 50).
    pub limit: Option<i64>,
}

/// Institutions by name, those in a ministry now first.
#[utoipa::path(get, path = "/v2/civic/entities", tag = "civic", params(EntitiesParams),
    responses((status = 200, body = Envelope<Vec<EntityRow>>), (status = 400, body = ErrorBody)))]
async fn entities(
    State(st): State<AppState>,
    Query(p): Query<EntitiesParams>,
    headers: HeaderMap,
) -> ApiResult {
    let rows = civic::entities(&st.pool, p.q.as_deref(), p.limit)
        .await
        .map_err(|e| fail(&st, e))?;
    let next = rows
        .first()
        .map(|r| {
            vec![Link::new(
                "entity",
                format!("/v2/civic/entities/{}", r.entity_key),
            )]
        })
        .unwrap_or_default();
    respond_payload(&headers, rows, meta(&st, next))
}

/// What an amending gazette changes, change by change: the clause as
/// printed, the change read from it, and whether it applied.
#[utoipa::path(get, path = "/v2/civic/instruments/{doc_id}/changes", tag = "civic",
    params(("doc_id" = String, Path, description = "an amendment's document id, e.g. gze-2458-65")),
    responses((status = 200, body = Envelope<AmendmentChanges>), (status = 404, body = ErrorBody)))]
async fn amendment_changes(
    State(st): State<AppState>,
    Path(doc_id): Path<String>,
    headers: HeaderMap,
) -> ApiResult {
    let c = civic::amendment_changes(&st.pool, &doc_id)
        .await
        .map_err(|e| fail(&st, e))?;
    let next = vec![Link::new("document", format!("/v2/documents/{doc_id}"))];
    respond_payload(&headers, c, meta(&st, next))
}

/// One institution and the ministries it sat under, oldest first.
#[utoipa::path(get, path = "/v2/civic/entities/{key}", tag = "civic",
    params(("key" = String, Path, description = "entity key, e.g. department-of-meteorology")),
    responses((status = 200, body = Envelope<EntityDetail>), (status = 404, body = ErrorBody)))]
async fn entity(
    State(st): State<AppState>,
    Path(key): Path<String>,
    headers: HeaderMap,
) -> ApiResult {
    let e = civic::entity(&st.pool, &key)
        .await
        .map_err(|e| fail(&st, e))?;
    let next = e
        .history
        .iter()
        .filter_map(|h| h.doc_id.as_deref())
        .next_back()
        .map(|d| vec![Link::new("document", format!("/v2/documents/{d}"))])
        .unwrap_or_default();
    respond_payload(&headers, e, meta(&st, next))
}

/// An archived gazette: its title, files (status, pages, size, hash) and the publisher's URL.
#[utoipa::path(get, path = "/v2/documents/{doc_id}", tag = "documents",
    params(("doc_id" = String, Path, description = "document id, e.g. gze-2412-08")),
    responses((status = 200, body = Envelope<Document>), (status = 404, body = ErrorBody)))]
async fn document(
    State(st): State<AppState>,
    Path(doc_id): Path<String>,
    headers: HeaderMap,
) -> ApiResult {
    let d = civic::document(&st.pool, &doc_id)
        .await
        .map_err(|e| fail(&st, e))?;
    let next = d
        .files
        .iter()
        .filter(|f| f.status == "ok")
        .map(|f| {
            Link::new(
                format!("pdf-{}", f.lang),
                format!("/v2/documents/{}/{}.pdf", d.doc_id, f.lang),
            )
        })
        .collect();
    respond_payload(&headers, d, meta(&st, next))
}

/// `<store>/raw/ab/cd/<sha>` for a hex sha256.
fn blob_path(root: &std::path::Path, sha: &str) -> PathBuf {
    root.join("raw").join(&sha[0..2]).join(&sha[2..4]).join(sha)
}

/// The archived PDF of one language, streamed from the blob store.
#[utoipa::path(get, path = "/v2/documents/{doc_id}/{lang}.pdf", tag = "documents",
    params(("doc_id" = String, Path, description = "document id, e.g. gze-2412-08"),
           ("lang" = String, Path, description = "`en`, `si` or `ta`")),
    responses((status = 200, description = "the PDF; `Link: <publisher URL>; rel=\"via\"`", content_type = "application/pdf"),
              (status = 404, body = ErrorBody), (status = 503, body = ErrorBody)))]
async fn document_pdf(
    State(st): State<AppState>,
    Path((doc_id, file)): Path<(String, String)>,
    headers: HeaderMap,
) -> ApiResult {
    let not_found = || {
        ApiError::new(
            StatusCode::NOT_FOUND,
            "not_found: no such file (expected /v2/documents/{doc_id}/{en|si|ta}.pdf)",
        )
        .with_version(st.catalog().data_version.clone())
    };
    let lang = file.strip_suffix(".pdf").ok_or_else(not_found)?;
    if !matches!(lang, "en" | "si" | "ta") {
        return Err(not_found());
    }
    let f = civic::pdf_file(&st.pool, &doc_id, lang)
        .await
        .map_err(|e| fail(&st, e))?;
    let Some(root) = st.store_dir.as_deref() else {
        return Err(ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "the document store is not configured",
        )
        .with_version(st.catalog().data_version.clone()));
    };
    let etag = format!("\"{}\"", f.sha256);
    if let Some(r) = crate::not_modified(&headers, &etag, "public, max-age=86400") {
        return Ok(r);
    }
    let path = blob_path(root, &f.sha256);
    let file = tokio::fs::File::open(&path).await.map_err(|e| {
        tracing::error!(path = %path.display(), error = %e, "archived PDF not readable");
        ApiError::new(
            StatusCode::NOT_FOUND,
            "not_found: the archived file is not in the store",
        )
        .with_version(st.catalog().data_version.clone())
    })?;
    let len = file.metadata().await.ok().map(|m| m.len());
    let body = Body::from_stream(stream::unfold(Some(file), |state| async move {
        let mut file = state?;
        let mut buf = vec![0u8; 256 * 1024];
        match file.read(&mut buf).await {
            Ok(0) => None,
            Ok(n) => {
                buf.truncate(n);
                Some((
                    Ok::<_, std::io::Error>(axum::body::Bytes::from(buf)),
                    Some(file),
                ))
            }
            Err(e) => Some((Err(e), None)),
        }
    }));
    let mut res = body.into_response();
    let h = res.headers_mut();
    h.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/pdf"),
    );
    if let Some(l) = len {
        h.insert(header::CONTENT_LENGTH, HeaderValue::from(l));
    }
    if let Ok(v) = HeaderValue::from_str(&format!("inline; filename=\"{doc_id}-{lang}.pdf\"")) {
        h.insert(header::CONTENT_DISPOSITION, v);
    }
    if let Some(src) = &f.source_url
        && let Ok(v) = HeaderValue::from_str(&format!("<{src}>; rel=\"via\""))
    {
        h.insert(header::LINK, v);
    }
    if let Ok(v) = HeaderValue::from_str(&etag) {
        h.insert(header::ETAG, v);
    }
    h.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=86400"),
    );
    Ok(res)
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, RwLock};

    use axum::body::Body;
    use http_body_util::BodyExt;
    use lanka_core::Catalog;
    use sqlx::{Connection, Executor, PgConnection};
    use tower::ServiceExt;

    use super::*;

    #[test]
    fn blob_paths_follow_the_store_layout() {
        let sha = "d2bf638391bfff8e05fc2e38d997268901e242212dc496033f5f62bec6a0a600";
        assert_eq!(
            blob_path(std::path::Path::new("/s"), sha),
            PathBuf::from(format!("/s/raw/d2/bf/{sha}"))
        );
    }

    fn state(pool: sqlx::PgPool, store: Option<PathBuf>) -> AppState {
        AppState {
            pool,
            catalog: Arc::new(RwLock::new(Arc::new(Catalog::default()))),
            started: std::time::Instant::now(),
            store_dir: store,
            app: None,
            grids: Arc::new(RwLock::new(Arc::new(crate::grids::GridSet::default()))),
            analysis: None,
        }
    }

    async fn get(app: &Router, uri: &str) -> (StatusCode, HeaderMap, Vec<u8>) {
        get_with(app, uri, &[]).await
    }

    async fn get_with(
        app: &Router,
        uri: &str,
        headers: &[(&str, &str)],
    ) -> (StatusCode, HeaderMap, Vec<u8>) {
        let mut req = axum::http::Request::get(uri);
        for (k, v) in headers {
            req = req.header(*k, *v);
        }
        let res = app
            .clone()
            .oneshot(req.body(Body::empty()).expect("request"))
            .await
            .expect("response");
        let (status, h) = (res.status(), res.headers().clone());
        let body = res
            .into_body()
            .collect()
            .await
            .expect("body")
            .to_bytes()
            .to_vec();
        (status, h, body)
    }

    fn json(b: &[u8]) -> serde_json::Value {
        serde_json::from_slice(b).expect("json")
    }

    /// Requests that never reach the database: validation and unknown files.
    #[tokio::test]
    async fn bad_requests_are_refused_before_any_query() {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .acquire_timeout(std::time::Duration::from_millis(300))
            .connect_lazy("postgres://nobody@127.0.0.1:1/none")
            .expect("lazy pool");
        let app = crate::router(state(pool, None));
        for (uri, want) in [
            ("/v2/civic/structure?at=yesterday", StatusCode::BAD_REQUEST),
            ("/v2/civic/diff", StatusCode::BAD_REQUEST),
            ("/v2/civic/diff?from=2024-13-40", StatusCode::BAD_REQUEST),
            (
                "/v2/civic/diff?from=2024-10-01&to=soon",
                StatusCode::BAD_REQUEST,
            ),
            ("/v2/documents/gze-2412-08/en.txt", StatusCode::NOT_FOUND),
            ("/v2/documents/gze-2412-08/fr.pdf", StatusCode::NOT_FOUND),
            ("/v2/documents/gze-2412-08/en", StatusCode::NOT_FOUND),
        ] {
            let (status, _, body) = get(&app, uri).await;
            assert_eq!(status, want, "{uri}");
            assert_eq!(json(&body)["success"], false, "{uri}");
        }
    }

    #[tokio::test]
    async fn the_routes_are_in_the_openapi_document() {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgres://nobody@127.0.0.1:1/none")
            .expect("lazy pool");
        let app = crate::router(state(pool, None));
        let (status, _, body) = get(&app, "/v2/openapi.json").await;
        assert_eq!(status, StatusCode::OK);
        let paths = &json(&body)["paths"];
        for p in [
            "/v2/civic/instruments",
            "/v2/civic/instruments/{doc_id}/changes",
            "/v2/civic/structure",
            "/v2/civic/diff",
            "/v2/civic/entities",
            "/v2/civic/entities/{key}",
            "/v2/documents/{doc_id}",
            "/v2/documents/{doc_id}/{lang}.pdf",
        ] {
            assert!(
                paths.get(p).is_some(),
                "{p} missing from the OpenAPI document"
            );
        }
    }

    /// The endpoints over a small database (gated like the other database
    /// tests): two baselines, an amendment, one archived PDF.
    #[tokio::test(flavor = "multi_thread")]
    async fn civic_endpoints_serve_the_built_structure() {
        let Ok(admin_url) = std::env::var("LANKA_TEST_DATABASE_URL") else {
            eprintln!("skipping: LANKA_TEST_DATABASE_URL not set");
            return;
        };
        let db = format!("lanka_it_civicapi_{}", std::process::id());
        let mut admin = PgConnection::connect(&admin_url).await.expect("admin");
        let drop = format!("DROP DATABASE IF EXISTS {db} WITH (FORCE)");
        admin
            .execute(sqlx::raw_sql(sqlx::AssertSqlSafe(drop.clone())))
            .await
            .expect("drop");
        admin
            .execute(sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
                "CREATE DATABASE {db}"
            ))))
            .await
            .expect("create");
        let base = admin_url
            .rsplit_once('/')
            .map_or(admin_url.as_str(), |(b, _)| b);
        let url = format!("{base}/{db}");
        let result = tokio::spawn(run_civic(url)).await;
        admin
            .execute(sqlx::raw_sql(sqlx::AssertSqlSafe(drop)))
            .await
            .expect("drop");
        if let Err(e) = result {
            std::panic::resume_unwind(e.into_panic());
        }
    }

    async fn run_civic(url: String) {
        let pool = lanka_core::db::connect(&url, lanka_core::db::PoolRole::Ingest)
            .await
            .expect("pool");
        lanka_core::db::migrate(&pool).await.expect("migrate");
        let sql = |q: &'static str| sqlx::query(q);
        // the archived PDF: a blob in a temporary store
        let sha = "ab12".to_owned() + &"0".repeat(60);
        let store = std::env::temp_dir().join(format!("lanka_civic_store_{}", std::process::id()));
        let blob = blob_path(&store, &sha);
        std::fs::create_dir_all(blob.parent().expect("parent")).expect("mkdir");
        std::fs::write(&blob, b"%PDF-1.4 a small test document").expect("write blob");

        for (id, num, sub, date) in [
            ("gze-1000-01", "1000", "01", "2020-01-01"),
            ("gze-2000-01", "2000", "01", "2021-01-01"),
            ("gze-1500-01", "1500", "01", "2020-06-01"),
        ] {
            sql("INSERT INTO docs.documents (doc_id, collection, number, sub_number, issued_on, title, upstream_id, meta_hash)
                 VALUES ($1, 'lk-gazette-extraordinary', $2, $3, $4::date, $5, $1, 'h')")
                .bind(id).bind(num).bind(sub).bind(date)
                .bind(serde_json::json!({"en": format!("Duties and Functions {id}"), "si": "", "ta": ""}))
                .execute(&pool).await.expect("document");
        }
        sql("INSERT INTO docs.files (doc_id, lang, upstream_path, status, sha256, bytes, pages, text_status)
             VALUES ('gze-2000-01', 'en', 'extra-gazette-content/G 1 (E).pdf', 'ok', $1, 30, 2, 'text'),
                    ('gze-2000-01', 'si', 'extra-gazette-content/G 1 (S).pdf', 'missing', NULL, NULL, NULL, NULL)")
            .bind(&sha).execute(&pool).await.expect("files");
        sql("INSERT INTO civic.instruments (doc_id, kind, effective_from, amends, head_of_state, parser, status) VALUES
              ('gze-1000-01', 'baseline', '2020-01-01', NULL, 'A. President', 't', 'validated'),
              ('gze-2000-01', 'baseline', '2021-01-01', NULL, 'B. President', 't', 'validated'),
              ('gze-1500-01', 'amendment', '2020-06-01', 'gze-1000-01', NULL, 't', 'validated')")
            .execute(&pool).await.expect("instruments");
        sql("INSERT INTO civic.portfolios (doc_id, seq, title, portfolio_key, first_page) VALUES
              ('gze-1000-01', 1, 'Minister of Alpha', 'alpha', 2), ('gze-1000-01', 2, 'Minister of Beta', 'beta', 4),
              ('gze-2000-01', 1, 'Minister of Alpha', 'alpha', 2)")
            .execute(&pool).await.expect("portfolios");
        sql("INSERT INTO civic.items (doc_id, seq, col, item_no, text, entity_key, page) VALUES
              ('gze-1000-01', 1, 'institution', 1, 'Department of Excise', 'department-of-excise', 2),
              ('gze-1000-01', 2, 'institution', 1, 'Sri Lanka Customs', 'sri-lanka-customs', 4),
              ('gze-1000-01', 1, 'function', 1, 'Formulation of policies', NULL, 2),
              ('gze-1000-01', 1, 'law', 1, 'Excise Ordinance (Chapter 52)', NULL, 2),
              ('gze-2000-01', 1, 'institution', 1, 'Department of Excise', 'department-of-excise', 2)")
            .execute(&pool).await.expect("items");
        sql(
            "INSERT INTO civic.entities (entity_key, name, kind, aliases) VALUES
              ('department-of-excise', 'Department of Excise', 'department', '{}'),
              ('sri-lanka-customs', 'Sri Lanka Customs', 'other', '{Customs}')",
        )
        .execute(&pool)
        .await
        .expect("entities");
        sql("INSERT INTO civic.assignments (entity_key, doc_id, seq, portfolio_title, portfolio_key, valid_from, valid_to, page) VALUES
              ('department-of-excise', 'gze-1000-01', 1, 'Minister of Alpha', 'alpha', '2020-01-01', NULL, 2),
              ('sri-lanka-customs', 'gze-1000-01', 2, 'Minister of Beta', 'beta', '2020-01-01', '2021-01-01', 4)")
            .execute(&pool).await.expect("assignments");

        let app = crate::router(state(pool, Some(store.clone())));

        // instruments: every kind and status, newest first, with counts
        let (status, h, body) = get(&app, "/v2/civic/instruments").await;
        assert_eq!(status, StatusCode::OK);
        assert!(h.get(header::ETAG).is_some());
        let v = json(&body);
        assert_eq!(v["success"], true);
        let rows = v["payload"].as_array().expect("rows");
        let ids: Vec<&str> = rows
            .iter()
            .map(|r| r["doc_id"].as_str().expect("id"))
            .collect();
        assert_eq!(ids, ["gze-2000-01", "gze-1500-01", "gze-1000-01"]);
        assert_eq!(rows[2]["portfolios"], 2);
        assert_eq!(rows[2]["institutions"], 2);
        assert_eq!(rows[1]["kind"], "amendment");
        assert_eq!(rows[1]["amends"], "gze-1000-01");
        assert_eq!(rows[0]["number"], "2000/01");
        assert_eq!(rows[0]["status"], "validated");
        assert_eq!(
            v["meta"]["source"][0]["licence"],
            "public-domain-official-text"
        );

        // structure at a date
        let (status, _, body) = get(&app, "/v2/civic/structure?at=2020-09-01").await;
        assert_eq!(status, StatusCode::OK);
        let p = &json(&body)["payload"];
        assert_eq!(p["instrument"]["doc_id"], "gze-1000-01");
        assert_eq!(p["composed_of"][0]["doc_id"], "gze-1000-01");
        assert_eq!(p["portfolios"][0]["short_title"], "Alpha");
        assert_eq!(p["portfolios"][0]["doc_id"], "gze-1000-01");
        assert_eq!(p["amendments_since"][0]["doc_id"], "gze-1500-01");
        assert_eq!(p["portfolios"][0]["institutions"][0]["kind"], "department");
        assert_eq!(
            p["portfolios"][0]["functions"][0]["text"],
            "Formulation of policies"
        );
        assert_eq!(p["portfolios"][0]["laws"][0]["item_no"], 1);
        // without `at`: today, which is after the second baseline
        let (_, _, body) = get(&app, "/v2/civic/structure").await;
        assert_eq!(
            json(&body)["payload"]["instrument"]["doc_id"],
            "gze-2000-01"
        );
        let (status, _, body) = get(&app, "/v2/civic/structure?at=2019-01-01").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(json(&body)["payload"]["problem"]["earliest"], "2020-01-01");

        // diff: Customs is not in the second baseline's assignments
        let (status, _, body) = get(&app, "/v2/civic/diff?from=2020-03-01&to=2021-06-01").await;
        assert_eq!(status, StatusCode::OK);
        let p = &json(&body)["payload"];
        assert_eq!(p["from"]["doc_id"], "gze-1000-01");
        assert_eq!(p["to"]["doc_id"], "gze-2000-01");
        assert_eq!(p["removed"][0]["entity_key"], "sri-lanka-customs");
        assert_eq!(p["removed"][0]["from"]["portfolio_key"], "beta");
        assert_eq!(p["removed"][0]["from"]["short_title"], "Beta");
        assert_eq!(p["removed"][0]["from"]["page"], 4);
        assert_eq!(p["portfolios"]["removed"][0], "Minister of Beta");
        assert_eq!(p["counts"]["removed"], 1);
        assert_eq!(p["counts"]["portfolios_removed"], 1);
        assert_eq!(p["counts"]["portfolios_renamed"], 0);
        assert_eq!(p["portfolios"]["kept"][0]["from_key"], "alpha");
        assert_eq!(
            get(&app, "/v2/civic/diff?from=2021-06-01&to=2020-03-01")
                .await
                .0,
            StatusCode::BAD_REQUEST
        );

        // entities
        let (_, _, body) = get(&app, "/v2/civic/entities?q=customs&limit=5").await;
        let rows = json(&body)["payload"].clone();
        assert_eq!(rows[0]["entity_key"], "sri-lanka-customs");
        assert!(
            rows[0]["current"].is_null(),
            "no assignment is open any more"
        );
        let (_, _, body) = get(&app, "/v2/civic/entities?q=excise").await;
        assert_eq!(
            json(&body)["payload"][0]["current"]["portfolio_title"],
            "Minister of Alpha"
        );
        assert_eq!(
            get(&app, "/v2/civic/entities?limit=0").await.0,
            StatusCode::BAD_REQUEST
        );
        let (status, _, body) = get(&app, "/v2/civic/entities/sri-lanka-customs").await;
        assert_eq!(status, StatusCode::OK);
        let p = &json(&body)["payload"];
        assert_eq!(p["aliases"][0], "Customs");
        assert_eq!(p["history"][0]["valid_to"], "2021-01-01");
        assert_eq!(p["history"][0]["doc_id"], "gze-1000-01");
        assert_eq!(p["history"][0]["page"], 4);
        assert_eq!(
            get(&app, "/v2/civic/entities/nobody").await.0,
            StatusCode::NOT_FOUND
        );

        // documents
        let (status, _, body) = get(&app, "/v2/documents/gze-2000-01").await;
        assert_eq!(status, StatusCode::OK);
        let p = &json(&body)["payload"];
        assert_eq!(p["number"], "2000/01");
        assert_eq!(p["files"][0]["lang"], "en");
        assert_eq!(p["files"][0]["pages"], 2);
        assert_eq!(p["files"][0]["sha256"], sha);
        assert_eq!(p["files"][1]["status"], "missing");
        assert_eq!(
            p["source_url"],
            "https://documents.gov.lk/api/content-file-proxy?file=extra-gazette-content%2FG%201%20%28E%29.pdf"
        );
        assert_eq!(
            get(&app, "/v2/documents/gze-0000-00").await.0,
            StatusCode::NOT_FOUND
        );

        // the PDF: the blob, its type and the publisher's URL in `Link`
        let (status, h, body) = get(&app, "/v2/documents/gze-2000-01/en.pdf").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, b"%PDF-1.4 a small test document");
        assert_eq!(
            h.get(header::CONTENT_TYPE).expect("type"),
            "application/pdf"
        );
        assert_eq!(h.get(header::CONTENT_LENGTH).expect("length"), "30");
        assert_eq!(
            h.get(header::LINK).expect("link").to_str().expect("link"),
            "<https://documents.gov.lk/api/content-file-proxy?file=extra-gazette-content%2FG%201%20%28E%29.pdf>; rel=\"via\""
        );
        let etag = h
            .get(header::ETAG)
            .expect("etag")
            .to_str()
            .expect("etag")
            .to_owned();
        let (status, _, body) = get_with(
            &app,
            "/v2/documents/gze-2000-01/en.pdf",
            &[("if-none-match", &etag)],
        )
        .await;
        assert_eq!((status, body.len()), (StatusCode::NOT_MODIFIED, 0));
        // not archived, no such language, no such document
        assert_eq!(
            get(&app, "/v2/documents/gze-2000-01/si.pdf").await.0,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            get(&app, "/v2/documents/gze-2000-01/ta.pdf").await.0,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            get(&app, "/v2/documents/gze-0000-00/en.pdf").await.0,
            StatusCode::NOT_FOUND
        );
        // no store configured: 503, not a panic
        let bare = crate::router(state(app_pool(&url).await, None));
        assert_eq!(
            get(&bare, "/v2/documents/gze-2000-01/en.pdf").await.0,
            StatusCode::SERVICE_UNAVAILABLE
        );
        let _ = std::fs::remove_dir_all(&store);
    }

    async fn app_pool(url: &str) -> sqlx::PgPool {
        lanka_core::db::connect(url, lanka_core::db::PoolRole::Api)
            .await
            .expect("api pool")
    }
}
