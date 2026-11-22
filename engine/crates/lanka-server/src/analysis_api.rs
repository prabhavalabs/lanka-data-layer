//! Analysis routes (WP-29, 11 §11.1):
//!
//! | Route | |
//! |---|---|
//! | `GET /v2/analysis/methods`, `/{id}` | catalogue: roles, supports, parameter/estimate schemas, min n, guards, citations, page |
//! | `POST /v2/analyses` | `ldl-analysis/1` → `303 See Other` to the result (stored, or computed now); 400 with problems/hints; 422 over the sync budget; 503 pool busy |
//! | `POST /v2/analyses/plan` | dry run: id, support, alignment, cost, guards known in advance |
//! | `GET /v2/analyses/{id}` | the result document, `immutable` |
//! | `GET /v2/analyses/{id}/outputs/{output}?format=json\|csv\|arrow` | frame/chart outputs |
//! | `GET /v2/analyses/{id}/bundle` | zip: result.json, outputs, README.md, datapackage.json, prov.jsonld |
//!
//! The engine runs on its own pool as `lanka_analysis` (reads the data
//! plane, writes `analysis.*`); without it every route but the method
//! catalogue answers 503.

use std::sync::Arc;

use arrow_array::{ArrayRef, Float64Array, RecordBatch, StringArray};
use arrow_ipc::writer::StreamWriter;
use arrow_schema::{DataType, Field, Schema};
use axum::Router;
use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use lanka_analysis::{Engine, EngineError};
use lanka_core::{CONTRACT_VERSION, Meta};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::AppState;

const IMMUTABLE: &str = "public, max-age=31536000, immutable";
pub const ANALYSIS_ROLE: &str = "lanka_analysis";

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/v2/analysis/methods", get(methods))
        .route("/v2/analysis/methods/{id}", get(method))
        .route("/v2/analyses", post(create))
        .route("/v2/analyses/plan", post(plan))
        .route("/v2/analyses/{id}", get(result))
        .route("/v2/analyses/{id}/outputs/{output}", get(output))
        .route("/v2/analyses/{id}/bundle", get(bundle))
}

/// Connects the analysis pool: `LANKA_ANALYSIS_DATABASE_URL`, or the
/// server's own connection options with `SET ROLE lanka_analysis`.
pub async fn connect(fallback: &sqlx::postgres::PgConnectOptions) -> anyhow::Result<sqlx::PgPool> {
    use sqlx::Executor;
    let (opts, set_role) = match std::env::var("LANKA_ANALYSIS_DATABASE_URL")
        .ok()
        .filter(|s| !s.is_empty())
    {
        Some(u) => (u.parse::<sqlx::postgres::PgConnectOptions>()?, false),
        None => (fallback.clone(), true),
    };
    let opts = opts
        .application_name("lanka-analysis")
        .options([("statement_timeout", "30s"), ("timezone", "UTC")]);
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(6)
        .min_connections(0)
        .acquire_timeout(std::time::Duration::from_secs(5))
        .after_connect(move |conn, _| {
            Box::pin(async move {
                if set_role {
                    conn.execute("SET ROLE lanka_analysis").await?;
                }
                Ok(())
            })
        })
        .connect_with(opts)
        .await?;
    let who: String = sqlx::query_scalar("SELECT current_user::text")
        .fetch_one(&pool)
        .await?;
    anyhow::ensure!(
        who == ANALYSIS_ROLE,
        "the analysis engine must run as {ANALYSIS_ROLE}, not {who}"
    );
    Ok(pool)
}

fn envelope_raw(payload: &[u8], data_version: Option<&str>) -> Vec<u8> {
    let meta = serde_json::to_vec(&Meta::bare(data_version.map(str::to_owned))).unwrap_or_default();
    let mut out = Vec::with_capacity(payload.len() + meta.len() + 64);
    out.extend_from_slice(br#"{"success":true,"message":"ok","payload":"#);
    out.extend_from_slice(payload);
    out.extend_from_slice(br#","meta":"#);
    out.extend_from_slice(&meta);
    out.push(b'}');
    out
}

fn json_response(status: StatusCode, body: Vec<u8>, cache: &'static str) -> Response {
    let mut res = (status, body).into_response();
    let h = res.headers_mut();
    h.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static(cache));
    res
}

fn problem(status: StatusCode, message: &str, payload: Value) -> Response {
    let body = json!({"success": false, "message": message, "payload": payload,
                      "meta": {"data_version": null, "contract_version": CONTRACT_VERSION, "dataset_versions": [],
                               "source": [], "licences": [], "next": []}});
    json_response(
        status,
        serde_json::to_vec(&body).unwrap_or_default(),
        "no-store",
    )
}

fn engine_error(e: EngineError) -> Response {
    match e {
        EngineError::Bad {
            message,
            problems,
            hints,
        } => problem(
            StatusCode::BAD_REQUEST,
            &message,
            json!({"problems": problems, "hints": hints}),
        ),
        EngineError::Busy => {
            let mut r = problem(
                StatusCode::SERVICE_UNAVAILABLE,
                "analysis pool busy; retry shortly",
                Value::Null,
            );
            r.headers_mut()
                .insert(header::RETRY_AFTER, HeaderValue::from_static("2"));
            r
        }
        EngineError::TooCostly { expected_ms } => problem(
            StatusCode::UNPROCESSABLE_ENTITY,
            "this analysis is over the synchronous budget; the interactive job lane arrives with WP-30 — narrow `within` or use a coarser level",
            json!({"expected_ms": expected_ms, "budget_ms": lanka_analysis::engine::SYNC_BUDGET_MS}),
        ),
        EngineError::Internal(e) => {
            tracing::error!(error = %format!("{e:#}"), "analysis failed");
            problem(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal error",
                Value::Null,
            )
        }
    }
}

fn unavailable() -> Response {
    problem(
        StatusCode::SERVICE_UNAVAILABLE,
        "the analysis engine is not available on this server (role lanka_analysis / blob store)",
        Value::Null,
    )
}

fn engine(st: &AppState) -> Option<Arc<Engine>> {
    st.analysis.clone()
}

async fn methods() -> Response {
    let reg = lanka_analysis::methods::Registry::builtin();
    let body = json!({
        "methods": reg.iter().map(|m| m.info().to_json()).collect::<Vec<_>>(),
        "guards": lanka_analysis::guards::registry_json(),
    });
    let mut r = axum::Json(lanka_core::Envelope::ok(body, Meta::bare(None))).into_response();
    r.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=300"),
    );
    r
}

async fn method(Path(id): Path<String>) -> Response {
    let reg = lanka_analysis::methods::Registry::builtin();
    match reg.get(&id) {
        Some(m) => {
            let mut r = axum::Json(lanka_core::Envelope::ok(
                m.info().to_json(),
                Meta::bare(None),
            ))
            .into_response();
            r.headers_mut().insert(
                header::CACHE_CONTROL,
                HeaderValue::from_static("public, max-age=300"),
            );
            r
        }
        None => problem(
            StatusCode::NOT_FOUND,
            &format!("not_found: no analysis method \"{id}\""),
            Value::Null,
        ),
    }
}

#[allow(clippy::result_large_err)]
fn parse_body(body: &Bytes) -> Result<Value, Response> {
    if body.len() > 262_144 {
        return Err(problem(
            StatusCode::PAYLOAD_TOO_LARGE,
            "requests are limited to 256 KB",
            Value::Null,
        ));
    }
    serde_json::from_slice(body).map_err(|e| {
        problem(
            StatusCode::BAD_REQUEST,
            "the body is not JSON",
            json!({"problems": [{"path": "", "message": e.to_string()}]}),
        )
    })
}

async fn create(State(st): State<AppState>, body: Bytes) -> Response {
    let Some(e) = engine(&st) else {
        return unavailable();
    };
    let v = match parse_body(&body) {
        Ok(v) => v,
        Err(r) => return r,
    };
    match e.run(&v).await {
        Ok(ran) => {
            let loc = format!("/v2/analyses/{}", ran.id);
            let body = json!({"success": true, "message": "see other",
                              "payload": {"id": ran.id, "state": ran.state, "href": loc, "computed": ran.computed},
                              "meta": Meta::bare(None)});
            let mut r = json_response(
                StatusCode::SEE_OTHER,
                serde_json::to_vec(&body).unwrap_or_default(),
                "no-store",
            );
            if let Ok(h) = HeaderValue::from_str(&loc) {
                r.headers_mut().insert(header::LOCATION, h);
            }
            r
        }
        Err(err) => engine_error(err),
    }
}

async fn plan(State(st): State<AppState>, body: Bytes) -> Response {
    let Some(e) = engine(&st) else {
        return unavailable();
    };
    let v = match parse_body(&body) {
        Ok(v) => v,
        Err(r) => return r,
    };
    match e.plan(&v).await {
        Ok(p) => json_response(
            StatusCode::OK,
            serde_json::to_vec(&lanka_core::Envelope::ok(
                p,
                Meta::bare(st.catalog().data_version.clone()),
            ))
            .unwrap_or_default(),
            "no-store",
        ),
        Err(err) => engine_error(err),
    }
}

fn valid_id(id: &str) -> bool {
    id.len() == 19
        && id.starts_with("an_")
        && id[3..]
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
}

fn not_found(id: &str) -> Response {
    problem(
        StatusCode::NOT_FOUND,
        &format!("not_found: no analysis \"{id}\""),
        Value::Null,
    )
}

async fn result(
    State(st): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(id): Path<String>,
) -> Response {
    let Some(e) = engine(&st) else {
        return unavailable();
    };
    if !valid_id(&id) {
        return not_found(&id);
    }
    let etag = format!("\"{id}\"");
    if headers
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.split(',').any(|t| t.trim() == etag))
    {
        let mut r = StatusCode::NOT_MODIFIED.into_response();
        r.headers_mut()
            .insert(header::CACHE_CONTROL, HeaderValue::from_static(IMMUTABLE));
        if let Ok(h) = HeaderValue::from_str(&etag) {
            r.headers_mut().insert(header::ETAG, h);
        }
        return r;
    }
    match e.get(&id).await {
        Ok(Some(ran)) => {
            let mut doc = serde_json::from_slice::<Value>(&ran.bytes).ok();
            let dv: Option<String> = doc
                .as_ref()
                .and_then(|d| d["provenance"]["data_version"].as_str().map(str::to_owned));
            let bytes = match doc.as_mut() {
                Some(d) => {
                    lanka_analysis::result::relink(d, e.base_url());
                    serde_json::to_vec(d).unwrap_or_else(|_| ran.bytes.to_vec())
                }
                None => ran.bytes.to_vec(),
            };
            let mut r = json_response(
                StatusCode::OK,
                envelope_raw(&bytes, dv.as_deref()),
                IMMUTABLE,
            );
            if let Ok(h) = HeaderValue::from_str(&etag) {
                r.headers_mut().insert(header::ETAG, h);
            }
            r
        }
        Ok(None) => not_found(&id),
        Err(err) => engine_error(err),
    }
}

#[derive(Debug, Deserialize)]
pub struct OutputParams {
    pub format: Option<String>,
}

/// Arrow IPC stream of a frame output: `code` + one column per field
/// (numbers → Float64, anything else → Utf8).
fn frame_arrow(data: &Value) -> anyhow::Result<Vec<u8>> {
    let codes: Vec<Option<String>> = data["codes"]
        .as_array()
        .map(|a| a.iter().map(|c| c.as_str().map(str::to_owned)).collect())
        .unwrap_or_default();
    let mut fields = vec![Field::new("code", DataType::Utf8, false)];
    let mut arrays: Vec<ArrayRef> = vec![Arc::new(StringArray::from(codes))];
    if let Some(cols) = data["columns"].as_object() {
        for (name, vals) in cols {
            let vals = vals.as_array().cloned().unwrap_or_default();
            if vals.iter().all(|v| v.is_number() || v.is_null()) {
                fields.push(Field::new(name, DataType::Float64, true));
                arrays.push(Arc::new(Float64Array::from(
                    vals.iter().map(Value::as_f64).collect::<Vec<_>>(),
                )));
            } else {
                fields.push(Field::new(name, DataType::Utf8, true));
                arrays.push(Arc::new(StringArray::from(
                    vals.iter()
                        .map(|v| match v {
                            Value::Null => None,
                            Value::String(s) => Some(s.clone()),
                            v => Some(v.to_string()),
                        })
                        .collect::<Vec<_>>(),
                )));
            }
        }
    }
    let schema = Arc::new(Schema::new(fields));
    let batch = RecordBatch::try_new(Arc::clone(&schema), arrays)?;
    let mut buf = Vec::new();
    {
        let mut w = StreamWriter::try_new(&mut buf, &schema)?;
        w.write(&batch)?;
        w.finish()?;
    }
    Ok(buf)
}

async fn output(
    State(st): State<AppState>,
    Path((id, out)): Path<(String, String)>,
    Query(q): Query<OutputParams>,
) -> Response {
    let Some(e) = engine(&st) else {
        return unavailable();
    };
    if !valid_id(&id) {
        return not_found(&id);
    }
    let data = match e.output(&id, &out).await {
        Ok(Some(d)) => d,
        Ok(None) => {
            return problem(
                StatusCode::NOT_FOUND,
                &format!("not_found: {id} has no output \"{out}\""),
                Value::Null,
            );
        }
        Err(err) => return engine_error(err),
    };
    let is_frame = data.get("codes").is_some();
    let typed = |ct: &'static str, body: Vec<u8>| {
        let mut r = (StatusCode::OK, body).into_response();
        r.headers_mut()
            .insert(header::CONTENT_TYPE, HeaderValue::from_static(ct));
        r.headers_mut()
            .insert(header::CACHE_CONTROL, HeaderValue::from_static(IMMUTABLE));
        r
    };
    match q.format.as_deref().unwrap_or("json") {
        "json" => json_response(
            StatusCode::OK,
            envelope_raw(&serde_json::to_vec(&data).unwrap_or_default(), None),
            IMMUTABLE,
        ),
        "csv" if is_frame => {
            let order: Vec<String> = data["columns"]
                .as_object()
                .map(|o| o.keys().cloned().collect())
                .unwrap_or_default();
            typed(
                "text/csv; charset=utf-8",
                lanka_analysis::bundle::frame_csv(&data, &order).into_bytes(),
            )
        }
        "arrow" if is_frame => match frame_arrow(&data) {
            Ok(b) => typed("application/vnd.apache.arrow.stream", b),
            Err(err) => engine_error(EngineError::Internal(err)),
        },
        f => problem(
            StatusCode::BAD_REQUEST,
            &format!(
                "format \"{f}\": json for every output; csv and arrow for frames (parquet/geojson with WP-30)"
            ),
            Value::Null,
        ),
    }
}

async fn bundle(State(st): State<AppState>, Path(id): Path<String>) -> Response {
    let Some(e) = engine(&st) else {
        return unavailable();
    };
    if !valid_id(&id) {
        return not_found(&id);
    }
    let ran = match e.get(&id).await {
        Ok(Some(r)) => r,
        Ok(None) => return not_found(&id),
        Err(err) => return engine_error(err),
    };
    let doc: Value = serde_json::from_slice(&ran.bytes).unwrap_or(Value::Null);
    let mut entries: Vec<(String, Vec<u8>)> = vec![("result.json".to_owned(), ran.bytes.to_vec())];
    let mut files: Vec<(String, &'static str)> = vec![("result.json".to_owned(), "json")];
    for o in doc["outputs"].as_array().into_iter().flatten() {
        let Some(oid) = o["id"].as_str() else {
            continue;
        };
        let Ok(Some(data)) = e.output(&id, oid).await else {
            continue;
        };
        entries.push((
            format!("outputs/{oid}.json"),
            serde_json::to_vec(&data).unwrap_or_default(),
        ));
        files.push((format!("outputs/{oid}.json"), "json"));
        if data.get("codes").is_some() {
            let order: Vec<String> = o["columns"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|c| c.as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default();
            entries.push((
                format!("outputs/{oid}.csv"),
                lanka_analysis::bundle::frame_csv(&data, &order).into_bytes(),
            ));
            files.push((format!("outputs/{oid}.csv"), "csv"));
        }
    }
    entries.push((
        "README.md".to_owned(),
        lanka_analysis::bundle::readme(&doc).into_bytes(),
    ));
    entries.push((
        "datapackage.json".to_owned(),
        serde_json::to_vec_pretty(&lanka_analysis::bundle::datapackage(&doc, &files))
            .unwrap_or_default(),
    ));
    entries.push((
        "prov.jsonld".to_owned(),
        serde_json::to_vec_pretty(&lanka_analysis::bundle::prov(&doc)).unwrap_or_default(),
    ));
    let mut r = (StatusCode::OK, lanka_analysis::bundle::zip(&entries)).into_response();
    let h = r.headers_mut();
    h.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/zip"),
    );
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static(IMMUTABLE));
    if let Ok(v) = HeaderValue::from_str(&format!("attachment; filename=\"{id}.zip\"")) {
        h.insert(header::CONTENT_DISPOSITION, v);
    }
    r
}

#[cfg(test)]
mod tests {
    use std::sync::RwLock;

    use axum::body::Body;
    use http_body_util::BodyExt;
    use lanka_core::Catalog;
    use tower::ServiceExt;

    use super::*;

    /// The router over a never-connected pool and no analysis engine.
    async fn send(req: axum::http::Request<Body>) -> (StatusCode, Value) {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .acquire_timeout(std::time::Duration::from_millis(300))
            .connect_lazy("postgres://nobody@127.0.0.1:1/none")
            .expect("lazy pool");
        let state = AppState {
            pool,
            catalog: Arc::new(RwLock::new(Arc::new(Catalog::default()))),
            started: std::time::Instant::now(),
            store_dir: None,
            app: None,
            grids: Arc::new(RwLock::new(Arc::new(crate::grids::GridSet::default()))),
            analysis: None,
        };
        let res = crate::router(state).oneshot(req).await.expect("response");
        let status = res.status();
        let bytes = res.into_body().collect().await.expect("body").to_bytes();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    #[tokio::test]
    async fn method_catalogue_and_unavailable_engine() {
        let get = |u: &str| {
            axum::http::Request::get(u)
                .body(Body::empty())
                .expect("req")
        };
        let (s, v) = send(get("/v2/analysis/methods")).await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(v["payload"]["methods"].as_array().map(Vec::len), Some(7));
        assert!(
            v["payload"]["guards"]
                .as_array()
                .is_some_and(|g| g.len() == 25)
        );
        let (s, v) = send(get("/v2/analysis/methods/relate")).await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(v["payload"]["min_n"]["block"], 10);
        assert_eq!(
            send(get("/v2/analysis/methods/gwr")).await.0,
            StatusCode::NOT_FOUND
        );
        let post = axum::http::Request::post("/v2/analyses")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"spec":"ldl-analysis/1"}"#))
            .expect("req");
        assert_eq!(send(post).await.0, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(
            send(get("/v2/analyses/an_0123456789abcdef")).await.0,
            StatusCode::SERVICE_UNAVAILABLE
        );
        assert!(valid_id("an_0123456789abcdef"));
        assert!(!valid_id("an_0123456789ABCDEF") && !valid_id("an_../../etc"));
    }
}
