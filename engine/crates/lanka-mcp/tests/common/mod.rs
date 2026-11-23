//! Shared fixtures: a synthetic catalogue, a never-connected pool, and a
//! Streamable HTTP client that drives the real `/mcp` router in-process.

#![allow(dead_code)]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use http_body_util::BodyExt;
use lanka_core::Catalog;
use lanka_core::catalog::{CurrentVersion, DatasetInfo, IndicatorInfo, LicenceInfo, SourceInfo};
use lanka_mcp::McpContext;
use serde_json::{Value, json};
use tower::ServiceExt;

pub fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

pub fn profile() -> Arc<lanka_core::Profile> {
    Arc::new(
        lanka_core::Profile::discover(&repo_root().join("registry"))
            .and_then(lanka_core::Profile::load)
            .expect("repository profile"),
    )
}

/// Two published datasets (an official one and an estimate), one features
/// dataset, their sources and licences.
pub fn catalog() -> Catalog {
    let mut c = Catalog {
        data_version: Some("20261122.3".into()),
        profile: Some(profile()),
        ..Catalog::default()
    };
    let ds =
        |id: &str, kind: &str, key: i32, src: &str, inds: &[&str], limits: &[&str]| DatasetInfo {
            id: id.into(),
            key,
            kind: kind.into(),
            title: format!("{id} title"),
            description: format!("{id} description"),
            method: String::new(),
            limits: limits.iter().map(|s| (*s).to_owned()).collect(),
            publication_class: "A".into(),
            time_from: Some("2012".into()),
            time_to: Some("2024".into()),
            source_ids: vec![src.into()],
            indicator_ids: inds.iter().map(|s| (*s).to_owned()).collect(),
            current: Some(CurrentVersion {
                version_id: i64::from(key),
                seq: 1,
                dsv: format!("dsv_{key:016x}"),
                vintage: None,
                row_count: 10,
                published_at: None,
            }),
        };
    for d in [
        ds(
            "census-demo",
            "indicators",
            1,
            "dcs",
            &["census.pop_total", "census.sex_ratio"],
            &["Census reference night 2024; no intercensal updates."],
        ),
        ds(
            "derived-flood",
            "indicators",
            2,
            "ldl",
            &["exp.flood_pop_share"],
            &["Modelled exposure."],
        ),
        ds("hospitals-demo", "features", 3, "osm", &[], &[]),
    ] {
        c.dataset_order.push(d.id.clone());
        c.dataset_by_key.insert(d.key, d.id.clone());
        c.datasets.insert(d.id.clone(), d);
    }
    let ind = |id: &str, ds: &str, class: &str, label: &str| IndicatorInfo {
        id: id.into(),
        key: 1,
        dataset_id: ds.into(),
        label: label.into(),
        unit: Some("{person}".into()),
        unit_label: Some("persons".into()),
        unit_mult: 0,
        dims: Vec::new(),
        geo_levels: vec!["national".into(), "adm1".into(), "adm2".into()],
        period_type: "point".into(),
        freq: "P".into(),
        measure_type: "count".into(),
        aggregation: "sum".into(),
        class: class.into(),
        completes: None,
        time_invariant: false,
    };
    for i in [
        ind(
            "census.pop_total",
            "census-demo",
            "official",
            "Total population",
        ),
        ind("census.sex_ratio", "census-demo", "official", "Sex ratio"),
        ind(
            "exp.flood_pop_share",
            "derived-flood",
            "estimated",
            "Population share in the 100-year flood zone",
        ),
    ] {
        c.indicators.insert(i.id.clone(), i);
    }
    let src = |id: &str, publisher: &str, lic: &str, attribution: &str| SourceInfo {
        id: id.into(),
        publisher: publisher.into(),
        title: format!("{id} source"),
        homepage: "https://example.org".into(),
        licence_id: lic.into(),
        attribution: attribution.into(),
        cadence: "annual".into(),
        access_method: "file".into(),
        themes: vec!["SOCI".into()],
        status: "active".into(),
    };
    c.sources.insert(
        "dcs".into(),
        src(
            "dcs",
            "Department of Census and Statistics",
            "lk-gov-unstated",
            "DCS, Census 2024",
        ),
    );
    c.sources.insert(
        "ldl".into(),
        src(
            "ldl",
            "Lanka Data Layer",
            "cc-by-4.0",
            "Lanka Data Layer (derived)",
        ),
    );
    c.sources.insert(
        "osm".into(),
        src(
            "osm",
            "OpenStreetMap contributors",
            "odbl",
            "© OpenStreetMap contributors",
        ),
    );
    let lic = |id: &str, status: &str| LicenceInfo {
        id: id.into(),
        status: status.into(),
        url: None,
        spdx: None,
        redistribution: "bulk".into(),
    };
    c.licences.insert(
        "lk-gov-unstated".into(),
        lic("lk-gov-unstated", "permission_unclear"),
    );
    c.licences
        .insert("cc-by-4.0".into(), lic("cc-by-4.0", "attribution"));
    c.licences.insert("odbl".into(), lic("odbl", "attribution"));
    c
}

/// A context over the synthetic catalogue and a pool that never connects:
/// everything that needs the database must fail cleanly, and nothing else may touch it.
pub fn context() -> McpContext {
    let pool = sqlx::postgres::PgPoolOptions::new()
        .acquire_timeout(Duration::from_millis(300))
        .connect_lazy("postgres://nobody@127.0.0.1:1/none")
        .expect("lazy pool");
    let cat = Arc::new(catalog());
    McpContext {
        pool,
        catalog: Arc::new(move || Arc::clone(&cat)),
        analysis: None,
        app: None,
        reverse: None,
    }
}

pub struct Client {
    pub router: Router,
    next_id: i64,
}

impl Client {
    pub fn new(ctx: McpContext) -> Self {
        Self {
            router: lanka_mcp::router(ctx),
            next_id: 1,
        }
    }

    /// POSTs one JSON-RPC message to `/mcp` with the headers the transport demands.
    pub async fn post(&self, body: Value, protocol_header: Option<&str>) -> (StatusCode, Value) {
        let mut req = Request::post("/mcp")
            .header(header::HOST, "mcp.example.org")
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::ACCEPT, "application/json, text/event-stream");
        if let Some(v) = protocol_header {
            req = req.header("mcp-protocol-version", v);
        }
        // SEP-2243 standard header (2026-07-28 requires it; older revisions ignore a matching one)
        if let Some(m) = body["method"].as_str() {
            req = req.header("mcp-method", m);
        }
        let res = self
            .router
            .clone()
            .oneshot(req.body(Body::from(body.to_string())).expect("request"))
            .await
            .expect("response");
        let status = res.status();
        let bytes = res.into_body().collect().await.expect("body").to_bytes();
        (
            status,
            serde_json::from_slice(&bytes)
                .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into_owned())),
        )
    }

    pub async fn rpc(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        let (status, v) = self
            .post(
                json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }),
                None,
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{method}: {v}");
        v
    }

    pub async fn initialize(&mut self, version: &str) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        let (status, v) = self
            .post(
                json!({
                    "jsonrpc": "2.0", "id": id, "method": "initialize",
                    "params": {
                        "protocolVersion": version, "capabilities": {},
                        "clientInfo": { "name": "test-client", "version": "1.0.0" }
                    }
                }),
                Some(version),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{v}");
        v
    }

    /// `tools/call`; returns the `result` object.
    pub async fn call(&mut self, name: &str, arguments: Value) -> Value {
        let v = self
            .rpc(
                "tools/call",
                json!({ "name": name, "arguments": arguments }),
            )
            .await;
        assert!(v.get("error").is_none(), "{name}: protocol error {v}");
        v["result"].clone()
    }
}

/// The text of a tool result.
pub fn text_of(result: &Value) -> String {
    result["content"][0]["text"]
        .as_str()
        .unwrap_or_default()
        .to_owned()
}
